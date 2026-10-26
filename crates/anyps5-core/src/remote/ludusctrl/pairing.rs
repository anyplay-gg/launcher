//! The PC's side of LudusCtrl's pairing: one attempt at a time, commit then reveal, the code on
//! the launcher's pairing question, and the answers that resends crossing the click still get.
//!
//! Pure: the caller hands in the time and owns the socket and the store.

use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    time::{Duration, Instant},
};

use super::{
    crypto::{
        Ephemeral, LinkKey, NONCE_SIZE, Point, commitment, confirmation_code, format_code,
        link_key, parse_point,
    },
    store::LudusStore,
    wire::{Id, PAIR_RESP_SIZE, PairRequest, PairState, Phase, is_pair_request, pair_response},
};
use crate::remote::{
    PairingRequest, control::MAX_DEVICES, security::constant_time_eq, security::random_session,
};

/// The question waits this long from the reveal.
pub const PROMPT_TIMEOUT: Duration = Duration::from_secs(60);
/// An attempt without a valid reveal frees the slot after this long.
pub const REVEAL_TIMEOUT: Duration = Duration::from_secs(10);
/// A finished attempt keeps answering its phone's resends this long.
pub const FINISHED_KEPT: Duration = Duration::from_secs(90);
const FINISHED_MAX: usize = 32;
/// One PAIR_REQ a second per address; less than this since the last one is dropped.
pub const REQUEST_INTERVAL: Duration = Duration::from_secs(1);
const RATE_TABLE_MAX: usize = 256;

pub const UNNAMED: &str = "Unnamed phone";

/// The name the PC shows: UTF-8 with replacement, no control or format characters (bidi
/// overrides included), whitespace collapsed.
pub fn display_name(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let kept: String = text
        .chars()
        .filter(|c| !c.is_control() && !is_format(*c))
        .collect();
    let name = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        UNNAMED.to_string()
    } else {
        name
    }
}

/// Unicode general category Cf (format characters), Unicode 16.
fn is_format(c: char) -> bool {
    matches!(
        u32::from(c),
        0x00AD
            | 0x0600..=0x0605
            | 0x061C
            | 0x06DD
            | 0x070F
            | 0x0890..=0x0891
            | 0x08E2
            | 0x180E
            | 0x200B..=0x200F
            | 0x202A..=0x202E
            | 0x2060..=0x2064
            | 0x2066..=0x206F
            | 0xFEFF
            | 0xFFF9..=0xFFFB
            | 0x110BD
            | 0x110CD
            | 0x13430..=0x1343F
            | 0x1BCA0..=0x1BCA3
            | 0x1D173..=0x1D17A
            | 0xE0001
            | 0xE0020..=0xE007F
    )
}

struct Open {
    id: u64,
    opened: Instant,
    key: LinkKey,
}

struct Attempt {
    phone_id: Id,
    phone_pub: Point,
    name: String,
    commitment: [u8; 16],
    started: Instant,
    /// Dropped once the reveal derived the key.
    ephemeral: Option<Ephemeral>,
    pc_pub: Point,
    prompt: Option<Open>,
}

impl Attempt {
    fn is(&self, phone_id: &Id, phone_pub: &Point) -> bool {
        self.phone_id == *phone_id && self.phone_pub == *phone_pub
    }
}

struct Finished {
    phone_id: Id,
    phone_pub: Point,
    at: Instant,
    response: [u8; PAIR_RESP_SIZE],
}

/// What a PAIR_REQ gave: the reply, and a question to put to the player.
#[derive(Default)]
pub struct Handled {
    pub reply: Option<[u8; PAIR_RESP_SIZE]>,
    pub ask: Option<PairingRequest>,
}

/// The end of a question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// Allowed and stored.
    Paired,
    /// Denied by the player.
    Denied,
    /// Allowed, but eight other phones are paired.
    Full,
    /// Allowed, but the store could not be written.
    Failed,
}

pub struct Pairing {
    pc_id: Id,
    current: Option<Attempt>,
    finished: VecDeque<Finished>,
    last_request: HashMap<IpAddr, Instant>,
}

impl Pairing {
    pub fn new(pc_id: Id) -> Pairing {
        Pairing {
            pc_id,
            current: None,
            finished: VecDeque::new(),
            last_request: HashMap::new(),
        }
    }

