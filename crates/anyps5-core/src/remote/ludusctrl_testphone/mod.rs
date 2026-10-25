//! A fake LudusCtrl phone for tests: the phone side of the DSLINK v2 wire protocol.
//!
//! Written from LudusCtrl's `docs/PROTOCOL.md` alone, not from the launcher's own receiver, so the
//! receiver is tested against the spec rather than against itself. The encoders and parsers here
//! follow the phone's rules from that document (`Protocol.decodeBeacon`, `PairingAttempt`,
//! `Protocol.decodeAck`); [`TestPhone`] speaks them over UDP, and [`conformance::run_conformance`]
//! walks a live receiver through the checks the spec asks of it. The golden vectors of the spec are
//! asserted in `tests.rs`.

pub mod conformance;
mod reference;
mod tests;

use std::{
    collections::VecDeque,
    fmt, io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket},
    thread,
    time::{Duration, Instant},
};

use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use p256::{PublicKey, SecretKey, elliptic_curve::sec1::ToSec1Point};
use sha2_0_11::{Digest, Sha256};

pub const INPUT_PORT: u16 = 45989;
pub const BEACON_PORT: u16 = 45990;
/// Byte 1 of every input, ack, PAIR_REQ and PAIR_RESP on UDP.
pub const PROTOCOL_VERSION: u8 = 2;
/// The beacon's own format version; it did not change with the input protocol.
pub const BEACON_VERSION: u8 = 1;

pub const INPUT_MAGIC: u8 = 0xD5;
pub const ACK_MAGIC: u8 = 0xD6;
pub const PAIR_REQ_MAGIC: u8 = 0xD7;
pub const PAIR_RESP_MAGIC: u8 = 0xD8;
pub const BEACON_MAGIC: &[u8; 4] = b"DSLK";

pub const INPUT_SIZE: usize = 36;
/// The input bytes the MAC covers (0..=27).
pub const INPUT_SIGNED: usize = 28;
/// The v1 body: bytes 0..=19 of the v2 packet, still sent by the browser client over WebSocket.
pub const INPUT_BODY_SIZE: usize = 20;
pub const ACK_SIZE: usize = 20;
/// An unsigned ack, and the bytes a signed ack's MAC covers.
pub const UNSIGNED_ACK_SIZE: usize = 12;
pub const PAIR_REQ_SIZE: usize = 128;
pub const PAIR_RESP_SIZE: usize = 84;
/// The PAIR_RESP bytes the MAC covers (0..=75).
pub const PAIR_RESP_SIGNED: usize = 76;
pub const BEACON_SIZE: usize = 16;
/// What a 1.1 PC sends: the beacon without the pc_id.
pub const BEACON_V11_SIZE: usize = 8;
pub const NAME_MAX: usize = 32;

/// The phone resends PAIR_REQ every second; the PC drops one that arrives less than a second
/// after the last from the same IP, so the harness keeps a little margin.
pub const PAIR_RESEND: Duration = Duration::from_millis(1050);
/// The PC's input watchdog.
pub const WATCHDOG_TIMEOUT: Duration = Duration::from_millis(1500);
/// The PC sends at most one signed ack per phone in this interval.
pub const ACK_INTERVAL: Duration = Duration::from_millis(200);
/// The phone only believes an ack for one of its last this many seqs.
pub const ACK_SEQ_WINDOW: usize = 256;

pub mod ack_flag {
    pub const PAD_LIVE: u8 = 0x01;
    pub const VERSION_MISMATCH: u8 = 0x02;
    pub const NOT_PAIRED: u8 = 0x04;
}

/// PAIR_RESP states.
pub mod pair_state {
    pub const WAITING: u8 = 0;
    pub const ACCEPTED: u8 = 1;
    pub const DENIED: u8 = 2;
    pub const BUSY: u8 = 3;
}

/// The layout byte: cosmetic only.
pub mod layout {
    pub const UNKNOWN: u8 = 0;
    pub const PLAYSTATION: u8 = 1;
    pub const XBOX: u8 = 2;
    pub const NINTENDO: u8 = 3;
}

