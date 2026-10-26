//! Random input at the browser client's parsers: the WebSocket frame parser, the v1 input body
//! and the HTTP request head. Seeded, so a failure repeats. A regression guard: by reading, no
//! parse can panic on garbage.

use std::time::{Duration, Instant};

use super::{
    http::{HEAD_MAX, HeadError, read_head},
    session::{BODY_SIZE, BODY_VERSION, FRAME_MAX, decode_body, opcode, parse_frame},
};
use crate::remote::ludusctrl::{garbage::Garbage, wire::INPUT_MAGIC};

const ROUNDS: usize = 4000;

/// A frame header that passes the first checks (final, masked, a known opcode) with a random
/// length form, and a random tail that may be shorter or longer than the length says.
fn frame_like(rng: &mut Garbage) -> Vec<u8> {
    let codes = [
        opcode::TEXT,
        opcode::BINARY,
        opcode::CLOSE,
        opcode::PING,
        opcode::PONG,
    ];
    let first = 0x80 | codes[rng.below(codes.len())];
    let mut bytes = vec![first];
    match rng.below(4) {
        0 => bytes.push(0x80 | 126),
        1 => bytes.push(0x80 | 127),
        _ => bytes.push(0x80 | rng.below(126) as u8),
    }
    let tail = rng.below(FRAME_MAX + 64);
    bytes.extend(rng.bytes(tail));
    bytes
}

#[test]
fn random_bytes_never_panic_the_frame_parser() {
    let mut rng = Garbage::new(0x5753_4652_414D_4553);
    let mut frames = 0;
    for round in 0..ROUNDS {
        let buffer = if round % 2 == 0 {
            let len = rng.below(FRAME_MAX + 64);
            rng.bytes(len)
        } else {
            frame_like(&mut rng)
        };
        if let Ok(Some((frame, used))) = parse_frame(&buffer) {
            assert!(used <= buffer.len(), "never past the bytes it was given");
            assert!(frame.payload.len() <= FRAME_MAX);
            frames += 1;
        }
        // Every prefix of the buffer is a frame still arriving.
        let cut = rng.below(buffer.len() + 1);
        if let Ok(Some((_, used))) = parse_frame(&buffer[..cut]) {
            assert!(used <= cut);
        }
    }
    assert!(frames > 0, "some garbage is a whole frame");
}

#[test]
fn random_bodies_never_panic_the_input_decoder() {
    let mut rng = Garbage::new(0x424F_4459_5F56_3101);
    let mut decoded = 0;
    for _ in 0..ROUNDS {
        let len = if rng.below(2) == 0 {
            BODY_SIZE
        } else {
            rng.below(64)
        };
        let mut body = rng.bytes(len);
        if body.len() >= 2 && rng.below(2) == 0 {
            body[0] = INPUT_MAGIC;
            body[1] = BODY_VERSION;
        }
        if let Some((_, state, _, _)) = decode_body(&body) {
            assert_eq!(state.buttons & !0xF7FF, 0, "the buttons are masked");
            decoded += 1;
        }
    }
    assert!(decoded > 0);
}

/// Pieces of a request head, put together at random.
fn head_like(rng: &mut Garbage) -> Vec<u8> {
    let pieces: [&[u8]; 14] = [
        b"GET ",
        b"/ ",
        b"/ws?x=1#t=abc ",
        b"HTTP/1.1\r\n",
        b"Host: 192.168.1.20:45991\r\n",
        b"Origin: https://192.168.1.20:45991\r\n",
        b"Upgrade: websocket\r\n",
        b"Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n",
        b"User-Agent: Mozilla/5.0 (iPhone)\r\n",
        b"\r\n",
        b": \r\n",
        b"\xff\xfe\x00",
        b"X: \xc3\x28\r\n",
        b"\r\n\r\n",
    ];
    let mut bytes = Vec::new();
    // Mostly a request line first, so the headers behind it are parsed too.
    if rng.below(4) != 0 {
        bytes.extend_from_slice(pieces[0]);
        bytes.extend_from_slice(pieces[1 + rng.below(2)]);
        bytes.extend_from_slice(pieces[3]);
    }
    for _ in 0..rng.below(12) {
        if rng.below(5) == 0 {
            let len = rng.below(40);
            bytes.extend(rng.bytes(len));
        } else {
            bytes.extend_from_slice(pieces[4 + rng.below(pieces.len() - 4)]);
        }
    }
    if rng.below(2) == 0 {
        bytes.extend_from_slice(
            b"
",
        );
    }
    bytes
}

#[test]
fn random_bytes_never_panic_the_request_head_parser() {
    let mut rng = Garbage::new(0x4854_5450_4845_4144);
    let mut requests = 0;
    for round in 0..ROUNDS {
        let bytes = if round % 2 == 0 {
            let len = rng.below(HEAD_MAX + 512);
            rng.bytes(len)
        } else {
            head_like(&mut rng)
        };
        // A reader that ends with the bytes: the head parses, or the connection is done.
        let deadline = Instant::now() + Duration::from_secs(5);
        match read_head(&mut bytes.as_slice(), deadline) {
            Ok((request, rest)) => {
                assert!(rest.len() <= bytes.len());
                assert!(!request.path.contains(['?', '#']), "the query is cut off");
                requests += 1;
            }
            Err(HeadError::Gone | HeadError::Bad) => {}
        }
    }
    assert!(requests > 0, "some pieces make a whole head");
}
