//! LudusCtrl's datagrams (DSLINK v2): the beacon, PAIR_REQ and PAIR_RESP, the 36-byte input packet
//! and the acks. Little-endian except the EC points; the golden hex is LudusCtrl's own. The phone's
//! side (encoding requests and input, reading answers) is only built for the tests.

#[cfg(test)]
use super::crypto::MAC_SIZE;
use super::crypto::{KID_SIZE, Kid, LinkKey, NONCE_SIZE, Point, mac};

pub const VERSION: u8 = 2;
pub const BEACON_VERSION: u8 = 1;

pub const INPUT_MAGIC: u8 = 0xD5;
pub const ACK_MAGIC: u8 = 0xD6;
pub const PAIR_REQ_MAGIC: u8 = 0xD7;
pub const PAIR_RESP_MAGIC: u8 = 0xD8;
pub const BEACON_MAGIC: &[u8; 4] = b"DSLK";

pub const INPUT_SIZE: usize = 36;
pub const SIGNED_ACK_SIZE: usize = 20;
pub const UNSIGNED_ACK_SIZE: usize = 12;
pub const PAIR_REQ_SIZE: usize = 128;
pub const PAIR_RESP_SIZE: usize = 84;
pub const BEACON_SIZE: usize = 16;

/// The bytes an input packet's MAC covers.
const INPUT_SIGNED: usize = 28;
/// The bytes a PAIR_RESP's MAC covers.
const RESP_SIGNED: usize = 76;
const ACK_SIGNED: usize = 12;

/// The longest device name a PAIR_REQ carries, in bytes.
pub const NAME_MAX: usize = 32;

/// Bit 11 is not a button; a client setting it cannot push it into the pad.
pub const BUTTON_MASK: u16 = 0xF7FF;

pub mod ack_flag {
    pub const PAD_LIVE: u8 = 0x01;
    pub const VERSION_MISMATCH: u8 = 0x02;
    pub const NOT_PAIRED: u8 = 0x04;
}

/// The flags bit of an input packet that says a controller is connected to the phone.
pub const FLAG_CONTROLLER: u8 = 0x01;