/// The button mask, XInput's `wButtons`.
pub mod button {
    pub const DPAD_UP: u16 = 0x0001;
    pub const DPAD_DOWN: u16 = 0x0002;
    pub const DPAD_LEFT: u16 = 0x0004;
    pub const DPAD_RIGHT: u16 = 0x0008;
    pub const START: u16 = 0x0010;
    pub const BACK: u16 = 0x0020;
    pub const LEFT_THUMB: u16 = 0x0040;
    pub const RIGHT_THUMB: u16 = 0x0080;
    pub const LEFT_SHOULDER: u16 = 0x0100;
    pub const RIGHT_SHOULDER: u16 = 0x0200;
    pub const GUIDE: u16 = 0x0400;
    /// Never set by a phone; the PC masks it off.
    pub const RESERVED: u16 = 0x0800;
    pub const A: u16 = 0x1000;
    pub const B: u16 = 0x2000;
    pub const X: u16 = 0x4000;
    pub const Y: u16 = 0x8000;
    /// What the PC lets through to the virtual pad.
    pub const DEFINED: u16 = 0xF7FF;
}

const LINK_KEY_INFO: &[u8] = b"LudusCtrl v2 link key";
const CONFIRM_INFO: &[u8] = b"LudusCtrl v2 confirm";
const COMMIT_LABEL: &[u8] = b"LudusCtrl v2 commit";

/// An X9.62 uncompressed P-256 point, `0x04 || X || Y`.
pub type Point = [u8; 65];
/// K, the per-phone link key.
pub type Key = [u8; 32];
pub type Kid = [u8; 4];
pub type Tag = [u8; 8];
pub type Nonce = [u8; 16];
/// A pc_id or phone_id.
pub type Id = [u8; 8];

pub fn random<const N: usize>() -> [u8; N] {
    let mut bytes = [0; N];
    getrandom::fill(&mut bytes).expect("the system random source works");
    bytes
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// --- crypto ----------------------------------------------------------------------------------

/// The first 16 bytes of `SHA-256("LudusCtrl v2 commit" || nonce)`.
pub fn commitment(nonce: &Nonce) -> [u8; 16] {
    let digest = Sha256::new()
        .chain_update(COMMIT_LABEL)
        .chain_update(nonce)
        .finalize();
    digest[..16].try_into().unwrap()
}

/// The first 4 bytes of `SHA-256(K)`.
pub fn kid(key: &Key) -> Kid {
    Sha256::digest(key)[..4].try_into().unwrap()
}

fn hmac(key: &Key, data: &[u8]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(data);
    mac
}

/// HMAC-SHA256 under K, truncated to 8 bytes.
pub fn mac(key: &Key, data: &[u8]) -> Tag {
    hmac(key, data).finalize().into_bytes()[..8]
        .try_into()
        .unwrap()
}

/// Constant-time check of an 8-byte tag.
pub fn mac_ok(key: &Key, data: &[u8], tag: &[u8]) -> bool {
    tag.len() == 8 && hmac(key, data).verify_truncated_left(tag).is_ok()
}

pub fn hkdf(ikm: &[u8], salt: &[u8], info: &[u8], out: &mut [u8]) {
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info, out)
        .expect("HKDF output far below 255 blocks");
}

/// `"260 329"`, `"000 042"`.
pub fn format_code(code: u32) -> String {
    let digits = format!("{:06}", code % 1_000_000);
    format!("{} {}", &digits[..3], &digits[3..])
}

/// What both sides derive from one ECDH result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Derived {
    pub key: Key,
    pub kid: Kid,
    /// 0..999999, shown on both screens.
    pub code: u32,
}

/// K from `salt = phone_pub || pc_pub`, the code from `salt || nonce`.
pub fn derive(z: &[u8; 32], phone_pub: &Point, pc_pub: &Point, nonce: &Nonce) -> Derived {
    let mut salt = Vec::with_capacity(65 + 65 + 16);
    salt.extend_from_slice(phone_pub);
    salt.extend_from_slice(pc_pub);
    let mut key = [0; 32];
    hkdf(z, &salt, LINK_KEY_INFO, &mut key);
    salt.extend_from_slice(nonce);
    let mut code = [0; 4];
    hkdf(z, &salt, CONFIRM_INFO, &mut code);
    Derived {
        key,
        kid: kid(&key),
        code: u32::from_be_bytes(code) % 1_000_000,
    }
}

/// A peer's key: only the 65-byte `0x04` form of a point on the curve. Compressed, all-zero and
/// off-curve points are refused.
pub fn parse_point(raw: &[u8]) -> Option<PublicKey> {
    if raw.len() != 65 || raw[0] != 0x04 {
        return None;
    }
    PublicKey::from_sec1_bytes(raw).ok()
}

/// A P-256 key pair for one pairing attempt.
#[derive(Clone)]
pub struct EphemeralKey(SecretKey);

impl EphemeralKey {
    pub fn random() -> Self {
        loop {
            // A random 32-byte string is a valid scalar except with probability about 2^-32.
            if let Some(key) = Self::from_scalar(&random()) {
                return key;
            }
        }
    }

