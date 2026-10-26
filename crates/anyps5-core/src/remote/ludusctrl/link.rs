//! The receiving end of the app's input over UDP: the acceptance order of every input datagram
//! (version, length, key id, MAC, session), the handover to the pad slot, and the acks.
//!
//! Pure apart from the slot it feeds: the caller hands in the time, the source and the store, and
//! sends what comes back.

use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};

use super::{
    crypto::mac_ok,
    slot::PadSlot,
    store::LudusStore,
    wire::{
        BUTTON_MASK, FLAG_CONTROLLER, INPUT_MAGIC, INPUT_SIZE, Id, Input, UNSIGNED_ACK_SIZE,
        VERSION, ack_flag, signed_ack, unsigned_ack,
    },
};
use crate::remote::vpad::PadState;

/// Signed acks to one phone at most this often.
pub const ACK_INTERVAL: Duration = Duration::from_millis(200);
/// Unsigned acks (mismatch, not paired) to one source at most this often.
pub const UNSIGNED_INTERVAL: Duration = Duration::from_secs(1);
const LIMITER_MAX: usize = 64;

/// Allows one event per key and interval; the table is cleared when it grows past 64 entries.
#[derive(Default)]
struct Limiter(HashMap<SocketAddr, Instant>);

impl Limiter {
    fn allow(&mut self, key: SocketAddr, now: Instant, every: Duration) -> bool {
        if self
            .0
            .get(&key)
            .is_some_and(|at| now.saturating_duration_since(*at) < every)
        {
            return false;
        }
        if self.0.len() >= LIMITER_MAX {
            self.0.clear();
        }
        self.0.insert(key, now);
        true
    }
}

#[derive(Default)]
pub struct Link {
    /// The newest `(session, seq)` of each phone, in memory only. Kept across the watchdog and
    /// across phones taking turns, so a stretch of silence never reopens a window for replaying
    /// that session.
    sessions: HashMap<Id, (u32, u32)>,
    /// Accepted packets from whichever phone held the pad, mod 2^32.
    received: u32,
    last_ack: HashMap<Id, Instant>,
    mismatch: Limiter,
    not_paired: Limiter,
}

/// Newer in a u32 half-window, so the sequence may wrap.
fn is_newer(seq: u32, newest: u32) -> bool {
    let ahead = seq.wrapping_sub(newest);
    ahead != 0 && ahead < 1 << 31
}

fn state_of(input: &Input) -> PadState {
    PadState {
        buttons: input.buttons & BUTTON_MASK,
        left_trigger: input.left_trigger,
        right_trigger: input.right_trigger,
        lx: input.lx,
        ly: input.ly,
        rx: input.rx,
        ry: input.ry,
    }
}

impl Link {
    /// One input datagram (first byte `0xD5`) from an allowed source. Returns the ack to send.
    pub fn input(
        &mut self,
        datagram: &[u8],
        source: SocketAddr,
        now: Instant,
        store: &mut LudusStore,
        slot: &PadSlot,
    ) -> Option<Vec<u8>> {
        if datagram.len() < 2 || datagram[0] != INPUT_MAGIC {
            return None;
        }
        if datagram[1] != VERSION {
            // Every v1 packet: say why nothing happens, never larger than what came in.
            if datagram.len() < UNSIGNED_ACK_SIZE
                || !self.mismatch.allow(source, now, UNSIGNED_INTERVAL)
            {
                return None;
            }
            let seq = u32::from_le_bytes(datagram[2..6].try_into().expect("4 bytes"));
            return Some(unsigned_ack(seq, ack_flag::VERSION_MISMATCH).to_vec());
        }
        if datagram.len() != INPUT_SIZE {
            return None;
        }
        let (input, kid, tag) = Input::parse(datagram)?;
        let candidates: Vec<usize> = store
            .phones()
            .iter()
            .enumerate()
            .filter(|(_, p)| p.kid == kid)
            .map(|(at, _)| at)
            .collect();
        if candidates.is_empty() {
            if !self.not_paired.allow(source, now, UNSIGNED_INTERVAL) {
                return None;
            }
            return Some(unsigned_ack(input.seq, ack_flag::NOT_PAIRED).to_vec());
        }
        // Every candidate is tried, so the time says nothing about which one matched. A bad MAC
        // gets no ack: a forger learns nothing about how far a guess got.
        let signed = Input::signed(datagram);
        let mut matched = None;
        for at in candidates {
            if mac_ok(&store.phones()[at].key, signed, tag) && matched.is_none() {
                matched = Some(at);
            }
        }
        let phone = &store.phones()[matched?];
        let (id, key, highest) = (phone.id, phone.key, phone.highest_session);

        let new_session = input.session > highest;
        if new_session {
            // Written before the packet counts: otherwise a recording of this session would pass
            // as new after a crash.
            if let Err(error) = store.raise_session(&id, input.session) {
                tracing::warn!("ludusctrl: could not save the session number: {error}");
            }
        } else if input.session < highest {
            return None;
        } else {
            match self.sessions.get(&id) {
                // After a restart the newest seq of this session is unknown: wait for the phone
                // to start a new one, which it does after 3 s without acks.
                Some((session, newest))
                    if *session == input.session && is_newer(input.seq, *newest) => {}
                _ => return None,
            }
        }
        self.sessions.insert(id, (input.session, input.seq));

        let applied = slot.app_input(
            id,
            source,
            new_session,
            &state_of(&input),
            input.layout,
            input.flags & FLAG_CONTROLLER != 0,
            now,
        );
        let mut flags = 0;
        if let Some(live) = applied {
            if live {
                flags |= ack_flag::PAD_LIVE;
            }
            self.received = self.received.wrapping_add(1);
        }
        let due = self
            .last_ack
            .get(&id)
            .is_none_or(|at| now.saturating_duration_since(*at) >= ACK_INTERVAL);
        if !due {
            return None;
        }
        self.last_ack.insert(id, now);
        Some(signed_ack(input.seq, self.received, flags, &key).to_vec())
    }