pub type Id = [u8; 8];

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn i16_at(b: &[u8], at: usize) -> i16 {
    i16::from_le_bytes([b[at], b[at + 1]])
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

/// The PC's announcement: the input port and the PC's id. The phone takes the host from the
/// datagram's source address.
pub fn beacon(input_port: u16, pc_id: &Id) -> [u8; BEACON_SIZE] {
    let mut b = [0u8; BEACON_SIZE];
    b[..4].copy_from_slice(BEACON_MAGIC);
    b[4] = BEACON_VERSION;
    b[5..7].copy_from_slice(&input_port.to_le_bytes());
    b[8..].copy_from_slice(pc_id);
    b
}

/// What a phone reads from a beacon: the port and, from a 16-byte one, the PC's id.
#[cfg(test)]
pub fn read_beacon(b: &[u8]) -> Option<(u16, Option<Id>)> {
    if b.len() < 8 || &b[..4] != BEACON_MAGIC || b[4] != BEACON_VERSION {
        return None;
    }
    let port = u16_at(b, 5);
    if port == 0 {
        return None;
    }
    let id = (b.len() >= BEACON_SIZE).then(|| b[8..16].try_into().expect("8 bytes"));
    Some((port, id))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// The field is the commitment to the nonce.
    Commit,
    /// The field is the nonce itself.
    Reveal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairRequest {
    pub phone_id: Id,
    pub phone_pub: Point,
    /// The raw name bytes (`name_len` of them), not yet decoded.
    pub name: Vec<u8>,
    pub phase: Phase,
    pub field: [u8; NONCE_SIZE],
}

/// Size, magic and version: the first check, before the rate limit.
pub fn is_pair_request(b: &[u8]) -> bool {
    b.len() == PAIR_REQ_SIZE && b[0] == PAIR_REQ_MAGIC && b[1] == VERSION
}

impl PairRequest {
    /// The fields, once [`is_pair_request`] passed: the name length, the phase and the point's
    /// first byte are checked. The point itself is checked only for a new attempt.
    pub fn parse(b: &[u8]) -> Option<PairRequest> {
        if !is_pair_request(b) {
            return None;
        }
        let name_len = usize::from(b[75]);
        let phase = match b[108] {
            0 => Phase::Commit,
            1 => Phase::Reveal,
            _ => return None,
        };
        if name_len > NAME_MAX || b[10] != 0x04 {
            return None;
        }
        Some(PairRequest {
            phone_id: b[2..10].try_into().expect("8 bytes"),
            phone_pub: b[10..75].try_into().expect("65 bytes"),
            name: b[76..76 + name_len].to_vec(),
            phase,
            field: b[109..125].try_into().expect("16 bytes"),
        })
    }

    #[cfg(test)]
    pub fn encode(&self) -> [u8; PAIR_REQ_SIZE] {
        let mut b = [0u8; PAIR_REQ_SIZE];
        b[0] = PAIR_REQ_MAGIC;
        b[1] = VERSION;
        b[2..10].copy_from_slice(&self.phone_id);
        b[10..75].copy_from_slice(&self.phone_pub);
        let name = &self.name[..self.name.len().min(NAME_MAX)];
        b[75] = name.len() as u8;
        b[76..76 + name.len()].copy_from_slice(name);
        b[108] = match self.phase {
            Phase::Commit => 0,
            Phase::Reveal => 1,
        };
        b[109..125].copy_from_slice(&self.field);
        b
    }
}

/// The answer to a PAIR_REQ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairState {
    Waiting = 0,
    Accepted = 1,
    Denied = 2,
    Busy = 3,
}

impl PairState {
    #[cfg(test)]
    pub fn from_byte(byte: u8) -> Option<PairState> {
        Some(match byte {
            0 => PairState::Waiting,
            1 => PairState::Accepted,
            2 => PairState::Denied,
            3 => PairState::Busy,
            _ => return None,
        })
    }
}

/// A PAIR_RESP. `key` signs it, which only state 1 carries; Busy sends no key at all.
pub fn pair_response(
    pc_id: &Id,
    pc_pub: Option<&Point>,
    state: PairState,
    key: Option<&LinkKey>,
) -> [u8; PAIR_RESP_SIZE] {
    let mut b = [0u8; PAIR_RESP_SIZE];
    b[0] = PAIR_RESP_MAGIC;
    b[1] = VERSION;
    b[2..10].copy_from_slice(pc_id);
    if let Some(public) = pc_pub {
        b[10..75].copy_from_slice(public);
    }
    b[75] = state as u8;
    if let (PairState::Accepted, Some(key)) = (state, key) {
        let tag = mac(key, &b[..RESP_SIGNED]);
        b[RESP_SIGNED..].copy_from_slice(&tag);
    }
    b
}

/// A PAIR_RESP as a phone reads it: the PC's id, its key, the state and the MAC bytes.
#[cfg(test)]
pub fn read_pair_response(b: &[u8]) -> Option<(Id, Point, PairState, [u8; MAC_SIZE])> {
    if b.len() != PAIR_RESP_SIZE || b[0] != PAIR_RESP_MAGIC || b[1] != VERSION {
        return None;
    }
    Some((
        b[2..10].try_into().expect("8 bytes"),
        b[10..75].try_into().expect("65 bytes"),
        PairState::from_byte(b[75])?,
        b[76..84].try_into().expect("8 bytes"),
    ))
}

/// The signed part of a PAIR_RESP, for the phone's check.
#[cfg(test)]
pub fn pair_response_signed(b: &[u8; PAIR_RESP_SIZE]) -> &[u8] {
    &b[..RESP_SIGNED]
}

/// What a controller on the phone is. Cosmetic: it never changes what a bit means.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PadLayout {
    #[default]
    Unknown,
    PlayStation,
    Xbox,
    Nintendo,
}

impl PadLayout {
    pub fn from_byte(byte: u8) -> PadLayout {
        match byte {
            1 => PadLayout::PlayStation,
            2 => PadLayout::Xbox,
            3 => PadLayout::Nintendo,
            _ => PadLayout::Unknown,
        }
    }

    pub fn byte(self) -> u8 {
        match self {
            PadLayout::Unknown => 0,
            PadLayout::PlayStation => 1,
            PadLayout::Xbox => 2,
            PadLayout::Nintendo => 3,
        }
    }
}