    /// The question that is up, if any.
    pub fn open_prompt(&self) -> Option<u64> {
        self.current
            .as_ref()
            .and_then(|a| a.prompt.as_ref())
            .map(|p| p.id)
    }

    fn rate_ok(&mut self, ip: IpAddr, now: Instant) -> bool {
        if let Some(last) = self.last_request.get(&ip)
            && now.saturating_duration_since(*last) < REQUEST_INTERVAL
        {
            return false;
        }
        if self.last_request.len() >= RATE_TABLE_MAX {
            self.last_request
                .retain(|_, at| now.saturating_duration_since(*at) < REQUEST_INTERVAL);
            if self.last_request.len() >= RATE_TABLE_MAX {
                self.last_request.clear();
            }
        }
        self.last_request.insert(ip, now);
        true
    }

    fn respond(&self, pc_pub: Option<&Point>, state: PairState) -> [u8; PAIR_RESP_SIZE] {
        pair_response(&self.pc_id, pc_pub, state, None)
    }

    /// One datagram that starts with the PAIR_REQ magic, from `ip`.
    pub fn handle(&mut self, datagram: &[u8], ip: IpAddr, now: Instant) -> Handled {
        if !is_pair_request(datagram) || !self.rate_ok(ip.to_canonical(), now) {
            return Handled::default();
        }
        let Some(request) = PairRequest::parse(datagram) else {
            return Handled::default();
        };
        self.expire(now);
        let (id, public) = (request.phone_id, request.phone_pub);
        if let Some(done) = self
            .finished
            .iter()
            .find(|f| f.phone_id == id && f.phone_pub == public)
        {
            return Handled {
                reply: Some(done.response),
                ask: None,
            };
        }
        let mut ask = None;
        if let Some(attempt) = self.current.as_mut()
            && attempt.is(&id, &public)
        {
            if attempt.prompt.is_none() && request.phase == Phase::Reveal {
                ask = reveal(attempt, &request.field, now);
            }
            let pc_pub = attempt.pc_pub;
            return Handled {
                reply: Some(self.respond(Some(&pc_pub), PairState::Waiting)),
                ask,
            };
        }
        if self.current.is_some() {
            // The open question never changes: a new request only learns that the slot is taken.
            return Handled {
                reply: Some(self.respond(None, PairState::Busy)),
                ask: None,
            };
        }
        if request.phase == Phase::Reveal || parse_point(&public).is_none() {
            // A reveal for an attempt this PC never saw (it restarted), or a bad point.
            return Handled::default();
        }
        let ephemeral = Ephemeral::generate();
        let pc_pub = ephemeral.public;
        self.current = Some(Attempt {
            phone_id: id,
            phone_pub: public,
            name: display_name(&request.name),
            commitment: request.field,
            started: now,
            ephemeral: Some(ephemeral),
            pc_pub,
            prompt: None,
        });
        Handled {
            reply: Some(self.respond(Some(&pc_pub), PairState::Waiting)),
            ask: None,
        }
    }

    /// The player's answer to question `id`. An answer for a question that has closed does
    /// nothing (`None`). Allowed, the phone is stored before the accepted state can go out.
    pub fn answer(
        &mut self,
        id: u64,
        allow: bool,
        store: &mut LudusStore,
        now: Instant,
    ) -> Option<Answer> {
        let attempt = self.current.as_ref()?;
        let open = attempt.prompt.as_ref().filter(|p| p.id == id)?;
        let answer = if !allow {
            Answer::Denied
        } else if !store.has(&attempt.phone_id) && store.len() >= MAX_DEVICES {
            Answer::Full
        } else {
            match store.add(attempt.phone_id, &attempt.name, open.key) {
                Ok(()) => Answer::Paired,
                Err(error) => {
                    tracing::warn!("ludusctrl: could not save the phone: {error}");
                    Answer::Failed
                }
            }
        };
        let state = if answer == Answer::Paired {
            PairState::Accepted
        } else {
            PairState::Denied
        };
        self.finish(state, now);
        Some(answer)
    }

    fn finish(&mut self, state: PairState, now: Instant) {
        let Some(attempt) = self.current.take() else {
            return;
        };
        let key = attempt.prompt.as_ref().map(|p| p.key);
        let response = pair_response(&self.pc_id, Some(&attempt.pc_pub), state, key.as_ref());
        self.finished.push_back(Finished {
            phone_id: attempt.phone_id,
            phone_pub: attempt.phone_pub,
            at: now,
            response,
        });
        while self.finished.len() > FINISHED_MAX {
            self.finished.pop_front();
        }
    }