    /// The phone was removed: its in-memory session goes.
    pub fn forget(&mut self, id: &Id) {
        self.sessions.remove(id);
        self.last_ack.remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        crypto::{LinkKey, kid, mac, mac_ok, tests::golden_key},
        slot::{
            Owner, PEER_SWITCH, WATCHDOG,
            tests::{Pad, slot},
        },
        wire::{PadLayout, ack_signed, read_ack},
    };
    use super::*;
    use std::sync::Arc;

    const A: Id = [1; 8];
    const B: Id = [2; 8];
    const KEY_B: LinkKey = [0xB0; 32];

    struct Rig {
        link: Link,
        store: LudusStore,
        slot: PadSlot,
        pad: Arc<Pad>,
        now: Instant,
        _dir: tempfile::TempDir,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let mut store = LudusStore::load(&dir.path().join("LudusCtrl.json"));
        store.add(A, "Pixel 8", golden_key()).unwrap();
        store.add(B, "Galaxy", KEY_B).unwrap();
        let (slot, pad) = slot();
        Rig {
            link: Link::default(),
            store,
            slot,
            pad,
            now: Instant::now(),
            _dir: dir,
        }
    }

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([192, 168, 1, 20], port))
    }

    fn packet(key: &LinkKey, session: u32, seq: u32, buttons: u16) -> Vec<u8> {
        Input {
            seq,
            buttons,
            lx: -100,
            ly: 2000,
            right_trigger: 40,
            flags: FLAG_CONTROLLER,
            layout: PadLayout::PlayStation,
            session,
            ..Input::default()
        }
        .encode(&kid(key), key)
        .to_vec()
    }

    impl Rig {
        fn send(&mut self, bytes: &[u8], from: SocketAddr) -> Option<Vec<u8>> {
            self.link
                .input(bytes, from, self.now, &mut self.store, &self.slot)
        }

        fn later(&mut self, millis: u64) {
            self.now += Duration::from_millis(millis);
        }

        fn highest(&self, id: Id) -> u32 {
            self.store
                .phones()
                .iter()
                .find(|p| p.id == id)
                .unwrap()
                .highest_session
        }
    }

    #[test]
    fn a_new_session_is_saved_applied_and_acked() {
        let mut rig = rig();
        let ack = rig
            .send(&packet(&golden_key(), 1, 10, 0x1000), addr(5000))
            .unwrap();
        let (seq, received, flags, tag) = read_ack(&ack).unwrap();
        assert_eq!((seq, received, flags), (10, 1, ack_flag::PAD_LIVE));
        assert!(mac_ok(&golden_key(), ack_signed(&ack), &tag.unwrap()));
        assert_eq!(rig.highest(A), 1, "written before it counts");
        assert_eq!(
            rig.pad.last(),
            Some(PadState {
                buttons: 0x1000,
                lx: -100,
                ly: 2000,
                right_trigger: 40,
                ..PadState::default()
            })
        );
        let holding = rig.slot.holding().unwrap();
        assert_eq!(holding.owner, Owner::App(A));
        assert_eq!(holding.layout, PadLayout::PlayStation);
        assert!(holding.controller && holding.pad_live);
    }

    #[test]
    fn the_golden_ack_comes_back_for_the_golden_packet() {
        let mut rig = rig();
        rig.link.received = 999;
        let golden = Input {
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
        .encode(&kid(&golden_key()), &golden_key());
        let ack = rig.send(&golden, addr(5000)).unwrap();
        let hex: String = ack.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, "d60278563412e80300000100099521f0c16e1df7");
    }

    #[test]
    fn seqs_must_move_forward_and_old_sessions_are_replays() {
        let mut rig = rig();
        let key = golden_key();
        rig.send(&packet(&key, 3, 100, 0), addr(1)).unwrap();
        rig.later(250);
        assert!(rig.send(&packet(&key, 3, 101, 1), addr(1)).is_some());
        let writes = rig.pad.count();
        rig.later(250);
        assert!(
            rig.send(&packet(&key, 3, 101, 2), addr(1)).is_none(),
            "the same seq"
        );
        assert!(
            rig.send(&packet(&key, 3, 50, 2), addr(1)).is_none(),
            "an older seq"
        );
        assert!(
            rig.send(&packet(&key, 2, 500, 2), addr(1)).is_none(),
            "an old session"
        );
        assert_eq!(rig.pad.count(), writes, "nothing applied");
        // The u32 wraps: 0xFFFFFFFF then 0 is forward.
        rig.send(&packet(&key, 4, u32::MAX, 0), addr(1)).unwrap();
        rig.later(250);
        assert!(rig.send(&packet(&key, 4, 0, 0), addr(1)).is_some());
        assert!(is_newer(5, 4) && !is_newer(4, 4) && !is_newer(4, 5));
        assert!(is_newer(0x7FFF_FFFF, 0) && !is_newer(0x8000_0000, 0));
    }

    #[test]
    fn after_a_restart_the_current_session_waits_for_a_new_one() {
        let mut rig = rig();
        let key = golden_key();
        rig.send(&packet(&key, 5, 1, 0), addr(1)).unwrap();
        // The PC restarts: the store keeps highest_session 5, the memory is empty.
        rig.link = Link::default();
        assert!(rig.send(&packet(&key, 5, 2, 0), addr(1)).is_none());
        assert!(
            rig.send(&packet(&key, 6, 0, 0), addr(1)).is_some(),
            "a new session"
        );
    }

    #[test]
    fn version_length_kid_and_mac_in_that_order() {
        let mut rig = rig();
        let good = packet(&golden_key(), 1, 7, 0);
        let mut v1 = good.clone();
        v1[1] = 1;
        let ack = rig.send(&v1, addr(1)).unwrap();
        assert_eq!(ack, unsigned_ack(7, ack_flag::VERSION_MISMATCH));
        assert!(rig.send(&v1, addr(1)).is_none(), "once a second per source");
        assert!(rig.send(&v1, addr(2)).is_some(), "per port, too");
        assert!(
            rig.send(&v1[..11], addr(3)).is_none(),
            "never larger than the request"
        );
        rig.later(1000);
        assert!(rig.send(&v1, addr(1)).is_some());

        assert!(rig.send(&good[..35], addr(1)).is_none(), "wrong length");

        let stranger = packet(&[0x55; 32], 1, 9, 0);
        let ack = rig.send(&stranger, addr(1)).unwrap();
        assert_eq!(ack, unsigned_ack(9, ack_flag::NOT_PAIRED));
        assert!(rig.send(&stranger, addr(1)).is_none());

        let mut forged = good.clone();
        forged[35] ^= 1;
        assert!(
            rig.send(&forged, addr(4)).is_none(),
            "a bad MAC gets no ack"
        );
        let mut tampered = good.clone();
        tampered[6] = 0xFF;
        assert!(rig.send(&tampered, addr(4)).is_none());
        assert_eq!(rig.pad.count(), 0);
        assert_eq!(rig.highest(A), 0, "nothing forged reached the session");
        assert!(rig.send(&good, addr(4)).is_some());
    }

    #[test]
    fn a_colliding_kid_tries_every_key() {
        let mut rig = rig();
        // A 32-bit KID can collide: C's KID is A's, and C's packets still find C's key.
        let key_c: LinkKey = [0xC0; 32];
        rig.store.add([3; 8], "C", key_c).unwrap();
        rig.store.set_kid(&[3; 8], kid(&golden_key()));
        let mut bytes = packet(&key_c, 1, 1, 0x4000);
        bytes[24..28].copy_from_slice(&kid(&golden_key()));
        let tag = mac(&key_c, &bytes[..28]);
        bytes[28..].copy_from_slice(&tag);
        let ack = rig.send(&bytes, addr(1)).unwrap();
        assert!(mac_ok(
            &key_c,
            ack_signed(&ack),
            &read_ack(&ack).unwrap().3.unwrap()
        ));
        assert_eq!(rig.highest([3; 8]), 1);
        assert_eq!(rig.highest(A), 0);
    }

    #[test]
    fn a_phone_that_does_not_hold_the_pad_is_acked_not_counted() {
        let mut rig = rig();
        rig.send(&packet(&golden_key(), 1, 1, 0x1000), addr(1))
            .unwrap();
        rig.send(&packet(&KEY_B, 1, 1, 0x2000), addr(2)).unwrap();
        assert_eq!(rig.pad.last().unwrap().buttons, 0x2000, "B took over");
        rig.later(200);
        // A continues its session while B is active: authenticated, acked, not applied.
        let ack = rig
            .send(&packet(&golden_key(), 1, 2, 0x1000), addr(1))
            .unwrap();
        let (_, received, flags, _) = read_ack(&ack).unwrap();
        assert_eq!(flags & ack_flag::PAD_LIVE, 0);
        assert_eq!(received, 2, "a refused packet is not counted");
        assert_eq!(rig.pad.last().unwrap().buttons, 0x2000);
        rig.later(PEER_SWITCH.as_millis() as u64);
        rig.send(&packet(&golden_key(), 1, 3, 0x1000), addr(1));
        assert_eq!(rig.pad.last().unwrap().buttons, 0x1000);
    }

    #[test]
    fn signed_acks_are_limited_per_phone() {
        let mut rig = rig();
        let key = golden_key();
        assert!(rig.send(&packet(&key, 1, 1, 0), addr(1)).is_some());
        rig.later(100);
        assert!(rig.send(&packet(&key, 1, 2, 0), addr(1)).is_none());
        rig.later(100);
        assert!(rig.send(&packet(&key, 1, 3, 0), addr(1)).is_some());
        // Every packet was applied, also the one without an ack.
        assert_eq!(rig.pad.count(), 3);
        assert!(
            rig.send(&packet(&KEY_B, 1, 1, 0), addr(2)).is_some(),
            "another phone has its own budget"
        );
    }

    #[test]
    fn bit_eleven_never_reaches_the_pad() {
        let mut rig = rig();
        let bytes = Input {
            seq: 1,
            buttons: 0xFFFF,
            flags: FLAG_CONTROLLER,
            session: 1,
            ..Input::default()
        }
        .encode(&kid(&golden_key()), &golden_key());
        rig.send(&bytes, addr(1)).unwrap();
        assert_eq!(rig.pad.last().unwrap().buttons, 0xF7FF);
    }

    #[test]
    fn the_session_survives_the_watchdog_but_not_its_replays() {
        let mut rig = rig();
        let key = golden_key();
        rig.send(&packet(&key, 1, 1, 0x1000), addr(1)).unwrap();
        rig.later(WATCHDOG.as_millis() as u64);
        assert!(rig.slot.tick(rig.now));
        assert_eq!(rig.pad.last(), Some(PadState::default()));
        rig.later(500);
        assert!(rig.send(&packet(&key, 1, 1, 0), addr(1)).is_none());
        assert!(rig.send(&packet(&key, 1, 2, 0), addr(1)).is_some());
        assert_eq!(rig.slot.holding().unwrap().owner, Owner::App(A));
    }

    #[test]
    fn a_browser_holding_the_pad_is_left_alone() {
        let mut rig = rig();
        rig.slot.claim_browser(4, addr(9), rig.now).unwrap();
        let ack = rig.send(&packet(&golden_key(), 1, 1, 1), addr(1)).unwrap();
        assert_eq!(read_ack(&ack).unwrap().2, 0, "acked, not live");
        assert_eq!(rig.pad.count(), 0);
        assert_eq!(rig.highest(A), 1, "still authenticated and recorded");
    }
}