    /// A key with a chosen scalar, for the golden vectors. None for 0 or a scalar >= n.
    pub fn from_scalar(scalar: &[u8; 32]) -> Option<Self> {
        SecretKey::from_slice(scalar).ok().map(Self)
    }

    pub fn public(&self) -> Point {
        self.0
            .public_key()
            .to_sec1_point(false)
            .as_bytes()
            .try_into()
            .expect("an uncompressed P-256 point is 65 bytes")
    }

    /// Z, the x-coordinate of the shared point.
    pub fn agree(&self, peer: &PublicKey) -> [u8; 32] {
        let shared = p256::ecdh::diffie_hellman(self.0.to_nonzero_scalar(), peer.as_affine());
        shared.raw_secret_bytes().as_slice().try_into().unwrap()
    }
}

impl fmt::Debug for EphemeralKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EphemeralKey({})", hex(&self.public()))
    }
}

// --- beacon ----------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Beacon {
    pub input_port: u16,
    /// None from a 1.1 PC (8-byte beacon).
    pub pc_id: Option<Id>,
}

impl Beacon {
    /// The phone's reading: at least 8 bytes, `DSLK`, version 1 and a non-zero port; 16 bytes or
    /// more also carry the pc_id.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < BEACON_V11_SIZE || &data[..4] != BEACON_MAGIC || data[4] != BEACON_VERSION {
            return None;
        }
        let input_port = u16::from_le_bytes([data[5], data[6]]);
        if input_port == 0 {
            return None;
        }
        let pc_id = (data.len() >= BEACON_SIZE).then(|| data[8..16].try_into().unwrap());
        Some(Self { input_port, pc_id })
    }

    /// What a PC sends (8 bytes when it has no pc_id, as a 1.1 PC).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(BEACON_SIZE);
        out.extend_from_slice(BEACON_MAGIC);
        out.push(BEACON_VERSION);
        out.extend_from_slice(&self.input_port.to_le_bytes());
        out.push(0);
        if let Some(id) = self.pc_id {
            out.extend_from_slice(&id);
        }
        out
    }
}

/// A UDP socket that hears beacons on `port` (0 for any free one). Loopback only, like every socket
/// of the harness: the receiver under test sends its beacon to `127.0.0.1:<port>`, never a broadcast,
/// so no test opens a port on the LAN (or asks the firewall for one).
pub fn beacon_listener(port: u16) -> io::Result<UdpSocket> {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, port))
}

/// The next beacon on `socket` that advertises `input_port` (other PCs on the LAN are skipped),
/// with its source address.
pub fn wait_beacon(
    socket: &UdpSocket,
    input_port: u16,
    timeout: Duration,
) -> io::Result<Option<(Beacon, SocketAddr)>> {
    let deadline = Instant::now() + timeout;
    let mut buf = [0; 2048];
    loop {
        let Some(left) = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
        else {
            return Ok(None);
        };
        socket.set_read_timeout(Some(left))?;
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Some(beacon) = Beacon::parse(&buf[..n])
                    && beacon.input_port == input_port
                {
                    return Ok(Some((beacon, from)));
                }
            }
            Err(e) if is_timeout(&e) => return Ok(None),
            // Windows reports an ICMP port unreachable for an earlier send as a receive error.
            Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
            Err(e) => return Err(e),
        }
    }
}

/// Throws away every datagram already queued on `socket`.
pub fn drain(socket: &UdpSocket) -> io::Result<()> {
    socket.set_nonblocking(true)?;
    let mut buf = [0; 2048];
    loop {
        match socket.recv_from(&mut buf) {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
            Err(e) => {
                socket.set_nonblocking(false)?;
                return Err(e);
            }
        }
    }
    socket.set_nonblocking(false)
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

// --- pairing ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Commit = 0,
    Reveal = 1,
}

/// The device name as the phone sends it: UTF-8, cut at a character boundary to fit 32 bytes.
pub fn name_bytes(name: &str) -> Vec<u8> {
    let mut end = 0;
    for (start, ch) in name.char_indices() {
        let next = start + ch.len_utf8();
        if next > NAME_MAX {
            break;
        }
        end = next;
    }
    name.as_bytes()[..end].to_vec()
}

