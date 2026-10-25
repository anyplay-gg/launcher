//! The real phone server against the real `fake-phone` binary, over loopback: the protocol round
//! trip, pairing, a key that is reused, phones that are turned away and a flood that is limited.

use std::{
    net::UdpSocket,
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};

use anyps5_core::remote::{
    InputPacket, Message, PadSink, PairingRequest, RemoteConfig, RemoteEvents, RemoteServer, button,
};

#[derive(Debug)]
enum Event {
    Pairing(PairingRequest),
    Connected(String),
    Request(Message),
}

struct Recorder(Mutex<Sender<Event>>);

impl Recorder {
    fn push(&self, event: Event) {
        let _ = self.0.lock().unwrap().send(event);
    }
}

impl RemoteEvents for Recorder {
    fn pairing(&self, request: PairingRequest) {
        self.push(Event::Pairing(request));
    }
    fn pairing_ended(&self, _: u64) {}
    fn connecting(&self, _: Option<String>) {}
    fn connected(&self, name: String) {
        self.push(Event::Connected(name));
    }
    fn disconnected(&self) {}
    fn latency(&self, _: f64) {}
    fn request(&self, message: Message) {
        self.push(Event::Request(message));
    }
    fn pad_ready(&self, _: bool) {}
}

struct Rig {
    server: RemoteServer,
    events: Receiver<Event>,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let (tx, events) = channel();
    let mut config = RemoteConfig::new("Test PC".into(), dir.path().join("RemoteDevices.json"));
    config.advertise = false;
    config.control_port = 0;
    let server = RemoteServer::start(config, Arc::new(Recorder(Mutex::new(tx)))).unwrap();
    server.publish(Message::Library {
        games: vec![],
        selected: None,
        running: None,
        language: "en".into(),
    });
    Rig {
        server,
        events,
        _dir: dir,
    }
}

/// What a run of the binary and the server's side of it came to.
struct Outcome {
    success: bool,
    output: String,
    pairings: usize,
    connected: Vec<String>,
    requests: Vec<Message>,
}

