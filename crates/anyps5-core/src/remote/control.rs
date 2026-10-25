//! The control channel: the WebSocket listener, one thread per socket, and the hub that owns the
//! pairing state machine.
//!
//! Port of the macOS `RemoteServer` state machine with the hardening of DESIGN.md "Phone as
//! controller": the key is checked before an old connection is taken over, a pending pairing times
//! out and is replaced by a new attempt instead of blocking everyone, and a denied phone has to wait.

use std::{
    collections::{HashMap, VecDeque},
    io,
    net::{IpAddr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, Sender, TryRecvError, channel},
    },
    thread,
    time::{Duration, Instant},
};

use socket2::{Domain, Protocol, Socket, Type};
use tungstenite::{
    Error as WsError, WebSocket,
    handshake::server::{ErrorResponse, Request, Response},
    http,
    protocol::WebSocketConfig,
};

use super::{
    RemoteEvents,
    devices::PairedDevices,
    protocol::{Message, PROTOCOL_VERSION},
    relay::Relay,
    security::{
        RateLimit, is_allowed_peer, is_on_link, local_subnets, random_code, random_session,
    },
};

/// The largest legitimate message is a saved pad layout.
const MAX_MESSAGE: usize = 512 * 1024;
/// Paired phones the list holds; the player removes one before pairing another.
pub const MAX_DEVICES: usize = 8;
const MAX_CLIENTS: usize = 8;
const MAX_CLIENTS_PER_IP: usize = 4;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// The phone pings every 2 s, so silence for this long means it is gone.
const IDLE_TIMEOUT: Duration = Duration::from_secs(10);
const PAIRING_TIMEOUT: Duration = Duration::from_secs(60);
/// A pending pairing this old may be replaced by another phone.
const STALE_PENDING: Duration = Duration::from_secs(20);
const DENY_COOLDOWN: Duration = Duration::from_secs(10);
const PROMPTS_PER_MINUTE: usize = 6;
const MAX_FRAMES_BEFORE_HELLO: u32 = 20;
/// A held direction repeats about 15 times a second; this leaves room and stops a flood.
const REQUESTS_PER_SECOND: f64 = 40.0;
const REQUEST_BURST: f64 = 80.0;
/// A phone that keeps flooding after its requests were dropped this often is disconnected.
const MAX_DROPPED: u32 = 400;

pub(super) enum Out {
    Text(String),
    Close,
}

pub(super) enum HubMessage {
    Connected {
        cid: u64,
        peer: IpAddr,
        out: Sender<Out>,
    },
    Frame {
        cid: u64,
        message: Message,
    },
    Closed {
        cid: u64,
    },
    Respond {
        request: u64,
        allow: bool,
    },
    Publish(Message),
    Forget(String),
    Stop,
}

/// Binds the control port dual-stack (IPv4 and IPv6), the preferred one first, then any.
pub(super) fn bind_listener(preferred: u16) -> io::Result<TcpListener> {
    for port in [preferred, 0] {
        if let Ok(socket) = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))
            && socket.set_only_v6(false).is_ok()
        {
            // On Windows SO_REUSEADDR would let another program share the port.
            #[cfg(unix)]
            let _ = socket.set_reuse_address(true);
            if socket
                .bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)).into())
                .is_ok()
                && socket.listen(16).is_ok()
            {
                return Ok(socket.into());
            }
        }
        if let Ok(listener) = TcpListener::bind(("0.0.0.0", port)) {
            return Ok(listener);
        }
    }
    Err(io::Error::other("no TCP port for the phone server"))
}

pub(super) fn spawn_listener(
    listener: TcpListener,
    hub: Sender<HubMessage>,
    stop: Arc<AtomicBool>,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let active: Arc<Mutex<Vec<IpAddr>>> = Arc::default();
    thread::Builder::new()
        .name("remote-accept".into())
        .spawn(move || {
            let mut next = 1u64;
            while !stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        let ip = peer.ip().to_canonical();
                        // Private and on a network this PC is plugged into (a routed or VPN peer
                        // is not the home network); loopback is for tests and tools.
                        if !is_allowed_peer(ip) || !is_on_link(ip, &local_subnets()) {
                            continue;
                        }
                        {
                            let mut list = active.lock().expect("clients");
                            let same = list.iter().filter(|a| **a == ip).count();
                            if list.len() >= MAX_CLIENTS || same >= MAX_CLIENTS_PER_IP {
                                continue;
                            }
                            list.push(ip);
                        }
                        let cid = next;
                        next += 1;
                        let hub = hub.clone();
                        let active = active.clone();
                        let spawned = thread::Builder::new()
                            .name(format!("remote-client-{cid}"))
                            .spawn(move || {
                                client(stream, cid, ip, &hub);
                                let mut list = active.lock().expect("clients");
                                if let Some(at) = list.iter().position(|a| *a == ip) {
                                    list.swap_remove(at);
                                }
                            });
                        if spawned.is_err() {
                            tracing::warn!("remote: could not start a client thread");
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(40));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(100)),
                }
            }
        })?;
    Ok(())
}

