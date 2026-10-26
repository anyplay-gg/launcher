//! The browser client against a TLS client over loopback: the page and its CSP, the handshake,
//! every close code and limit, the token, the certificate and the pad slot shared with the app.
//! Written from LudusCtrl's PROTOCOL.md, section "Browser client".

use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::{Mutex, mpsc},
};

use rustls::{
    ClientConfig, ClientConnection, RootCertStore,
    client::{WebPkiServerVerifier, danger::ServerCertVerifier},
    pki_types::{ServerName, UnixTime},
};
use sha2::Digest;

use super::{
    session::{FRAME_MAX, Frame, Violation, decode_body, encode_frame, opcode, parse_frame},
    tls::{Identity, MAX_ADDRESSES, load_or_mint, mint, unix_now},
    *,
};
use crate::remote::{
    ludusctrl::{
        BeaconTarget, LudusConfig, LudusEvents, LudusServer, LudusStatus, LudusVia, PadLayout,
        PadTarget,
    },
    vpad::PadState,
};

// ---- the rig ----

#[derive(Default)]
struct Pad {
    writes: Mutex<Vec<PadState>>,
}

impl PadTarget for Pad {
    fn write(&self, state: &PadState) -> bool {
        self.writes.lock().unwrap().push(*state);
        true
    }
    fn release(&self) {}
}

impl Pad {
    fn last(&self) -> Option<PadState> {
        self.writes.lock().unwrap().last().copied()
    }
    fn count(&self) -> usize {
        self.writes.lock().unwrap().len()
    }
}

struct Statuses(Mutex<mpsc::Sender<LudusStatus>>);

impl LudusEvents for Statuses {
    fn pairing(&self, _: crate::remote::PairingRequest) {}
    fn pairing_ended(&self, _: u64) {}
    fn status(&self, status: LudusStatus) {
        let _ = self.0.lock().unwrap().send(status);
    }
}

struct Rig {
    server: LudusServer,
    pad: Arc<Pad>,
    statuses: mpsc::Receiver<LudusStatus>,
    info: BrowserInfo,
    cert: Vec<u8>,
    dir: tempfile::TempDir,
}

fn timers() -> Timers {
    Timers {
        handshake: Duration::from_secs(2),
        head: Duration::from_secs(2),
        frame: Duration::from_secs(5),
        linger: Duration::from_millis(500),
        slice: Duration::from_millis(100),
    }
}

fn browser_config(dir: &tempfile::TempDir, timers: Timers) -> BrowserConfig {
    let mut config = BrowserConfig::new(dir.path().join("LudusCtrlWeb.json")).with_timers(timers);
    config.port = 0;
    config.bind = Some(Ipv4Addr::LOCALHOST.into());
    config
}

fn rig_with(timers: Timers) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let mut config = LudusConfig::new(dir.path().join("LudusCtrl.json"));
    config.port = 0;
    config.bind = Some(Ipv4Addr::LOCALHOST.into());
    config.beacon = None::<BeaconTarget>;
    let (tx, statuses) = mpsc::channel();
    let server = LudusServer::start(config, Arc::new(Statuses(Mutex::new(tx)))).unwrap();
    let pad = Arc::new(Pad::default());
    server.set_pad(Some(pad.clone()));
    let info = server.start_browser(browser_config(&dir, timers)).unwrap();
    let cert = Identity::read(&dir.path().join("LudusCtrlWeb.json"))
        .expect("the certificate was saved")
        .cert
        .to_vec();
    Rig {
        server,
        pad,
        statuses,
        info,
        cert,
        dir,
    }
}

fn rig() -> Rig {
    rig_with(timers())
}

impl Rig {
    fn token(&self) -> String {
        let url = &self.info.urls[0];
        url.split("#t=").nth(1).unwrap().to_string()
    }

    fn tcp(&self) -> TcpStream {
        TcpStream::connect((Ipv4Addr::LOCALHOST, self.info.port)).unwrap()
    }

    fn tls(&self) -> Client {
        Client::connect(self.tcp(), &self.cert)
    }

    /// An authenticated browser holding the pad.
    fn browser(&self) -> Client {
        let mut client = self.tls();
        assert_eq!(client.upgrade(""), 101);
        client.send_text(&format!("auth:{}", self.token()));
        assert_eq!(client.text(), "ready");
        assert_eq!(client.text(), "padkind:x360");
        client
    }

    fn wait_status(&self, want: impl Fn(&LudusStatus) -> bool) -> LudusStatus {
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if let Ok(status) = self.statuses.recv_timeout(Duration::from_millis(100))
                && want(&status)
            {
                return status;
            }
        }
        panic!("no such status");
    }
}

type ClientStream = StreamOwned<ClientConnection, TcpStream>;

struct Client {
    stream: ClientStream,
    buffer: Vec<u8>,
}