    /// The server stops: the attempt ends as denied. Returns the question that closed.
    pub fn cancel(&mut self, now: Instant) -> Option<u64> {
        let prompt = self.open_prompt();
        self.finish(PairState::Denied, now);
        prompt
    }

    /// The timers: a question unanswered for a minute and an attempt without a reveal end as
    /// denied. Returns the question that closed, if one did.
    pub fn expire(&mut self, now: Instant) -> Option<u64> {
        self.finished
            .retain(|f| now.saturating_duration_since(f.at) < FINISHED_KEPT);
        let attempt = self.current.as_ref()?;
        let since = |at: Instant| now.saturating_duration_since(at);
        let (late, prompt) = match &attempt.prompt {
            Some(open) => (since(open.opened) >= PROMPT_TIMEOUT, Some(open.id)),
            None => (since(attempt.started) >= REVEAL_TIMEOUT, None),
        };
        if !late {
            return None;
        }
        self.finish(PairState::Denied, now);
        prompt
    }
}

/// A reveal for the current attempt: on a matching nonce the key and the code are derived, the
/// PC's private key is dropped and the question opens. A wrong nonce is ignored, so a third party
/// who saw the commit cannot cancel the attempt with it.
fn reveal(attempt: &mut Attempt, nonce: &[u8; NONCE_SIZE], now: Instant) -> Option<PairingRequest> {
    if !constant_time_eq(&commitment(nonce), &attempt.commitment) {
        return None;
    }
    let phone = parse_point(&attempt.phone_pub)?;
    let ephemeral = attempt.ephemeral.take()?;
    let z = ephemeral.shared(&phone);
    let key = link_key(&z, &attempt.phone_pub, &attempt.pc_pub);
    let code = confirmation_code(&z, &attempt.phone_pub, &attempt.pc_pub, nonce);
    let id = random_session();
    attempt.prompt = Some(Open {
        id,
        opened: now,
        key,
    });
    Some(PairingRequest {
        id,
        device_name: attempt.name.clone(),
        code: format_code(code),
    })
}

#[cfg(test)]
mod tests {
    use super::super::{
        crypto::{kid, mac_ok, tests::*},
        wire::{pair_response_signed, read_pair_response},
    };
    use super::*;

