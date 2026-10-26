//! The harness against the golden vectors of `docs/PROTOCOL.md`, the phone's rejection rules,
//! and [`run_conformance`] against the spec-model receiver in `reference.rs`.

use std::{net::SocketAddr, time::Duration};

use super::{
    conformance::{RemoteReceiver, check, run_conformance},
    reference::{Quirk, ReferencePc},
    *,
};

fn unhex(text: &str) -> Vec<u8> {
    let text: String = text.split_whitespace().collect();
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn arr<const N: usize>(text: &str) -> [u8; N] {
    unhex(text).try_into().unwrap()
}

// The fixed inputs of the golden vectors.
const PHONE_SCALAR: [u8; 32] = [0x11; 32];
const PC_SCALAR: [u8; 32] = [0x22; 32];
const PHONE_ID: Id = [1, 2, 3, 4, 5, 6, 7, 8];
const PC_ID: Id = [0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8];
const NONCE: Nonce = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
const NAME: &str = "Pixel 8";

const PHONE_PUB: &str = "040217e617f0b6443928278f96999e69a23a4f2c152bdf6d6cdf66e5b80282d4ed\
                         194a7debcb97712d2dda3ca85aa8765a56f45fc758599652f2897c65306e5794";
const PC_PUB: &str = "04d65a93977caa3d1b081852ff57a79e465f1660577304baead505dd3a48589cf3\
                      50185e895372df6221ea3a137557e473fddb6755f05bd507c3c533fce9c91285";
const Z: &str = "ccfc261f58193c98ca4ad4a53bbac6f0ee29bc4d48438090446908622ca79af6";
const K: &str = "78fb82829c51a3cc9762af1ca02bd7749f797d182d18971b695bb78c28ab7a24";
const KID: &str = "10253095";
const COMMITMENT: &str = "d06ad9ddcb46f53c44d669f7d08fd932";

const PAIR_REQ_COMMIT: &str = "
    d7020102030405060708040217e617f0b6443928278f96999e69a23a4f2c152b
    df6d6cdf66e5b80282d4ed194a7debcb97712d2dda3ca85aa8765a56f45fc75859
    9652f2897c65306e579407506978656c2038000000000000000000000000000000
    0000000000000000000000d06ad9ddcb46f53c44d669f7d08fd932000000";
const PAIR_REQ_REVEAL: &str = "
    d7020102030405060708040217e617f0b6443928278f96999e69a23a4f2c152b
    df6d6cdf66e5b80282d4ed194a7debcb97712d2dda3ca85aa8765a56f45fc75859
    9652f2897c65306e579407506978656c2038000000000000000000000000000000
    0000000000000000000001000102030405060708090a0b0c0d0e0f000000";
const PAIR_RESP_ACCEPTED: &str = "
    d802a1a2a3a4a5a6a7a804d65a93977caa3d1b081852ff57a79e465f1660577304ba
    ead505dd3a48589cf350185e895372df6221ea3a137557e473fddb6755f05bd507c3
    c533fce9c9128501f1980601122fd826";
const PAIR_RESP_WAITING: &str = "
    d802a1a2a3a4a5a6a7a804d65a93977caa3d1b081852ff57a79e465f1660577304ba
    ead505dd3a48589cf350185e895372df6221ea3a137557e473fddb6755f05bd507c3
    c533fce9c91285000000000000000000";
const INPUT_V2: &str = "d5027856341200100080ff7f39306cd9ff8001020700000010253095bee0eb8534c2d722";
const SIGNED_ACK: &str = "d60278563412e80300000100099521f0c16e1df7";
const BEACON: &str = "44534c4b01a5b300a1a2a3a4a5a6a7a8";

fn phone_key() -> EphemeralKey {
    EphemeralKey::from_scalar(&PHONE_SCALAR).unwrap()
}

fn pc_key() -> EphemeralKey {
    EphemeralKey::from_scalar(&PC_SCALAR).unwrap()
}

fn golden_attempt() -> PairingAttempt {
    PairingAttempt::with_secrets(PHONE_ID, NAME, phone_key(), NONCE)
}

fn golden_key() -> Key {
    arr(K)
}

/// The input of the golden v2 packet.
fn golden_input() -> Input {
    Input {
        seq: 0x1234_5678,
        pad: PadState {
            buttons: button::A,
            left_x: -32768,
            left_y: 32767,
            right_x: 12345,
            right_y: -9876,
            left_trigger: 255,
            right_trigger: 128,
        },
        pad_connected: true,
        layout: layout::XBOX,
    }
}

// --- golden vectors --------------------------------------------------------------------------

#[test]
fn keys_and_derivations_match_the_golden_vectors() {
    let phone = phone_key();
    let pc = pc_key();
    assert_eq!(hex(&phone.public()), PHONE_PUB);
    assert_eq!(hex(&pc.public()), PC_PUB);

    let z_phone = phone.agree(&parse_point(&pc.public()).unwrap());
    let z_pc = pc.agree(&parse_point(&phone.public()).unwrap());
    assert_eq!(hex(&z_phone), Z);
    assert_eq!(z_phone, z_pc);

    let derived = derive(&z_phone, &phone.public(), &pc.public(), &NONCE);
    assert_eq!(hex(&derived.key), K);
    assert_eq!(hex(&derived.kid), KID);
    assert_eq!(derived.code, 260329);
    assert_eq!(format_code(derived.code), "260 329");
    assert_eq!(format_code(42), "000 042");
    assert_eq!(hex(&commitment(&NONCE)), COMMITMENT);
}

#[test]
fn hkdf_matches_rfc_5869_case_a1() {
    let ikm = [0x0b; 22];
    let salt = unhex("000102030405060708090a0b0c");
    let info = unhex("f0f1f2f3f4f5f6f7f8f9");
    let mut okm = [0; 42];
    hkdf(&ikm, &salt, &info, &mut okm);
    assert_eq!(
        hex(&okm),
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
    );
}

#[test]
fn pair_requests_match_the_golden_vectors() {
    let mut attempt = golden_attempt();
    assert_eq!(hex(&attempt.request()), unhex_str(PAIR_REQ_COMMIT));

    // The PC's state 0 fixes its key and the code; from then on the phone reveals.
    let waiting = PairResp::new(PC_ID, pc_key().public(), pair_state::WAITING, None);
    assert_eq!(hex(&waiting.encode()), unhex_str(PAIR_RESP_WAITING));
    assert_eq!(
        attempt.on_response(&waiting.encode()),
        Some(PairEvent::Code(260329))
    );
    assert_eq!(attempt.derived().map(|d| d.key), Some(golden_key()));
    assert_eq!(hex(&attempt.request()), unhex_str(PAIR_REQ_REVEAL));
}

fn unhex_str(text: &str) -> String {
    text.split_whitespace().collect()
}

#[test]
fn pair_response_accepted_matches_the_golden_vector() {
    let accepted = PairResp::new(
        PC_ID,
        pc_key().public(),
        pair_state::ACCEPTED,
        Some(&golden_key()),
    );
    assert_eq!(hex(&accepted.encode()), unhex_str(PAIR_RESP_ACCEPTED));

    let mut attempt = golden_attempt();
    let event = attempt.on_response(&unhex(PAIR_RESP_ACCEPTED));
    assert_eq!(
        event,
        Some(PairEvent::Accepted(Paired {
            pc_id: PC_ID,
            key: golden_key(),
            kid: arr(KID),
            code: 260329,
        }))
    );
}

#[test]
fn input_packets_match_the_golden_vectors() {
    let packet = golden_input().encode(7, &golden_key(), &arr(KID));
    assert_eq!(hex(&packet), INPUT_V2);

    let bodies = [
        (
            Input::neutral(0),
            "d502000000000000000000000000000000000000",
        ),
        (
            Input {
                seq: 1,
                pad: PadState {
                    buttons: button::A | button::DPAD_UP,
                    left_x: 32767,
                    left_y: -32768,
                    right_x: 0,
                    right_y: -1,
                    left_trigger: 255,
                    right_trigger: 0,
                },
                pad_connected: true,
                layout: layout::PLAYSTATION,
            },
            "d502010000000110ff7f00800000ffffff000101",
        ),
        (
            Input {
                seq: 0x1234_5678,
                pad: PadState {
                    buttons: button::DEFINED,
                    left_x: 1234,
                    left_y: -1234,
                    right_x: -16384,
                    right_y: 16384,
                    left_trigger: 128,
                    right_trigger: 64,
                },
                pad_connected: true,
                layout: layout::XBOX,
            },
            "d50278563412fff7d2042efb00c0004080400102",
        ),
        (
            Input {
                seq: 0xFFFF_FFFF,
                pad: PadState {
                    buttons: button::Y,
                    right_trigger: 255,
                    ..PadState::NEUTRAL
                },
                pad_connected: false,
                layout: layout::NINTENDO,
            },
            "d502ffffffff0080000000000000000000ff0003",
        ),
    ];
    for (input, want) in bodies {
        assert_eq!(hex(&input.body(PROTOCOL_VERSION)), want, "{input:?}");
    }

    // Session -1 as an Int is ffffffff at bytes 20..=23.
    let packet = Input::neutral(0).encode(-1i32 as u32, &golden_key(), &arr(KID));
    assert_eq!(hex(&packet[20..24]), "ffffffff");
}

#[test]
fn v1_bodies_match_the_golden_vectors() {
    let cases = [
        (
            Input {
                seq: 1,
                pad_connected: true,
                layout: layout::UNKNOWN,
                ..Input::default()
            },
            "d501010000000000000000000000000000000100",
        ),
        (
            Input {
                seq: 0xFFFF_FFFE,
                pad: PadState {
                    buttons: button::DEFINED,
                    ..golden_input().pad
                },
                ..golden_input()
            },
            "d501fefffffffff70080ff7f39306cd9ff800102",
        ),
        (
            Input {
                seq: 0x1234_5678,
                pad: PadState {
                    buttons: button::A,
                    left_y: -1,
                    right_x: 1,
                    ..PadState::NEUTRAL
                },
                pad_connected: false,
                layout: layout::PLAYSTATION,
            },
            "d5017856341200100000ffff0100000000000001",
        ),
    ];
    for (input, want) in cases {
        assert_eq!(hex(&input.body(1)), want, "{input:?}");
    }
}

#[test]
fn acks_match_the_golden_vectors() {
    let key = golden_key();
    let signed = Ack::encode_signed(0x1234_5678, 1000, ack_flag::PAD_LIVE, &key);
    assert_eq!(hex(&signed), SIGNED_ACK);
    assert_eq!(
        Ack::parse(&unhex(SIGNED_ACK), Some(&key)),
        Some(Ack::Signed {
            seq: 0x1234_5678,
            received: 1000,
            pad_live: true,
            version_mismatch: false,
        })
    );

    let mismatch = Ack::encode_unsigned(0x1234_5678, 0, ack_flag::VERSION_MISMATCH);
    assert_eq!(hex(&mismatch), "d60278563412000000000200");
    assert_eq!(
        Ack::parse(&mismatch, Some(&key)),
        Some(Ack::VersionMismatch {
            seq: 0x1234_5678,
            version: 2,
        })
    );
    let not_paired = Ack::encode_unsigned(7, 0, ack_flag::NOT_PAIRED);
    assert_eq!(hex(&not_paired), "d60207000000000000000400");
    assert_eq!(
        Ack::parse(&not_paired, None),
        Some(Ack::NotPaired { seq: 7 })
    );
}

#[test]
fn beacon_matches_the_golden_vector() {
    let beacon = Beacon {
        input_port: INPUT_PORT,
        pc_id: Some(PC_ID),
    };
    assert_eq!((INPUT_PORT, BEACON_PORT), (45989, 45990));
    assert_eq!(hex(&beacon.encode()), BEACON);
    assert_eq!(Beacon::parse(&unhex(BEACON)), Some(beacon));
    // The first 8 bytes are a 1.1 PC's whole beacon.
    let old = &unhex(BEACON)[..BEACON_V11_SIZE];
    assert_eq!(hex(old), "44534c4b01a5b300");
    assert_eq!(
        Beacon::parse(old),
        Some(Beacon {
            input_port: INPUT_PORT,
            pc_id: None,
        })
    );
}

/// A second set with other inputs, not in the spec: computed by LudusCtrl's Python receiver
/// (`pairing.py`, `dualsense_link.py`) and copied here as its output, to cross-check the harness
/// beyond the vectors the spec prints.
#[test]
fn a_second_vector_set_matches_the_python_receiver() {
    let phone = EphemeralKey::from_scalar(&[0x33; 32]).unwrap();
    let pc = EphemeralKey::from_scalar(&[0x44; 32]).unwrap();
    let phone_id: Id = arr("f0e1d2c3b4a59687");
    let pc_id: Id = arr("0011223344556677");
    let nonce: Nonce = std::array::from_fn(|i| 0xff - i as u8);
    assert_eq!(
        hex(&phone.public()),
        "0451a7580833898ea1b183cbd7350a4099078c6ef1c1e18e970cd7683035f25e7d0110522712b0b5a7cff081685486984a94e6831edac46e7360fa9d834a7a81a1"
    );
    assert_eq!(
        hex(&pc.public()),
        "045b36890dacbd7c9a96bb74a1ee28b3d2d75b72e09a20ef25cf8e6fd8a9f0350d0e14bed8d4682a34d83538bdff5b96e89a6666ec0db5745d02fa1210072df75a"
    );
    let z = phone.agree(&parse_point(&pc.public()).unwrap());
    assert_eq!(
        hex(&z),
        "5238f9f956812e75918895390fa057a2063aa66d4d5eefbe6ecdcf78342d2922"
    );
    let derived = derive(&z, &phone.public(), &pc.public(), &nonce);
    assert_eq!(
        hex(&derived.key),
        "c2c0ecbd024a79b9224dc370d071aecb865a909dd0f591c79bbe322d13605e6e"
    );
    assert_eq!(hex(&derived.kid), "88c7b5ac");
    assert_eq!(derived.code, 95638);
    assert_eq!(format_code(derived.code), "095 638");
    assert_eq!(hex(&commitment(&nonce)), "29db6b0f7c8f4c4099586888644de9ba");

    let mut attempt = PairingAttempt::with_secrets(phone_id, "Žuta Nokia 3310 ☎", phone, nonce);
    assert_eq!(
        hex(&attempt.request()),
        "d702f0e1d2c3b4a596870451a7580833898ea1b183cbd7350a4099078c6ef1c1e18e970cd7683035f25e7d0110522712b0b5a7cff081685486984a94e6831edac46e7360fa9d834a7a81a114c5bd757461204e6f6b6961203333313020e2988e0000000000000000000000000029db6b0f7c8f4c4099586888644de9ba000000"
    );
    let waiting = PairResp::new(pc_id, pc.public(), pair_state::WAITING, None);
    assert_eq!(
        hex(&waiting.encode()),
        "d8020011223344556677045b36890dacbd7c9a96bb74a1ee28b3d2d75b72e09a20ef25cf8e6fd8a9f0350d0e14bed8d4682a34d83538bdff5b96e89a6666ec0db5745d02fa1210072df75a000000000000000000"
    );
    assert_eq!(
        attempt.on_response(&waiting.encode()),
        Some(PairEvent::Code(derived.code))
    );
    assert_eq!(
        hex(&attempt.request()),
        "d702f0e1d2c3b4a596870451a7580833898ea1b183cbd7350a4099078c6ef1c1e18e970cd7683035f25e7d0110522712b0b5a7cff081685486984a94e6831edac46e7360fa9d834a7a81a114c5bd757461204e6f6b6961203333313020e2988e00000000000000000000000001fffefdfcfbfaf9f8f7f6f5f4f3f2f1f0000000"
    );
    let accepted = unhex(
        "d8020011223344556677045b36890dacbd7c9a96bb74a1ee28b3d2d75b72e09a20ef25cf8e6fd8a9f0350d0e14bed8d4682a34d83538bdff5b96e89a6666ec0db5745d02fa1210072df75a0146390e0dd9207d34",
    );
    assert_eq!(
        PairResp::new(pc_id, pc.public(), pair_state::ACCEPTED, Some(&derived.key)).encode(),
        accepted.as_slice()
    );
    assert!(matches!(
        attempt.on_response(&accepted),
        Some(PairEvent::Accepted(_))
    ));

    let input = Input {
        seq: 0xFFFF_FFFF,
        pad: PadState {
            buttons: button::DEFINED,
            left_x: 1,
            left_y: -1,
            right_x: -32768,
            right_y: 32767,
            left_trigger: 0,
            right_trigger: 255,
        },
        pad_connected: false,
        layout: layout::NINTENDO,
    };
    let packet = input.encode(0x8000_0001, &derived.key, &derived.kid);
    assert_eq!(
        hex(&packet),
        "d502fffffffffff70100ffff0080ff7f00ff00030100008088c7b5ac6e17a8280d4e9943"
    );
    let ack = Ack::encode_signed(0xFFFF_FFFF, 0xDEAD_BEEF, ack_flag::PAD_LIVE, &derived.key);
    assert_eq!(hex(&ack), "d602ffffffffefbeadde010055241a049e7a9a65");
    let beacon = Beacon {
        input_port: INPUT_PORT,
        pc_id: Some(pc_id),
    };
    assert_eq!(hex(&beacon.encode()), "44534c4b01a5b3000011223344556677");
}

// --- the phone's rules -----------------------------------------------------------------------

#[test]
fn beacons_the_phone_refuses() {
    let good = unhex(BEACON);
    assert_eq!(Beacon::parse(&good[..7]), None, "under 8 bytes");
    let mut bad = good.clone();
    bad[0] = b'X';
    assert_eq!(Beacon::parse(&bad), None, "magic");
    let mut bad = good.clone();
    bad[4] = 2;
    assert_eq!(Beacon::parse(&bad), None, "beacon version");
    let mut bad = good.clone();
    bad[5..7].copy_from_slice(&[0, 0]);
    assert_eq!(Beacon::parse(&bad), None, "port 0");
    // 9..15 bytes: a port, no pc_id.
    assert_eq!(Beacon::parse(&good[..12]).unwrap().pc_id, None);
}

#[test]
fn points_must_be_uncompressed_and_on_the_curve() {
    let good = pc_key().public();
    assert!(parse_point(&good).is_some());
    assert!(parse_point(&[0; 65]).is_none(), "all zero");
    assert!(parse_point(&good[..64]).is_none(), "short");
    let mut compressed = [0; 33];
    compressed[0] = 0x02 | (good[64] & 1);
    compressed[1..].copy_from_slice(&good[1..33]);
    assert!(
        PublicKey::from_sec1_bytes(&compressed).is_ok(),
        "a real compressed point"
    );
    assert!(
        parse_point(&compressed).is_none(),
        "compressed form refused"
    );
    let mut off_curve = good;
    off_curve[64] ^= 0x01;
    assert!(parse_point(&off_curve).is_none(), "off the curve");
    // x = p (not < p) with any y.
    let mut too_big = [0xff; 65];
    too_big[0] = 0x04;
    assert!(parse_point(&too_big).is_none(), "coordinates >= p");
}

#[test]
fn pair_responses_the_phone_ignores() {
    let good = unhex(PAIR_RESP_ACCEPTED);
    for (what, data) in [
        ("83 bytes", good[..83].to_vec()),
        ("85 bytes", [good.as_slice(), &[0]].concat()),
        ("magic", [&[0xD7], &good[1..]].concat()),
        ("version 1", [&good[..1], &[1], &good[2..]].concat()),
    ] {
        assert_eq!(golden_attempt().on_response(&data), None, "{what}");
    }

    // Wrong MAC: the key is fixed, but no acceptance.
    let mut forged = good.clone();
    forged[83] ^= 0x01;
    let mut attempt = golden_attempt();
    assert_eq!(attempt.on_response(&forged), None);
    assert_eq!(attempt.code(), Some(260329));

    // A zero key in state 0 or 1 fixes nothing.
    let zero = PairResp::new(PC_ID, [0; 65], pair_state::WAITING, None);
    let mut attempt = golden_attempt();
    assert_eq!(attempt.on_response(&zero.encode()), None);
    assert_eq!(attempt.code(), None);

    // An invalid point fixes nothing either; the next valid one still can.
    let mut off = pc_key().public();
    off[64] ^= 1;
    let invalid = PairResp::new(PC_ID, off, pair_state::WAITING, None);
    assert_eq!(attempt.on_response(&invalid.encode()), None);
    assert_eq!(attempt.pc_pub(), None);
    let waiting = unhex(PAIR_RESP_WAITING);
    assert_eq!(attempt.on_response(&waiting), Some(PairEvent::Code(260329)));
}

#[test]
fn first_key_wins() {
    let mut attempt = golden_attempt();
    assert_eq!(
        attempt.on_response(&unhex(PAIR_RESP_WAITING)),
        Some(PairEvent::Code(260329))
    );

    // Someone else's key now: state 0, state 1 (even with a MAC under its own K), state 2 and
    // state 3 are all ignored.
    let attacker = EphemeralKey::random();
    let z = attacker.agree(&parse_point(attempt.phone_pub()).unwrap());
    let theirs = derive(&z, attempt.phone_pub(), &attacker.public(), &NONCE);
    for state in [
        pair_state::WAITING,
        pair_state::ACCEPTED,
        pair_state::DENIED,
    ] {
        let resp = PairResp::new(PC_ID, attacker.public(), state, Some(&theirs.key));
        assert_eq!(attempt.on_response(&resp.encode()), None, "state {state}");
    }
    let busy = PairResp::new(PC_ID, [0; 65], pair_state::BUSY, None);
    assert_eq!(
        attempt.on_response(&busy.encode()),
        None,
        "busy after the key is fixed"
    );

    // Denied by the fixed key ends the attempt.
    let denied = PairResp::new(PC_ID, pc_key().public(), pair_state::DENIED, None);
    assert_eq!(
        attempt.on_response(&denied.encode()),
        Some(PairEvent::Declined)
    );
    // And the real acceptance still verifies.
    assert!(matches!(
        attempt.on_response(&unhex(PAIR_RESP_ACCEPTED)),
        Some(PairEvent::Accepted(_))
    ));
}

#[test]
fn busy_and_denied_before_a_key() {
    let busy = PairResp::new(PC_ID, [0; 65], pair_state::BUSY, None);
    assert_eq!(
        golden_attempt().on_response(&busy.encode()),
        Some(PairEvent::Busy)
    );
    let denied = PairResp::new(PC_ID, [0; 65], pair_state::DENIED, None);
    assert_eq!(
        golden_attempt().on_response(&denied.encode()),
        Some(PairEvent::Declined)
    );
    let unknown = PairResp::new(PC_ID, pc_key().public(), 4, None);
    assert_eq!(golden_attempt().on_response(&unknown.encode()), None);
}

#[test]
fn a_man_in_the_middle_shows_a_different_code() {
    // The attacker answers the phone with its own key: both ends derive a K, but the codes
    // differ (the code covers the nonce), and the PC's MAC does not verify under the phone's K.
    let mut attempt = golden_attempt();
    let attacker = EphemeralKey::random();
    let to_phone = PairResp::new(PC_ID, attacker.public(), pair_state::WAITING, None);
    let Some(PairEvent::Code(phone_code)) = attempt.on_response(&to_phone.encode()) else {
        panic!("the phone fixes the first key");
    };
    assert_ne!(
        phone_code, 260329,
        "code mismatch: the user sees two different codes"
    );
    assert_eq!(attempt.on_response(&unhex(PAIR_RESP_ACCEPTED)), None);
}

#[test]
fn a_reveal_must_match_the_commitment() {
    let attempt = golden_attempt();
    let commit = attempt.request();
    let field: [u8; 16] = commit[109..125].try_into().unwrap();
    assert_eq!(field, commitment(&NONCE));
    let mut wrong = NONCE;
    wrong[15] ^= 0x80;
    assert_ne!(commitment(&wrong), field);
    let bad = attempt.request_with(Phase::Reveal, &wrong);
    assert_eq!(bad[108], 1);
    assert_eq!(&bad[109..125], &wrong);
}

#[test]
fn acks_the_phone_ignores() {
    let key = golden_key();
    let good = unhex(SIGNED_ACK);
    let mut forged = good.clone();
    forged[19] ^= 1;
    assert_eq!(Ack::parse(&forged, Some(&key)), None, "wrong MAC");
    let mut flipped = good.clone();
    flipped[10] = 0;
    assert_eq!(
        Ack::parse(&flipped, Some(&key)),
        None,
        "pad-live flipped after signing"
    );
    assert_eq!(Ack::parse(&good, None), None, "signed ack with no key");
    assert_eq!(
        Ack::parse(&good, Some(&[0; 32])),
        None,
        "another phone's key"
    );
    assert_eq!(Ack::parse(&good[..11], Some(&key)), None, "under 12 bytes");
    assert_eq!(Ack::parse(&good[..19], Some(&key)), None, "19 bytes");
    let mut magic = good.clone();
    magic[0] = 0xD5;
    assert_eq!(Ack::parse(&magic, Some(&key)), None, "magic");

    // Pad-live is never believed without a MAC.
    let live = Ack::encode_unsigned(1, 5, ack_flag::PAD_LIVE);
    assert_eq!(Ack::parse(&live, Some(&key)), None);
    // A future PC's version reads as a mismatch, whatever the length.
    let mut future = good.clone();
    future[1] = 3;
    assert_eq!(
        Ack::parse(&future, Some(&key)),
        Some(Ack::VersionMismatch {
            seq: 0x1234_5678,
            version: 3,
        })
    );
}

#[test]
fn input_macs_cover_bytes_0_to_27() {
    let key = golden_key();
    let packet = unhex(INPUT_V2);
    assert!(mac_ok(
        &key,
        &packet[..INPUT_SIGNED],
        &packet[INPUT_SIGNED..]
    ));
    for byte in 0..INPUT_SIGNED {
        let mut tampered = packet.clone();
        tampered[byte] ^= 0x01;
        assert!(
            !mac_ok(&key, &tampered[..INPUT_SIGNED], &tampered[INPUT_SIGNED..]),
            "byte {byte}"
        );
    }
    // A different session is a different MAC.
    let other = golden_input().encode(8, &key, &arr(KID));
    assert_ne!(other[INPUT_SIGNED..], packet[INPUT_SIGNED..]);
}

#[test]
fn names_are_cut_at_a_character_boundary() {
    assert_eq!(name_bytes("Pixel 8"), b"Pixel 8");
    let long = "x".repeat(31) + "Ž";
    assert_eq!(
        name_bytes(&long),
        "x".repeat(31).as_bytes(),
        "a 2-byte char at byte 31"
    );
    let exact = "x".repeat(30) + "Ž";
    assert_eq!(name_bytes(&exact).len(), 32);
    let request = pair_request(
        &PHONE_ID,
        &phone_key().public(),
        &name_bytes(&long),
        Phase::Commit,
        &[0; 16],
    );
    assert_eq!(request[75], 31);
}

#[test]
fn sticks_and_triggers_scale_to_both_rails() {
    assert_eq!(axis(1.0), 32767);
    assert_eq!(axis(-1.0), -32768);
    assert_eq!(axis(0.0), 0);
    assert_eq!(axis(2.0), 32767, "clamped");
    assert_eq!(axis(0.5), 16384, "16383.5 rounds up");
    assert_eq!(axis(-0.5), -16384);
    assert_eq!(trigger(1.0), 255);
    assert_eq!(trigger(0.5), 128, "127.5 rounds up");
    assert_eq!(trigger(-1.0), 0);
}

#[test]
fn the_launchers_pad_state_converts_field_for_field() {
    let written = crate::remote::vpad::PadState {
        buttons: button::A | button::GUIDE,
        left_trigger: 1,
        right_trigger: 2,
        lx: -32768,
        ly: 32767,
        rx: 3,
        ry: -4,
    };
    assert_eq!(
        PadState::from(written),
        PadState {
            buttons: 0x1400,
            left_x: -32768,
            left_y: 32767,
            right_x: 3,
            right_y: -4,
            left_trigger: 1,
            right_trigger: 2,
        }
    );
}

// --- the phone on a socket -------------------------------------------------------------------

#[test]
fn the_phone_pairs_with_a_receiver_that_allows_on_its_own() {
    let pc = ReferencePc::start(None, true, Quirk::None).unwrap();
    let mut phone = TestPhone::new(pc.addr()).unwrap();
    assert!(phone.local_addr().unwrap().ip().is_loopback());
    let lan = SocketAddr::from(([192, 168, 1, 2], INPUT_PORT));
    assert!(TestPhone::new(lan).is_err(), "never a socket on the LAN");
    assert!(
        beacon_listener(0)
            .unwrap()
            .local_addr()
            .unwrap()
            .ip()
            .is_loopback()
    );
    let mut codes = Vec::new();
    let paired = phone
        .pair(Duration::from_secs(8), |code| codes.push(code))
        .unwrap();
    assert_eq!(codes, [paired.code]);
    assert_eq!(phone.session(), 0, "a new pairing starts the counter at 0");

    // Any session above the PC's highest opens a link, whatever its seq.
    phone.set_session(41);
    assert_eq!(phone.start_session(), 42);
    phone.set_next_seq(0xFFFF_FFFF);
    let seq = phone.send(PadState::NEUTRAL).unwrap();
    assert!(matches!(
        phone.ack_for(seq, Duration::from_millis(500)).unwrap(),
        Some(Ack::Signed { pad_live: true, .. })
    ));
    // The seq wraps: 0 is newer than 0xFFFFFFFF.
    std::thread::sleep(ACK_INTERVAL);
    let seq = phone.send(PadState::NEUTRAL).unwrap();
    assert_eq!(seq, 0);
    assert!(
        phone
            .ack_for(seq, Duration::from_millis(500))
            .unwrap()
            .is_some()
    );
}

#[test]
fn the_reference_receiver_passes_conformance() {
    let beacon = beacon_listener(0).unwrap();
    let mut pc = ReferencePc::start(Some(beacon), false, Quirk::None).unwrap();
    let report = run_conformance(&mut pc).unwrap_or_else(|failure| panic!("{failure}"));
    assert!(report.skipped.is_empty(), "skipped {:?}", report.skipped);
    assert_eq!(report.passed.len(), 32);
    let release = report.watchdog_release.unwrap();
    assert!(release >= WATCHDOG_TIMEOUT, "released after {release:?}");
}

fn conformance_fails_on(quirk: Quirk) -> &'static str {
    let mut pc = ReferencePc::start(None, true, quirk).unwrap();
    match run_conformance(&mut pc) {
        Ok(report) => panic!("{quirk:?} passed: {report:?}"),
        Err(failure) => failure.check,
    }
}