/// What the server sent: a frame, the end of the stream, or nothing for a while.
#[derive(Debug, PartialEq, Eq)]
enum Got {
    Frame(u8, Vec<u8>),
    Eof,
    Timeout,
}

fn client_config(cert: &[u8]) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(cert.to_vec()))
        .unwrap();
    let config =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
    Arc::new(config)
}

/// A browser's frame: final, masked (unless told otherwise).
fn masked(first: u8, payload: &[u8], mask: bool) -> Vec<u8> {
    let key = [0x12, 0x34, 0x56, 0x78];
    let mut out = vec![first];
    let bit = if mask { 0x80 } else { 0 };
    match payload.len() {
        n if n < 126 => out.push(bit | n as u8),
        n if n <= 0xFFFF => {
            out.push(bit | 126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(bit | 127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    if mask {
        out.extend_from_slice(&key);
        out.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
    } else {
        out.extend_from_slice(payload);
    }
    out
}

impl Client {
    fn connect(tcp: TcpStream, cert: &[u8]) -> Client {
        tcp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let name = ServerName::IpAddress(Ipv4Addr::LOCALHOST.into());
        let conn = ClientConnection::new(client_config(cert), name).unwrap();
        Client {
            stream: StreamOwned::new(conn, tcp),
            buffer: Vec::new(),
        }
    }

    fn write(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).unwrap();
        self.stream.flush().unwrap();
    }

    /// Everything up to the end of the stream.
    fn all(&mut self) -> String {
        let mut out = Vec::new();
        let _ = self.stream.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    fn get(&mut self, request: &str) -> String {
        self.write(request.as_bytes());
        self.all()
    }

    /// Sends an upgrade request with `extra` headers and returns the status code.
    fn upgrade(&mut self, extra: &str) -> u16 {
        let request = format!(
            "GET /ws HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{extra}\r\n"
        );
        self.write(request.as_bytes());
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match self.stream.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                _ => break,
            }
        }
        let head = String::from_utf8_lossy(&head);
        if head.contains("101") {
            assert!(head.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));
        }
        head.split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0)
    }

    fn send_text(&mut self, text: &str) {
        self.write(&masked(0x81, text.as_bytes(), true));
    }

    fn send_binary(&mut self, body: &[u8]) {
        self.write(&masked(0x82, body, true));
    }

    fn got(&mut self, wait: Duration) -> Got {
        let until = Instant::now() + wait;
        loop {
            if self.buffer.len() >= 2 {
                let length = usize::from(self.buffer[1] & 0x7F);
                let (length, at) = match length {
                    126 if self.buffer.len() >= 4 => (
                        usize::from(u16::from_be_bytes([self.buffer[2], self.buffer[3]])),
                        4,
                    ),
                    126 => (usize::MAX, 0),
                    n => (n, 2),
                };
                if length != usize::MAX && self.buffer.len() >= at + length {
                    let opcode = self.buffer[0] & 0x0F;
                    assert_eq!(self.buffer[0] & 0xF0, 0x80, "final, no reserved bits");
                    assert_eq!(self.buffer[1] & 0x80, 0, "the server does not mask");
                    let payload = self.buffer[at..at + length].to_vec();
                    self.buffer.drain(..at + length);
                    return Got::Frame(opcode, payload);
                }
            }
            if Instant::now() >= until {
                return Got::Timeout;
            }
            self.stream
                .sock
                .set_read_timeout(Some(Duration::from_millis(50)))
                .unwrap();
            let mut chunk = [0u8; 8192];
            match self.stream.read(&mut chunk) {
                Ok(0) => return Got::Eof,
                Ok(n) => self.buffer.extend_from_slice(&chunk[..n]),
                Err(e) if http::is_timeout(&e) => {}
                Err(_) => return Got::Eof,
            }
        }
    }

    /// The next frame that is not a pong of ours.
    fn frame(&mut self) -> (u8, Vec<u8>) {
        match self.got(Duration::from_secs(3)) {
            Got::Frame(op, payload) => (op, payload),
            other => panic!("expected a frame, got {other:?}"),
        }
    }

    fn text(&mut self) -> String {
        loop {
            let (op, payload) = self.frame();
            if op == opcode::TEXT {
                return String::from_utf8(payload).unwrap();
            }
            assert_ne!(op, opcode::CLOSE, "closed while a text was expected");
        }
    }

    /// The text frames until the next one that starts with `prefix`.
    fn text_starting(&mut self, prefix: &str) -> String {
        loop {
            let text = self.text();
            if text.starts_with(prefix) {
                return text;
            }
        }
    }

    /// The close frame's code and reason; texts on the way are skipped.
    fn close_code(&mut self) -> (u16, String) {
        loop {
            let (op, payload) = self.frame();
            if op == opcode::CLOSE {
                let code = u16::from_be_bytes([payload[0], payload[1]]);
                let reason = String::from_utf8_lossy(&payload[2..]).into_owned();
                // Answer it, as a browser does.
                self.write(&masked(0x88, &payload[..2], true));
                return (code, reason);
            }
        }
    }
}