// The error type is fixed by tungstenite's callback trait.
#[allow(clippy::result_large_err)]
fn refuse_origin(request: &Request, response: Response) -> Result<Response, ErrorResponse> {
    // Browsers always send Origin on a WebSocket upgrade; the phone app does not. Refusing it keeps
    // a web page from talking to the launcher through the player's own browser.
    if request.headers().contains_key("origin") {
        let refusal = http::Response::builder()
            .status(http::StatusCode::FORBIDDEN)
            .body(None)
            .expect("static response");
        return Err(refusal);
    }
    Ok(response)
}

fn is_timeout(error: &WsError) -> bool {
    matches!(error, WsError::Io(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut))
}

fn client(stream: TcpStream, cid: u64, peer: IpAddr, hub: &Sender<HubMessage>) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT));
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE))
        .max_frame_size(Some(MAX_MESSAGE));
    let Ok(mut socket) = tungstenite::accept_hdr_with_config(stream, refuse_origin, Some(config))
    else {
        return;
    };
    set_keepalive(socket.get_ref());
    let _ = socket
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(25)));
    let (out, queue) = channel();
    if hub.send(HubMessage::Connected { cid, peer, out }).is_err() {
        return;
    }
    run_client(&mut socket, cid, hub, &queue);
    let _ = hub.send(HubMessage::Closed { cid });
}

fn set_keepalive(stream: &TcpStream) {
    let socket = socket2::SockRef::from(stream);
    let keepalive = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(5))
        .with_interval(Duration::from_secs(2));
    let _ = socket.set_tcp_keepalive(&keepalive);
}

