//! LudusCtrl's browser client: for a phone that cannot run the app (an iPhone), this PC serves a
//! page over HTTPS on the LAN and the page sends the phone's controller over a WebSocket. Written
//! from the "Browser client" section of LudusCtrl's `docs/PROTOCOL.md`; docs/ludusctrl-web-api.md
//! is the interface and the list of where the launcher differs.
//!
//! The bridge holds no pad: it feeds the [`PadSlot`] of the running [`LudusServer`](super::LudusServer),
//! the same one the app's UDP receiver feeds, so this PC never shows a second controller. The
//! token in the link (and its QR code) is the proof that the player sees the PC's screen; it is
//! new at every start and every New Link.

mod http;
mod page;
mod session;
mod tls;

use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rustls::{ServerConfig, ServerConnection, StreamOwned};
use socket2::{Domain, Protocol, Socket, Type};

pub use tls::Certificate;

use super::{PadSlot, allowed_source};
use crate::paths::AppPaths;
use crate::remote::{
    RemoteError, discovery::lan_addresses, security::local_subnets, security::random_bytes,
};

/// The TCP port of the page and its WebSocket. A busy port falls back to a free one; the links
/// carry whichever is bound.
pub const LUDUS_WEB_PORT: u16 = 45991;
/// The most connections at a time, page loads included.
pub const MAX_CONNECTIONS: usize = 16;
const ACCEPT_SLEEP: Duration = Duration::from_millis(40);

type Stream = StreamOwned<ServerConnection, TcpStream>;
/// The device names of the connections that hold or claim the pad, for the status.
pub(super) type Devices = Mutex<HashMap<u64, String>>;

#[derive(Clone, Debug)]
pub struct BrowserConfig {
    /// `<config>/LudusCtrlWeb.json`: the certificate and its key.
    pub cert_file: PathBuf,
    pub port: u16,
    /// Listen on this address only; `None` is every interface. Tests bind loopback, so a test
    /// run never asks the firewall for a port.
    pub bind: Option<IpAddr>,
    timers: Timers,
}

impl BrowserConfig {
    pub fn new(cert_file: PathBuf) -> BrowserConfig {
        BrowserConfig {
            cert_file,
            port: LUDUS_WEB_PORT,
            bind: None,
            timers: Timers::default(),
        }
    }

    pub fn file_in(paths: &AppPaths) -> PathBuf {
        paths.config.join("LudusCtrlWeb.json")
    }

    #[cfg(test)]
    fn with_timers(mut self, timers: Timers) -> BrowserConfig {
        self.timers = timers;
        self
    }
}

/// The protocol's deadlines; tests shorten them.
#[derive(Clone, Copy, Debug)]
struct Timers {
    /// The TLS handshake.
    handshake: Duration,
    /// The request head.
    head: Duration,
    /// The first frame (`auth:`), and any frame after it (idle).
    frame: Duration,
    /// Reading for the peer's close after ours.
    linger: Duration,
    /// How often a waiting read looks at the stop flag and the token.
    slice: Duration,
}

impl Default for Timers {
    fn default() -> Self {
        Timers {
            handshake: Duration::from_secs(5),
            head: Duration::from_secs(10),
            frame: Duration::from_secs(5),
            linger: Duration::from_secs(1),
            slice: Duration::from_millis(250),
        }
    }
}

/// What the settings show of the browser client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserInfo {
    /// One link per LAN address, the most likely first: `https://192.168.1.23:45991/#t=<token>`.
    pub urls: Vec<String>,
    /// The TCP port really bound.
    pub port: u16,
    /// False when 45991 was busy (maybe the LudusCtrl app for PC): the links use `port`.
    pub preferred_port: bool,
    pub certificate: Certificate,
    /// SHA-256 of the certificate, "AB:CD:...".
    pub fingerprint: String,
}

/// What the connections share.
pub(super) struct Context {
    slot: Arc<PadSlot>,
    devices: Arc<Devices>,
    /// The token and its generation (raised by New Link).
    token: Mutex<(String, u64)>,
    stop: AtomicBool,
    /// Packets applied since the start, for the acks.
    received: AtomicU32,
    timers: Timers,
    /// The CSP's script and style hashes of the page.
    csp: (String, String),
    /// The last pad switch asked for (one per 2 s).
    last_switch: Mutex<Option<Instant>>,
    tls: Arc<ServerConfig>,
}

impl Context {
    fn token(&self) -> (String, u64) {
        self.token.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn devices(&self) -> MutexGuard<'_, HashMap<u64, String>> {
        self.devices.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A pad switch at `now` is allowed (and counted) when the last one is 2 s old.
    fn may_switch(&self, now: Instant) -> bool {
        let mut last = self.last_switch.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|at| now.saturating_duration_since(at) < session::SWITCH_EVERY) {
            return false;
        }
        *last = Some(now);
        true
    }
}