fn body(seq: u32, buttons: u16, flags: u8, layout: u8) -> [u8; 20] {
    let mut b = [0u8; 20];
    b[0] = 0xD5;
    b[1] = 1;
    b[2..6].copy_from_slice(&seq.to_le_bytes());
    b[6..8].copy_from_slice(&buttons.to_le_bytes());
    b[18] = flags;
    b[19] = layout;
    b
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

// ---- units ----

#[test]
fn the_v1_golden_bodies_decode() {
    let (seq, state, layout, controller) =
        decode_body(&hex("d501010000000000000000000000000000000100")).unwrap();
    assert_eq!((seq, controller, layout), (1, true, PadLayout::Unknown));
    assert_eq!(state, PadState::default());

    let (seq, state, layout, controller) =
        decode_body(&hex("d501fefffffffff70080ff7f39306cd9ff800102")).unwrap();
    assert_eq!(
        (seq, controller, layout),
        (0xFFFF_FFFE, true, PadLayout::Xbox)
    );
    assert_eq!(
        state,
        PadState {
            buttons: 0xF7FF,
            lx: -32768,
            ly: 32767,
            rx: 12345,
            ry: -9876,
            left_trigger: 255,
            right_trigger: 128,
        }
    );

    let (seq, state, layout, controller) =
        decode_body(&hex("d5017856341200100000ffff0100000000000001")).unwrap();
    assert_eq!(
        (seq, controller, layout),
        (0x1234_5678, false, PadLayout::PlayStation)
    );
    assert_eq!((state.buttons, state.ly, state.rx), (0x1000, -1, 1));

    // Bit 11 never reaches the pad; v2, other sizes and other magic are not bodies.
    let (_, state, ..) = decode_body(&body(0, 0xFFFF, 1, 0)).unwrap();
    assert_eq!(state.buttons, 0xF7FF);
    let mut v2 = body(0, 0, 1, 0);
    v2[1] = 2;
    assert!(decode_body(&v2).is_none());
    assert!(decode_body(&body(0, 0, 1, 0)[..19]).is_none());
    let mut wrong = body(0, 0, 1, 0);
    wrong[0] = 0xD6;
    assert!(decode_body(&wrong).is_none());
}

#[test]
fn frames_outside_the_protocol_earn_their_close_code() {
    let text = masked(0x81, b"hi", true);
    assert_eq!(
        parse_frame(&text).unwrap(),
        Some((
            Frame {
                opcode: opcode::TEXT,
                payload: b"hi".to_vec()
            },
            text.len()
        ))
    );
    for cut in 0..text.len() {
        assert_eq!(parse_frame(&text[..cut]), Ok(None), "incomplete at {cut}");
    }
    let protocol = [
        masked(0x01, b"hi", true),         // a fragment (not final)
        masked(0x80, b"hi", true),         // a continuation
        masked(0xC1, b"hi", true),         // RSV1
        masked(0xA1, b"hi", true),         // RSV2
        masked(0x83, b"hi", true),         // an unknown data opcode
        masked(0x8B, b"", true),           // an unknown control opcode
        masked(0x81, b"hi", false),        // unmasked
        masked(0x89, &[0u8; 126], true),   // a control frame over 125 bytes
        masked(0x09, b"", true),           // a fragmented control frame
        masked(0x82, &[0u8; 5000], false), // unmasked comes before too big
    ];
    for frame in protocol {
        assert_eq!(
            parse_frame(&frame),
            Err(Violation::Protocol),
            "{:02x?}",
            &frame[..2]
        );
    }
    let big = masked(0x82, &[0u8; FRAME_MAX + 1], true);
    assert_eq!(
        parse_frame(&big[..4]),
        Err(Violation::TooBig),
        "known from the header"
    );
    let largest = masked(0x82, &[7u8; FRAME_MAX], true);
    assert_eq!(
        parse_frame(&largest).unwrap().unwrap().0.payload.len(),
        FRAME_MAX
    );
    let wide = masked(0x82, &[0u8; 1], true);
    assert!(parse_frame(&wide).unwrap().is_some());
    // The server's frames.
    assert_eq!(encode_frame(opcode::TEXT, b"ready")[..2], [0x81, 5]);
    assert_eq!(
        encode_frame(opcode::BINARY, &[0; 300])[..4],
        [0x82, 126, 1, 44]
    );
}

#[test]
fn tokens_are_22_url_safe_characters_and_new_each_time() {
    let a = new_token();
    let b = new_token();
    assert_eq!(a.len(), 22);
    assert_ne!(a, b);
    assert!(
        a.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    );
}

#[test]
fn origin_and_device_rules() {
    assert!(http::origin_ok(None, "192.168.1.2:45991"));
    assert!(http::origin_ok(
        Some("HTTPS://192.168.1.2:45991"),
        "192.168.1.2:45991"
    ));
    assert!(!http::origin_ok(
        Some("https://evil.example"),
        "192.168.1.2:45991"
    ));
    assert!(!http::origin_ok(
        Some("http://192.168.1.2:45991"),
        "192.168.1.2:45991"
    ));
    let iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15";
    assert_eq!(http::device(Some(iphone)).as_deref(), Some("iPhone"));
    let android = "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 Chrome/140.0 Mobile";
    assert_eq!(http::device(Some(android)).as_deref(), Some("Android"));
    assert_eq!(http::device(Some("curl/8.0")), None);
    assert_eq!(http::device(None), None);
}

#[test]
fn the_csp_pins_the_inline_script_and_style_of_the_served_page() {
    let html = std::str::from_utf8(page::INDEX).unwrap();
    assert!(
        !html.contains('\r'),
        "the page is checked out with LF endings"
    );
    assert_eq!(page::inline_blocks(html, "script").len(), 1);
    assert_eq!(page::inline_blocks(html, "style").len(), 1);
    let (script, style) = page::sources();
    assert!(script.starts_with("'sha256-") && !script.contains(' '));
    assert!(style.starts_with("'sha256-") && !style.contains(' '));
    let csp = page::csp(&(script, style), "192.168.1.2:45991");
    assert!(csp.contains("connect-src 'self' wss://192.168.1.2:45991"));
    assert!(!csp.contains("unsafe-inline"));
    let tiny = "<style>a{}</style><script src=x></script><script>1</script><scripts>";
    assert_eq!(page::inline_blocks(tiny, "script"), ["1"]);
    assert_eq!(page::inline_blocks(tiny, "style"), ["a{}"]);
}

fn verifier(cert: &[u8]) -> Arc<WebPkiServerVerifier> {
    let mut roots = RootCertStore::empty();
    roots
        .add(rustls::pki_types::CertificateDer::from(cert.to_vec()))
        .unwrap();
    WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()
    .unwrap()
}

fn covers(identity: &Identity, name: &str) -> bool {
    let name = ServerName::try_from(name.to_string()).unwrap();
    verifier(&identity.cert)
        .verify_server_cert(&identity.cert, &[], &name, &[], UnixTime::now())
        .is_ok()
}

fn ip(text: &str) -> Ipv4Addr {
    text.parse().unwrap()
}

#[test]
fn the_certificate_covers_every_address_and_nothing_else() {
    let addresses = [ip("192.168.1.23"), ip("10.0.0.5")];
    let identity = mint(&addresses, None, unix_now()).unwrap();
    for name in ["192.168.1.23", "10.0.0.5", "127.0.0.1", "localhost"] {
        assert!(covers(&identity, name), "{name}");
    }
    for name in ["192.168.1.24", "example.com"] {
        assert!(!covers(&identity, name), "{name}");
    }
    // A leaf with CA:false: it is not a CA a phone could install.
    let der = identity.cert.as_ref();
    let basic_constraints = [0x06, 0x03, 0x55, 0x1D, 0x13];
    assert!(der.windows(5).any(|w| w == basic_constraints));
    assert_eq!(identity.fingerprint().split(':').count(), 32);
    let days = (identity.expires - unix_now()) / 86_400;
    assert_eq!(days, 825);
}

#[test]
fn the_certificate_is_kept_until_an_address_is_new_or_its_end_is_near() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("LudusCtrlWeb.json");
    let now = unix_now();
    let home = [ip("192.168.1.23")];
    let (first, state) = load_or_mint(&file, &home, now).unwrap();
    assert_eq!(state, Certificate::New);
    let (again, state) = load_or_mint(&file, &home, now).unwrap();
    assert_eq!(state, Certificate::Kept);
    assert_eq!(again.cert, first.cert);
    // A virtual adapter going away is no reason: a subset is covered.
    let (_, state) = load_or_mint(&file, &[], now).unwrap();
    assert_eq!(state, Certificate::Kept);

    // Ethernet as well: reissued, the key kept, both addresses covered, the new one first.
    let both = [ip("10.0.0.5"), ip("192.168.1.23")];
    let (moved, state) = load_or_mint(&file, &[ip("10.0.0.5")], now).unwrap();
    assert_eq!(state, Certificate::Reissued);
    assert_ne!(moved.cert, first.cert);
    assert_eq!(moved.key(), first.key());
    assert_eq!(moved.addresses, both);
    assert!(covers(&moved, "192.168.1.23") && covers(&moved, "10.0.0.5"));
    // Back on Wi-Fi: no new certificate.
    let (_, state) = load_or_mint(&file, &home, now).unwrap();
    assert_eq!(state, Certificate::Kept);

    // 30 days before its end.
    let late = moved.expires - 29 * 86_400;
    let (renewed, state) = load_or_mint(&file, &home, late).unwrap();
    assert_eq!(state, Certificate::Reissued);
    assert!(renewed.expires > moved.expires);

    // A broken file is a new certificate.
    std::fs::write(&file, "{ not json").unwrap();
    let (_, state) = load_or_mint(&file, &home, now).unwrap();
    assert_eq!(state, Certificate::New);

    // A key that is not the certificate's is not used.
    let (good, _) = load_or_mint(&file, &home, now).unwrap();
    let other = mint(&home, None, now).unwrap();
    let mut text: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let other_key = match other.key() {
        rustls::pki_types::PrivateKeyDer::Pkcs8(k) => {
            base64::engine::general_purpose::STANDARD.encode(k.secret_pkcs8_der())
        }
        _ => unreachable!(),
    };
    text["key"] = other_key.into();
    std::fs::write(&file, text.to_string()).unwrap();
    assert!(Identity::read(&file).is_none());
    let _ = good;

    // At most 32 addresses, the current ones first.
    let many: Vec<Ipv4Addr> = (1..=40).map(|i| Ipv4Addr::new(10, 1, 0, i)).collect();
    let (capped, _) = load_or_mint(&file, &many, now).unwrap();
    assert_eq!(capped.addresses.len(), MAX_ADDRESSES);
    assert_eq!(capped.addresses[0], many[0]);
}