/// PAIR_REQ, 128 bytes. `name` is used as given (at most 32 bytes), `field` is the commitment in
/// the commit phase and the nonce in the reveal phase.
pub fn pair_request(
    phone_id: &Id,
    phone_pub: &Point,
    name: &[u8],
    phase: Phase,
    field: &[u8; 16],
) -> [u8; PAIR_REQ_SIZE] {
    let name = &name[..name.len().min(NAME_MAX)];
    let mut out = [0; PAIR_REQ_SIZE];
    out[0] = PAIR_REQ_MAGIC;
    out[1] = PROTOCOL_VERSION;
    out[2..10].copy_from_slice(phone_id);
    out[10..75].copy_from_slice(phone_pub);
    out[75] = name.len() as u8;
    out[76..76 + name.len()].copy_from_slice(name);
    out[108] = phase as u8;
    out[109..125].copy_from_slice(field);
    out
}

/// PAIR_RESP, 84 bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairResp {
    pub pc_id: Id,
    pub pc_pub: Point,
    pub state: u8,
    pub mac: Tag,
}

impl PairResp {
    /// What a PC sends: in state 1 the MAC over bytes 0..=75 under `key`, otherwise zero.
    pub fn new(pc_id: Id, pc_pub: Point, state: u8, key: Option<&Key>) -> Self {
        let mut resp = Self {
            pc_id,
            pc_pub,
            state,
            mac: [0; 8],
        };
        if let (pair_state::ACCEPTED, Some(key)) = (state, key) {
            resp.mac = mac(key, &resp.head());
        }
        resp
    }

    /// Exactly 84 bytes, magic `0xD8`, version 2.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() != PAIR_RESP_SIZE || data[0] != PAIR_RESP_MAGIC || data[1] != PROTOCOL_VERSION
        {
            return None;
        }
        Some(Self {
            pc_id: data[2..10].try_into().unwrap(),
            pc_pub: data[10..75].try_into().unwrap(),
            state: data[75],
            mac: data[76..84].try_into().unwrap(),
        })
    }

    pub fn head(&self) -> [u8; PAIR_RESP_SIGNED] {
        let mut out = [0; PAIR_RESP_SIGNED];
        out[0] = PAIR_RESP_MAGIC;
        out[1] = PROTOCOL_VERSION;
        out[2..10].copy_from_slice(&self.pc_id);
        out[10..75].copy_from_slice(&self.pc_pub);
        out[75] = self.state;
        out
    }

    pub fn encode(&self) -> [u8; PAIR_RESP_SIZE] {
        let mut out = [0; PAIR_RESP_SIZE];
        out[..PAIR_RESP_SIGNED].copy_from_slice(&self.head());
        out[PAIR_RESP_SIGNED..].copy_from_slice(&self.mac);
        out
    }
}

/// A finished pairing, as the phone stores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paired {
    pub pc_id: Id,
    pub key: Key,
    pub kid: Kid,
    pub code: u32,
}

/// What a PAIR_RESP meant to the attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairEvent {
    /// State 0: show this code and keep resending.
    Code(u32),
    /// State 1 with a MAC that verifies under K.
    Accepted(Paired),
    /// State 2: Deny, timeout, or no reveal in time.
    Declined,
    /// State 3 before any PC key was fixed.
    Busy,
}

/// The phone's side of one pairing attempt (`PairingAttempt` in the spec).
#[derive(Clone, Debug)]
pub struct PairingAttempt {
    pub phone_id: Id,
    name: Vec<u8>,
    key: EphemeralKey,
    phone_pub: Point,
    nonce: Nonce,
    /// The PC's key, fixed by the first answer that carried a valid one.
    pc_pub: Option<Point>,
    derived: Option<Derived>,
}

impl PairingAttempt {
    pub fn new(phone_id: Id, name: &str) -> Self {
        Self::with_secrets(phone_id, name, EphemeralKey::random(), random())
    }

    pub fn with_secrets(phone_id: Id, name: &str, key: EphemeralKey, nonce: Nonce) -> Self {
        Self {
            phone_id,
            name: name_bytes(name),
            phone_pub: key.public(),
            key,
            nonce,
            pc_pub: None,
            derived: None,
        }
    }

    pub fn phone_pub(&self) -> &Point {
        &self.phone_pub
    }

    pub fn nonce(&self) -> &Nonce {
        &self.nonce
    }

    pub fn pc_pub(&self) -> Option<&Point> {
        self.pc_pub.as_ref()
    }

    /// The code the phone shows, once the PC's key is fixed.
    pub fn code(&self) -> Option<u32> {
        self.derived.map(|d| d.code)
    }

    pub fn derived(&self) -> Option<Derived> {
        self.derived
    }

