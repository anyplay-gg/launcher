//! The phone protocol: the JSON control messages and the binary pad packets.
//!
//! Wire-compatible with the macOS launcher's `RemoteProtocol.swift` (utkuhalis/AnyPS5-ARM), which the
//! phone app speaks. Control messages are one JSON object per WebSocket text frame, tagged by `type`.
//! Pad packets are fixed little-endian UDP datagrams; all three magics read as ASCII in memory.

use serde::{Deserialize, Serialize};

/// The only protocol version the phone sends in `hello`.
pub const PROTOCOL_VERSION: u32 = 1;

/// One game as the phone's library list shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteGame {
    pub id: String,
    pub name: String,
    /// `installed`, `update`, `locked`, `installing`, `starting`, `running` or `unavailable`.
    pub state: String,
    /// A display string such as `12/40`.
    pub trophies: Option<String>,
    /// An opaque tag that changes when the pictures change.
    pub art: Option<String>,
    /// The phone's own pad layout for this game, as the JSON text it sent.
    pub layout: Option<String>,
}

/// A control message, in either direction.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Message {
    /// Phone: first message; the only one accepted before the welcome.
    Hello {
        protocol: u32,
        #[serde(rename = "deviceID")]
        device_id: String,
        device_name: String,
        #[serde(default)]
        key: Option<String>,
    },
    /// Launcher: the 4-digit code the phone shows next to the pairing card.
    Pairing {
        code: String,
    },
    /// Launcher: accepted. `session` is a decimal u64 string, the UDP capability.
    Welcome {
        session: String,
        udp_port: u16,
        /// The phone shows this as the PC's name (the field kept its macOS name).
        mac_name: String,
        key: String,
    },
    /// Launcher: refused (`protocol`, `busy`, `denied`); the socket closes after it.
    Rejected {
        reason: String,
    },
    /// Phone: measured one-way latency.
    Latency {
        milliseconds: f64,
    },
    /// Launcher: the library, sent after the welcome and on every change.
    Library {
        games: Vec<RemoteGame>,
        selected: Option<String>,
        running: Option<String>,
        language: String,
    },
    /// Phone: asks for the pictures of a game.
    WantArt {
        id: String,
    },
    /// Launcher: base64 JPEGs.
    Art {
        id: String,
        icon: Option<String>,
        background: Option<String>,
    },
    /// Phone: a launcher button (`left right up down select back options l1 r1`).
    Command {
        name: String,
    },
    Select {
        id: String,
    },
    Launch {
        id: String,
    },
    Close,
    /// Phone: the PS button.
    Home,
    SaveLayout {
        id: String,
        layout: Option<String>,
    },
    /// Launcher: a trophy was earned.
    Trophy {
        name: String,
        detail: String,
        grade: String,
        icon: Option<String>,
    },
}

impl Message {
    /// Parses one frame. Unknown types and malformed JSON give `None`: both sides ignore them.
    pub fn parse(text: &str) -> Option<Message> {
        serde_json::from_str(text).ok()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("messages serialise")
    }
}

pub const INPUT_MAGIC: u32 = 0x4952_5041;
pub const OUTPUT_MAGIC: u32 = 0x4F52_5041;
pub const HELLO_MAGIC: u32 = 0x4852_5041;
pub const INPUT_SIZE: usize = 80;
pub const OUTPUT_SIZE: usize = 32;
pub const HELLO_SIZE: usize = 16;

/// Button bits of the input packet (libScePad). There is no PS bit: the PS button is `Home`.
pub mod button {
    pub const L3: u32 = 0x2;
    pub const R3: u32 = 0x4;
    pub const OPTIONS: u32 = 0x8;
    pub const UP: u32 = 0x10;
    pub const RIGHT: u32 = 0x20;
    pub const DOWN: u32 = 0x40;
    pub const LEFT: u32 = 0x80;
    pub const L2: u32 = 0x100;
    pub const R2: u32 = 0x200;
    pub const L1: u32 = 0x400;
    pub const R1: u32 = 0x800;
    pub const TRIANGLE: u32 = 0x1000;
    pub const CIRCLE: u32 = 0x2000;
    pub const CROSS: u32 = 0x4000;
    pub const SQUARE: u32 = 0x8000;
    pub const TOUCHPAD: u32 = 0x10_0000;
    pub const ALL: u32 = L3
        | R3
        | OPTIONS
        | UP
        | RIGHT
        | DOWN
        | LEFT
        | L2
        | R2
        | L1
        | R1
        | TRIANGLE
        | CIRCLE
        | CROSS
        | SQUARE
        | TOUCHPAD;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchPoint {
    pub active: bool,
    pub id: u8,
    pub x: u16,
    pub y: u16,
}

/// One 80-byte input packet from the phone: the full pad state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputPacket {
    pub sequence: u32,
    pub has_motion: bool,
    pub sent_micros: u64,
    pub session: u64,
    pub buttons: u32,
    /// LX, LY, RX, RY: 0 to 255, 128 centre, 0 is up or left.
    pub sticks: [u8; 4],
    pub l2: u8,
    pub r2: u8,
    pub touch: [TouchPoint; 2],
    pub accel: [f32; 3],
    pub gyro: [f32; 3],
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().expect("8 bytes"))
}
fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().expect("4 bytes"))
}