// ---- over loopback ----

#[test]
fn the_page_is_served_with_its_csp_and_nothing_else_is() {
    let rig = rig();
    assert_eq!(rig.info.urls.len(), 1);
    assert!(rig.info.urls[0].starts_with(&format!("https://127.0.0.1:{}/#t=", rig.info.port)));
    let page = rig
        .tls()
        .get("GET / HTTP/1.1\r\nHost: 127.0.0.1:45991\r\n\r\n");
    let (head, body) = page.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(head.contains("Content-Type: text/html; charset=utf-8"));
    assert!(head.contains("Cache-Control: no-store"));
    assert_eq!(body.as_bytes(), page::INDEX);
    let csp = head
        .lines()
        .find_map(|l| l.strip_prefix("Content-Security-Policy: "))
        .unwrap();
    for (tag, directive) in [("script", "script-src"), ("style", "style-src")] {
        let blocks = page::inline_blocks(body, tag);
        let hash = base64::engine::general_purpose::STANDARD
            .encode(sha2::Sha256::digest(blocks[0].as_bytes()));
        assert!(
            csp.contains(&format!("{directive} 'sha256-{hash}'")),
            "{csp}"
        );
    }
    assert!(csp.contains("connect-src 'self' wss://127.0.0.1:45991"));

    let index = rig
        .tls()
        .get("GET /index.html?x=1 HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert!(index.starts_with("HTTP/1.1 200 OK"));
    for request in [
        "GET /app.js HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        "GET /x HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        "POST / HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\n\r\n",
    ] {
        assert!(
            rig.tls().get(request).starts_with("HTTP/1.1 404"),
            "{request}"
        );
    }
    assert!(
        rig.tls()
            .get("NONSENSE\r\n\r\n")
            .starts_with("HTTP/1.1 400")
    );
    // One request per connection.
    let two = rig
        .tls()
        .get("GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\nGET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert_eq!(two.matches("HTTP/1.1 200").count(), 1);
}

#[test]
fn the_upgrade_needs_its_headers_and_the_same_origin() {
    let rig = rig();
    let mut client = rig.tls();
    client.write(b"GET /ws HTTP/1.1\r\nHost: 127.0.0.1\r\nUpgrade: websocket\r\n\r\n");
    assert!(client.all().starts_with("HTTP/1.1 400"), "no key");
    let mut client = rig.tls();
    client.write(
        b"GET /ws HTTP/1.1\r\nHost: 127.0.0.1\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
    );
    assert!(client.all().starts_with("HTTP/1.1 400"), "no upgrade");
    assert_eq!(rig.tls().upgrade("Origin: https://evil.example\r\n"), 403);
    assert_eq!(rig.tls().upgrade("Origin: http://127.0.0.1\r\n"), 403);
    assert_eq!(rig.tls().upgrade("Origin: https://127.0.0.1\r\n"), 101);
    assert_eq!(rig.tls().upgrade(""), 101, "no Origin: not a browser");
}

#[test]
fn a_wrong_token_is_denied_and_a_missing_one_times_out() {
    let rig = rig_with(Timers {
        frame: Duration::from_millis(400),
        ..timers()
    });
    let mut client = rig.tls();
    assert_eq!(client.upgrade(""), 101);
    client.send_text("auth:AAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(client.text(), "denied");
    assert_eq!(client.close_code(), (1008, "bad token".into()));

    let mut client = rig.tls();
    client.upgrade("");
    client.send_binary(&body(0, 0, 1, 0));
    assert_eq!(client.text(), "denied", "the first frame must be auth:");
    assert_eq!(client.close_code().0, 1008);

    let mut client = rig.tls();
    client.upgrade("");
    let started = Instant::now();
    assert_eq!(client.close_code(), (1008, "no token".into()));
    assert!(started.elapsed() >= Duration::from_millis(400));
}

#[test]
fn input_reaches_the_one_pad_and_is_acked_at_most_every_200_ms() {
    let rig = rig();
    let mut client = rig.tls();
    client.upgrade("User-Agent: Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)\r\n");
    // auth: and the first packet in one write, as a pipelining client might.
    let mut both = masked(0x81, format!("auth:{}", rig.token()).as_bytes(), true);
    both.extend(masked(0x82, &body(7, 0x1000 | 0x0800, 1, 1), true));
    client.write(&both);
    assert_eq!(client.text(), "ready");
    assert_eq!(client.text(), "padkind:x360");
    assert_eq!(client.text(), "ack:7:1:1");
    assert_eq!(rig.pad.last().unwrap().buttons, 0x1000, "bit 11 masked");

    let status = rig.wait_status(|s| matches!(s, LudusStatus::Linked { .. }));
    assert_eq!(
        status,
        LudusStatus::Linked {
            via: LudusVia::Browser {
                peer: Ipv4Addr::LOCALHOST.into(),
                device: Some("iPhone".into()),
            },
            layout: PadLayout::PlayStation,
            controller: true,
            pad_live: true,
        }
    );

    // 60 packets over about a second (the page's 60 Hz): about five acks, never closer than 200 ms.
    let started = Instant::now();
    for seq in 8..68u32 {
        client.send_binary(&body(seq, 0x2000, 1, 1));
        std::thread::sleep(Duration::from_millis(16));
    }
    let elapsed = started.elapsed();
    client.send_text("ping:end");
    let mut acks = 0;
    loop {
        let text = client.text();
        if text == "pong:end" {
            break;
        }
        assert!(text.starts_with("ack:"), "{text}");
        acks += 1;
    }
    let most = elapsed.as_millis() / 200 + 1;
    assert!(
        acks >= 3 && acks as u128 <= most,
        "{acks} acks in {elapsed:?}"
    );
    assert_eq!(rig.pad.last().unwrap().buttons, 0x2000);

    // No controller on the phone: the pad is held at neutral.
    client.send_binary(&body(100, 0x2000, 0, 1));
    client.send_text("ping:n");
    client.text_starting("pong:n");
    assert_eq!(rig.pad.last(), Some(PadState::default()));
    // Other binaries are ignored with no reply.
    let count = rig.pad.count();
    client.send_binary(&[0xD5, 2, 0, 0]);
    let mut v2 = body(101, 1, 1, 0);
    v2[1] = 2;
    client.send_binary(&v2);
    client.send_text("ping:after");
    assert_eq!(client.text_starting("pong:"), "pong:after");
    assert_eq!(rig.pad.count(), count);
}

#[test]
fn pings_pad_kinds_and_websocket_pings_are_answered() {
    let rig = rig();
    let mut client = rig.browser();
    client.send_text("ping:abc");
    assert_eq!(client.text(), "pong:abc");
    let long = "é".repeat(40); // 80 bytes; 59 cuts a character in two
    client.send_text(&format!("ping:{long}"));
    let pong = client.text();
    assert_eq!(pong, format!("pong:{}", "é".repeat(29)));
    client.write(&masked(0x89, b"hello", true));
    assert_eq!(client.frame(), (opcode::PONG, b"hello".to_vec()));
    client.send_text("pad:x360");
    assert_eq!(client.text(), "padkind:x360");
    client.send_text("pad:ds4");
    assert_eq!(client.text(), "paderr:wait a moment before switching again");
    client.send_text("pad:gamecube");
    assert_eq!(client.text(), "paderr:unknown pad kind");
    client.send_text("hello");
    client.send_text("ping:1");
    assert_eq!(client.text(), "pong:1", "other text is ignored");
    client.write(&masked(0x88, &1000u16.to_be_bytes(), true));
    assert_eq!(
        client.frame(),
        (opcode::CLOSE, 1000u16.to_be_bytes().to_vec())
    );
    assert_eq!(client.got(Duration::from_secs(2)), Got::Eof);
}

#[test]
fn a_ds4_switch_is_refused_with_the_reason() {
    let rig = rig();
    let mut client = rig.browser();
    client.send_text("pad:ds4");
    assert_eq!(client.text(), "paderr:this PC has only the Xbox 360 pad");
}

#[test]
fn every_frame_violation_closes_with_its_code() {
    let rig = rig();
    let cases: [(Vec<u8>, u16); 5] = [
        (masked(0x82, &[0u8; FRAME_MAX + 1], true), 1009),
        (masked(0x81, b"ping:x", false), 1002),
        (masked(0x01, b"ping:", true), 1002),
        (masked(0xC1, b"ping:x", true), 1002),
        (masked(0x89, &[0u8; 126], true), 1002),
    ];
    for (frame, code) in cases {
        let mut client = rig.browser();
        client.write(&frame);
        assert_eq!(client.close_code().0, code, "{:02x?}", &frame[..2]);
    }
    // A frame of exactly 4096 bytes is accepted (and ignored).
    let mut client = rig.browser();
    client.send_binary(&[0u8; FRAME_MAX]);
    client.send_text("ping:ok");
    assert_eq!(client.text(), "pong:ok");
}

#[test]
fn a_frame_split_across_reads_survives() {
    let rig = rig();
    let mut client = rig.browser();
    let frame = masked(0x82, &body(3, 0x4000, 1, 2), true);
    client.write(&frame[..9]);
    std::thread::sleep(Duration::from_millis(300)); // three read slices
    client.write(&frame[9..]);
    assert_eq!(client.text(), "ack:3:1:1");
    assert_eq!(rig.pad.last().unwrap().buttons, 0x4000);
}

#[test]
fn one_browser_at_a_time_and_a_silent_one_is_taken_over() {
    let rig = rig();
    let mut first = rig.browser();
    first.send_binary(&body(0, 1, 1, 0));
    first.text_starting("ack:");
    let mut second = rig.tls();
    second.upgrade("");
    second.send_text(&format!("auth:{}", rig.token()));
    assert_eq!(second.text(), "busy");
    assert_eq!(second.close_code().0, 1000);
    // The first goes silent: after 1.5 s (and the 0.25 s tick) the next one takes over.
    std::thread::sleep(Duration::from_millis(1800));
    let mut third = rig.browser();
    third.send_binary(&body(0, 2, 1, 0));
    assert_eq!(
        third.text_starting("ack:"),
        "ack:0:2:1",
        "the bridge counts every applied packet"
    );
    assert_eq!(rig.pad.last().unwrap().buttons, 2);
    // The first learns it on its next frame.
    first.send_binary(&body(1, 1, 1, 0));
    assert_eq!(first.text(), "evicted");
    assert_eq!(first.close_code().0, 1000);
    assert_eq!(rig.pad.last().unwrap().buttons, 2, "not applied");
}

#[test]
fn silence_is_evicted_then_idle() {
    let rig = rig_with(Timers {
        frame: Duration::from_millis(2200),
        ..timers()
    });
    let mut client = rig.browser();
    client.send_binary(&body(0, 1, 1, 0));
    client.text_starting("ack:");
    // The watchdog drops the claim at 1.5 to 1.75 s, the idle deadline comes at 2.2 s.
    assert_eq!(client.text(), "evicted");
    assert_eq!(client.close_code(), (1001, "idle".into()));
    assert_eq!(
        rig.pad.last(),
        Some(PadState::default()),
        "released to neutral"
    );

    // Idle while still holding (a deadline shorter than the watchdog): 1001 alone.
    let rig = rig_with(Timers {
        frame: Duration::from_millis(500),
        ..timers()
    });
    let mut client = rig.browser();
    client.send_binary(&body(0, 1, 1, 0));
    client.text_starting("ack:");
    assert_eq!(client.close_code(), (1001, "idle".into()));
}

#[test]
fn the_app_and_a_browser_share_the_one_pad() {
    let rig = rig();
    let slot = rig.server.pad_slot();
    let phone = [9u8; 8];
    let from = SocketAddr::from((Ipv4Addr::LOCALHOST, 1));
    let state = PadState {
        buttons: 0x8000,
        ..PadState::default()
    };
    let now = Instant::now();
    assert_eq!(
        slot.app_input(phone, from, true, &state, PadLayout::Xbox, true, now),
        Some(true)
    );
    let mut client = rig.tls();
    client.upgrade("");
    client.send_text(&format!("auth:{}", rig.token()));
    assert_eq!(client.text(), "busy:app");
    assert_eq!(client.close_code().0, 1000);

    // The app goes quiet; the browser claims, and the app's packets are no longer applied.
    std::thread::sleep(Duration::from_millis(1800));
    let mut client = rig.browser();
    client.send_binary(&body(0, 0x0001, 1, 0));
    client.text_starting("ack:");
    assert_eq!(
        slot.app_input(
            phone,
            from,
            true,
            &state,
            PadLayout::Xbox,
            true,
            Instant::now()
        ),
        None
    );
    assert_eq!(rig.pad.last().unwrap().buttons, 0x0001);
    // The browser leaves: the pad is neutral and free for the app again.
    client.write(&masked(0x88, &1000u16.to_be_bytes(), true));
    assert_eq!(client.frame().0, opcode::CLOSE);
    std::thread::sleep(Duration::from_millis(200));
    assert!(slot.holding().is_none());
    assert_eq!(rig.pad.last(), Some(PadState::default()));
    assert_eq!(
        slot.app_input(
            phone,
            from,
            true,
            &state,
            PadLayout::Xbox,
            true,
            Instant::now()
        ),
        Some(true)
    );
}

#[test]
fn new_link_voids_the_old_one_and_stop_closes_everything() {
    let rig = rig();
    let old = rig.token();
    let mut client = rig.browser();
    let info = rig.server.new_browser_link().unwrap();
    assert_ne!(rig.token(), info.urls[0].split("#t=").nth(1).unwrap());
    assert_eq!(info.certificate, rig.info.certificate);
    assert_eq!(
        info.fingerprint, rig.info.fingerprint,
        "the certificate stays"
    );
    assert_eq!(client.text(), "denied");
    assert_eq!(client.close_code().0, 1008);
    let mut stale = rig.tls();
    stale.upgrade("");
    stale.send_text(&format!("auth:{old}"));
    assert_eq!(stale.text(), "denied");

    let new = info.urls[0].split("#t=").nth(1).unwrap().to_string();
    let mut client = rig.tls();
    client.upgrade("");
    client.send_text(&format!("auth:{new}"));
    assert_eq!(client.text(), "ready");
    client.send_binary(&body(0, 1, 1, 0));
    client.text_starting("ack:");
    rig.server.stop_browser();
    assert_eq!(client.close_code(), (1001, "stopping".into()));
    assert!(rig.server.browser().is_none());
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, rig.info.port)).is_err());
    std::thread::sleep(Duration::from_millis(100));
    assert!(rig.server.pad_slot().holding().is_none());
}

