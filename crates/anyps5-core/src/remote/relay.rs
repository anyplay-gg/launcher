//! The pad channel: a LAN-facing UDP socket for the phone and a loopback "bridge" socket for the
//! game. A dumb forwarder with identity checks (the macOS launcher's `RemoteRelay`), plus a
//! [`PadSink`] seam so the same packets can also feed an OS virtual controller.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use socket2::{Domain, Protocol, Socket, Type};

use super::{
    protocol::{INPUT_SIZE, InputPacket, OUTPUT_SIZE, OutputPacket, input_session, is_game_hello},
    security::{RateLimit, is_allowed_peer},
};

/// Where the phone's pad state also goes. Called on the relay thread: keep it quick.
pub trait PadSink: Send + Sync {
    fn input(&self, packet: &InputPacket);
    /// The phone left or the session ended: release everything.
    fn ended(&self);
}

type ReadyCallback = Arc<dyn Fn(bool) + Send + Sync>;

const PHONE_PACKETS_PER_SECOND: f64 = 2000.0;
const PHONE_PACKET_BURST: f64 = 500.0;

/// A game that stops greeting for this long counts as gone.
const GAME_SILENCE: Duration = Duration::from_secs(3);

#[derive(Default)]
struct State {
    session: u64,
    phone_ip: Option<IpAddr>,
    phone: Option<SocketAddr>,
    game: Option<SocketAddr>,
    last_hello: Option<Instant>,
    ready: bool,
}

struct Shared {
    phone_socket: UdpSocket,
    bridge_socket: UdpSocket,
    state: Mutex<State>,
    sink: Mutex<Option<Arc<dyn PadSink>>>,
    stop: AtomicBool,
    on_ready: ReadyCallback,
}

pub struct Relay {
    shared: Arc<Shared>,
    phone_port: u16,
    bridge_port: u16,
}

/// The LAN-facing UDP socket: every interface (dual-stack where IPv6 exists), or only `bind` when
/// given (tests bind loopback, so no firewall prompt appears).
pub(super) fn bind_phone_socket(bind: Option<IpAddr>, preferred: u16) -> io::Result<UdpSocket> {
    if let Some(ip) = bind {
        return UdpSocket::bind((ip, preferred)).or_else(|_| UdpSocket::bind((ip, 0)));
    }
    for port in [preferred, 0] {
        if let Ok(socket) = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP))
            && socket.set_only_v6(false).is_ok()
            && socket
                .bind(&SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)).into())
                .is_ok()
        {
            return Ok(socket.into());
        }
        // No IPv6 on this machine: plain IPv4.
        if let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)) {
            return Ok(socket);
        }
    }
    Err(io::Error::other("no UDP port for the phone"))
}

fn bind_bridge_socket(preferred: u16) -> io::Result<UdpSocket> {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, preferred))
        .or_else(|_| UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)))
}

impl Relay {
    /// `phone_port` and `bridge_port` are preferences; a busy port falls back to a random one.
    /// `bind` limits the phone socket to one address (`None`: every interface).
    pub fn start(
        bind: Option<IpAddr>,
        phone_port: u16,
        bridge_port: u16,
        on_ready: ReadyCallback,
    ) -> io::Result<Relay> {
        let phone_socket = bind_phone_socket(bind, phone_port)?;
        let bridge_socket = bind_bridge_socket(bridge_port)?;
        for socket in [&phone_socket, &bridge_socket] {
            socket.set_read_timeout(Some(Duration::from_millis(100)))?;
        }
        let phone_port = phone_socket.local_addr()?.port();
        let bridge_port = bridge_socket.local_addr()?.port();
        let shared = Arc::new(Shared {
            phone_socket,
            bridge_socket,
            state: Mutex::new(State::default()),
            sink: Mutex::new(None),
            stop: AtomicBool::new(false),
            on_ready,
        });
        let phone_side = shared.clone();
        thread::Builder::new()
            .name("remote-relay-phone".into())
            .spawn(move || phone_side.pump_phone())?;
        let game_side = shared.clone();
        thread::Builder::new()
            .name("remote-relay-game".into())
            .spawn(move || game_side.pump_game())?;
        Ok(Relay {
            shared,
            phone_port,
            bridge_port,
        })
    }