fn new_token() -> String {
    URL_SAFE_NO_PAD.encode(random_bytes::<16>())
}

/// The running bridge.
pub(super) struct Bridge {
    ctx: Arc<Context>,
    accept: Option<JoinHandle<()>>,
    info: BrowserInfo,
    bind: Option<IpAddr>,
}

impl Bridge {
    pub fn start(
        config: BrowserConfig,
        slot: Arc<PadSlot>,
        devices: Arc<Devices>,
    ) -> Result<Bridge, RemoteError> {
        let addresses = addresses(config.bind);
        let (identity, certificate) =
            tls::load_or_mint(&config.cert_file, &addresses, tls::unix_now())
                .map_err(|e| io::Error::other(format!("certificate: {e}")))?;
        let tls =
            tls::server_config(&identity).map_err(|e| io::Error::other(format!("TLS: {e}")))?;
        let listener = bind_listener(config.bind, config.port)?;
        let port = listener.local_addr()?.port();
        let preferred_port = port == config.port;
        if !preferred_port {
            tracing::warn!(
                "ludusctrl: browser port {} is busy, using {port}",
                config.port
            );
        }
        let ctx = Arc::new(Context {
            slot,
            devices,
            token: Mutex::new((new_token(), 0)),
            stop: AtomicBool::new(false),
            received: AtomicU32::new(0),
            timers: config.timers,
            csp: page::sources(),
            last_switch: Mutex::new(None),
            tls,
        });
        let info = BrowserInfo {
            urls: Vec::new(),
            port,
            preferred_port,
            certificate,
            fingerprint: identity.fingerprint(),
        };
        let accepting = ctx.clone();
        listener.set_nonblocking(true)?;
        let accept = thread::Builder::new()
            .name("ludusctrl-web".into())
            .spawn(move || accept_loop(&listener, &accepting))?;
        let mut bridge = Bridge {
            ctx,
            accept: Some(accept),
            info,
            bind: config.bind,
        };
        bridge.info.urls = bridge.urls(&addresses);
        Ok(bridge)
    }

    fn urls(&self, addresses: &[Ipv4Addr]) -> Vec<String> {
        let (token, _) = self.ctx.token();
        let port = self.info.port;
        let hosts: Vec<String> = match self.bind {
            Some(IpAddr::V4(ip)) if !ip.is_unspecified() => vec![ip.to_string()],
            Some(IpAddr::V6(ip)) if !ip.is_unspecified() => vec![format!("[{ip}]")],
            _ => addresses.iter().map(ToString::to_string).collect(),
        };
        hosts
            .into_iter()
            .map(|host| format!("https://{host}:{port}/#t={token}"))
            .collect()
    }

    pub fn info(&self) -> BrowserInfo {
        self.info.clone()
    }

    /// A new token: every open page is closed with `denied`.
    pub fn new_link(&mut self) -> BrowserInfo {
        {
            let mut token = self.ctx.token.lock().unwrap_or_else(|e| e.into_inner());
            *token = (new_token(), token.1 + 1);
        }
        self.info.urls = self.urls(&addresses(self.bind));
        self.info()
    }

    pub fn stop(&mut self) {
        self.ctx.stop.store(true, Ordering::Relaxed);
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The addresses the certificate and the links cover: the LAN addresses, or none when bound to
/// one address (loopback in tests).
fn addresses(bind: Option<IpAddr>) -> Vec<Ipv4Addr> {
    match bind {
        None => lan_addresses(),
        Some(ip) if ip.is_unspecified() => lan_addresses(),
        Some(_) => Vec::new(),
    }
}

/// No other program may bind over the port: no `SO_REUSEADDR` on Windows (it would let one share
/// it) and `SO_EXCLUSIVEADDRUSE`, so not even a program that sets `SO_REUSEADDR` can.
fn exclusive(socket: &Socket) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows::Win32::Networking::WinSock::{
            SO_EXCLUSIVEADDRUSE, SOCKET, SOL_SOCKET, setsockopt,
        };
        let on = 1i32.to_ne_bytes();
        // SAFETY: a valid socket handle and a 4-byte BOOL, as the option expects.
        let result = unsafe {
            setsockopt(
                SOCKET(socket.as_raw_socket() as usize),
                SOL_SOCKET,
                SO_EXCLUSIVEADDRUSE,
                Some(&on),
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    #[cfg(unix)]
    socket.set_reuse_address(true)?;
    Ok(())
}

fn listen(address: SocketAddr, dual_stack: bool) -> io::Result<TcpListener> {
    let socket = Socket::new(
        Domain::for_address(address),
        Type::STREAM,
        Some(Protocol::TCP),
    )?;
    if dual_stack {
        socket.set_only_v6(false)?;
    }
    exclusive(&socket)?;
    socket.bind(&address.into())?;
    socket.listen(MAX_CONNECTIONS as i32)?;
    Ok(socket.into())
}

fn bind_listener(bind: Option<IpAddr>, preferred: u16) -> io::Result<TcpListener> {
    for port in [preferred, 0] {
        let listener = match bind {
            Some(ip) => listen(SocketAddr::new(ip, port), false),
            None => listen(SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), port), true)
                .or_else(|_| listen(SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), port), false)),
        };
        if listener.is_ok() || port == 0 {
            return listener;
        }
    }
    Err(io::Error::other("no TCP port for the browser client"))
}