/// One input packet: a full snapshot of the phone's controller. Sticks are signed with up
/// positive and the buttons are XInput's, so the virtual pad takes them as they are.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub seq: u32,
    pub buttons: u16,
    pub lx: i16,
    pub ly: i16,
    pub rx: i16,
    pub ry: i16,
    pub left_trigger: u8,
    pub right_trigger: u8,
    pub flags: u8,
    pub layout: PadLayout,
    pub session: u32,
}

impl Input {
    /// The fields of a 36-byte packet, with its KID and MAC; nothing is verified here.
    pub fn parse(b: &[u8]) -> Option<(Input, Kid, &[u8])> {
        if b.len() != INPUT_SIZE || b[0] != INPUT_MAGIC || b[1] != VERSION {
            return None;
        }
        let input = Input {
            seq: u32_at(b, 2),
            buttons: u16_at(b, 6),
            lx: i16_at(b, 8),
            ly: i16_at(b, 10),
            rx: i16_at(b, 12),
            ry: i16_at(b, 14),
            left_trigger: b[16],
            right_trigger: b[17],
            flags: b[18],
            layout: PadLayout::from_byte(b[19]),
            session: u32_at(b, 20),
        };
        let kid: Kid = b[24..24 + KID_SIZE].try_into().expect("4 bytes");
        Some((input, kid, &b[INPUT_SIGNED..]))
    }

    /// The signed part of a packet that [`Input::parse`] accepted.
    pub fn signed(b: &[u8]) -> &[u8] {
        &b[..INPUT_SIGNED]
    }

    /// The 20-byte body (bytes 0..19), the same as the v1 packet with version 2.
    #[cfg(test)]
    pub fn body(&self) -> [u8; 20] {
        let mut b = [0u8; 20];
        b[0] = INPUT_MAGIC;
        b[1] = VERSION;
        b[2..6].copy_from_slice(&self.seq.to_le_bytes());
        b[6..8].copy_from_slice(&self.buttons.to_le_bytes());
        b[8..10].copy_from_slice(&self.lx.to_le_bytes());
        b[10..12].copy_from_slice(&self.ly.to_le_bytes());
        b[12..14].copy_from_slice(&self.rx.to_le_bytes());
        b[14..16].copy_from_slice(&self.ry.to_le_bytes());
        b[16] = self.left_trigger;
        b[17] = self.right_trigger;
        b[18] = self.flags;
        b[19] = self.layout.byte();
        b
    }

    /// The packet a phone sends, signed under `key`.
    #[cfg(test)]
    pub fn encode(&self, kid: &Kid, key: &LinkKey) -> [u8; INPUT_SIZE] {
        let mut b = [0u8; INPUT_SIZE];
        b[..20].copy_from_slice(&self.body());
        b[20..24].copy_from_slice(&self.session.to_le_bytes());
        b[24..28].copy_from_slice(kid);
        let tag = mac(key, &b[..INPUT_SIGNED]);
        b[INPUT_SIGNED..].copy_from_slice(&tag);
        b
    }
}

/// The 12-byte head of every ack.
fn ack_head(seq: u32, received: u32, flags: u8) -> [u8; ACK_SIGNED] {
    let mut b = [0u8; ACK_SIGNED];
    b[0] = ACK_MAGIC;
    b[1] = VERSION;
    b[2..6].copy_from_slice(&seq.to_le_bytes());
    b[6..10].copy_from_slice(&received.to_le_bytes());
    b[10] = flags;
    b
}

pub fn signed_ack(seq: u32, received: u32, flags: u8, key: &LinkKey) -> [u8; SIGNED_ACK_SIZE] {
    let mut b = [0u8; SIGNED_ACK_SIZE];
    let head = ack_head(seq, received, flags);
    b[..ACK_SIGNED].copy_from_slice(&head);
    b[ACK_SIGNED..].copy_from_slice(&mac(key, &head));
    b
}

/// Version mismatch or not paired: there is no key to sign with.
pub fn unsigned_ack(seq: u32, flags: u8) -> [u8; UNSIGNED_ACK_SIZE] {
    ack_head(seq, 0, flags)
}

/// An ack as the phone reads it: seq, received, flags and, for the signed form, the MAC.
#[cfg(test)]
pub fn read_ack(b: &[u8]) -> Option<(u32, u32, u8, Option<[u8; MAC_SIZE]>)> {
    if b.len() < UNSIGNED_ACK_SIZE || b[0] != ACK_MAGIC {
        return None;
    }
    let tag = (b.len() == SIGNED_ACK_SIZE).then(|| b[12..20].try_into().expect("8 bytes"));
    Some((u32_at(b, 2), u32_at(b, 6), b[10], tag))
}

