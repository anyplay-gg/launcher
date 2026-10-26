//! One browser client's WebSocket: the frames, authentication, the claim on the pad slot, input,
//! acks and the close codes of LudusCtrl's protocol. The framing is written here rather than taken
//! from tungstenite, because the protocol accepts only single, unfragmented, masked frames of at
//! most 4096 bytes and names a close code for each violation; tungstenite would reassemble
//! fragmented messages.

use std::{
    io::{self, Read as _, Write},
    net::SocketAddr,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use super::{Context, Stream, http::is_timeout};
use crate::remote::{
    ludusctrl::{
        Owner, PadSlot, Refusal,
        wire::{BUTTON_MASK, FLAG_CONTROLLER, INPUT_MAGIC, PadLayout},
    },
    security::constant_time_eq,
    vpad::PadState,
};

/// The largest frame accepted.
pub const FRAME_MAX: usize = 4096;
/// The largest control frame (RFC 6455).
const CONTROL_MAX: u64 = 125;
/// The v1 body's size and version.
pub const BODY_SIZE: usize = 20;
pub const BODY_VERSION: u8 = 1;
/// At most one ack this often per connection.
pub const ACK_EVERY: Duration = Duration::from_millis(200);
/// At most one pad switch this often.
pub const SWITCH_EVERY: Duration = Duration::from_secs(2);
/// The most of a ping's id echoed back.
pub const PING_ID_MAX: usize = 59;

pub mod opcode {
    pub const TEXT: u8 = 0x1;
    pub const BINARY: u8 = 0x2;
    pub const CLOSE: u8 = 0x8;
    pub const PING: u8 = 0x9;
    pub const PONG: u8 = 0xA;
}

/// One frame from the browser, unmasked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub opcode: u8,
    pub payload: Vec<u8>,
}

/// A frame the protocol does not accept, by the close code it earns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Violation {
    /// 1002: reserved bits, a fragment or continuation, an unknown opcode, a control frame over
    /// 125 bytes, an unmasked frame.
    Protocol,
    /// 1009: over 4096 bytes.
    TooBig,
}

/// The frame at the start of `buffer` and the bytes it took; `None` while it is incomplete. The
/// header is judged as soon as it is complete, so a frame that is too big is refused before its
/// payload arrives.
pub fn parse_frame(buffer: &[u8]) -> Result<Option<(Frame, usize)>, Violation> {
    let [first, second, ..] = *buffer else {
        return Ok(None);
    };
    let fin = first & 0x80 != 0;
    let reserved = first & 0x70 != 0;
    let opcode = first & 0x0F;
    let masked = second & 0x80 != 0;
    let known = matches!(
        opcode,
        opcode::TEXT | opcode::BINARY | opcode::CLOSE | opcode::PING | opcode::PONG
    );
    if reserved || !fin || !known || !masked {
        return Err(Violation::Protocol);
    }
    let (length, mut at) = match second & 0x7F {
        126 => match buffer.get(2..4) {
            Some(b) => (u64::from(u16::from_be_bytes([b[0], b[1]])), 4),
            None => return Ok(None),
        },
        127 => match buffer.get(2..10) {
            Some(b) => (u64::from_be_bytes(b.try_into().expect("8 bytes")), 10),
            None => return Ok(None),
        },
        n => (u64::from(n), 2),
    };
    if opcode >= opcode::CLOSE && length > CONTROL_MAX {
        return Err(Violation::Protocol);
    }
    if length > FRAME_MAX as u64 {
        return Err(Violation::TooBig);
    }
    let length = length as usize;
    let Some(mask) = buffer.get(at..at + 4) else {
        return Ok(None);
    };
    let mask: [u8; 4] = mask.try_into().expect("4 bytes");
    at += 4;
    let Some(payload) = buffer.get(at..at + length) else {
        return Ok(None);
    };
    let payload = payload
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ mask[i % 4])
        .collect();
    Ok(Some((Frame { opcode, payload }, at + length)))
}