    /// The next PAIR_REQ: the commitment until the PC's key is known, the nonce from then on.
    pub fn request(&self) -> [u8; PAIR_REQ_SIZE] {
        match self.pc_pub {
            None => self.request_with(Phase::Commit, &commitment(&self.nonce)),
            Some(_) => self.request_with(Phase::Reveal, &self.nonce),
        }
    }

    /// A PAIR_REQ of this attempt with any phase and field, for building bad ones.
    pub fn request_with(&self, phase: Phase, field: &[u8; 16]) -> [u8; PAIR_REQ_SIZE] {
        pair_request(&self.phone_id, &self.phone_pub, &self.name, phase, field)
    }

    /// Feeds one datagram from the PC's port. None means it is to be ignored.
    pub fn on_response(&mut self, data: &[u8]) -> Option<PairEvent> {
        let resp = PairResp::parse(data)?;
        match resp.state {
            pair_state::WAITING | pair_state::ACCEPTED => {
                if resp.pc_pub == [0; 65] {
                    return None;
                }
                match self.pc_pub {
                    // First key wins; an invalid point fixes nothing.
                    None => {
                        let peer = parse_point(&resp.pc_pub)?;
                        let z = self.key.agree(&peer);
                        self.derived = Some(derive(&z, &self.phone_pub, &resp.pc_pub, &self.nonce));
                        self.pc_pub = Some(resp.pc_pub);
                    }
                    Some(known) if known != resp.pc_pub => return None,
                    Some(_) => {}
                }
                let derived = self.derived?;
                if resp.state == pair_state::WAITING {
                    return Some(PairEvent::Code(derived.code));
                }
                mac_ok(&derived.key, &resp.head(), &resp.mac).then_some(PairEvent::Accepted(
                    Paired {
                        pc_id: resp.pc_id,
                        key: derived.key,
                        kid: derived.kid,
                        code: derived.code,
                    },
                ))
            }
            pair_state::DENIED => match self.pc_pub {
                Some(known) if known != resp.pc_pub => None,
                _ => Some(PairEvent::Declined),
            },
            pair_state::BUSY => self.pc_pub.is_none().then_some(PairEvent::Busy),
            _ => None,
        }
    }
}

// --- input -----------------------------------------------------------------------------------

/// What the virtual pad receives: buttons, sticks (Y positive up) and triggers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadState {
    pub buttons: u16,
    pub left_x: i16,
    pub left_y: i16,
    pub right_x: i16,
    pub right_y: i16,
    pub left_trigger: u8,
    pub right_trigger: u8,
}

impl PadState {
    pub const NEUTRAL: Self = Self {
        buttons: 0,
        left_x: 0,
        left_y: 0,
        right_x: 0,
        right_y: 0,
        left_trigger: 0,
        right_trigger: 0,
    };

    /// What the PC writes for this state: bit 11 masked off.
    pub fn masked(self) -> Self {
        Self {
            buttons: self.buttons & button::DEFINED,
            ..self
        }
    }
}

/// What a receiver wrote to the launcher's virtual pad (`vpad::PadState`, which a `PadTarget` gets),
/// in the harness's terms: the same XInput state, field for field.
impl From<super::vpad::PadState> for PadState {
    fn from(state: super::vpad::PadState) -> Self {
        Self {
            buttons: state.buttons,
            left_x: state.lx,
            left_y: state.ly,
            right_x: state.rx,
            right_y: state.ry,
            left_trigger: state.left_trigger,
            right_trigger: state.right_trigger,
        }
    }
}

/// A stick axis in -1.0..=1.0 as the client encodes it: x32768 below zero, x32767 above, rounded
/// (ties towards positive infinity, as Kotlin's `roundToInt` and JavaScript's `Math.round`).
pub fn axis(value: f32) -> i16 {
    let v = value.clamp(-1.0, 1.0);
    let scaled = v * if v < 0.0 { 32768.0 } else { 32767.0 };
    (scaled + 0.5).floor().clamp(-32768.0, 32767.0) as i16
}

/// A trigger in 0.0..=1.0, x255, rounded.
pub fn trigger(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
}

/// One input snapshot, before the session, KID and MAC are added.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub seq: u32,
    pub pad: PadState,
    /// Flags bit 0: a physical pad is connected to the phone.
    pub pad_connected: bool,
    pub layout: u8,
}