/// The signed head of an ack.
#[cfg(test)]
pub fn ack_signed(b: &[u8]) -> &[u8] {
    &b[..ACK_SIGNED]
}

#[cfg(test)]
mod tests {
    use super::super::crypto::{commitment, kid, mac_ok, tests::*};
    use super::*;

    const PHONE_ID: Id = [1, 2, 3, 4, 5, 6, 7, 8];
    const PC_ID: Id = [0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8];

    fn request(phase: Phase) -> PairRequest {
        PairRequest {
            phone_id: PHONE_ID,
            phone_pub: phone().public,
            name: b"Pixel 8".to_vec(),
            phase,
            field: match phase {
                Phase::Commit => commitment(&NONCE),
                Phase::Reveal => NONCE,
            },
        }
    }

    #[test]
    fn pair_requests_match_the_golden_hex() {
        let commit = concat!(
            "d7020102030405060708040217e617f0b6443928278f96999e69a23a4f2c152b",
            "df6d6cdf66e5b80282d4ed194a7debcb97712d2dda3ca85aa8765a56f45fc75859",
            "9652f2897c65306e579407506978656c2038000000000000000000000000000000",
            "0000000000000000000000d06ad9ddcb46f53c44d669f7d08fd932000000"
        );
        let reveal = concat!(
            "d7020102030405060708040217e617f0b6443928278f96999e69a23a4f2c152b",
            "df6d6cdf66e5b80282d4ed194a7debcb97712d2dda3ca85aa8765a56f45fc75859",
            "9652f2897c65306e579407506978656c2038000000000000000000000000000000",
            "0000000000000000000001000102030405060708090a0b0c0d0e0f000000"
        );
        for (phase, golden) in [(Phase::Commit, commit), (Phase::Reveal, reveal)] {
            let bytes = request(phase).encode();
            assert_eq!(hex(&bytes), golden);
            assert_eq!(PairRequest::parse(&bytes), Some(request(phase)));
        }
    }

    #[test]
    fn malformed_pair_requests_are_refused() {
        let good = request(Phase::Commit).encode();
        assert!(PairRequest::parse(&good[..127]).is_none());
        let mut long = good.to_vec();
        long.push(0);
        assert!(PairRequest::parse(&long).is_none());
        for (at, value) in [(0, 0xD5), (1, 1), (75, 33), (108, 2), (10, 0x02)] {
            let mut bad = good;
            bad[at] = value;
            assert!(PairRequest::parse(&bad).is_none(), "byte {at} = {value}");
        }
        // The reserved bytes are not checked.
        let mut reserved = good;
        reserved[127] = 9;
        assert!(PairRequest::parse(&reserved).is_some());
    }

    #[test]
    fn pair_responses_match_the_golden_hex() {
        let head = concat!(
            "d802a1a2a3a4a5a6a7a804d65a93977caa3d1b081852ff57a79e465f1660577304ba",
            "ead505dd3a48589cf350185e895372df6221ea3a137557e473fddb6755f05bd507c3",
            "c533fce9c91285"
        );
        let key = golden_key();
        let accepted = pair_response(&PC_ID, Some(&pc().public), PairState::Accepted, Some(&key));
        assert_eq!(hex(&accepted), format!("{head}01f1980601122fd826"));
        let (id, public, state, tag) = read_pair_response(&accepted).unwrap();
        assert_eq!(
            (id, public, state),
            (PC_ID, pc().public, PairState::Accepted)
        );
        assert!(mac_ok(&key, pair_response_signed(&accepted), &tag));
        let waiting = pair_response(&PC_ID, Some(&pc().public), PairState::Waiting, Some(&key));
        assert_eq!(hex(&waiting), format!("{head}00{}", "00".repeat(8)));
        let busy = pair_response(&PC_ID, None, PairState::Busy, None);
        assert_eq!(&busy[10..75], &[0u8; 65]);
        assert_eq!(busy[75], 3);
    }

