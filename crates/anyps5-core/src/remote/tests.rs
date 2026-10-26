//! End-to-end tests of the phone server against a real WebSocket client on loopback.

use std::{
    net::TcpStream,
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};

use tungstenite::{WebSocket, client::IntoClientRequest, stream::MaybeTlsStream};

use super::*;

#[derive(Debug, PartialEq)]
enum Event {
    Pairing(PairingRequest),
    PairingEnded(u64),
    Connecting(Option<String>),
    Connected(String),
    Disconnected,
    Latency(f64),
    Request(Message),
}

struct Recorder(Mutex<Sender<Event>>);

impl RemoteEvents for Recorder {
    fn pairing(&self, request: PairingRequest) {
        self.push(Event::Pairing(request));
    }
    fn pairing_ended(&self, id: u64) {
        self.push(Event::PairingEnded(id));
    }
    fn connecting(&self, name: Option<String>) {
        self.push(Event::Connecting(name));
    }
    fn connected(&self, name: String) {
        self.push(Event::Connected(name));
    }
    fn disconnected(&self) {
        self.push(Event::Disconnected);
    }
    fn latency(&self, milliseconds: f64) {
        self.push(Event::Latency(milliseconds));
    }
    fn request(&self, message: Message) {
        self.push(Event::Request(message));
    }
    fn pad_ready(&self, _: bool) {}
}

impl Recorder {
    fn push(&self, event: Event) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

struct Rig {
    server: RemoteServer,
    events: Receiver<Event>,
    _dir: tempfile::TempDir,
}

fn rig(allow_all: bool) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let (tx, events) = channel();
    let mut config = RemoteConfig::new("Test PC".into(), dir.path().join("RemoteDevices.json"));
    config.advertise = false;
    config.control_port = 0;
    config.allow_all = allow_all;
    config.bind = Some(std::net::Ipv4Addr::LOCALHOST.into());
    let server = RemoteServer::start(config, Arc::new(Recorder(Mutex::new(tx)))).unwrap();
    Rig {
        server,
        events,
        _dir: dir,
    }
}

impl Rig {
    fn next(&self) -> Event {
        self.events
            .recv_timeout(Duration::from_secs(3))
            .expect("an event")
    }

    fn quiet(&self) -> bool {
        self.events
            .recv_timeout(Duration::from_millis(400))
            .is_err()
    }

    fn phone(&self) -> Phone {
        Phone::connect(self.server.control_port())
    }
}

struct Phone {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
}

impl Phone {
    fn connect(port: u16) -> Phone {
        let (ws, _) = tungstenite::connect(format!("ws://127.0.0.1:{port}/")).unwrap();
        Phone::timed(ws)
    }

    fn timed(mut ws: WebSocket<MaybeTlsStream<TcpStream>>) -> Phone {
        if let MaybeTlsStream::Plain(stream) = ws.get_mut() {
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
        }
        Phone { ws }
    }

    fn send(&mut self, message: &Message) {
        self.ws
            .send(tungstenite::Message::text(message.to_json()))
            .unwrap();
    }

    fn hello(&mut self, id: &str, name: &str, key: Option<&str>) {
        self.send(&Message::Hello {
            protocol: 1,
            device_id: id.into(),
            device_name: name.into(),
            key: key.map(str::to_string),
        });
    }