#[test]
fn a_restart_gets_the_same_port_and_certificate_and_a_new_token() {
    let rig = rig();
    let mut client = rig.browser();
    client.send_binary(&body(0, 1, 1, 0));
    client.text_starting("ack:");
    client.write(&masked(0x88, &1000u16.to_be_bytes(), true));
    client.frame();
    rig.server.stop_browser();
    let mut config = browser_config(&rig.dir, timers());
    config.port = rig.info.port;
    let again = rig.server.start_browser(config).unwrap();
    assert!(again.preferred_port, "the port is free again at once");
    assert_eq!(again.port, rig.info.port);
    assert_eq!(again.certificate, Certificate::Kept);
    assert_eq!(again.fingerprint, rig.info.fingerprint);
    assert_ne!(again.urls, rig.info.urls);
}

#[test]
fn a_busy_port_falls_back_and_cannot_be_shared() {
    let rig = rig();
    // Nobody can bind over the running port, not even with SO_REUSEADDR.
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
    socket.set_reuse_address(true).unwrap();
    let taken = SocketAddr::from((Ipv4Addr::LOCALHOST, rig.info.port));
    assert!(socket.bind(&taken.into()).is_err());

    let dir = tempfile::tempdir().unwrap();
    let mut config = LudusConfig::new(dir.path().join("LudusCtrl.json"));
    config.port = 0;
    config.bind = Some(Ipv4Addr::LOCALHOST.into());
    config.beacon = None;
    let (tx, _rx) = mpsc::channel();
    let other = LudusServer::start(config, Arc::new(Statuses(Mutex::new(tx)))).unwrap();
    let mut browser = browser_config(&dir, timers());
    browser.port = rig.info.port;
    let info = other.start_browser(browser).unwrap();
    assert!(!info.preferred_port);
    assert_ne!(info.port, rig.info.port);
}