/// Counts a connection while it lives.
struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

fn accept_loop(listener: &TcpListener, ctx: &Arc<Context>) {
    let open = Arc::new(AtomicUsize::new(0));
    let mut next = 1u64;
    while !ctx.stopping() {
        match listener.accept() {
            Ok((stream, peer)) => {
                // Private and on a network this PC is plugged into; loopback for tests and tools.
                if !allowed_source(peer.ip(), &local_subnets()) {
                    continue;
                }
                if open.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
                    open.fetch_sub(1, Ordering::Relaxed);
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                let guard = Slot(open.clone());
                let connection = next;
                next += 1;
                let ctx = ctx.clone();
                let spawned = thread::Builder::new()
                    .name(format!("ludusctrl-web-{connection}"))
                    .spawn(move || {
                        let _guard = guard;
                        serve(&ctx, stream, peer, connection);
                    });
                if spawned.is_err() {
                    tracing::warn!("ludusctrl: could not start a browser connection thread");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_SLEEP);
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// The TLS handshake on the connection's own thread, within its deadline.
fn handshake(ctx: &Context, mut tcp: TcpStream) -> Option<Stream> {
    let mut conn = ServerConnection::new(ctx.tls.clone()).ok()?;
    let deadline = Instant::now() + ctx.timers.handshake;
    while conn.is_handshaking() {
        if Instant::now() >= deadline {
            return None;
        }
        // A peer that closes in the middle is an UnexpectedEof error.
        match conn.complete_io(&mut tcp) {
            Ok(_) => {}
            Err(error) if http::is_timeout(&error) => {}
            Err(_) => return None,
        }
    }
    Some(StreamOwned::new(conn, tcp))
}

/// Ends a connection: TLS close_notify, our half of TCP shut, and what the peer still sends read
/// for a moment, so Windows does not answer it with a reset that eats the last answer.
fn hang_up(mut stream: Stream, linger: Duration) {
    stream.conn.send_close_notify();
    let _ = stream.flush();
    let _ = stream.sock.shutdown(Shutdown::Write);
    let until = Instant::now() + linger;
    let mut sink = [0u8; 512];
    while Instant::now() < until {
        match stream.sock.read(&mut sink) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) if http::is_timeout(&error) => {}
            Err(_) => break,
        }
    }
}

fn serve(ctx: &Context, tcp: TcpStream, peer: SocketAddr, connection: u64) {
    let timers = ctx.timers;
    let started = Instant::now();
    if tcp.set_nonblocking(false).is_err()
        || tcp.set_read_timeout(Some(timers.slice)).is_err()
        || tcp.set_write_timeout(Some(timers.frame)).is_err()
    {
        return;
    }
    let _ = tcp.set_nodelay(true);
    let Some(mut stream) = handshake(ctx, tcp) else {
        return;
    };
    let (request, leftover) = match http::read_head(&mut stream, started + timers.head) {
        Ok(parsed) => parsed,
        Err(http::HeadError::Gone) => return hang_up(stream, Duration::ZERO),
        Err(http::HeadError::Bad) => {
            let _ = http::error(&mut stream, "400 Bad Request", "");
            return hang_up(stream, timers.linger);
        }
    };
    let host = request.host.clone().unwrap_or_default();
    let answered = if request.method != "GET" {
        http::error(&mut stream, "404 Not Found", "")
    } else if request.path == "/ws" {
        if !request.websocket || request.key.is_none() {
            http::error(&mut stream, "400 Bad Request", "")
        } else if !http::origin_ok(request.origin.as_deref(), &host) {
            http::error(&mut stream, "403 Forbidden", "")
        } else {
            let key = request.key.clone().unwrap_or_default();
            if http::switching(&mut stream, &key).is_err() {
                return;
            }
            let device = http::device(request.user_agent.as_deref());
            let stream = session::run(ctx, stream, leftover, connection, peer, device);
            return hang_up(stream, Duration::ZERO);
        }
    } else if page::is_page(&request.path) {
        let extra = format!(
            "Content-Security-Policy: {}\r\n",
            page::csp(&ctx.csp, &host)
        );
        http::respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            &extra,
            page::INDEX,
        )
    } else {
        http::error(&mut stream, "404 Not Found", "")
    };
    if answered.is_ok() {
        hang_up(stream, timers.linger);
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod garbage;