impl Input {
    /// Bytes 0..=19 with the given version byte (2 on UDP, 1 for the browser body).
    pub fn body(&self, version: u8) -> [u8; INPUT_BODY_SIZE] {
        let p = &self.pad;
        let mut out = [0; INPUT_BODY_SIZE];
        out[0] = INPUT_MAGIC;
        out[1] = version;
        out[2..6].copy_from_slice(&self.seq.to_le_bytes());
        out[6..8].copy_from_slice(&p.buttons.to_le_bytes());
        out[8..10].copy_from_slice(&p.left_x.to_le_bytes());
        out[10..12].copy_from_slice(&p.left_y.to_le_bytes());
        out[12..14].copy_from_slice(&p.right_x.to_le_bytes());
        out[14..16].copy_from_slice(&p.right_y.to_le_bytes());
        out[16] = p.left_trigger;
        out[17] = p.right_trigger;
        out[18] = u8::from(self.pad_connected);
        out[19] = self.layout;
        out
    }

    /// The 36-byte v2 packet, MAC over bytes 0..=27 under `key`.
    pub fn encode(&self, session: u32, key: &Key, kid: &Kid) -> [u8; INPUT_SIZE] {
        let mut out = [0; INPUT_SIZE];
        out[..INPUT_BODY_SIZE].copy_from_slice(&self.body(PROTOCOL_VERSION));
        out[20..24].copy_from_slice(&session.to_le_bytes());
        out[24..28].copy_from_slice(kid);
        let tag = mac(key, &out[..INPUT_SIGNED]);
        out[INPUT_SIGNED..].copy_from_slice(&tag);
        out
    }

    /// The final packet of a link: all zero, flags 0.
    pub fn neutral(seq: u32) -> Self {
        Self {
            seq,
            ..Self::default()
        }
    }
}

// --- acks ------------------------------------------------------------------------------------

/// An ack as the phone reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ack {
    /// 20 bytes, version 2, MAC verified under K.
    Signed {
        seq: u32,
        received: u32,
        pad_live: bool,
        version_mismatch: bool,
    },
    /// Another version byte (any length), or a 12-byte v2 ack with bit 1.
    VersionMismatch { seq: u32, version: u8 },
    /// A 12-byte v2 ack with bit 2: the PC has no key for this KID.
    NotPaired { seq: u32 },
}

impl Ack {
    /// The phone's rules: at least 12 bytes and magic `0xD6`; a 20-byte v2 ack only if its MAC
    /// verifies under `key`; pad-live is never believed without one.
    pub fn parse(data: &[u8], key: Option<&Key>) -> Option<Self> {
        if data.len() < UNSIGNED_ACK_SIZE || data[0] != ACK_MAGIC {
            return None;
        }
        let version = data[1];
        let seq = u32::from_le_bytes(data[2..6].try_into().unwrap());
        let flags = data[10];
        if version != PROTOCOL_VERSION {
            return Some(Self::VersionMismatch { seq, version });
        }
        match data.len() {
            ACK_SIZE => {
                let key = key?;
                mac_ok(key, &data[..UNSIGNED_ACK_SIZE], &data[UNSIGNED_ACK_SIZE..]).then(|| {
                    Self::Signed {
                        seq,
                        received: u32::from_le_bytes(data[6..10].try_into().unwrap()),
                        pad_live: flags & ack_flag::PAD_LIVE != 0,
                        version_mismatch: flags & ack_flag::VERSION_MISMATCH != 0,
                    }
                })
            }
            UNSIGNED_ACK_SIZE if flags & ack_flag::NOT_PAIRED != 0 => Some(Self::NotPaired { seq }),
            UNSIGNED_ACK_SIZE if flags & ack_flag::VERSION_MISMATCH != 0 => {
                Some(Self::VersionMismatch { seq, version })
            }
            _ => None,
        }
    }

    pub fn seq(&self) -> u32 {
        match *self {
            Self::Signed { seq, .. }
            | Self::VersionMismatch { seq, .. }
            | Self::NotPaired { seq } => seq,
        }
    }

    /// What a PC sends for an accepted packet.
    pub fn encode_signed(seq: u32, received: u32, flags: u8, key: &Key) -> [u8; ACK_SIZE] {
        let head = Self::encode_unsigned(seq, received, flags);
        let mut out = [0; ACK_SIZE];
        out[..UNSIGNED_ACK_SIZE].copy_from_slice(&head);
        out[UNSIGNED_ACK_SIZE..].copy_from_slice(&mac(key, &head));
        out
    }

    /// The 12-byte head; on its own, the unsigned form (`received` is 0 there).
    pub fn encode_unsigned(seq: u32, received: u32, flags: u8) -> [u8; UNSIGNED_ACK_SIZE] {
        let mut out = [0; UNSIGNED_ACK_SIZE];
        out[0] = ACK_MAGIC;
        out[1] = PROTOCOL_VERSION;
        out[2..6].copy_from_slice(&seq.to_le_bytes());
        out[6..10].copy_from_slice(&received.to_le_bytes());
        out[10] = flags;
        out
    }
}