#[test]
fn conformance_catches_a_missing_replay_check() {
    assert_eq!(conformance_fails_on(Quirk::AcceptStale), check::REPLAY);
}

#[test]
fn conformance_catches_a_wrong_code() {
    assert_eq!(
        conformance_fails_on(Quirk::CodeWithoutNonce),
        check::PROMPT_CODE
    );
}

#[test]
fn conformance_catches_a_skipped_mac_check() {
    assert_eq!(conformance_fails_on(Quirk::NoMacCheck), check::BAD_MAC);
}

#[test]
fn conformance_catches_a_missing_watchdog() {
    assert_eq!(
        conformance_fails_on(Quirk::NoWatchdog),
        check::WATCHDOG_RELEASE
    );
}

/// Runs the checks against a receiver in another process, for example LudusCtrl's own Python
/// receiver behind a small control script: `LUDUSCTRL_CONFORMANCE=input=127.0.0.1:P,control=
/// 127.0.0.1:C[,beacon=B][,pad_live=0]` (see [`RemoteReceiver`] for the control protocol). The
/// receiver must listen on loopback, and with `beacon=B` send its beacon to `127.0.0.1:B`.
#[test]
#[ignore = "needs a receiver started by hand; see LUDUSCTRL_CONFORMANCE"]
fn an_external_receiver_passes_conformance() {
    let spec = std::env::var("LUDUSCTRL_CONFORMANCE").expect("LUDUSCTRL_CONFORMANCE is set");
    let mut input: Option<SocketAddr> = None;
    let mut control: Option<SocketAddr> = None;
    let mut beacon = None;
    let mut pad_live = true;
    for part in spec.split(',') {
        let (name, value) = part.split_once('=').expect("name=value");
        match name.trim() {
            "input" => input = Some(value.parse().unwrap()),
            "control" => control = Some(value.parse().unwrap()),
            "beacon" => beacon = Some(beacon_listener(value.parse().unwrap()).unwrap()),
            "pad_live" => pad_live = value != "0",
            other => panic!("unknown field {other}"),
        }
    }
    let mut pc = RemoteReceiver::connect(
        input.expect("input="),
        control.expect("control="),
        beacon,
        pad_live,
    )
    .unwrap();
    let report = run_conformance(&mut pc).unwrap_or_else(|failure| panic!("{failure}"));
    println!("{report:#?}");
}
