//! A phone over loopback UDP against the real server: beacon, pairing with the code confirmed on
//! the pairing question, signed input into a recording pad, the acks and the watchdog.

use std::{
    net::UdpSocket,
    sync::{
        Mutex,
        mpsc::{Receiver, Sender, channel},
    },
};

use super::{
    crypto::{
        Ephemeral, LinkKey, commitment, confirmation_code, format_code, kid, link_key, mac_ok,
        parse_point,
    },
    wire::{
        FLAG_CONTROLLER, Input, PairRequest, PairState, Phase, ack_flag, ack_signed,
        pair_response_signed, read_ack, read_beacon, read_pair_response,
    },
    *,
};
use crate::remote::security::random_bytes;

#[derive(Debug)]
enum Event {
    Pairing(PairingRequest),
    Ended(u64),
    Status(LudusStatus),
}

struct Recorder(Mutex<Sender<Event>>);

impl LudusEvents for Recorder {
    fn pairing(&self, request: PairingRequest) {
        let _ = self.0.lock().unwrap().send(Event::Pairing(request));
    }
    fn pairing_ended(&self, id: u64) {
        let _ = self.0.lock().unwrap().send(Event::Ended(id));
    }
    fn status(&self, status: LudusStatus) {
        let _ = self.0.lock().unwrap().send(Event::Status(status));
    }
}

/// The virtual pad, without a driver.
#[derive(Default)]
struct Pad(Mutex<Vec<PadState>>);

impl PadTarget for Pad {
    fn write(&self, state: &PadState) -> bool {
        self.0.lock().unwrap().push(*state);
        true
    }
    fn release(&self) {}
}

struct Rig {
    server: LudusServer,
    events: Receiver<Event>,
    beacons: UdpSocket,
    pad: Arc<Pad>,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let beacons = UdpSocket::bind("127.0.0.1:0").unwrap();
    let SocketAddr::V4(to) = beacons.local_addr().unwrap() else {
        panic!("an IPv4 socket");
    };
    let mut config = LudusConfig::new(dir.path().join("LudusCtrl.json"));
    config.port = 0;
    config.beacon = Some(BeaconTarget {
        to,
        from: Some(vec![Ipv4Addr::LOCALHOST]),
    });
    let (tx, events) = channel();
    let server = LudusServer::start(config, Arc::new(Recorder(Mutex::new(tx)))).unwrap();
    let pad = Arc::new(Pad::default());
    server.set_pad(Some(pad.clone()));
    Rig {
        server,
        events,
        beacons,
        pad,
        _dir: dir,
    }
}

impl Rig {
    fn next(&self) -> Event {
        self.events
            .recv_timeout(Duration::from_secs(3))
            .expect("an event")
    }

    fn beacon(&self, wait: Duration) -> Option<Vec<u8>> {
        self.beacons.set_read_timeout(Some(wait)).unwrap();
        let mut buffer = [0u8; 64];
        self.beacons
            .recv_from(&mut buffer)
            .ok()
            .map(|(n, _)| buffer[..n].to_vec())
    }

    fn drain_beacons(&self) {
        while self.beacon(Duration::from_millis(20)).is_some() {}
    }
}

/// The phone side, written from the protocol document.
struct Phone {
    socket: UdpSocket,
    pc: SocketAddr,
    id: [u8; 8],
    ephemeral: Ephemeral,
    nonce: [u8; 16],
    key: Option<LinkKey>,
    seq: u32,
}

impl Phone {
    fn new(port: u16) -> Phone {
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(800)))
            .unwrap();
        Phone {
            socket,
            pc: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            id: random_bytes::<8>(),
            ephemeral: Ephemeral::generate(),
            nonce: random_bytes::<16>(),
            key: None,
            seq: 0,
        }
    }

    fn recv(&self) -> Option<Vec<u8>> {
        let mut buffer = [0u8; 128];
        self.socket
            .recv_from(&mut buffer)
            .ok()
            .map(|(n, _)| buffer[..n].to_vec())
    }

    fn pair_request(&self, phase: Phase) -> Option<[u8; 84]> {
        let request = PairRequest {
            phone_id: self.id,
            phone_pub: self.ephemeral.public,
            name: "Test\u{202E} Phone".as_bytes().to_vec(),
            phase,
            field: match phase {
                Phase::Commit => commitment(&self.nonce),
                Phase::Reveal => self.nonce,
            },
        };
        self.socket.send_to(&request.encode(), self.pc).unwrap();
        self.recv().map(|bytes| bytes.try_into().expect("84 bytes"))
    }

    fn send(&mut self, session: u32, buttons: u16) -> Option<Vec<u8>> {
        let key = self.key.expect("paired");
        self.seq = self.seq.wrapping_add(1);
        let packet = Input {
            seq: self.seq,
            buttons,
            lx: 1234,
            ly: -32768,
            left_trigger: 77,
            flags: FLAG_CONTROLLER,
            layout: PadLayout::Xbox,
            session,
            ..Input::default()
        }
        .encode(&kid(&key), &key);
        self.socket.send_to(&packet, self.pc).unwrap();
        self.recv()
    }
}