// --- the phone on a socket -------------------------------------------------------------------

#[derive(Debug)]
pub enum PairError {
    Io(io::Error),
    Declined,
    /// Nothing usable arrived before the deadline; the last event heard, if any.
    TimedOut(Option<PairEvent>),
}

impl fmt::Display for PairError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "socket error: {e}"),
            Self::Declined => f.write_str("the PC declined (state 2)"),
            Self::TimedOut(last) => write!(f, "timed out, last event {last:?}"),
        }
    }
}

impl std::error::Error for PairError {}

impl From<io::Error> for PairError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// A phone on a UDP socket, talking to one receiver.
///
/// Like the app it keeps a process-wide seq that starts at 0 and wraps, a session counter per
/// paired PC, and the last 256 seqs it sent, so it only believes acks for those.
#[derive(Debug)]
pub struct TestPhone {
    socket: UdpSocket,
    pc: SocketAddr,
    pub phone_id: Id,
    pub name: String,
    paired: Option<Paired>,
    session: u32,
    next_seq: u32,
    sent: VecDeque<u32>,
    last_pair_request: Option<Instant>,
}

impl TestPhone {
    /// A phone with a random phone_id, bound to loopback. `pc` must be a loopback address: the
    /// harness never opens a socket on the LAN.
    pub fn new(pc: SocketAddr) -> io::Result<Self> {
        Ok(Self {
            socket: bind_towards(pc)?,
            pc,
            phone_id: random(),
            name: "Conformance phone".into(),
            paired: None,
            session: 0,
            next_seq: 0,
            sent: VecDeque::with_capacity(ACK_SEQ_WINDOW),
            last_pair_request: None,
        })
    }