#[test]
fn connections_are_capped_and_a_stalled_handshake_is_dropped() {
    let rig = rig_with(Timers {
        handshake: Duration::from_millis(1500),
        ..timers()
    });
    // Sixteen connections that never start TLS hold every place.
    let held: Vec<TcpStream> = (0..MAX_CONNECTIONS).map(|_| rig.tcp()).collect();
    std::thread::sleep(Duration::from_millis(300));
    let mut seventeenth = rig.tcp();
    seventeenth
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let started = Instant::now();
    let mut byte = [0u8; 1];
    let closed = matches!(seventeenth.read(&mut byte), Ok(0) | Err(_));
    assert!(
        closed && started.elapsed() < Duration::from_millis(450),
        "closed at once"
    );
    // The stalled ones are dropped at the handshake deadline.
    let mut first = held.into_iter().next().unwrap();
    first
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    assert!(matches!(first.read(&mut byte), Ok(0) | Err(_)));
    assert!(started.elapsed() < Duration::from_millis(2000));
    std::thread::sleep(Duration::from_millis(300));
    // Places are free again.
    let page = rig.tls().get("GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert!(page.starts_with("HTTP/1.1 200"));
}

#[test]
fn a_stopped_server_has_no_browser_client() {
    let rig = rig();
    rig.server.stop();
    assert!(rig.server.browser().is_none());
    assert!(
        rig.server
            .start_browser(browser_config(&rig.dir, timers()))
            .is_err()
    );
}