/// The rate limit allows one PAIR_REQ a second per address, like the phone's own resend.
fn resend_pause() {
    thread::sleep(Duration::from_millis(1050));
}

#[test]
fn beacon_pair_confirm_and_play() {
    let rig = rig();
    let port = rig.server.port();

    // The beacon names the input port and this PC's id.
    let beacon = rig.beacon(Duration::from_secs(3)).expect("a beacon");
    let (advertised, pc_id) = read_beacon(&beacon).unwrap();
    assert_eq!(advertised, port);
    let pc_id = pc_id.expect("a 16-byte beacon");

    // Commit: the PC answers with its key; the phone derives the key and the code.
    let mut phone = Phone::new(port);
    let waiting = phone.pair_request(Phase::Commit).expect("an answer");
    let (id, pc_pub, state, _) = read_pair_response(&waiting).unwrap();
    assert_eq!((id, state), (pc_id, PairState::Waiting));
    let z = phone.ephemeral.shared(&parse_point(&pc_pub).unwrap());
    let key = link_key(&z, &phone.ephemeral.public, &pc_pub);
    let code = format_code(confirmation_code(
        &z,
        &phone.ephemeral.public,
        &pc_pub,
        &phone.nonce,
    ));

    // Reveal: the question comes up on the PC with the same code.
    resend_pause();
    let still = phone.pair_request(Phase::Reveal).expect("an answer");
    assert_eq!(read_pair_response(&still).unwrap().2, PairState::Waiting);
    let Event::Pairing(question) = rig.next() else {
        panic!("expected the pairing question");
    };
    assert_eq!(question.code, code);
    assert_eq!(question.device_name, "Test Phone", "no bidi override");

    // Allow, and the phone's next resend gets state 1 under the key.
    rig.server.respond(question.id, true);
    assert!(matches!(rig.next(), Event::Ended(id) if id == question.id));
    resend_pause();
    let accepted = phone.pair_request(Phase::Reveal).expect("an answer");
    let (_, _, state, tag) = read_pair_response(&accepted).unwrap();
    assert_eq!(state, PairState::Accepted);
    assert!(mac_ok(&key, pair_response_signed(&accepted), &tag));
    phone.key = Some(key);
    let paired = rig.server.paired();
    assert_eq!(paired.len(), 1);
    assert_eq!(paired[0].id, store::to_hex(&phone.id));
    assert_eq!(paired[0].name, "Test Phone");

    // Input: signed, applied to the pad, acked with pad live.
    let ack = phone.send(1, 0x1000 | 0x0400).expect("an ack");
    let (seq, received, flags, tag) = read_ack(&ack).unwrap();
    assert_eq!((seq, received), (phone.seq, 1));
    assert_eq!(flags, ack_flag::PAD_LIVE);
    assert!(mac_ok(&key, ack_signed(&ack), &tag.unwrap()));
    assert_eq!(
        rig.pad.0.lock().unwrap().last().copied(),
        Some(PadState {
            buttons: 0x1400,
            lx: 1234,
            ly: -32768,
            left_trigger: 77,
            ..PadState::default()
        })
    );
    let Event::Status(LudusStatus::Linked {
        via,
        layout,
        controller,
        pad_live,
    }) = rig.next()
    else {
        panic!("expected the link");
    };
    assert_eq!(
        via,
        LudusVia::App {
            name: "Test Phone".into()
        }
    );
    assert_eq!(layout, PadLayout::Xbox);
    assert!(controller && pad_live);

    // No beacon while linked: the phone keeps the link alive for longer than a beacon interval.
    rig.drain_beacons();
    for _ in 0..7 {
        thread::sleep(Duration::from_millis(350));
        phone.send(1, 0);
    }
    assert!(rig.beacon(Duration::from_millis(10)).is_none());

    // Silence: the watchdog releases the pad, the link drops and the beacon is back at once.
    let Event::Status(status) = rig.next() else {
        panic!("expected the link to drop");
    };
    assert_eq!(status, LudusStatus::Waiting);
    assert_eq!(rig.pad.0.lock().unwrap().last(), Some(&PadState::default()));
    assert!(rig.beacon(Duration::from_millis(600)).is_some());

    // A recorded packet is not played back; a new session is welcome.
    phone.seq = 0;
    assert!(
        phone.send(1, 0x2000).is_none(),
        "an old seq of this session"
    );
    assert!(phone.send(2, 0x2000).is_some());
    assert_eq!(rig.pad.0.lock().unwrap().last().unwrap().buttons, 0x2000);

    // Removed in the settings: the pad goes to neutral and the next packet hears "not paired".
    rig.server.forget(&store::to_hex(&phone.id));
    assert!(rig.server.paired().is_empty());
    let ack = phone.send(3, 0).expect("an unsigned ack");
    assert_eq!(read_ack(&ack).unwrap().2, ack_flag::NOT_PAIRED);
    assert_eq!(ack.len(), 12);
}