    fn golden_input() -> Input {
        Input {
            seq: 0x1234_5678,
            buttons: 0x1000,
            lx: -32768,
            ly: 32767,
            rx: 12345,
            ry: -9876,
            left_trigger: 255,
            right_trigger: 128,
            flags: 1,
            layout: PadLayout::Xbox,
            session: 7,
        }
    }

    #[test]
    fn the_input_packet_matches_the_golden_hex() {
        let key = golden_key();
        let bytes = golden_input().encode(&kid(&key), &key);
        assert_eq!(
            hex(&bytes),
            "d5027856341200100080ff7f39306cd9ff8001020700000010253095bee0eb8534c2d722"
        );
        let (input, packet_kid, tag) = Input::parse(&bytes).unwrap();
        assert_eq!(input, golden_input());
        assert_eq!(packet_kid, kid(&key));
        assert!(mac_ok(&key, Input::signed(&bytes), tag));
    }

    #[test]
    fn input_bodies_match_the_golden_hex() {
        let cases = [
            (Input::default(), "d502000000000000000000000000000000000000"),
            (
                Input {
                    seq: 1,
                    buttons: 0x1001,
                    lx: 32767,
                    ly: -32768,
                    ry: -1,
                    left_trigger: 255,
                    flags: 1,
                    layout: PadLayout::PlayStation,
                    ..Input::default()
                },
                "d502010000000110ff7f00800000ffffff000101",
            ),
            (
                Input {
                    seq: 0x1234_5678,
                    buttons: 0xF7FF,
                    lx: 1234,
                    ly: -1234,
                    rx: -16384,
                    ry: 16384,
                    left_trigger: 128,
                    right_trigger: 64,
                    flags: 1,
                    layout: PadLayout::Xbox,
                    session: 0,
                },
                "d50278563412fff7d2042efb00c0004080400102",
            ),
            (
                Input {
                    seq: 0xFFFF_FFFF,
                    buttons: 0x8000,
                    right_trigger: 255,
                    layout: PadLayout::Nintendo,
                    ..Input::default()
                },
                "d502ffffffff0080000000000000000000ff0003",
            ),
        ];
        for (input, golden) in cases {
            assert_eq!(hex(&input.body()), golden);
        }
        let key = golden_key();
        let bytes = Input {
            session: u32::MAX,
            ..Input::default()
        }
        .encode(&kid(&key), &key);
        assert_eq!(hex(&bytes[20..24]), "ffffffff");
    }

    #[test]
    fn wrong_inputs_do_not_parse() {
        let key = golden_key();
        let good = golden_input().encode(&kid(&key), &key);
        assert!(Input::parse(&good[..35]).is_none());
        let mut v1 = good;
        v1[1] = 1;
        assert!(Input::parse(&v1).is_none());
        assert!(Input::parse(&[0u8; 36]).is_none());
    }

    #[test]
    fn acks_match_the_golden_hex() {
        let key = golden_key();
        let signed = signed_ack(0x1234_5678, 1000, ack_flag::PAD_LIVE, &key);
        assert_eq!(hex(&signed), "d60278563412e80300000100099521f0c16e1df7");
        let (seq, received, flags, tag) = read_ack(&signed).unwrap();
        assert_eq!((seq, received, flags), (0x1234_5678, 1000, 1));
        assert!(mac_ok(&key, ack_signed(&signed), &tag.unwrap()));
        assert_eq!(
            hex(&unsigned_ack(0x1234_5678, ack_flag::VERSION_MISMATCH)),
            "d60278563412000000000200"
        );
        assert_eq!(
            hex(&unsigned_ack(7, ack_flag::NOT_PAIRED)),
            "d60207000000000000000400"
        );
        assert_eq!(read_ack(&unsigned_ack(7, 4)).unwrap().3, None);
    }

    #[test]
    fn the_beacon_matches_the_golden_hex() {
        let bytes = beacon(45989, &PC_ID);
        assert_eq!(hex(&bytes), "44534c4b01a5b300a1a2a3a4a5a6a7a8");
        assert_eq!(read_beacon(&bytes), Some((45989, Some(PC_ID))));
        assert_eq!(read_beacon(&bytes[..8]), Some((45989, None)), "a 1.1 PC");
        assert_eq!(read_beacon(&bytes[..7]), None);
        let mut no_port = bytes;
        no_port[5..7].copy_from_slice(&[0, 0]);
        assert_eq!(read_beacon(&no_port), None);
    }
}