/// A frame from the server: final, unmasked.
pub fn encode_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 10);
    out.push(0x80 | opcode);
    match payload.len() {
        n if n < 126 => out.push(n as u8),
        n if n <= usize::from(u16::MAX) => {
            out.push(126);
            out.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            out.push(127);
            out.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    out.extend_from_slice(payload);
    out
}

fn close_payload(code: u16, reason: &str) -> Vec<u8> {
    let mut payload = code.to_be_bytes().to_vec();
    payload.extend_from_slice(reason.as_bytes());
    payload
}

/// A v1 body (20 bytes, `D5 01`) as its seq, pad state, layout and whether a controller is there.
pub fn decode_body(body: &[u8]) -> Option<(u32, PadState, PadLayout, bool)> {
    if body.len() != BODY_SIZE || body[0] != INPUT_MAGIC || body[1] != BODY_VERSION {
        return None;
    }
    let i16_at = |at: usize| i16::from_le_bytes([body[at], body[at + 1]]);
    let seq = u32::from_le_bytes(body[2..6].try_into().expect("4 bytes"));
    let state = PadState {
        buttons: u16::from_le_bytes([body[6], body[7]]) & BUTTON_MASK,
        lx: i16_at(8),
        ly: i16_at(10),
        rx: i16_at(12),
        ry: i16_at(14),
        left_trigger: body[16],
        right_trigger: body[17],
    };
    let controller = body[18] & FLAG_CONTROLLER != 0;
    Some((seq, state, PadLayout::from_byte(body[19]), controller))
}

/// The first `PING_ID_MAX` bytes of `id`, cut at a character boundary.
fn ping_id(id: &str) -> &str {
    let mut end = id.len().min(PING_ID_MAX);
    while !id.is_char_boundary(end) {
        end -= 1;
    }
    &id[..end]
}

/// How a session ends.
enum End {
    /// Our close frame, then the linger.
    Close(u16, &'static str),
    /// A text, then our close frame.
    Say(&'static str, u16, &'static str),
    /// Nothing more to send: the peer closed (its close was echoed), or the socket failed.
    Gone,
}

enum Read {
    Frame(Frame),
    /// The read slice passed with no complete frame.
    Nothing,
    End(End),
}

struct Session<'a> {
    ctx: &'a Context,
    slot: &'a PadSlot,
    stream: Stream,
    buffer: Vec<u8>,
    connection: u64,
    peer: SocketAddr,
    last_frame: Instant,
    last_ack: Option<Instant>,
}

impl Session<'_> {
    fn write(&mut self, opcode: u8, payload: &[u8]) -> io::Result<()> {
        self.stream.write_all(&encode_frame(opcode, payload))?;
        self.stream.flush()
    }

    fn send(&mut self, text: String) -> Result<(), End> {
        self.write(opcode::TEXT, text.as_bytes())
            .map_err(|_| End::Gone)
    }

    /// The next frame, or what the slice brought instead.
    fn next(&mut self) -> Read {
        match parse_frame(&self.buffer) {
            Ok(Some((frame, used))) => {
                self.buffer.drain(..used);
                self.last_frame = Instant::now();
                return Read::Frame(frame);
            }
            Ok(None) => {}
            Err(Violation::Protocol) => return Read::End(End::Close(1002, "protocol error")),
            Err(Violation::TooBig) => return Read::End(End::Close(1009, "too big")),
        }
        let mut chunk = [0u8; FRAME_MAX + 16];
        match self.stream.read(&mut chunk) {
            Ok(0) => Read::End(End::Gone),
            Ok(n) => {
                self.buffer.extend_from_slice(&chunk[..n]);
                Read::Nothing
            }
            Err(error) if is_timeout(&error) || error.kind() == io::ErrorKind::Interrupted => {
                Read::Nothing
            }
            Err(_) => Read::End(End::Gone),
        }
    }

    /// The next frame; control frames are answered here. `Ok(None)`: the slice passed.
    fn read(&mut self) -> Result<Option<Frame>, End> {
        loop {
            match self.next() {
                Read::Frame(frame) if frame.opcode == opcode::CLOSE => {
                    // The code is echoed back and the session ends.
                    let code = frame.payload.get(..2).unwrap_or(&[]).to_vec();
                    let _ = self.write(opcode::CLOSE, &code);
                    return Err(End::Gone);
                }
                Read::Frame(frame) if frame.opcode == opcode::PING => {
                    self.write(opcode::PONG, &frame.payload)
                        .map_err(|_| End::Gone)?;
                    return Ok(Some(frame));
                }
                Read::Frame(frame) => return Ok(Some(frame)),
                // Bytes arrived but no whole frame yet: look again before waiting.
                Read::Nothing if !self.buffer.is_empty() && self.complete() => {}
                Read::Nothing => return Ok(None),
                Read::End(end) => return Err(end),
            }
        }
    }

    /// Whether the buffer holds a whole frame (or a violation) to act on.
    fn complete(&self) -> bool {
        !matches!(parse_frame(&self.buffer), Ok(None))
    }

    /// Whether the next frame, already here, is another input packet.
    fn input_follows(&self) -> bool {
        matches!(parse_frame(&self.buffer), Ok(Some((frame, _)))
            if frame.opcode == opcode::BINARY && decode_body(&frame.payload).is_some())
    }

    fn holds(&self) -> bool {
        self.slot
            .holding()
            .is_some_and(|h| h.owner == Owner::Browser(self.connection))
    }

    fn idle(&self) -> bool {
        self.last_frame.elapsed() >= self.ctx.timers.frame
    }

    /// The first frame must be `auth:<token>`, within the frame deadline.
    fn authenticate(&mut self) -> Result<u64, End> {
        loop {
            if self.ctx.stopping() {
                return Err(End::Close(1001, "stopping"));
            }
            match self.read()? {
                Some(frame) => {
                    let (token, generation) = self.ctx.token();
                    let given = (frame.opcode == opcode::TEXT)
                        .then(|| std::str::from_utf8(&frame.payload).ok())
                        .flatten()
                        .and_then(|text| text.strip_prefix("auth:"));
                    return match given {
                        Some(given) if constant_time_eq(given.as_bytes(), token.as_bytes()) => {
                            Ok(generation)
                        }
                        _ => Err(End::Say("denied", 1008, "bad token")),
                    };
                }
                None if self.idle() => return Err(End::Close(1008, "no token")),
                None => {}
            }
        }
    }

    fn claim(&mut self) -> Result<(), End> {
        match self
            .slot
            .claim_browser(self.connection, self.peer, Instant::now())
        {
            Ok(()) => {
                self.send("ready".into())?;
                self.send("padkind:x360".into())
            }
            Err(Refusal::BusyApp) => Err(End::Say("busy:app", 1000, "")),
            Err(Refusal::Busy) => Err(End::Say("busy", 1000, "")),
        }
    }

    fn input(&mut self, body: &[u8]) -> Result<(), End> {
        let Some((seq, state, layout, controller)) = decode_body(body) else {
            // Ignored, with no reply.
            return Ok(());
        };
        if self.input_follows() {
            // Only the newest snapshot matters: a backlog after a stall is not replayed.
            return Ok(());
        }
        let now = Instant::now();
        let Some(live) = self
            .slot
            .browser_input(self.connection, &state, layout, controller, now)
        else {
            return Err(End::Say("evicted", 1000, ""));
        };
        let received = self
            .ctx
            .received
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1);
        if self
            .last_ack
            .is_none_or(|at| now.saturating_duration_since(at) >= ACK_EVERY)
        {
            self.last_ack = Some(now);
            self.send(format!("ack:{seq}:{received}:{}", u8::from(live)))?;
        }
        Ok(())
    }

    /// `pad:<kind>`. The launcher's virtual pad is an Xbox 360 controller and nothing else, so
    /// `x360` is the only switch that works.
    fn switch(&mut self, kind: &str) -> Result<(), End> {
        let reply = if !matches!(kind, "x360" | "ds4") {
            "paderr:unknown pad kind".to_string()
        } else if !self.ctx.may_switch(Instant::now()) {
            "paderr:wait a moment before switching again".to_string()
        } else if kind == "x360" {
            "padkind:x360".to_string()
        } else {
            "paderr:this PC has only the Xbox 360 pad".to_string()
        };
        self.send(reply)
    }

    fn text(&mut self, payload: &[u8]) -> Result<(), End> {
        let Ok(text) = std::str::from_utf8(payload) else {
            return Ok(());
        };
        if let Some(id) = text.strip_prefix("ping:") {
            let reply = format!("pong:{}", ping_id(id));
            self.send(reply)
        } else if let Some(kind) = text.strip_prefix("pad:") {
            self.switch(kind)
        } else {
            Ok(())
        }
    }

    /// The claimed connection until it ends.
    fn run(&mut self, generation: u64) -> End {
        loop {
            if self.ctx.stopping() {
                return End::Close(1001, "stopping");
            }
            if self.ctx.token().1 != generation {
                // New Link: the page's token is old now.
                return End::Say("denied", 1008, "bad token");
            }
            let done = match self.read() {
                Ok(Some(frame)) if !self.holds() => {
                    // The watchdog dropped the claim; the connection learns it on its next frame.
                    let _ = frame;
                    Err(End::Say("evicted", 1000, ""))
                }
                Ok(Some(frame)) => match frame.opcode {
                    opcode::BINARY => self.input(&frame.payload),
                    opcode::TEXT => self.text(&frame.payload),
                    _ => Ok(()),
                },
                Ok(None) if self.idle() => {
                    if self.holds() {
                        Err(End::Close(1001, "idle"))
                    } else {
                        Err(End::Say("evicted", 1001, "idle"))
                    }
                }
                Ok(None) => Ok(()),
                Err(end) => Err(end),
            };
            if let Err(end) = done {
                return end;
            }
        }
    }

    /// Our close frame, then up to the linger for the peer's close, so the socket does not end
    /// in a reset that would eat the text before it.
    fn close(&mut self, code: u16, reason: &'static str) {
        if self
            .write(opcode::CLOSE, &close_payload(code, reason))
            .is_err()
        {
            return;
        }
        let until = Instant::now() + self.ctx.timers.linger;
        while Instant::now() < until {
            match self.next() {
                Read::Frame(frame) if frame.opcode == opcode::CLOSE => break,
                Read::Frame(_) | Read::Nothing => {}
                Read::End(_) => break,
            }
        }
    }

    fn finish(&mut self, end: End) {
        match end {
            End::Close(code, reason) => self.close(code, reason),
            End::Say(text, code, reason) => {
                if self.send(text.into()).is_ok() {
                    self.close(code, reason);
                }
            }
            End::Gone => {}
        }
    }
}

/// A WebSocket after its 101: `leftover` is what the client sent past the request head.
pub fn run(
    ctx: &Context,
    stream: Stream,
    leftover: Vec<u8>,
    connection: u64,
    peer: SocketAddr,
    device: Option<String>,
) -> Stream {
    let slot = ctx.slot.clone();
    let mut session = Session {
        ctx,
        slot: &slot,
        stream,
        buffer: leftover,
        connection,
        peer,
        last_frame: Instant::now(),
        last_ack: None,
    };
    let end = match session.authenticate() {
        Err(end) => end,
        Ok(generation) => {
            if let Some(device) = device {
                ctx.devices().insert(connection, device);
            }
            match session.claim() {
                Err(end) => end,
                Ok(()) => session.run(generation),
            }
        }
    };
    session.finish(end);
    slot.release(Owner::Browser(connection), Instant::now());
    ctx.devices().remove(&connection);
    session.stream
}