#[test]
fn a_denied_phone_hears_state_two_and_unpaired_input_is_told_so() {
    let rig = rig();
    let mut phone = Phone::new(rig.server.port());
    let (_, first_pub, ..) =
        read_pair_response(&phone.pair_request(Phase::Commit).unwrap()).unwrap();
    resend_pause();
    phone.pair_request(Phase::Reveal).unwrap();
    let Event::Pairing(question) = rig.next() else {
        panic!("expected the pairing question");
    };
    rig.server.respond(question.id, false);
    resend_pause();
    let denied = phone.pair_request(Phase::Reveal).unwrap();
    let (_, pc_pub, state, tag) = read_pair_response(&denied).unwrap();
    assert_eq!((state, tag), (PairState::Denied, [0; 8]));
    assert_eq!(pc_pub, first_pub);
    assert!(rig.server.paired().is_empty());

    // An unpaired phone's input hears "not paired"; a v1 packet hears "version mismatch".
    phone.key = Some([3; 32]);
    let ack = phone.send(1, 0).unwrap();
    assert_eq!(read_ack(&ack).unwrap().2, ack_flag::NOT_PAIRED);
    let mut v1 = [0u8; 20];
    v1[0] = 0xD5;
    v1[1] = 1;
    phone.socket.send_to(&v1, phone.pc).unwrap();
    let ack = phone.recv().unwrap();
    assert_eq!(read_ack(&ack).unwrap().2, ack_flag::VERSION_MISMATCH);
}

#[test]
fn only_the_home_network_may_send() {
    let on_link: Vec<Subnet> = vec![("192.168.1.20".parse().unwrap(), 24)];
    let ok = |ip: &str| allowed_source(ip.parse().unwrap(), &on_link);
    assert!(ok("192.168.1.77") && ok("::ffff:192.168.1.77"));
    assert!(ok("127.0.0.1") && ok("::1"));
    assert!(!ok("192.168.2.77"), "routed in from another subnet");
    assert!(!ok("8.8.8.8"));
    let tailscale: Vec<Subnet> = vec![("100.101.1.2".parse().unwrap(), 10)];
    assert!(
        !allowed_source("100.101.1.3".parse().unwrap(), &tailscale),
        "carrier-grade NAT is left out on purpose"
    );
}

#[test]
fn stop_ends_an_open_question() {
    let rig = rig();
    let phone = Phone::new(rig.server.port());
    phone.pair_request(Phase::Commit).unwrap();
    resend_pause();
    phone.pair_request(Phase::Reveal).unwrap();
    let Event::Pairing(question) = rig.next() else {
        panic!("expected the pairing question");
    };
    rig.server.stop();
    assert!(matches!(rig.next(), Event::Ended(id) if id == question.id));
    // The answer for a question that has closed approves nothing.
    rig.server.respond(question.id, true);
    assert!(rig.server.paired().is_empty());
}
