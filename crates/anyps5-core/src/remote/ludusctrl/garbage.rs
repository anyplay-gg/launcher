//! Random datagrams at the receive path: random bytes of random lengths, and random PAIR_REQ and
//! INPUT headers that pass the first checks, fed to `Link::input` and `Pairing::handle`. Seeded,
//! so a failure repeats. A regression guard: by reading, no parse can panic on garbage.

use std::{
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};

use super::{
    crypto::{Ephemeral, commitment, kid, tests::golden_key},
    link::Link,
    pairing::{Pairing, REQUEST_INTERVAL},
    slot::tests::slot,
    store::LudusStore,
    wire::{INPUT_MAGIC, INPUT_SIZE, NAME_MAX, PAIR_REQ_MAGIC, PAIR_REQ_SIZE, VERSION},
};

/// SplitMix64: enough randomness for garbage, and the same run from the same seed.
pub(super) struct Garbage(u64);

impl Garbage {
    pub(super) fn new(seed: u64) -> Garbage {
        Garbage(seed)
    }

    pub(super) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `n`.
    pub(super) fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    pub(super) fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next() as u8).collect()
    }
}

const ROUNDS: usize = 4000;
/// The receive buffer is 512 bytes.
const LONGEST: usize = 512;

fn source(rng: &mut Garbage) -> SocketAddr {
    let last = 1 + rng.below(250) as u8;
    SocketAddr::from(([192, 168, 1, last], 40000 + rng.below(20000) as u16))
}

/// A random INPUT packet: the right magic, version and length, sometimes the KID of a paired
/// phone, so the MAC check runs on garbage too.
fn input_header(rng: &mut Garbage, paired_kid: [u8; 4]) -> Vec<u8> {
    let mut bytes = rng.bytes(INPUT_SIZE);
    bytes[0] = INPUT_MAGIC;
    bytes[1] = VERSION;
    if rng.below(2) == 0 {
        bytes[24..28].copy_from_slice(&paired_kid);
    }
    bytes
}

/// A random PAIR_REQ: the right magic, version and length; often a name length, phase and point
/// prefix that pass, so the request gets as far as the point check and the attempt.
fn pair_header(rng: &mut Garbage) -> Vec<u8> {
    let mut bytes = rng.bytes(PAIR_REQ_SIZE);
    bytes[0] = PAIR_REQ_MAGIC;
    bytes[1] = VERSION;
    if rng.below(4) != 0 {
        bytes[10] = 0x04;
        bytes[75] = rng.below(NAME_MAX + 1) as u8;
        bytes[108] = rng.below(2) as u8;
    }
    bytes
}

#[test]
fn random_datagrams_never_panic_the_link() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = LudusStore::load(&dir.path().join("LudusCtrl.json"));
    store.add([1; 8], "Pixel 8", golden_key()).unwrap();
    let paired_kid = kid(&golden_key());
    let (pad_slot, _pad) = slot();
    let mut link = Link::default();
    let mut now = Instant::now();
    let mut rng = Garbage::new(0x4C55_4453_4354_524C);
    let mut acks = 0;
    for round in 0..ROUNDS {
        let datagram = match round % 3 {
            0 => {
                let len = rng.below(LONGEST + 1);
                rng.bytes(len)
            }
            1 => input_header(&mut rng, paired_kid),
            _ => {
                // Any version and length behind the input magic, the unsigned replies' path.
                let len = rng.below(64);
                let mut bytes = rng.bytes(len);
                if let Some(first) = bytes.first_mut() {
                    *first = INPUT_MAGIC;
                }
                bytes
            }
        };
        now += Duration::from_millis(rng.below(400) as u64);
        if let Some(ack) = link.input(&datagram, source(&mut rng), now, &mut store, &pad_slot) {
            assert!(ack.len() <= datagram.len(), "never larger than the request");
            acks += 1;
        }
    }
    // Unpaired KIDs and other versions earn rate-limited replies; a garbage MAC earns none.
    assert!(acks > 0 && acks < ROUNDS);
}

#[test]
fn random_datagrams_never_panic_the_pairing() {
    let mut pairing = Pairing::new([0xA1; 8]);
    let mut now = Instant::now();
    let mut rng = Garbage::new(0x5041_4952_5F52_4551);
    let mut asked = 0;
    for round in 0..ROUNDS {
        now += REQUEST_INTERVAL;
        let ip: IpAddr = source(&mut rng).ip();
        match round % 4 {
            0 => {
                let len = rng.below(LONGEST + 1);
                pairing.handle(&rng.bytes(len), ip, now);
            }
            1 => {
                pairing.handle(&pair_header(&mut rng), ip, now);
            }
            _ => {
                // A commit, then a reveal of the same nonce: the point is a real one or garbage
                // behind 0x04, so the point check and a new question both run.
                let mut commit = pair_header(&mut rng);
                commit[10] = 0x04;
                commit[75] = rng.below(NAME_MAX + 1) as u8;
                commit[108] = 0;
                let nonce: [u8; 16] = rng.bytes(16).try_into().unwrap();
                if rng.below(2) == 0 {
                    commit[10..75].copy_from_slice(&Ephemeral::generate().public);
                }
                commit[109..125].copy_from_slice(&commitment(&nonce));
                pairing.handle(&commit, ip, now);
                let mut reveal = commit.clone();
                reveal[108] = 1;
                reveal[109..125].copy_from_slice(&nonce);
                now += REQUEST_INTERVAL;
                if pairing.handle(&reveal, ip, now).ask.is_some() {
                    asked += 1;
                }
                // Let the attempt run out now and then, so the next one can start.
                if rng.below(3) == 0 {
                    now += Duration::from_secs(61);
                }
            }
        }
    }
    assert!(asked > 0, "real points reached the question");
}