    pub fn pc(&self) -> SocketAddr {
        self.pc
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    /// Moves to a new source port, as after Wi-Fi roaming.
    pub fn rebind(&mut self) -> io::Result<()> {
        self.socket = bind_towards(self.pc)?;
        Ok(())
    }

    pub fn send_raw(&self, data: &[u8]) -> io::Result<()> {
        self.socket.send_to(data, self.pc).map(|_| ())
    }

    /// The next datagram from the PC's port within `timeout`. The source IP is not checked, as on
    /// the phone (a docked laptop may answer from its other address).
    pub fn recv(&self, timeout: Duration) -> io::Result<Option<Vec<u8>>> {
        let deadline = Instant::now() + timeout;
        let mut buf = [0; 2048];
        loop {
            let Some(left) = deadline
                .checked_duration_since(Instant::now())
                .filter(|d| !d.is_zero())
            else {
                return Ok(None);
            };
            self.socket.set_read_timeout(Some(left))?;
            match self.socket.recv_from(&mut buf) {
                Ok((n, from)) if from.port() == self.pc.port() => {
                    return Ok(Some(buf[..n].to_vec()));
                }
                Ok(_) => {}
                Err(e) if is_timeout(&e) => return Ok(None),
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
                Err(e) => return Err(e),
            }
        }
    }

    /// Sends a PAIR_REQ, first waiting out [`PAIR_RESEND`] since the last one so the PC's
    /// per-IP rate limit does not drop it.
    pub fn send_pair_request(&mut self, request: &[u8]) -> io::Result<()> {
        if let Some(last) = self.last_pair_request {
            let due = last + PAIR_RESEND;
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
        self.last_pair_request = Some(Instant::now());
        self.send_raw(request)
    }

    /// Sends `request` and returns the first PAIR_RESP that answers it within `timeout`, with what
    /// it meant to `attempt` (None: ignored by the phone's rules).
    pub fn exchange(
        &mut self,
        attempt: &mut PairingAttempt,
        request: &[u8],
        timeout: Duration,
    ) -> io::Result<Option<(PairResp, Option<PairEvent>)>> {
        self.send_pair_request(request)?;
        let deadline = Instant::now() + timeout;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Some(data) = self.recv(left)? else { break };
            if let Some(resp) = PairResp::parse(&data) {
                return Ok(Some((resp, attempt.on_response(&data))));
            }
        }
        Ok(None)
    }

    /// The whole pairing, as the app runs it: resend every second, `on_code` once when the code
    /// is first known (where a test clicks Allow), until state 1 or `deadline`.
    pub fn pair(
        &mut self,
        deadline: Duration,
        mut on_code: impl FnMut(u32),
    ) -> Result<Paired, PairError> {
        let mut attempt = PairingAttempt::new(self.phone_id, &self.name);
        let end = Instant::now() + deadline;
        let mut last = None;
        let mut told = false;
        while Instant::now() < end {
            let request = attempt.request();
            let Some((_, event)) = self.exchange(&mut attempt, &request, PAIR_RESEND)? else {
                continue;
            };
            last = event.or(last);
            match event {
                Some(PairEvent::Code(code)) if !told => {
                    told = true;
                    on_code(code);
                }
                Some(PairEvent::Accepted(paired)) => {
                    if !told {
                        on_code(paired.code);
                    }
                    self.set_paired(paired);
                    return Ok(paired);
                }
                Some(PairEvent::Declined) => return Err(PairError::Declined),
                _ => {}
            }
        }
        Err(PairError::TimedOut(last))
    }

    /// Stores a pairing: its session counter restarts at 0.
    pub fn set_paired(&mut self, paired: Paired) {
        self.paired = Some(paired);
        self.session = 0;
    }

    pub fn paired(&self) -> Option<&Paired> {
        self.paired.as_ref()
    }

    /// Takes `stored + 1`, as every new link does.
    pub fn start_session(&mut self) -> u32 {
        self.session = self.session.wrapping_add(1);
        self.session
    }

    pub fn session(&self) -> u32 {
        self.session
    }

    pub fn set_session(&mut self, session: u32) {
        self.session = session;
    }

    /// The seq the next [`TestPhone::input`] gets.
    pub fn next_seq(&self) -> u32 {
        self.next_seq
    }

    pub fn set_next_seq(&mut self, seq: u32) {
        self.next_seq = seq;
    }

    /// A snapshot with the next seq (a physical pad connected, Xbox layout).
    pub fn input(&mut self, pad: PadState) -> Input {
        let seq = self.next_seq;
        self.next_seq = seq.wrapping_add(1);
        Input {
            seq,
            pad,
            pad_connected: true,
            layout: layout::XBOX,
        }
    }

    /// `input` as a v2 packet of the current session under the paired key.
    pub fn packet(&self, input: &Input) -> [u8; INPUT_SIZE] {
        let paired = self.paired.as_ref().expect("the phone is paired");
        input.encode(self.session, &paired.key, &paired.kid)
    }

    /// Sends any datagram that carries `seq`, and remembers the seq so its ack is believed.
    pub fn send_with_seq(&mut self, seq: u32, data: &[u8]) -> io::Result<()> {
        if self.sent.len() == ACK_SEQ_WINDOW {
            self.sent.pop_front();
        }
        self.sent.push_back(seq);
        self.send_raw(data)
    }

    pub fn send_input(&mut self, input: &Input) -> io::Result<[u8; INPUT_SIZE]> {
        let packet = self.packet(input);
        self.send_with_seq(input.seq, &packet)?;
        Ok(packet)
    }

    /// Sends `pad` with the next seq; returns the packet's seq.
    pub fn send(&mut self, pad: PadState) -> io::Result<u32> {
        let input = self.input(pad);
        self.send_input(&input)?;
        Ok(input.seq)
    }

    /// The next ack within `timeout` that the phone believes: from the PC's port, parsed under
    /// the paired key, for one of the last 256 seqs sent.
    pub fn recv_ack(&mut self, timeout: Duration) -> io::Result<Option<Ack>> {
        let deadline = Instant::now() + timeout;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            let Some(data) = self.recv(left)? else { break };
            let key = self.paired.as_ref().map(|p| &p.key);
            if let Some(ack) = Ack::parse(&data, key)
                && self.sent.contains(&ack.seq())
            {
                return Ok(Some(ack));
            }
        }
        Ok(None)
    }

    /// The ack for `seq` within `timeout`; acks for other seqs are skipped.
    pub fn ack_for(&mut self, seq: u32, timeout: Duration) -> io::Result<Option<Ack>> {
        let deadline = Instant::now() + timeout;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.recv_ack(left)? {
                Some(ack) if ack.seq() == seq => return Ok(Some(ack)),
                Some(_) => {}
                None => break,
            }
        }
        Ok(None)
    }
}

fn bind_towards(pc: SocketAddr) -> io::Result<UdpSocket> {
    let local: IpAddr = match pc.ip() {
        IpAddr::V4(ip) if ip.is_loopback() => Ipv4Addr::LOCALHOST.into(),
        IpAddr::V6(ip) if ip.is_loopback() => Ipv6Addr::LOCALHOST.into(),
        other => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("the test phone only talks to loopback, not {other}"),
            ));
        }
    };
    UdpSocket::bind((local, 0))
}