    const PHONE_ID: Id = [1, 2, 3, 4, 5, 6, 7, 8];
    const PC_ID: Id = [0xa1; 8];

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([192, 168, 1, last])
    }

    /// A phone with the golden key pair and nonce.
    fn request(phase: Phase) -> [u8; 128] {
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
        .encode()
    }

    fn other(phase: Phase) -> [u8; 128] {
        let mut bytes = request(phase);
        bytes[2] = 9;
        bytes
    }

    struct Rig {
        pairing: Pairing,
        store: LudusStore,
        now: Instant,
        _dir: tempfile::TempDir,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        Rig {
            pairing: Pairing::new(PC_ID),
            store: LudusStore::load(&dir.path().join("LudusCtrl.json")),
            now: Instant::now(),
            _dir: dir,
        }
    }

    impl Rig {
        /// Sends `bytes` one second after the last request, from `ip`.
        fn send(&mut self, bytes: &[u8], from: IpAddr) -> Handled {
            self.now += REQUEST_INTERVAL;
            self.pairing.handle(bytes, from, self.now)
        }

        fn state(handled: &Handled) -> PairState {
            read_pair_response(&handled.reply.expect("a reply"))
                .unwrap()
                .2
        }

        /// Commit and reveal: the question is up.
        fn ask(&mut self) -> PairingRequest {
            let first = self.send(&request(Phase::Commit), ip(2));
            assert_eq!(Rig::state(&first), PairState::Waiting);
            let reveal = self.send(&request(Phase::Reveal), ip(2));
            assert_eq!(Rig::state(&reveal), PairState::Waiting);
            reveal.ask.expect("the question")
        }
    }

    /// The code and key the phone derives from the PC key it heard.
    fn phone_side(response: &[u8; 84]) -> (String, LinkKey) {
        let (_, pc_pub, _, _) = read_pair_response(response).unwrap();
        let phone = phone();
        let z = phone.shared(&parse_point(&pc_pub).unwrap());
        (
            format_code(confirmation_code(&z, &phone.public, &pc_pub, &NONCE)),
            link_key(&z, &phone.public, &pc_pub),
        )
    }

    #[test]
    fn commit_reveal_allow_gives_both_sides_the_same_code_and_key() {
        let mut rig = rig();
        let first = rig.send(&request(Phase::Commit), ip(2));
        assert!(first.ask.is_none(), "no question before the reveal");
        let reveal = rig.send(&request(Phase::Reveal), ip(2));
        let question = reveal.ask.expect("the question");
        let (code, key) = phone_side(&first.reply.unwrap());
        assert_eq!(question.code, code);
        assert_eq!(question.device_name, "Pixel 8");
        assert_eq!(rig.pairing.open_prompt(), Some(question.id));

        // Resends while the question is up keep waiting and ask nothing new.
        let again = rig.send(&request(Phase::Reveal), ip(2));
        assert_eq!(Rig::state(&again), PairState::Waiting);
        assert!(again.ask.is_none());

        let answer = rig
            .pairing
            .answer(question.id, true, &mut rig.store, rig.now);
        assert_eq!(answer, Some(Answer::Paired));
        assert_eq!(
            rig.store.phones()[0].key,
            key,
            "stored before state 1 goes out"
        );
        assert_eq!(rig.store.phones()[0].kid, kid(&key));
        let done = rig.send(&request(Phase::Reveal), ip(2)).reply.unwrap();
        let (id, _, state, tag) = read_pair_response(&done).unwrap();
        assert_eq!((id, state), (PC_ID, PairState::Accepted));
        assert!(mac_ok(&key, pair_response_signed(&done), &tag));
        assert_eq!(rig.pairing.open_prompt(), None);
    }

    #[test]
    fn a_wrong_reveal_is_ignored_and_the_right_one_still_works() {
        let mut rig = rig();
        rig.send(&request(Phase::Commit), ip(2));
        let mut wrong = request(Phase::Reveal);
        wrong[109] ^= 0xFF;
        let handled = rig.send(&wrong, ip(3));
        assert_eq!(Rig::state(&handled), PairState::Waiting);
        assert!(handled.ask.is_none());
        assert!(rig.send(&request(Phase::Reveal), ip(2)).ask.is_some());
    }

    #[test]
    fn deny_and_a_stale_answer() {
        let mut rig = rig();
        let question = rig.ask();
        assert_eq!(
            rig.pairing
                .answer(question.id + 1, true, &mut rig.store, rig.now),
            None,
            "an answer meant for another question approves nothing"
        );
        assert_eq!(
            rig.pairing
                .answer(question.id, false, &mut rig.store, rig.now),
            Some(Answer::Denied)
        );
        assert!(rig.store.is_empty(), "nothing is stored on Deny");
        let resend = rig.send(&request(Phase::Reveal), ip(2));
        let (_, _, state, tag) = read_pair_response(&resend.reply.unwrap()).unwrap();
        assert_eq!((state, tag), (PairState::Denied, [0; 8]));
        assert_eq!(
            rig.pairing
                .answer(question.id, true, &mut rig.store, rig.now),
            None
        );
    }

    #[test]
    fn one_attempt_at_a_time() {
        let mut rig = rig();
        rig.ask();
        let busy = rig.send(&other(Phase::Commit), ip(3)).reply.unwrap();
        let (_, pc_pub, state, _) = read_pair_response(&busy).unwrap();
        assert_eq!((state, pc_pub), (PairState::Busy, [0; 65]));
        // The same phone with a new key is another attempt too.
        let mut new_key = request(Phase::Commit);
        new_key[10..75].copy_from_slice(&pc().public);
        let handled = rig.send(&new_key, ip(2));
        assert_eq!(Rig::state(&handled), PairState::Busy);
        assert!(rig.pairing.open_prompt().is_some(), "the question stays");
    }

    #[test]
    fn a_reveal_nobody_asked_for_gets_no_reply() {
        let mut rig = rig();
        assert!(rig.send(&request(Phase::Reveal), ip(2)).reply.is_none());
    }

    #[test]
    fn requests_are_limited_per_address() {
        let mut rig = rig();
        let now = rig.now;
        assert!(
            rig.pairing
                .handle(&request(Phase::Commit), ip(2), now)
                .reply
                .is_some()
        );
        let soon = now + Duration::from_millis(999);
        assert!(
            rig.pairing
                .handle(&request(Phase::Commit), ip(2), soon)
                .reply
                .is_none()
        );
        assert!(
            rig.pairing
                .handle(&other(Phase::Commit), ip(3), soon)
                .reply
                .is_some(),
            "another address has its own second"
        );
        let later = now + REQUEST_INTERVAL;
        assert!(
            rig.pairing
                .handle(&request(Phase::Commit), ip(2), later)
                .reply
                .is_some()
        );
        // The port is not part of the key: an IPv4-mapped form is the same address.
        let mapped: IpAddr = "::ffff:192.168.1.2".parse().unwrap();
        assert!(
            rig.pairing
                .handle(&request(Phase::Commit), mapped, later)
                .reply
                .is_none()
        );
    }

    #[test]
    fn the_timers_end_attempts_as_denied() {
        let mut rig = rig();
        rig.send(&request(Phase::Commit), ip(2));
        // The reveal comes ten seconds after the commit: too late, the answer is state 2.
        rig.now += REVEAL_TIMEOUT - REQUEST_INTERVAL;
        let late = rig.send(&request(Phase::Reveal), ip(2));
        assert_eq!(Rig::state(&late), PairState::Denied);
        assert!(late.ask.is_none());

        let mut rig = super::tests::rig();
        let question = rig.ask();
        let opened = rig.now;
        assert_eq!(rig.pairing.expire(opened + Duration::from_secs(59)), None);
        assert_eq!(
            rig.pairing.expire(opened + PROMPT_TIMEOUT),
            Some(question.id)
        );
        assert_eq!(
            rig.pairing
                .answer(question.id, true, &mut rig.store, rig.now),
            None
        );
        rig.now = opened + PROMPT_TIMEOUT;
        assert_eq!(
            Rig::state(&rig.send(&request(Phase::Reveal), ip(2))),
            PairState::Denied
        );
        // Ninety seconds later the finished answer is gone and a commit starts over.
        rig.now += FINISHED_KEPT;
        let fresh = rig.send(&request(Phase::Commit), ip(2));
        assert_eq!(Rig::state(&fresh), PairState::Waiting);
    }

    #[test]
    fn the_reveal_hold_frees_the_slot() {
        let mut rig = rig();
        rig.send(&request(Phase::Commit), ip(2));
        rig.now += REVEAL_TIMEOUT;
        let handled = rig.send(&other(Phase::Commit), ip(3));
        assert_eq!(Rig::state(&handled), PairState::Waiting, "the slot is free");
    }

    #[test]
    fn eight_phones_fill_the_list() {
        let mut rig = rig();
        for n in 0..MAX_DEVICES as u8 {
            rig.store.add([n + 100; 8], "x", [n; 32]).unwrap();
        }
        let question = rig.ask();
        assert_eq!(
            rig.pairing
                .answer(question.id, true, &mut rig.store, rig.now),
            Some(Answer::Full)
        );
        assert_eq!(rig.store.len(), MAX_DEVICES);
        assert_eq!(
            Rig::state(&rig.send(&request(Phase::Reveal), ip(2))),
            PairState::Denied
        );
    }

    #[test]
    fn bad_points_and_malformed_requests_get_no_reply() {
        let mut rig = rig();
        let mut off_curve = request(Phase::Commit);
        off_curve[74] ^= 1;
        assert!(rig.send(&off_curve, ip(2)).reply.is_none());
        let mut long_name = request(Phase::Commit);
        long_name[75] = 33;
        assert!(rig.send(&long_name, ip(2)).reply.is_none());
        assert!(
            rig.send(&request(Phase::Commit)[..100], ip(2))
                .reply
                .is_none()
        );
    }

    #[test]
    fn names_are_cleaned_for_the_question() {
        assert_eq!(display_name(b"Pixel 8"), "Pixel 8");
        assert_eq!(display_name(b"  Google \t\n Pixel  "), "Google Pixel");
        assert_eq!(
            display_name("Evil\u{202E}lexiP\u{200B}".as_bytes()),
            "EvillexiP"
        );
        assert_eq!(display_name(b"\x01\x7f"), UNNAMED);
        assert_eq!(display_name(b""), UNNAMED);
        assert_eq!(display_name(&[0xFF, b'a']), "\u{FFFD}a");
    }
}