/// Runs `fake-phone phone` with a script and answers its pairing questions with `answer`
/// (`None`: nobody answers).
fn run(rig: &Rig, key_file: &Path, id: &str, script: &str, answer: Option<bool>) -> Outcome {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fake-phone"))
        .args([
            "phone",
            "--addr",
            &format!("127.0.0.1:{}", rig.server.control_port()),
            "--key-file",
            key_file.to_str().unwrap(),
            "--device-id",
            id,
            "--name",
            "Fake Phone",
            "--stream-hz",
            "0",
            "--script",
            script,
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the fake phone starts");
    let mut outcome = Outcome {
        success: false,
        output: String::new(),
        pairings: 0,
        connected: vec![],
        requests: vec![],
    };
    let until = Instant::now() + Duration::from_secs(20);
    let status = loop {
        while let Ok(event) = rig.events.try_recv() {
            match event {
                Event::Pairing(request) => {
                    outcome.pairings += 1;
                    if let Some(allow) = answer {
                        rig.server.respond(request.id, allow);
                    }
                }
                Event::Connected(name) => outcome.connected.push(name),
                Event::Request(message) => outcome.requests.push(message),
            }
        }
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() > until {
            let _ = child.kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // Whatever the server heard before the phone left.
    std::thread::sleep(Duration::from_millis(100));
    while let Ok(event) = rig.events.try_recv() {
        match event {
            Event::Pairing(_) => outcome.pairings += 1,
            Event::Connected(name) => outcome.connected.push(name),
            Event::Request(message) => outcome.requests.push(message),
        }
    }
    let output = child.wait_with_output().unwrap();
    outcome.output = String::from_utf8_lossy(&output.stdout).into_owned();
    outcome.success = status.is_some_and(|s| s.success());
    outcome
}

#[test]
fn a_phone_is_paired_and_the_protocol_round_trips() {
    let rig = rig();
    let keys = tempfile::tempdir().unwrap();
    let key_file = keys.path().join("key.txt");
    let outcome = run(
        &rig,
        &key_file,
        "phone-one",
        "wait200,expect:library,select:g1,launch:g1,home,close,command:select",
        Some(true),
    );
    assert!(outcome.success, "{}", outcome.output);
    assert_eq!(outcome.pairings, 1);
    assert_eq!(outcome.connected, ["Fake Phone"]);
    assert_eq!(
        outcome.requests,
        [
            Message::Select { id: "g1".into() },
            Message::Launch { id: "g1".into() },
            Message::Home,
            Message::Close,
            Message::Command {
                name: "select".into()
            },
        ]
    );
    let paired = rig.server.paired();
    assert_eq!(paired.len(), 1);
    assert_eq!(paired[0].id, "phone-one");
    assert!(
        std::fs::read_to_string(&key_file).unwrap().trim() == paired[0].key,
        "the phone got the key the server stored"
    );
}

#[test]
fn the_stored_key_is_welcomed_without_a_second_question() {
    let rig = rig();
    let keys = tempfile::tempdir().unwrap();
    let key_file = keys.path().join("key.txt");
    let first = run(&rig, &key_file, "phone-one", "wait100", Some(true));
    assert!(first.success, "{}", first.output);
    assert_eq!(first.pairings, 1);
    let second = run(&rig, &key_file, "phone-one", "wait100,expect:library", None);
    assert!(second.success, "{}", second.output);
    assert_eq!(second.pairings, 0, "no question for a known phone");
    assert_eq!(second.connected, ["Fake Phone"]);
}

#[test]
fn a_denied_phone_is_rejected_and_stays_unpaired() {
    let rig = rig();
    let keys = tempfile::tempdir().unwrap();
    let outcome = run(
        &rig,
        &keys.path().join("key.txt"),
        "stranger",
        "expect:rejected",
        Some(false),
    );
    assert!(outcome.success, "{}", outcome.output);
    assert!(outcome.output.contains("OK rejected"));
    assert!(outcome.connected.is_empty());
    assert!(rig.server.paired().is_empty());
}

#[test]
fn a_phone_with_a_wrong_key_is_asked_about_like_a_stranger() {
    let rig = rig();
    let keys = tempfile::tempdir().unwrap();
    let key_file = keys.path().join("key.txt");
    let first = run(&rig, &key_file, "phone-one", "wait100", Some(true));
    assert!(first.success, "{}", first.output);
    // Another phone claims the same device id with a key it made up.
    std::fs::write(&key_file, "bm90IHRoZSBrZXk=").unwrap();
    let outcome = run(&rig, &key_file, "phone-one", "expect:rejected", Some(false));
    assert!(outcome.success, "{}", outcome.output);
    assert_eq!(
        outcome.pairings, 1,
        "the wrong key earned a question, not a welcome"
    );
    assert!(outcome.connected.is_empty());
}

#[test]
fn nobody_answering_means_nothing_gets_through() {
    let rig = rig();
    let keys = tempfile::tempdir().unwrap();
    // The fake phone waits for a welcome that never comes; the server hears nothing of it.
    let mut child = Command::new(env!("CARGO_BIN_EXE_fake-phone"))
        .args([
            "phone",
            "--addr",
            &format!("127.0.0.1:{}", rig.server.control_port()),
            "--key-file",
            keys.path().join("k").to_str().unwrap(),
            "--script",
            "launch:g1,close",
            "--stream-hz",
            "0",
        ])
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(800));
    let mut requests = 0;
    let mut pairings = 0;
    while let Ok(event) = rig.events.try_recv() {
        match event {
            Event::Request(_) => requests += 1,
            Event::Pairing(_) => pairings += 1,
            Event::Connected(_) => panic!("welcomed without a question"),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(pairings, 1);
    assert_eq!(requests, 0);
}

#[test]
fn a_flood_of_requests_is_limited() {
    let rig = rig();
    let keys = tempfile::tempdir().unwrap();
    let outcome = run(
        &rig,
        &keys.path().join("key.txt"),
        "phone-one",
        "wait100,flood:300,wait300",
        Some(true),
    );
    assert!(outcome.success, "{}", outcome.output);
    let heard = outcome.requests.len();
    // The burst is 80 and 40 a second are added: a quick 300 gets far less than all through.
    assert!(heard >= 40, "the first of them are heard: {heard}");
    assert!(heard < 200, "the flood was limited: {heard} of 300");
}

#[test]
fn pad_packets_from_a_phone_without_a_session_reach_nothing() {
    use std::sync::atomic::{AtomicU32, Ordering};
    struct Count(AtomicU32);
    impl PadSink for Count {
        fn input(&self, _: &InputPacket) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn ended(&self) {}
    }
    let rig = rig();
    let sink = Arc::new(Count(AtomicU32::new(0)));
    rig.server.set_pad_sink(Some(sink.clone()));
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut packet = InputPacket::neutral(0x1234_5678);
    packet.buttons = button::CROSS;
    let target = ("127.0.0.1", rig.server.info().udp_port);
    for _ in 0..20 {
        socket.send_to(&packet.encode(), target).unwrap();
    }
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(sink.0.load(Ordering::SeqCst), 0);
}