    pub fn phone_port(&self) -> u16 {
        self.phone_port
    }

    pub fn bridge_port(&self) -> u16 {
        self.bridge_port
    }

    /// A phone was welcomed: its input is accepted from now on, from that address only.
    pub fn begin(&self, session: u64, phone_ip: IpAddr) {
        let mut state = self.shared.state.lock().expect("relay state");
        state.session = session;
        state.phone_ip = Some(phone_ip.to_canonical());
        state.phone = None;
    }

    /// The control connection ended: the session stops matching.
    pub fn end(&self) {
        {
            let mut state = self.shared.state.lock().expect("relay state");
            state.session = 0;
            state.phone_ip = None;
            state.phone = None;
        }
        if let Some(sink) = self.shared.sink.lock().expect("sink").clone() {
            sink.ended();
        }
    }

    pub fn set_sink(&self, sink: Option<Arc<dyn PadSink>>) {
        *self.shared.sink.lock().expect("sink") = sink;
    }

    #[cfg(test)]
    pub fn game_is_ready(&self) -> bool {
        self.shared.state.lock().expect("relay state").ready
    }

    /// Sends a feedback packet to the phone's last known address (used by the virtual controller).
    pub fn send_to_phone(&self, packet: &OutputPacket) {
        let target = self.shared.state.lock().expect("relay state").phone;
        if let Some(target) = target {
            let _ = self.shared.phone_socket.send_to(&packet.encode(), target);
        }
    }

    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(super) fn recoverable(error: &io::Error) -> bool {
    // On Windows a datagram sent to a closed port (a game that exited) makes the next receive on
    // the same socket fail with WSAECONNRESET; that says nothing about the socket.
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock
            | io::ErrorKind::TimedOut
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::Interrupted
            | io::ErrorKind::ConnectionRefused
    ) || error.raw_os_error() == Some(10054)
}

impl Shared {
    fn pump_phone(&self) {
        let mut buffer = [0u8; 512];
        // A phone sends 120 to 250 packets a second; ten times that is a flood.
        let mut limit = RateLimit::new(PHONE_PACKETS_PER_SECOND, PHONE_PACKET_BURST);
        while !self.stop.load(Ordering::Relaxed) {
            let (size, source) = match self.phone_socket.recv_from(&mut buffer) {
                Ok(received) => received,
                Err(error) if recoverable(&error) => continue,
                Err(_) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
            };
            let datagram = &buffer[..size.min(buffer.len())];
            if size != INPUT_SIZE || !is_allowed_peer(source.ip()) || !limit.allow(Instant::now()) {
                continue;
            }
            let Some(session) = input_session(datagram) else {
                continue;
            };
            let game = {
                let mut state = self.state.lock().expect("relay state");
                if state.session == 0
                    || session != state.session
                    || state.phone_ip != Some(source.ip().to_canonical())
                {
                    continue;
                }
                state.phone = Some(source);
                state.game
            };
            if let Some(game) = game {
                let _ = self.bridge_socket.send_to(datagram, game);
            }
            if let Some(sink) = self.sink.lock().expect("sink").clone()
                && let Some(packet) = InputPacket::parse(datagram)
            {
                sink.input(&packet);
            }
        }
    }