fn header_ok(b: &[u8], size: usize, magic: u32) -> bool {
    b.len() == size
        && u32_at(b, 0) == magic
        && u16_at(b, 4) == 1
        && usize::from(u16_at(b, 6)) == size
}

impl InputPacket {
    /// Validates like the engine does: size, magic, version, size field, touch bounds, finite
    /// floats. Unknown button bits are masked off.
    pub fn parse(b: &[u8]) -> Option<InputPacket> {
        if !header_ok(b, INPUT_SIZE, INPUT_MAGIC) {
            return None;
        }
        let touch_at = |at: usize| {
            let point = TouchPoint {
                active: b[at] != 0,
                id: b[at + 1],
                x: u16_at(b, at + 2),
                y: u16_at(b, at + 4),
            };
            (point.x < 1920 && point.y < 1080).then_some(point)
        };
        let mut accel = [0.0; 3];
        let mut gyro = [0.0; 3];
        for i in 0..3 {
            accel[i] = f32_at(b, 56 + 4 * i);
            gyro[i] = f32_at(b, 68 + 4 * i);
            if !accel[i].is_finite() || !gyro[i].is_finite() {
                return None;
            }
        }
        Some(InputPacket {
            sequence: u32_at(b, 8),
            has_motion: u32_at(b, 12) & 1 != 0,
            sent_micros: u64_at(b, 16),
            session: u64_at(b, 24),
            buttons: u32_at(b, 32) & button::ALL,
            sticks: [b[36], b[37], b[38], b[39]],
            l2: b[40],
            r2: b[41],
            touch: [touch_at(44)?, touch_at(50)?],
            accel,
            gyro,
        })
    }

    pub fn encode(&self) -> [u8; INPUT_SIZE] {
        let mut b = [0u8; INPUT_SIZE];
        b[0..4].copy_from_slice(&INPUT_MAGIC.to_le_bytes());
        b[4..6].copy_from_slice(&1u16.to_le_bytes());
        b[6..8].copy_from_slice(&(INPUT_SIZE as u16).to_le_bytes());
        b[8..12].copy_from_slice(&self.sequence.to_le_bytes());
        b[12..16].copy_from_slice(&u32::from(self.has_motion).to_le_bytes());
        b[16..24].copy_from_slice(&self.sent_micros.to_le_bytes());
        b[24..32].copy_from_slice(&self.session.to_le_bytes());
        b[32..36].copy_from_slice(&self.buttons.to_le_bytes());
        b[36..40].copy_from_slice(&self.sticks);
        b[40] = self.l2;
        b[41] = self.r2;
        for (n, point) in self.touch.iter().enumerate() {
            let at = 44 + 6 * n;
            b[at] = u8::from(point.active);
            b[at + 1] = point.id;
            b[at + 2..at + 4].copy_from_slice(&point.x.to_le_bytes());
            b[at + 4..at + 6].copy_from_slice(&point.y.to_le_bytes());
        }
        for i in 0..3 {
            b[56 + 4 * i..60 + 4 * i].copy_from_slice(&self.accel[i].to_le_bytes());
            b[68 + 4 * i..72 + 4 * i].copy_from_slice(&self.gyro[i].to_le_bytes());
        }
        b
    }

    /// A pad at rest: sticks centred, nothing pressed, gravity on the accelerometer.
    pub fn neutral(session: u64) -> InputPacket {
        InputPacket {
            sequence: 0,
            has_motion: false,
            sent_micros: 0,
            session,
            buttons: 0,
            sticks: [128; 4],
            l2: 0,
            r2: 0,
            touch: [TouchPoint::default(); 2],
            accel: [0.0, 9.80665, 0.0],
            gyro: [0.0; 3],
        }
    }
}