    fn recv(&mut self) -> Option<Message> {
        loop {
            match self.ws.read() {
                Ok(tungstenite::Message::Text(text)) => return Message::parse(text.as_str()),
                Ok(tungstenite::Message::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }
}

fn welcome_key(message: Option<Message>) -> String {
    match message {
        Some(Message::Welcome { key, .. }) => key,
        other => panic!("expected welcome, got {other:?}"),
    }
}

fn pair(rig: &Rig, phone: &mut Phone, id: &str) -> String {
    phone.hello(id, "Pixel", None);
    let Some(Message::Pairing { code }) = phone.recv() else {
        panic!("expected a code");
    };
    assert_eq!(rig.next(), Event::Connecting(Some("Pixel".into())));
    let Event::Pairing(request) = rig.next() else {
        panic!("expected a pairing event");
    };
    assert_eq!(request.code, code);
    rig.server.respond(request.id, true);
    let key = welcome_key(phone.recv());
    assert_eq!(rig.next(), Event::PairingEnded(request.id));
    assert_eq!(rig.next(), Event::Connected("Pixel".into()));
    key
}

#[test]
fn an_unknown_phone_is_asked_and_then_welcomed_with_a_key_and_the_library() {
    let rig = rig(false);
    let library = Message::Library {
        games: vec![],
        selected: None,
        running: None,
        language: "en".into(),
    };
    rig.server.publish(library.clone());
    let mut phone = rig.phone();
    let key = pair(&rig, &mut phone, "dev1");
    assert_eq!(key.len(), 44);
    assert_eq!(phone.recv(), Some(library));
    assert_eq!(rig.server.paired().len(), 1);
    assert_eq!(rig.server.paired()[0].name, "Pixel");
}

#[test]
fn a_known_key_is_welcomed_without_a_prompt() {
    let rig = rig(false);
    let mut first = rig.phone();
    let key = pair(&rig, &mut first, "dev1");
    drop(first);
    assert_eq!(rig.next(), Event::Disconnected);

    let mut again = rig.phone();
    again.hello("dev1", "Pixel", Some(&key));
    assert_eq!(welcome_key(again.recv()), key);
    assert_eq!(rig.next(), Event::Connecting(Some("Pixel".into())));
    assert_eq!(rig.next(), Event::Connected("Pixel".into()));
}

#[test]
fn a_denied_phone_is_rejected_and_has_to_wait() {
    let rig = rig(false);
    let mut phone = rig.phone();
    phone.hello("dev1", "Pixel", None);
    assert!(matches!(phone.recv(), Some(Message::Pairing { .. })));
    let Event::Connecting(_) = rig.next() else {
        panic!()
    };
    let Event::Pairing(request) = rig.next() else {
        panic!()
    };
    rig.server.respond(request.id, false);
    assert_eq!(
        phone.recv(),
        Some(Message::Rejected {
            reason: "denied".into()
        })
    );
    assert_eq!(rig.next(), Event::PairingEnded(request.id));
    assert_eq!(rig.next(), Event::Connecting(None));
    assert!(rig.server.paired().is_empty());

    let mut retry = rig.phone();
    retry.hello("dev1", "Pixel", None);
    assert_eq!(
        retry.recv(),
        Some(Message::Rejected {
            reason: "denied".into()
        })
    );
}

#[test]
fn a_second_phone_is_busy_but_the_same_phone_takes_over_with_its_key() {
    let rig = rig(false);
    let mut first = rig.phone();
    let key = pair(&rig, &mut first, "dev1");

    let mut other = rig.phone();
    other.hello("dev2", "Other", None);
    assert_eq!(
        other.recv(),
        Some(Message::Rejected {
            reason: "busy".into()
        })
    );

    // The device id travels in clear: knowing it must not be enough to kick the phone.
    let mut intruder = rig.phone();
    intruder.hello("dev1", "Intruder", Some("wrong"));
    assert_eq!(
        intruder.recv(),
        Some(Message::Rejected {
            reason: "busy".into()
        })
    );
    first.send(&Message::Latency { milliseconds: 4.0 });
    assert_eq!(rig.next(), Event::Latency(4.0));

    let mut again = rig.phone();
    again.hello("dev1", "Pixel", Some(&key));
    assert_eq!(welcome_key(again.recv()), key);
}

#[test]
fn a_waiting_pairing_is_replaced_by_the_same_phone_trying_again() {
    let rig = rig(false);
    let mut first = rig.phone();
    first.hello("dev1", "Pixel", None);
    assert!(matches!(first.recv(), Some(Message::Pairing { .. })));
    let mut second = rig.phone();
    second.hello("dev1", "Pixel", None);
    assert!(matches!(second.recv(), Some(Message::Pairing { .. })));
    let mut stranger = rig.phone();
    stranger.hello("dev9", "Late", None);
    assert_eq!(
        stranger.recv(),
        Some(Message::Rejected {
            reason: "busy".into()
        })
    );
}

#[test]
fn another_protocol_is_rejected() {
    let rig = rig(false);
    let mut phone = rig.phone();
    phone.send(&Message::Hello {
        protocol: 2,
        device_id: "d".into(),
        device_name: "n".into(),
        key: None,
    });
    assert_eq!(
        phone.recv(),
        Some(Message::Rejected {
            reason: "protocol".into()
        })
    );
}

#[test]
fn requests_are_heard_only_from_the_welcomed_phone() {
    let rig = rig(true);
    let mut early = rig.phone();
    early.send(&Message::Launch { id: "g".into() });
    assert!(rig.quiet());

    let mut phone = rig.phone();
    phone.hello("dev1", "Pixel", None);
    let _ = (rig.next(), rig.next(), rig.next(), rig.next());
    for message in [
        Message::Select { id: "g".into() },
        Message::Home,
        Message::Command {
            name: "left".into(),
        },
    ] {
        phone.send(&message);
        assert_eq!(rig.next(), Event::Request(message));
    }
    // Messages that only the launcher sends are not requests.
    phone.send(&Message::Pairing { code: "1".into() });
    assert!(rig.quiet());
}

#[test]
fn the_end_of_the_connection_is_reported() {
    let rig = rig(true);
    let mut phone = rig.phone();
    phone.hello("dev1", "Pixel", None);
    let _ = (rig.next(), rig.next(), rig.next(), rig.next());
    drop(phone);
    assert_eq!(rig.next(), Event::Disconnected);
}

#[test]
fn a_pending_phone_that_leaves_clears_the_card() {
    let rig = rig(false);
    let mut phone = rig.phone();
    phone.hello("dev1", "Pixel", None);
    let _ = (rig.next(), rig.next());
    drop(phone);
    assert!(matches!(rig.next(), Event::PairingEnded(_)));
    assert_eq!(rig.next(), Event::Connecting(None));
}

#[test]
fn a_web_page_cannot_connect() {
    let rig = rig(true);
    let mut request = format!("ws://127.0.0.1:{}/", rig.server.control_port())
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Origin", "https://evil.example".parse().unwrap());
    assert!(tungstenite::connect(request).is_err());
    assert!(rig.quiet());
}

#[test]
fn an_oversized_message_ends_the_connection() {
    let rig = rig(true);
    let mut phone = rig.phone();
    let big = "x".repeat(600 * 1024);
    let _ = phone.ws.send(tungstenite::Message::text(format!(
        r#"{{"type":"saveLayout","id":"g","layout":"{big}"}}"#
    )));
    assert!(phone.recv().is_none());
}

#[test]
fn the_welcome_names_the_relay_and_input_reaches_the_sink() {
    use std::{net::UdpSocket, sync::atomic::AtomicU32};

    struct Last(AtomicU32);
    impl PadSink for Last {
        fn input(&self, packet: &InputPacket) {
            self.0
                .store(packet.buttons, std::sync::atomic::Ordering::SeqCst);
        }
        fn ended(&self) {
            self.0.store(0, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let rig = rig(true);
    let sink = Arc::new(Last(AtomicU32::new(0)));
    rig.server.set_pad_sink(Some(sink.clone()));
    let mut phone = rig.phone();
    phone.hello("dev1", "Pixel", None);
    let _ = phone.recv();
    let Some(Message::Welcome {
        session, udp_port, ..
    }) = phone.recv()
    else {
        panic!("welcome");
    };
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut packet = InputPacket::neutral(session.parse().unwrap());
    packet.buttons = button::CROSS;
    let buttons = || sink.0.load(std::sync::atomic::Ordering::SeqCst);
    // A phone streams input, so keep sending until it lands: a busy CI machine can take longer
    // than any fixed wait to read the first packet.
    let deadline = Instant::now() + Duration::from_secs(10);
    while buttons() != button::CROSS && Instant::now() < deadline {
        udp.send_to(&packet.encode(), ("127.0.0.1", udp_port))
            .unwrap();
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(buttons(), button::CROSS);
    drop(phone);
    let deadline = Instant::now() + Duration::from_secs(10);
    while buttons() != 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(buttons(), 0);
}

#[test]
fn the_bridge_environment_names_the_loopback_port() {
    let rig = rig(false);
    let env = rig.server.bridge_environment();
    assert!(env["ANYPS5_REMOTE_PAD"].starts_with("127.0.0.1:"));
}