fn run_client(
    socket: &mut WebSocket<TcpStream>,
    cid: u64,
    hub: &Sender<HubMessage>,
    queue: &Receiver<Out>,
) {
    let mut last_seen = Instant::now();
    loop {
        loop {
            match queue.try_recv() {
                Ok(Out::Text(text)) => {
                    if socket.send(tungstenite::Message::text(text)).is_err() {
                        return;
                    }
                }
                Ok(Out::Close) => {
                    let _ = socket.close(None);
                    let _ = socket.flush();
                    return;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        match socket.read() {
            Ok(frame) => {
                last_seen = Instant::now();
                let text = match &frame {
                    tungstenite::Message::Text(text) => Some(text.as_str().to_string()),
                    tungstenite::Message::Binary(bytes) => {
                        std::str::from_utf8(bytes).ok().map(str::to_string)
                    }
                    tungstenite::Message::Close(_) => return,
                    _ => None,
                };
                if let Some(message) = text.and_then(|t| Message::parse(&t))
                    && hub.send(HubMessage::Frame { cid, message }).is_err()
                {
                    return;
                }
            }
            Err(error) if is_timeout(&error) => {}
            Err(_) => return,
        }
        // Pongs queued by the library go out here.
        let _ = socket.flush();
        if last_seen.elapsed() > IDLE_TIMEOUT {
            return;
        }
    }
}

enum Stage {
    Pending { request: u64, since: Instant },
    Welcomed,
}

struct Current {
    cid: u64,
    device_id: String,
    device_name: String,
    peer: IpAddr,
    stage: Stage,
}

struct Peer {
    out: Sender<Out>,
    peer: IpAddr,
    frames_before_welcome: u32,
    /// Requests of the welcomed phone: a hand cannot press more than this.
    requests: RateLimit,
    dropped: u32,
}

pub(super) struct Hub {
    pub devices: PairedDevices,
    pub events: Arc<dyn RemoteEvents>,
    pub relay: Arc<Relay>,
    pub name: String,
    pub allow_all: bool,
    clients: HashMap<u64, Peer>,
    current: Option<Current>,
    next_request: u64,
    prompts: VecDeque<Instant>,
    denied: HashMap<String, Instant>,
    library: Option<Message>,
}

impl Hub {
    pub fn new(
        devices: PairedDevices,
        events: Arc<dyn RemoteEvents>,
        relay: Arc<Relay>,
        name: String,
        allow_all: bool,
    ) -> Hub {
        Hub {
            devices,
            events,
            relay,
            name,
            allow_all,
            clients: HashMap::new(),
            current: None,
            next_request: 1,
            prompts: VecDeque::new(),
            denied: HashMap::new(),
            library: None,
        }
    }

    pub fn run(mut self, rx: Receiver<HubMessage>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(HubMessage::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(message) => self.handle(message),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.expire_pairing();
        }
        self.drop_current(None);
        for peer in self.clients.values() {
            let _ = peer.out.send(Out::Close);
        }
    }

    fn handle(&mut self, message: HubMessage) {
        match message {
            HubMessage::Connected { cid, peer, out } => {
                self.clients.insert(
                    cid,
                    Peer {
                        out,
                        peer,
                        frames_before_welcome: 0,
                        requests: RateLimit::new(REQUESTS_PER_SECOND, REQUEST_BURST),
                        dropped: 0,
                    },
                );
            }
            HubMessage::Frame { cid, message } => self.frame(cid, message),
            HubMessage::Closed { cid } => {
                self.clients.remove(&cid);
                if self.current.as_ref().is_some_and(|c| c.cid == cid) {
                    self.drop_current(None);
                }
            }
            HubMessage::Respond { request, allow } => self.respond(request, allow),
            HubMessage::Publish(message) => {
                if matches!(message, Message::Library { .. }) {
                    self.library = Some(message.clone());
                }
                if let Some(current) = &self.current
                    && matches!(current.stage, Stage::Welcomed)
                {
                    self.send(current.cid, &message);
                }
            }
            HubMessage::Forget(id) => {
                if let Err(error) = self.devices.remove(&id) {
                    tracing::warn!("remote: could not save the phone list: {error}");
                }
                if self.current.as_ref().is_some_and(|c| c.device_id == id) {
                    self.drop_current(Some("denied"));
                }
            }
            HubMessage::Stop => {}
        }
    }

    fn send(&self, cid: u64, message: &Message) {
        if let Some(peer) = self.clients.get(&cid) {
            let _ = peer.out.send(Out::Text(message.to_json()));
        }
    }

    fn close(&self, cid: u64, reason: Option<&str>) {
        if let Some(reason) = reason {
            self.send(
                cid,
                &Message::Rejected {
                    reason: reason.to_string(),
                },
            );
        }
        if let Some(peer) = self.clients.get(&cid) {
            let _ = peer.out.send(Out::Close);
        }
    }

    /// Ends the current phone (pending or welcomed): tells it why, closes it, tells the UI.
    fn drop_current(&mut self, reason: Option<&str>) {
        let Some(current) = self.current.take() else {
            return;
        };
        if reason.is_some() || self.clients.contains_key(&current.cid) {
            self.close(current.cid, reason);
        }
        self.relay.end();
        match current.stage {
            Stage::Pending { request, .. } => {
                self.events.pairing_ended(request);
                self.events.connecting(None);
            }
            Stage::Welcomed => self.events.disconnected(),
        }
    }

    fn frame(&mut self, cid: u64, message: Message) {
        let welcomed = self
            .current
            .as_ref()
            .is_some_and(|c| c.cid == cid && matches!(c.stage, Stage::Welcomed));
        if let Message::Hello {
            protocol,
            device_id,
            device_name,
            key,
        } = message
        {
            self.hello(cid, protocol, device_id, device_name, key);
            return;
        }
        if !welcomed {
            if let Some(peer) = self.clients.get_mut(&cid) {
                peer.frames_before_welcome += 1;
                if peer.frames_before_welcome > MAX_FRAMES_BEFORE_HELLO {
                    self.close(cid, None);
                }
            }
            return;
        }
        if let Some(peer) = self.clients.get_mut(&cid)
            && !peer.requests.allow(Instant::now())
        {
            peer.dropped += 1;
            if peer.dropped > MAX_DROPPED {
                self.close(cid, Some("busy"));
            }
            return;
        }
        match message {
            Message::Latency { milliseconds } if milliseconds.is_finite() => {
                self.events.latency(milliseconds);
            }
            Message::WantArt { .. }
            | Message::Command { .. }
            | Message::Select { .. }
            | Message::Launch { .. }
            | Message::Close
            | Message::Home
            | Message::Create
            | Message::SaveLayout { .. } => self.events.request(message),
            _ => {}
        }
    }

    fn hello(
        &mut self,
        cid: u64,
        protocol: u32,
        device_id: String,
        device_name: String,
        key: Option<String>,
    ) {
        if protocol != PROTOCOL_VERSION {
            self.close(cid, Some("protocol"));
            return;
        }
        let Some(peer) = self.clients.get(&cid).map(|p| p.peer) else {
            return;
        };
        if self.current.as_ref().is_some_and(|c| c.cid == cid) {
            return;
        }
        let device_name: String = device_name.chars().take(48).collect();
        if device_id.is_empty() || device_id.len() > 64 {
            self.close(cid, Some("protocol"));
            return;
        }
        let key_ok = key
            .as_deref()
            .is_some_and(|key| self.devices.is_known(&device_id, key));
        if let Some(current) = &self.current {
            let pending_since = match current.stage {
                Stage::Pending { since, .. } => Some(since),
                Stage::Welcomed => None,
            };
            let takeover = if current.device_id == device_id {
                // The same phone coming back. A welcomed connection is only replaced by proof of
                // the key; an unfinished pairing is replaced by the retry.
                key_ok || pending_since.is_some()
            } else {
                pending_since.is_some_and(|since| since.elapsed() > STALE_PENDING)
            };
            if !takeover {
                self.close(cid, Some("busy"));
                return;
            }
            self.drop_current(Some("busy"));
        }
        self.current = Some(Current {
            cid,
            device_id: device_id.clone(),
            device_name: device_name.clone(),
            peer,
            stage: Stage::Pending {
                request: 0,
                since: Instant::now(),
            },
        });
        self.events.connecting(Some(device_name.clone()));
        if key_ok {
            let key = key.unwrap_or_default();
            self.welcome(&key);
            return;
        }
        let now = Instant::now();
        if self
            .denied
            .get(&device_id)
            .is_some_and(|until| *until > now)
        {
            self.drop_current(Some("denied"));
            return;
        }
        while self
            .prompts
            .front()
            .is_some_and(|at| now.duration_since(*at) > Duration::from_secs(60))
        {
            self.prompts.pop_front();
        }
        if self.prompts.len() >= PROMPTS_PER_MINUTE {
            self.drop_current(Some("busy"));
            return;
        }
        self.prompts.push_back(now);
        let request = self.next_request;
        self.next_request += 1;
        let code = random_code();
        if let Some(current) = &mut self.current {
            current.stage = Stage::Pending {
                request,
                since: now,
            };
        }
        self.send(cid, &Message::Pairing { code: code.clone() });
        self.events.pairing(super::PairingRequest {
            id: request,
            device_name,
            code,
        });
        if self.allow_all {
            self.respond(request, true);
        }
    }

    fn respond(&mut self, request: u64, allow: bool) {
        let Some(current) = &self.current else {
            return;
        };
        let Stage::Pending {
            request: pending, ..
        } = current.stage
        else {
            return;
        };
        if pending != request {
            return;
        }
        if !allow {
            self.denied
                .insert(current.device_id.clone(), Instant::now() + DENY_COOLDOWN);
            self.drop_current(Some("denied"));
            return;
        }
        let (id, name) = (current.device_id.clone(), current.device_name.clone());
        if !self.devices.has_id(&id) && self.devices.list().len() >= MAX_DEVICES {
            self.drop_current(Some("busy"));
            return;
        }
        match self.devices.add(&id, &name) {
            Ok(device) => {
                self.events.pairing_ended(request);
                self.welcome(&device.key);
            }
            Err(error) => {
                tracing::warn!("remote: could not save the phone: {error}");
                self.drop_current(Some("busy"));
            }
        }
    }

    fn welcome(&mut self, key: &str) {
        let Some(current) = &mut self.current else {
            return;
        };
        let session = random_session();
        current.stage = Stage::Welcomed;
        let (cid, name, peer) = (current.cid, current.device_name.clone(), current.peer);
        self.relay.begin(session, peer);
        self.send(
            cid,
            &Message::Welcome {
                session: session.to_string(),
                udp_port: self.relay.phone_port(),
                mac_name: self.name.clone(),
                key: key.to_string(),
            },
        );
        self.events.connected(name);
        if let Some(library) = &self.library {
            self.send(cid, library);
        }
    }

    fn expire_pairing(&mut self) {
        let expired = self.current.as_ref().is_some_and(
            |c| matches!(c.stage, Stage::Pending { since, .. } if since.elapsed() > PAIRING_TIMEOUT),
        );
        if expired {
            self.drop_current(Some("denied"));
        }
    }
}