/// The session id of a datagram that passes the input header check, for the relay's cheap filter.
pub fn input_session(b: &[u8]) -> Option<u64> {
    header_ok(b, INPUT_SIZE, INPUT_MAGIC).then(|| u64_at(b, 24))
}

/// One 32-byte output packet from the game: feedback and the latency echo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputPacket {
    pub echo_sequence: u32,
    pub wait_micros: u32,
    pub echo_sent_micros: u64,
    pub large_motor: u8,
    pub small_motor: u8,
    pub light_valid: bool,
    pub light: [u8; 3],
    pub l2_effect: u8,
    pub r2_effect: u8,
}

impl OutputPacket {
    pub fn parse(b: &[u8]) -> Option<OutputPacket> {
        if !header_ok(b, OUTPUT_SIZE, OUTPUT_MAGIC) {
            return None;
        }
        Some(OutputPacket {
            echo_sequence: u32_at(b, 8),
            wait_micros: u32_at(b, 12),
            echo_sent_micros: u64_at(b, 16),
            large_motor: b[24],
            small_motor: b[25],
            light_valid: b[26] != 0,
            light: [b[27], b[28], b[29]],
            l2_effect: b[30],
            r2_effect: b[31],
        })
    }

    pub fn encode(&self) -> [u8; OUTPUT_SIZE] {
        let mut b = [0u8; OUTPUT_SIZE];
        b[0..4].copy_from_slice(&OUTPUT_MAGIC.to_le_bytes());
        b[4..6].copy_from_slice(&1u16.to_le_bytes());
        b[6..8].copy_from_slice(&(OUTPUT_SIZE as u16).to_le_bytes());
        b[8..12].copy_from_slice(&self.echo_sequence.to_le_bytes());
        b[12..16].copy_from_slice(&self.wait_micros.to_le_bytes());
        b[16..24].copy_from_slice(&self.echo_sent_micros.to_le_bytes());
        b[24] = self.large_motor;
        b[25] = self.small_motor;
        b[26] = u8::from(self.light_valid);
        b[27..30].copy_from_slice(&self.light);
        b[30] = self.l2_effect;
        b[31] = self.r2_effect;
        b
    }
}

/// The 16-byte hello a game sends to the bridge once a second.
pub fn game_hello() -> [u8; HELLO_SIZE] {
    let mut b = [0u8; HELLO_SIZE];
    b[0..4].copy_from_slice(&HELLO_MAGIC.to_le_bytes());
    b[4..6].copy_from_slice(&1u16.to_le_bytes());
    b[6..8].copy_from_slice(&(HELLO_SIZE as u16).to_le_bytes());
    b
}

