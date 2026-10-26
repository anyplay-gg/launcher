//! The one HTTP request of a browser client connection: its head, the page's files, and the checks
//! before a WebSocket upgrade.

use std::{
    io::{self, Read, Write},
    time::Instant,
};

/// The longest request head read.
pub const HEAD_MAX: usize = 8 * 1024;
const HEADERS_MAX: usize = 32;

/// What the server needs of a request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// Without the query.
    pub path: String,
    pub host: Option<String>,
    pub origin: Option<String>,
    /// `Upgrade: websocket`.
    pub websocket: bool,
    pub key: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug)]
pub enum HeadError {
    /// Nothing usable: the connection is closed without an answer.
    Gone,
    /// Malformed or too long: 400.
    Bad,
}

pub fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

/// Reads up to the end of the head (at most [`HEAD_MAX`] bytes, until `deadline`). Returns the
/// request and the bytes read past its head (a client may send its first frame at once).
pub fn read_head(
    stream: &mut impl Read,
    deadline: Instant,
) -> Result<(Request, Vec<u8>), HeadError> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(parsed) = parse(&buffer)? {
            return Ok(parsed);
        }
        if buffer.len() >= HEAD_MAX {
            return Err(HeadError::Bad);
        }
        if Instant::now() >= deadline {
            return Err(HeadError::Gone);
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Err(HeadError::Gone),
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
            Err(error) if is_timeout(&error) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return Err(HeadError::Gone),
        }
    }
}

fn parse(buffer: &[u8]) -> Result<Option<(Request, Vec<u8>)>, HeadError> {
    let mut headers = [httparse::EMPTY_HEADER; HEADERS_MAX];
    let mut request = httparse::Request::new(&mut headers);
    let length = match request.parse(buffer) {
        Ok(httparse::Status::Complete(length)) => length,
        Ok(httparse::Status::Partial) => return Ok(None),
        Err(_) => return Err(HeadError::Bad),
    };
    if length > HEAD_MAX {
        return Err(HeadError::Bad);
    }
    let path = request.path.unwrap_or("/");
    let path = path.split(['?', '#']).next().unwrap_or("/").to_string();
    let mut out = Request {
        method: request.method.unwrap_or("").to_string(),
        path,
        ..Request::default()
    };
    for header in request.headers.iter() {
        let value = String::from_utf8_lossy(header.value).trim().to_string();
        let name = header.name;
        if name.eq_ignore_ascii_case("host") {
            out.host = Some(value);
        } else if name.eq_ignore_ascii_case("origin") {
            out.origin = Some(value);
        } else if name.eq_ignore_ascii_case("upgrade") {
            out.websocket = value
                .split(',')
                .any(|v| v.trim().eq_ignore_ascii_case("websocket"));
        } else if name.eq_ignore_ascii_case("sec-websocket-key") {
            out.key = Some(value);
        } else if name.eq_ignore_ascii_case("user-agent") {
            out.user_agent = Some(value);
        }
    }
    Ok(Some((out, buffer[length..].to_vec())))
}

/// The upgrade's Origin check: absent, or `https://` + Host (case-insensitive).
pub fn origin_ok(origin: Option<&str>, host: &str) -> bool {
    match origin {
        None => true,
        Some(origin) => origin.eq_ignore_ascii_case(&format!("https://{host}")),
    }
}

/// A coarse name for the device a browser runs on, from its User-Agent.
pub fn device(user_agent: Option<&str>) -> Option<String> {
    let ua = user_agent?;
    let name = if ua.contains("iPhone") {
        "iPhone"
    } else if ua.contains("iPad") {
        "iPad"
    } else if ua.contains("Android") {
        "Android"
    } else if ua.contains("Macintosh") {
        "Mac"
    } else if ua.contains("Windows") {
        "Windows"
    } else if ua.contains("Linux") {
        "Linux"
    } else {
        return None;
    };
    Some(name.to_string())
}

/// The headers every answer carries.
const COMMON: &str = "Cache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n";

/// Writes one complete answer.
pub fn respond(
    stream: &mut impl Write,
    status: &str,
    content_type: &str,
    extra: &str,
    body: &[u8],
) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{COMMON}{extra}\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

/// A short plain-text error answer.
pub fn error(stream: &mut impl Write, status: &str, extra: &str) -> io::Result<()> {
    respond(
        stream,
        status,
        "text/plain; charset=utf-8",
        extra,
        status.as_bytes(),
    )
}

/// The 101 answer of a WebSocket upgrade.
pub fn switching(stream: &mut impl Write, key: &str) -> io::Result<()> {
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream.write_all(head.as_bytes())?;
    stream.flush()
}