    fn pump_game(&self) {
        let mut buffer = [0u8; 512];
        while !self.stop.load(Ordering::Relaxed) {
            match self.bridge_socket.recv_from(&mut buffer) {
                Ok((size, source)) => {
                    let datagram = &buffer[..size.min(buffer.len())];
                    if is_game_hello(datagram) {
                        let announce = {
                            let mut state = self.state.lock().expect("relay state");
                            state.game = Some(source);
                            state.last_hello = Some(Instant::now());
                            !std::mem::replace(&mut state.ready, true)
                        };
                        if announce {
                            (self.on_ready)(true);
                        }
                    } else if size == OUTPUT_SIZE && OutputPacket::parse(datagram).is_some() {
                        let target = {
                            let state = self.state.lock().expect("relay state");
                            (state.game == Some(source))
                                .then_some(state.phone)
                                .flatten()
                        };
                        if let Some(target) = target {
                            let _ = self.phone_socket.send_to(datagram, target);
                        }
                    }
                }
                Err(error) if recoverable(&error) => {}
                Err(_) => thread::sleep(Duration::from_millis(10)),
            }
            let gone = {
                let mut state = self.state.lock().expect("relay state");
                let silent = state
                    .last_hello
                    .is_none_or(|at| at.elapsed() > GAME_SILENCE);
                let gone = state.ready && silent;
                if gone {
                    state.ready = false;
                    state.game = None;
                }
                gone
            };
            if gone {
                (self.on_ready)(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::protocol::{button, game_hello};
    use std::sync::atomic::AtomicUsize;

    struct Count(AtomicUsize, AtomicUsize);
    impl PadSink for Count {
        fn input(&self, _: &InputPacket) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn ended(&self) {
            self.1.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn relay() -> Relay {
        Relay::start(Some(Ipv4Addr::LOCALHOST.into()), 0, 0, Arc::new(|_| {})).unwrap()
    }

    fn recv(socket: &UdpSocket) -> Option<Vec<u8>> {
        let mut buffer = [0u8; 512];
        socket
            .set_read_timeout(Some(Duration::from_millis(600)))
            .ok()?;
        socket
            .recv_from(&mut buffer)
            .ok()
            .map(|(n, _)| buffer[..n].to_vec())
    }

    fn pad(session: u64, buttons: u32) -> Vec<u8> {
        let mut packet = InputPacket::neutral(session);
        packet.buttons = buttons;
        packet.encode().to_vec()
    }

    #[test]
    fn input_reaches_the_game_and_output_reaches_the_phone() {
        let relay = relay();
        let phone = UdpSocket::bind("127.0.0.1:0").unwrap();
        let game = UdpSocket::bind("127.0.0.1:0").unwrap();
        let to_phone = ("127.0.0.1", relay.phone_port());
        let to_bridge = ("127.0.0.1", relay.bridge_port());
        relay.begin(42, "127.0.0.1".parse().unwrap());
        game.send_to(&game_hello(), to_bridge).unwrap();
        thread::sleep(Duration::from_millis(150));
        assert!(relay.game_is_ready());

        phone.send_to(&pad(42, button::CROSS), to_phone).unwrap();
        let seen = recv(&game).expect("input forwarded");
        assert_eq!(seen, pad(42, button::CROSS));

        let output = OutputPacket {
            large_motor: 200,
            ..OutputPacket::default()
        };
        game.send_to(&output.encode(), to_bridge).unwrap();
        assert_eq!(recv(&phone).expect("output forwarded"), output.encode());
    }

    #[test]
    fn foreign_sessions_oversize_and_ended_sessions_are_dropped() {
        let relay = relay();
        let phone = UdpSocket::bind("127.0.0.1:0").unwrap();
        let game = UdpSocket::bind("127.0.0.1:0").unwrap();
        let to_phone = ("127.0.0.1", relay.phone_port());
        relay.begin(42, "127.0.0.1".parse().unwrap());
        game.send_to(&game_hello(), ("127.0.0.1", relay.bridge_port()))
            .unwrap();
        thread::sleep(Duration::from_millis(100));

        phone.send_to(&pad(43, 0), to_phone).unwrap();
        phone.send_to(&[0u8; 600], to_phone).unwrap();
        assert!(recv(&game).is_none());

        relay.end();
        phone.send_to(&pad(42, 0), to_phone).unwrap();
        assert!(recv(&game).is_none());
    }

    #[test]
    fn a_stranger_cannot_inject_output_or_hijack_the_phone_address() {
        let relay = relay();
        let phone = UdpSocket::bind("127.0.0.1:0").unwrap();
        let game = UdpSocket::bind("127.0.0.1:0").unwrap();
        let stranger = UdpSocket::bind("127.0.0.1:0").unwrap();
        let to_bridge = ("127.0.0.1", relay.bridge_port());
        relay.begin(7, "127.0.0.1".parse().unwrap());
        game.send_to(&game_hello(), to_bridge).unwrap();
        // The bridge thread has to have read the greeting before the pad arrives.
        thread::sleep(Duration::from_millis(150));
        phone
            .send_to(&pad(7, 0), ("127.0.0.1", relay.phone_port()))
            .unwrap();
        assert!(recv(&game).is_some());
        stranger
            .send_to(&OutputPacket::default().encode(), to_bridge)
            .unwrap();
        assert!(recv(&phone).is_none());
    }

    #[test]
    fn a_new_game_hello_redirects_the_bridge() {
        let relay = relay();
        let phone = UdpSocket::bind("127.0.0.1:0").unwrap();
        let first = UdpSocket::bind("127.0.0.1:0").unwrap();
        let second = UdpSocket::bind("127.0.0.1:0").unwrap();
        let to_bridge = ("127.0.0.1", relay.bridge_port());
        relay.begin(9, "127.0.0.1".parse().unwrap());
        first.send_to(&game_hello(), to_bridge).unwrap();
        second.send_to(&game_hello(), to_bridge).unwrap();
        thread::sleep(Duration::from_millis(100));
        phone
            .send_to(&pad(9, 0), ("127.0.0.1", relay.phone_port()))
            .unwrap();
        assert!(recv(&second).is_some());
        assert!(recv(&first).is_none());
    }

    #[test]
    fn the_sink_sees_input_and_the_end() {
        let relay = relay();
        let sink = Arc::new(Count(AtomicUsize::new(0), AtomicUsize::new(0)));
        relay.set_sink(Some(sink.clone()));
        relay.begin(5, "127.0.0.1".parse().unwrap());
        let phone = UdpSocket::bind("127.0.0.1:0").unwrap();
        phone
            .send_to(&pad(5, 0), ("127.0.0.1", relay.phone_port()))
            .unwrap();
        thread::sleep(Duration::from_millis(200));
        assert_eq!(sink.0.load(Ordering::SeqCst), 1);
        relay.end();
        assert_eq!(sink.1.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn the_relay_survives_a_reset_from_a_closed_game_port() {
        let relay = relay();
        let phone = UdpSocket::bind("127.0.0.1:0").unwrap();
        relay.begin(3, "127.0.0.1".parse().unwrap());
        // A game that said hello and then exited: the bridge sends into a closed port.
        let gone = UdpSocket::bind("127.0.0.1:0").unwrap();
        gone.send_to(&game_hello(), ("127.0.0.1", relay.bridge_port()))
            .unwrap();
        thread::sleep(Duration::from_millis(100));
        drop(gone);
        let to_phone = ("127.0.0.1", relay.phone_port());
        for _ in 0..3 {
            phone.send_to(&pad(3, 0), to_phone).unwrap();
            thread::sleep(Duration::from_millis(50));
        }
        let game = UdpSocket::bind("127.0.0.1:0").unwrap();
        game.send_to(&game_hello(), ("127.0.0.1", relay.bridge_port()))
            .unwrap();
        thread::sleep(Duration::from_millis(100));
        phone.send_to(&pad(3, button::CIRCLE), to_phone).unwrap();
        assert_eq!(recv(&game).expect("still relaying"), pad(3, button::CIRCLE));
    }
}
