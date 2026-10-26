//! `fake-phone browser`: a browser client of LudusCtrl's browser bridge, for runs by hand against a
//! running launcher. It opens the link the launcher shows, takes the token from its fragment,
//! skips certificate verification (a test tool talking to a self-signed PC), and sends the v1
//! body of LudusCtrl's PROTOCOL.md every 16 ms, printing everything the PC sends.
//!
//! ```text
//! fake-phone browser --url "https://127.0.0.1:45991/#t=TOKEN" --press cross+r1 --seconds 3
//! ```

use std::{
    collections::BTreeMap,
    io,
    net::TcpStream,
    sync::Arc,
    time::{Duration, Instant},
};

use rustls::{
    ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, ring, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use tungstenite::Message;

/// Accepts the PC's self-signed certificate, but still checks the handshake signatures.
#[derive(Debug)]
struct AnyCertificate(Arc<CryptoProvider>);

impl ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

/// XInput `wButtons`, as the browser body carries them.
fn button(name: &str) -> Result<u16, String> {
    Ok(match name {
        "up" => 0x0001,
        "down" => 0x0002,
        "left" => 0x0004,
        "right" => 0x0008,
        "options" | "start" => 0x0010,
        "create" | "back" => 0x0020,
        "l3" => 0x0040,
        "r3" => 0x0080,
        "l1" => 0x0100,
        "r1" => 0x0200,
        "ps" | "guide" => 0x0400,
        "cross" | "a" => 0x1000,
        "circle" | "b" => 0x2000,
        "square" | "x" => 0x4000,
        "triangle" | "y" => 0x8000,
        _ => return Err(format!("unknown button {name}")),
    })
}

fn body(seq: u32, buttons: u16) -> Vec<u8> {
    let mut b = vec![0u8; 20];
    b[0] = 0xD5;
    b[1] = 1;
    b[2..6].copy_from_slice(&seq.to_le_bytes());
    b[6..8].copy_from_slice(&buttons.to_le_bytes());
    b[18] = 1; // a controller is connected
    b
}

pub fn browser(options: &BTreeMap<String, String>) -> Result<(), String> {
    let url = options.get("url").ok_or("--url is required")?;
    let (base, token) = url.split_once("#t=").ok_or("the URL has no #t= token")?;
    let host = base
        .strip_prefix("https://")
        .ok_or("the URL is not https://")?
        .trim_end_matches('/');
    let buttons = match options.get("press").map(String::as_str) {
        None | Some("") => 0,
        Some(names) => names
            .split('+')
            .map(button)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .fold(0, |all, b| all | b),
    };
    let seconds: f64 = options
        .get("seconds")
        .map_or(Ok(3.0), |s| s.parse())
        .map_err(|_| "--seconds is a number")?;

    let provider = Arc::new(ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(AnyCertificate(provider)))
        .with_no_client_auth();
    let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    let name = name.trim_start_matches('[').trim_end_matches(']');
    let server_name = ServerName::try_from(name.to_string()).map_err(|e| e.to_string())?;
    let tcp = TcpStream::connect(host).map_err(|e| format!("connect {host}: {e}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    let conn = ClientConnection::new(Arc::new(config), server_name).map_err(|e| e.to_string())?;
    let stream = StreamOwned::new(conn, tcp);
    let (mut ws, _) = tungstenite::client(format!("wss://{host}/ws"), stream)
        .map_err(|e| format!("upgrade: {e}"))?;
    println!("connected to {host}");
    ws.send(Message::text(format!("auth:{token}")))
        .map_err(|e| e.to_string())?;
    ws.get_mut()
        .sock
        .set_read_timeout(Some(Duration::from_millis(5)))
        .map_err(|e| e.to_string())?;

    let until = Instant::now() + Duration::from_secs_f64(seconds);
    let mut ready = false;
    let mut seq = 0u32;
    let mut next_send = Instant::now();
    let mut next_ping = Instant::now();
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                println!("< {text}");
                match text.as_str() {
                    "ready" => ready = true,
                    "busy" | "busy:app" | "denied" | "evicted" => ready = false,
                    _ => {}
                }
            }
            Ok(Message::Close(frame)) => {
                println!("< close {frame:?}");
                return Ok(());
            }
            Ok(other) => println!("< {other:?}"),
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed) => return Ok(()),
            Err(e) => return Err(format!("read: {e}")),
        }
        let now = Instant::now();
        if now >= until {
            if ready {
                // The final neutral packet, then close.
                let mut neutral = body(seq, 0);
                neutral[18] = 0;
                let _ = ws.send(Message::binary(neutral));
            }
            let _ = ws.close(None);
            let _ = ws.flush();
            return Ok(());
        }
        if ready && now >= next_send {
            ws.send(Message::binary(body(seq, buttons)))
                .map_err(|e| e.to_string())?;
            seq = seq.wrapping_add(1);
            next_send = now + Duration::from_millis(16);
        }
        if ready && now >= next_ping {
            let _ = ws.send(Message::text(format!("ping:{seq}")));
            next_ping = now + Duration::from_secs(1);
        }
    }
}