pub fn is_game_hello(b: &[u8]) -> bool {
    header_ok(b, HELLO_SIZE, HELLO_MAGIC)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn golden_input() -> InputPacket {
        InputPacket {
            sequence: 7,
            sent_micros: 1_234_567,
            session: 0x1122_3344_5566_7788,
            buttons: button::CROSS | button::R1,
            sticks: [128, 0, 128, 128],
            r2: 255,
            ..InputPacket::neutral(0)
        }
    }

    #[test]
    fn input_packet_matches_the_golden_hex() {
        let expected = concat!(
            "41505249010050000700000000000000",
            "87d61200000000008877665544332211",
            "004800008000808000ff000000000000",
            "0000000000000000000000000ae81c41",
            "00000000000000000000000000000000"
        );
        assert_eq!(hex(&golden_input().encode()), expected);
        let bytes = golden_input().encode();
        assert_eq!(InputPacket::parse(&bytes), Some(golden_input()));
        assert_eq!(input_session(&bytes), Some(0x1122_3344_5566_7788));
    }

    #[test]
    fn output_packet_matches_the_golden_hex() {
        let packet = OutputPacket {
            echo_sequence: 7,
            wait_micros: 350,
            echo_sent_micros: 1_234_567,
            large_motor: 255,
            small_motor: 0,
            light_valid: true,
            light: [255, 30, 30],
            ..OutputPacket::default()
        };
        assert_eq!(
            hex(&packet.encode()),
            "4150524f01002000070000005e01000087d6120000000000ff0001ff1e1e0000"
        );
        assert_eq!(OutputPacket::parse(&packet.encode()), Some(packet));
    }

    #[test]
    fn game_hello_matches_the_golden_hex() {
        assert_eq!(hex(&game_hello()), "41505248010010000000000000000000");
        assert!(is_game_hello(&game_hello()));
        assert!(!is_game_hello(&golden_input().encode()));
    }

    #[test]
    fn invalid_input_packets_are_dropped() {
        let good = golden_input().encode();
        assert!(InputPacket::parse(&good[..79]).is_none());
        let mut bad = good;
        bad[0] ^= 1;
        assert!(InputPacket::parse(&bad).is_none());
        let mut bad = good;
        bad[4] = 2;
        assert!(InputPacket::parse(&bad).is_none());
        let mut bad = good;
        bad[44] = 1;
        bad[46..48].copy_from_slice(&1920u16.to_le_bytes());
        assert!(InputPacket::parse(&bad).is_none());
        let mut bad = good;
        bad[68..72].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(InputPacket::parse(&bad).is_none());
    }

    #[test]
    fn unknown_button_bits_are_masked() {
        let mut bytes = golden_input().encode();
        bytes[32..36].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        assert_eq!(InputPacket::parse(&bytes).unwrap().buttons, button::ALL);
    }

    #[test]
    fn hello_with_and_without_key() {
        let with =
            r#"{"type":"hello","protocol":1,"deviceID":"9f3a","deviceName":"Pixel 8","key":null}"#;
        let without = r#"{"type":"hello","protocol":1,"deviceID":"9f3a","deviceName":"Pixel 8"}"#;
        let expected = Message::Hello {
            protocol: 1,
            device_id: "9f3a".into(),
            device_name: "Pixel 8".into(),
            key: None,
        };
        assert_eq!(Message::parse(with), Some(expected.clone()));
        assert_eq!(Message::parse(without), Some(expected));
    }

    #[test]
    fn every_message_round_trips_with_the_phone_field_names() {
        let games = vec![RemoteGame {
            id: "g".into(),
            name: "Game".into(),
            state: "installed".into(),
            trophies: Some("3/50".into()),
            art: None,
            layout: None,
        }];
        let cases: Vec<(Message, &str)> = vec![
            (
                Message::Pairing {
                    code: "4821".into(),
                },
                r#"{"type":"pairing","code":"4821"}"#,
            ),
            (
                Message::Welcome {
                    session: "18446744073709551615".into(),
                    udp_port: 47702,
                    mac_name: "PC".into(),
                    key: "k".into(),
                },
                r#"{"type":"welcome","session":"18446744073709551615","udpPort":47702,"macName":"PC","key":"k"}"#,
            ),
            (
                Message::Rejected {
                    reason: "busy".into(),
                },
                r#"{"type":"rejected","reason":"busy"}"#,
            ),
            (
                Message::Latency { milliseconds: 3.5 },
                r#"{"type":"latency","milliseconds":3.5}"#,
            ),
            (
                Message::Library {
                    games,
                    selected: Some("g".into()),
                    running: None,
                    language: "en".into(),
                },
                r#"{"type":"library","games":[{"id":"g","name":"Game","state":"installed","trophies":"3/50","art":null,"layout":null}],"selected":"g","running":null,"language":"en"}"#,
            ),
            (
                Message::WantArt { id: "g".into() },
                r#"{"type":"wantArt","id":"g"}"#,
            ),
            (
                Message::Art {
                    id: "g".into(),
                    icon: Some("AA".into()),
                    background: None,
                },
                r#"{"type":"art","id":"g","icon":"AA","background":null}"#,
            ),
            (
                Message::Command { name: "l1".into() },
                r#"{"type":"command","name":"l1"}"#,
            ),
            (
                Message::Select { id: "g".into() },
                r#"{"type":"select","id":"g"}"#,
            ),
            (
                Message::Launch { id: "g".into() },
                r#"{"type":"launch","id":"g"}"#,
            ),
            (Message::Close, r#"{"type":"close"}"#),
            (Message::Home, r#"{"type":"home"}"#),
            (
                Message::SaveLayout {
                    id: "g".into(),
                    layout: Some("{}".into()),
                },
                r#"{"type":"saveLayout","id":"g","layout":"{}"}"#,
            ),
            (
                Message::Trophy {
                    name: "First".into(),
                    detail: "d".into(),
                    grade: "gold".into(),
                    icon: None,
                },
                r#"{"type":"trophy","name":"First","detail":"d","grade":"gold","icon":null}"#,
            ),
        ];
        for (message, json) in cases {
            assert_eq!(message.to_json(), json);
            assert_eq!(Message::parse(json), Some(message));
        }
    }

    #[test]
    fn unknown_types_and_garbage_are_ignored() {
        assert_eq!(Message::parse(r#"{"type":"future","x":1}"#), None);
        assert_eq!(Message::parse("not json"), None);
        assert_eq!(
            Message::parse(r#"{"type":"close","extra":true}"#),
            Some(Message::Close)
        );
    }
}
