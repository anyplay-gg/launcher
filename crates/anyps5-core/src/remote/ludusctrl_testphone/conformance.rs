//! The checks a LudusCtrl receiver must pass, run by a [`TestPhone`] against a live receiver.
//!
//! The receiver's test starts it on loopback with a fake virtual pad, wraps it in a
//! [`PcUnderTest`] and calls [`run_conformance`]:
//!
//! ```ignore
//! let beacon = ludusctrl_testphone::beacon_listener(0)?; // the receiver beacons to this port
//! let mut rig = MyRig::start(beacon.local_addr()?.port(), beacon); // implements PcUnderTest
//! let report = run_conformance(&mut rig).unwrap_or_else(|failure| panic!("{failure}"));
//! ```
//!
//! A run takes about 20 s, most of it the spec's own timers (the 1 s PAIR_REQ rate limit, the
//! 2 s beacon interval, the 1.5 s watchdog). It stops at the first failed check and names it.
//! [`RemoteReceiver`] drives a receiver in another process the same way.

use std::{
    io::{self, BufRead, BufReader, Write},
    net::{SocketAddr, TcpStream, UdpSocket},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use super::*;

/// The receiver as the harness sees it. Only [`PcUnderTest::input_addr`] touches the network;
/// the rest is what a person at the PC would see or do.
pub trait PcUnderTest {
    /// The receiver's UDP input port (pairing, input, acks).
    fn input_addr(&self) -> SocketAddr;

    /// A socket the receiver's beacon reaches (see [`beacon_listener`]), handed over once.
    /// None skips the beacon checks.
    fn beacon_socket(&mut self) -> Option<UdpSocket> {
        None
    }

    /// The codes of every pairing prompt the PC has opened so far, oldest first, whether or not
    /// it is still open.
    fn prompt_codes(&mut self) -> Vec<u32>;

    /// Clicks Allow on the open prompt. A receiver that allows on its own (the test mode of the
    /// spec) can do nothing here.
    fn allow(&mut self);

    /// What the receiver last wrote to its virtual pad, neutral after a release. None when it
    /// cannot be observed: the pad checks are then skipped.
    fn pad(&mut self) -> Option<PadState>;

    /// Whether the receiver has a working virtual pad, so its acks must say pad-live.
    fn pad_live(&self) -> bool {
        true
    }
}

pub mod check {
    pub const BEACON: &str = "beacon: seen, advertising the input port and a pc_id";
    pub const COMMIT: &str = "pairing: the commit is answered with state 0 and a valid PC key";
    pub const PC_ID: &str = "pairing: the pc_id is the beacon's";
    pub const WRONG_REVEAL: &str = "pairing: a reveal that misses the commitment is ignored";
    pub const PROMPT_CODE: &str = "pairing: the reveal opens one prompt showing the phone's code";
    pub const ACCEPTED: &str = "pairing: Allow answers state 1 with a MAC under K";
    pub const FINISHED: &str = "pairing: a resend after Allow gets state 1 again";
    pub const NOT_PAIRED: &str = "an unknown KID gets an unsigned not-paired ack";
    pub const V1_MISMATCH: &str = "a v1 packet gets an unsigned version-mismatch ack";
    pub const FIRST_INPUT: &str = "input: the first packet of a session is accepted and acked";
    pub const PAD_WRITTEN: &str = "input: the snapshot reaches the pad, bit 11 masked";
    pub const IN_ORDER: &str = "input: newer packets are accepted in order, each counted";
    pub const BURST: &str = "input: a burst leaves the pad on its last snapshot";
    pub const ROAMING: &str = "input: a new source port is followed, acks go there";
    pub const BEACON_PAUSED: &str = "beacon: paused while a phone is linked";
    pub const REPLAY: &str = "stale: a replayed older packet is dropped without an ack";
    pub const SAME_SEQ: &str = "stale: a second packet with the newest seq is dropped";
    pub const HALF_WINDOW: &str = "stale: a seq 2^31 ahead counts as older and is dropped";
    pub const BAD_MAC: &str = "forged: a packet with a wrong MAC is dropped without an ack";
    pub const TAMPERED: &str = "forged: a packet changed after signing is dropped";
    pub const TRUNCATED: &str = "length: a 35-byte packet is dropped";
    pub const PADDED: &str = "length: a 37-byte packet is dropped";
    pub const WRONG_KID: &str = "a wrong KID gets a not-paired ack and is not applied";
    pub const OTHER_VERSION: &str = "a version 3 packet gets a mismatch ack and is not applied";
    pub const NEW_SESSION: &str = "session: a higher session is accepted";
    pub const OLD_SESSION: &str = "session: a newer seq in an older session is dropped";
    pub const WATCHDOG_HOLD: &str = "watchdog: the pad is held 1.2 s after the last packet";
    pub const WATCHDOG_RELEASE: &str = "watchdog: the pad is released by 2.25 s";
    pub const BEACON_RESUMED: &str = "beacon: resumes once the link drops";
    pub const AFTER_WATCHDOG: &str = "session: the same session carries on after the watchdog";
    pub const NO_PAD: &str = "input: flags bit 0 clear releases the pad";
    pub const FINAL_NEUTRAL: &str = "input: the final neutral packet releases the pad";
}

/// The first check that failed.
#[derive(Debug)]
pub struct Failure {
    pub check: &'static str,
    pub detail: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LudusCtrl conformance failed: {}: {}",
            self.check, self.detail
        )
    }
}

impl std::error::Error for Failure {}

#[derive(Debug, Default)]
pub struct Report {
    pub passed: Vec<&'static str>,
    pub skipped: Vec<&'static str>,
    pub pc_id: Id,
    /// The code both sides showed.
    pub code: u32,
    /// From the last packet to the pad going neutral.
    pub watchdog_release: Option<Duration>,
}

type Outcome<T = ()> = Result<T, Failure>;

fn fail<T>(check: &'static str, detail: impl Into<String>) -> Outcome<T> {
    Err(Failure {
        check,
        detail: detail.into(),
    })
}

fn net(check: &'static str) -> impl FnOnce(io::Error) -> Failure {
    move |e| Failure {
        check,
        detail: format!("socket error: {e}"),
    }
}

/// How long the harness waits for an ack it expects, or for one it must not get. The waits for
/// what must come stay under the watchdog and return as soon as it does, so a busy machine only
/// slows them.
const ACK_WAIT: Duration = Duration::from_millis(1200);
const NO_ACK_WAIT: Duration = Duration::from_millis(350);
const PAD_WAIT: Duration = Duration::from_millis(1200);

/// One run at a time in a process: the checks time the receiver against the spec's timers, and
/// runs side by side on a busy test machine starve each other's receive loops.
static ONE_RUN: Mutex<()> = Mutex::new(());

/// Runs every check against `pc`, in order: beacon, pairing, unsigned acks, input, stale and
/// forged packets, sessions, the watchdog, release.
pub fn run_conformance(pc: &mut dyn PcUnderTest) -> Result<Report, Failure> {
    let _one_run = ONE_RUN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let phone = TestPhone::new(pc.input_addr()).map_err(net("set up the phone"))?;
    let beacon = pc.beacon_socket();
    let observable = pc.pad().is_some();
    let mut run = Run {
        pc,
        phone,
        beacon,
        observable,
        report: Report::default(),
        last_ack_send: None,
        last_send: Instant::now(),
        first: None,
        current: PadState::NEUTRAL,
    };
    run.all()?;
    Ok(run.report)
}

struct Run<'a> {
    pc: &'a mut dyn PcUnderTest,
    phone: TestPhone,
    beacon: Option<UdpSocket>,
    observable: bool,
    report: Report,
    /// When the last packet that should be acked went out, to keep clear of the ack limiter.
    last_ack_send: Option<Instant>,
    last_send: Instant,
    /// The first input packet, replayed later.
    first: Option<(u32, [u8; INPUT_SIZE])>,
    /// What the pad holds between the stale and forged checks.
    current: PadState,
}

impl Run<'_> {
    fn all(&mut self) -> Outcome {
        let beacon_id = self.beacon_seen()?;
        let paired = self.pairing(beacon_id)?;
        self.phone.set_paired(paired);
        self.unsigned_acks()?;
        self.input()?;
        self.stale_and_forged()?;
        self.watchdog()?;
        self.release()
    }

    fn pass(&mut self, check: &'static str) {
        self.report.passed.push(check);
    }

    fn skip(&mut self, check: &'static str) {
        self.report.skipped.push(check);
    }

    // --- beacon ------------------------------------------------------------------------------

    fn beacon_seen(&mut self) -> Outcome<Option<Id>> {
        let port = self.phone.pc().port();
        let Some(socket) = &self.beacon else {
            self.skip(check::BEACON);
            return Ok(None);
        };
        // The interval is 2 s; a little more covers a beacon that just went out.
        let Some((beacon, _)) =
            wait_beacon(socket, port, Duration::from_millis(4500)).map_err(net(check::BEACON))?
        else {
            return fail(check::BEACON, format!("no beacon for port {port} in 4.5 s"));
        };
        let Some(pc_id) = beacon.pc_id else {
            return fail(check::BEACON, "an 8-byte beacon: a 1.2 PC sends its pc_id");
        };
        self.pass(check::BEACON);
        Ok(Some(pc_id))
    }

    // --- pairing -----------------------------------------------------------------------------

    fn exchange(
        &mut self,
        check: &'static str,
        attempt: &mut PairingAttempt,
        request: &[u8],
    ) -> Outcome<Option<(PairResp, Option<PairEvent>)>> {
        self.phone
            .exchange(attempt, request, PAIR_RESEND)
            .map_err(net(check))
    }

    fn pairing(&mut self, beacon_id: Option<Id>) -> Outcome<Paired> {
        let mut attempt = PairingAttempt::new(self.phone.phone_id, &self.phone.name);

        let mut commit = None;
        for _ in 0..4 {
            let request = attempt.request();
            match self.exchange(check::COMMIT, &mut attempt, &request)? {
                Some((resp, Some(PairEvent::Code(_)))) => {
                    commit = Some(resp);
                    break;
                }
                Some((resp, event)) => {
                    return fail(
                        check::COMMIT,
                        format!(
                            "answered with state {}, read by the phone as {event:?}",
                            resp.state
                        ),
                    );
                }
                None => {}
            }
        }
        let Some(commit) = commit else {
            return fail(check::COMMIT, "no PAIR_RESP to four commits");
        };
        self.pass(check::COMMIT);

        match beacon_id {
            Some(id) if id != commit.pc_id => {
                return fail(
                    check::PC_ID,
                    format!("beacon {}, PAIR_RESP {}", hex(&id), hex(&commit.pc_id)),
                );
            }
            Some(_) => self.pass(check::PC_ID),
            None => self.skip(check::PC_ID),
        }

        let before = self.pc.prompt_codes().len();
        let mut wrong = *attempt.nonce();
        wrong[0] ^= 0x01;
        let request = attempt.request_with(Phase::Reveal, &wrong);
        match self.exchange(check::WRONG_REVEAL, &mut attempt, &request)? {
            Some((resp, Some(PairEvent::Code(_)))) if resp.state == pair_state::WAITING => {}
            other => {
                return fail(check::WRONG_REVEAL, format!("want state 0, got {other:?}"));
            }
        }
        thread::sleep(Duration::from_millis(200));
        let after = self.pc.prompt_codes();
        if after.len() != before {
            return fail(
                check::WRONG_REVEAL,
                format!("prompts {before} -> {after:?}"),
            );
        }
        self.pass(check::WRONG_REVEAL);

        let code = attempt.code().expect("the PC key is fixed");
        let request = attempt.request();
        let mut paired = match self.exchange(check::PROMPT_CODE, &mut attempt, &request)? {
            Some((_, Some(PairEvent::Code(_)))) => None,
            Some((_, Some(PairEvent::Accepted(paired)))) => Some(paired),
            other => return fail(check::PROMPT_CODE, format!("the reveal got {other:?}")),
        };
        let deadline = Instant::now() + Duration::from_secs(1);
        let prompts = loop {
            let prompts = self.pc.prompt_codes();
            if prompts.len() > before || Instant::now() >= deadline {
                break prompts;
            }
            thread::sleep(Duration::from_millis(20));
        };
        match &prompts[before.min(prompts.len())..] {
            [shown] if *shown == code => self.pass(check::PROMPT_CODE),
            [shown] => {
                return fail(
                    check::PROMPT_CODE,
                    format!(
                        "the PC shows {}, the phone {}",
                        format_code(*shown),
                        format_code(code)
                    ),
                );
            }
            new => return fail(check::PROMPT_CODE, format!("new prompts {new:?}")),
        }
        self.report.code = code;

        if paired.is_none() {
            self.pc.allow();
            for _ in 0..4 {
                let request = attempt.request();
                match self.exchange(check::ACCEPTED, &mut attempt, &request)? {
                    Some((_, Some(PairEvent::Accepted(p)))) => {
                        paired = Some(p);
                        break;
                    }
                    Some((_, Some(PairEvent::Code(_)))) | None => {}
                    Some((resp, event)) => {
                        return fail(
                            check::ACCEPTED,
                            format!("state {} after Allow, read as {event:?}", resp.state),
                        );
                    }
                }
            }
        }
        let Some(paired) = paired else {
            return fail(
                check::ACCEPTED,
                "no state 1 with a valid MAC within 4 resends",
            );
        };
        if paired.code != code || paired.pc_id != commit.pc_id {
            return fail(
                check::ACCEPTED,
                format!("{paired:?} does not match the attempt"),
            );
        }
        self.pass(check::ACCEPTED);

        let request = attempt.request();
        match self.exchange(check::FINISHED, &mut attempt, &request)? {
            Some((_, Some(PairEvent::Accepted(again)))) if again == paired => {}
            other => return fail(check::FINISHED, format!("got {other:?}")),
        }
        self.pass(check::FINISHED);

        self.report.pc_id = paired.pc_id;
        Ok(paired)
    }

    // --- unsigned acks -----------------------------------------------------------------------

    fn unsigned_acks(&mut self) -> Outcome {
        let stranger: Key = random();
        let input = self.phone.input(PadState::NEUTRAL);
        let packet = input.encode(1, &stranger, &kid(&stranger));
        self.send(check::NOT_PAIRED, input.seq, &packet)?;
        match self
            .phone
            .ack_for(input.seq, ACK_WAIT)
            .map_err(net(check::NOT_PAIRED))?
        {
            Some(Ack::NotPaired { .. }) => self.pass(check::NOT_PAIRED),
            other => return fail(check::NOT_PAIRED, format!("got {other:?}")),
        }

        let input = self.phone.input(PadState::NEUTRAL);
        self.send(check::V1_MISMATCH, input.seq, &input.body(1))?;
        match self
            .phone
            .ack_for(input.seq, ACK_WAIT)
            .map_err(net(check::V1_MISMATCH))?
        {
            Some(Ack::VersionMismatch {
                version: PROTOCOL_VERSION,
                ..
            }) => {
                self.pass(check::V1_MISMATCH);
            }
            other => return fail(check::V1_MISMATCH, format!("got {other:?}")),
        }
        Ok(())
    }

    // --- input -------------------------------------------------------------------------------

    fn send(&mut self, check: &'static str, seq: u32, data: &[u8]) -> Outcome {
        self.last_send = Instant::now();
        self.phone.send_with_seq(seq, data).map_err(net(check))
    }

    /// Waits until a new packet can be acked: the PC sends one signed ack per 200 ms per phone.
    fn clear_ack_limiter(&self) {
        if let Some(last) = self.last_ack_send {
            let due = last + ACK_INTERVAL + Duration::from_millis(30);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                thread::sleep(wait);
            }
        }
    }

    /// Sends `input`, which must be accepted: a signed ack for its seq with the right pad-live
    /// bit, and `expect` on the pad. Returns the ack's received count and the packet.
    fn accepted(
        &mut self,
        check: &'static str,
        input: Input,
        expect: PadState,
    ) -> Outcome<(u32, [u8; INPUT_SIZE])> {
        let acked = self.acked(check, input)?;
        self.pad_becomes(check, expect)?;
        Ok(acked)
    }

    /// [`Run::accepted`] without the pad check.
    fn acked(&mut self, check: &'static str, input: Input) -> Outcome<(u32, [u8; INPUT_SIZE])> {
        self.clear_ack_limiter();
        let packet = self.phone.packet(&input);
        self.send(check, input.seq, &packet)?;
        self.last_ack_send = Some(self.last_send);
        let received = match self
            .phone
            .ack_for(input.seq, ACK_WAIT)
            .map_err(net(check))?
        {
            Some(Ack::Signed {
                received,
                pad_live,
                version_mismatch: false,
                ..
            }) => {
                if pad_live != self.pc.pad_live() {
                    return fail(check, format!("seq {}: pad-live {pad_live}", input.seq));
                }
                received
            }
            other => {
                return fail(
                    check,
                    format!("seq {}: want a signed ack, got {other:?}", input.seq),
                );
            }
        };
        Ok((received, packet))
    }

    fn accept(&mut self, check: &'static str, pad: PadState) -> Outcome<(u32, [u8; INPUT_SIZE])> {
        let input = self.phone.input(pad);
        self.accepted(check, input, pad.masked())
    }

    fn pad_becomes(&mut self, check: &'static str, want: PadState) -> Outcome {
        if !self.observable {
            return Ok(());
        }
        let deadline = Instant::now() + PAD_WAIT;
        loop {
            let pad = self.pc.pad();
            if pad == Some(want) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return fail(check, format!("pad {pad:?}, want {want:?}"));
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn pad_is(&mut self, check: &'static str, want: PadState) -> Outcome {
        if !self.observable {
            return Ok(());
        }
        match self.pc.pad() {
            Some(pad) if pad == want => Ok(()),
            pad => fail(check, format!("pad {pad:?}, want {want:?}")),
        }
    }

    fn input(&mut self) -> Outcome {
        self.phone.start_session();

        let a = PadState {
            buttons: button::A | button::DPAD_UP | button::RESERVED,
            left_x: -32768,
            left_y: 32767,
            right_x: 12345,
            right_y: -9876,
            left_trigger: 255,
            right_trigger: 128,
        };
        let input = self.phone.input(a);
        let (r0, first_packet) = self.acked(check::FIRST_INPUT, input)?;
        self.pass(check::FIRST_INPUT);
        self.pad_becomes(check::PAD_WRITTEN, a.masked())?;
        if self.observable {
            self.pass(check::PAD_WRITTEN);
        } else {
            self.skip(check::PAD_WRITTEN);
        }
        self.first = Some((input.seq, first_packet));

        let b = PadState {
            buttons: button::B | button::RIGHT_SHOULDER,
            left_x: 1,
            left_y: -1,
            ..PadState::NEUTRAL
        };
        let (r1, _) = self.accept(check::IN_ORDER, b)?;
        if r1 != r0.wrapping_add(1) {
            return fail(check::IN_ORDER, format!("received {r0} then {r1}"));
        }
        self.pass(check::IN_ORDER);

        for i in 1..=5 {
            let pad = PadState {
                left_x: i * 1000,
                right_trigger: i as u8,
                ..b
            };
            let input = self.phone.input(pad);
            let packet = self.phone.packet(&input);
            self.send(check::BURST, input.seq, &packet)?;
            thread::sleep(Duration::from_millis(3));
        }
        self.last_ack_send = Some(self.last_send);
        let c = PadState {
            buttons: button::Y,
            right_y: 32767,
            ..PadState::NEUTRAL
        };
        let (r2, _) = self.accept(check::BURST, c)?;
        if r2 != r0.wrapping_add(7) {
            return fail(
                check::BURST,
                format!("received {r0} at the first packet, {r2} after 7 more"),
            );
        }
        self.pass(check::BURST);

        self.phone.rebind().map_err(net(check::ROAMING))?;
        let d = PadState {
            buttons: button::X | button::START,
            left_y: -32768,
            ..PadState::NEUTRAL
        };
        self.accept(check::ROAMING, d)?;
        self.pass(check::ROAMING);
        self.current = d;

        // Keepalives for longer than the 2 s beacon interval.
        if let Some(socket) = &self.beacon {
            drain(socket).map_err(net(check::BEACON_PAUSED))?;
        }
        for _ in 0..6 {
            thread::sleep(Duration::from_millis(450));
            let input = self.phone.input(d);
            let packet = self.phone.packet(&input);
            self.send(check::BEACON_PAUSED, input.seq, &packet)?;
        }
        self.last_ack_send = Some(self.last_send);
        let port = self.phone.pc().port();
        match &self.beacon {
            Some(socket) => {
                let heard = wait_beacon(socket, port, Duration::from_millis(50))
                    .map_err(net(check::BEACON_PAUSED))?;
                if heard.is_some() {
                    return fail(check::BEACON_PAUSED, "a beacon while linked");
                }
                self.pass(check::BEACON_PAUSED);
            }
            None => self.skip(check::BEACON_PAUSED),
        }
        Ok(())
    }

    // --- stale and forged --------------------------------------------------------------------

    /// Refreshes the link with an accepted packet, then sends what `build` makes from the phone
    /// as it is after the refresh: (seq, datagram, the ack it must get or None for no ack). It
    /// must not change the pad. Building after the refresh keeps each packet's seq newer than
    /// everything accepted, unless staleness is the point, so a drop has one cause only.
    fn refused(
        &mut self,
        check: &'static str,
        build: impl FnOnce(&mut TestPhone) -> (u32, Vec<u8>, Option<Ack>),
    ) -> Outcome {
        let current = self.current;
        self.accept(check, current)?;
        let (seq, data, ack) = build(&mut self.phone);
        self.clear_ack_limiter();
        self.send(check, seq, &data)?;
        let wait = if ack.is_some() { ACK_WAIT } else { NO_ACK_WAIT };
        let got = self.phone.ack_for(seq, wait).map_err(net(check))?;
        if got != ack {
            return fail(check, format!("seq {seq}: want {ack:?}, got {got:?}"));
        }
        self.pad_is(check, current.masked())?;
        self.pass(check);
        Ok(())
    }

    fn stale_and_forged(&mut self) -> Outcome {
        // What every refused packet carries, so applying one shows on the pad.
        let e = PadState {
            buttons: button::GUIDE | button::BACK,
            left_trigger: 77,
            ..PadState::NEUTRAL
        };
        let signed = move |phone: &mut TestPhone, change: fn(&mut [u8; INPUT_SIZE])| {
            let input = phone.input(e);
            let mut packet = phone.packet(&input);
            change(&mut packet);
            (input.seq, packet.to_vec(), None)
        };

        let (first_seq, first_packet) = self.first.expect("the input checks ran");
        self.refused(check::REPLAY, |_| (first_seq, first_packet.to_vec(), None))?;

        self.same_seq(check::SAME_SEQ)?;

        self.refused(check::HALF_WINDOW, |phone| {
            let newest = phone.next_seq().wrapping_sub(1);
            let input = Input {
                seq: newest.wrapping_add(0x8000_0000),
                ..phone.input(e)
            };
            (input.seq, phone.packet(&input).to_vec(), None)
        })?;

        self.refused(check::BAD_MAC, |phone| {
            signed(phone, |p| p[INPUT_SIZE - 1] ^= 0x01)
        })?;
        self.refused(check::TAMPERED, |phone| signed(phone, |p| p[6] ^= 0x02))?;
        self.refused(check::TRUNCATED, |phone| {
            let (seq, packet, ack) = signed(phone, |_| {});
            (seq, packet[..INPUT_SIZE - 1].to_vec(), ack)
        })?;
        self.refused(check::PADDED, |phone| {
            let (seq, mut packet, ack) = signed(phone, |_| {});
            packet.push(0);
            (seq, packet, ack)
        })?;

        self.refused(check::WRONG_KID, |phone| {
            let paired = *phone.paired().expect("paired");
            let input = phone.input(e);
            let mut wrong_kid = paired.kid;
            wrong_kid[0] ^= 0x80;
            let packet = input.encode(phone.session(), &paired.key, &wrong_kid);
            (
                input.seq,
                packet.to_vec(),
                Some(Ack::NotPaired { seq: input.seq }),
            )
        })?;

        self.refused(check::OTHER_VERSION, |phone| {
            let (seq, mut packet, _) = signed(phone, |_| {});
            packet[1] = 3;
            let ack = Ack::VersionMismatch {
                seq,
                version: PROTOCOL_VERSION,
            };
            (seq, packet, Some(ack))
        })?;

        let old_session = self.phone.session();
        self.phone.start_session();
        let f = PadState {
            buttons: button::LEFT_THUMB | button::RIGHT_THUMB,
            right_x: -32768,
            ..PadState::NEUTRAL
        };
        self.accept(check::NEW_SESSION, f)?;
        self.pass(check::NEW_SESSION);
        self.current = f;

        self.refused(check::OLD_SESSION, |phone| {
            let paired = *phone.paired().expect("paired");
            let input = phone.input(e);
            let packet = input.encode(old_session, &paired.key, &paired.kid);
            (input.seq, packet.to_vec(), None)
        })
    }

    /// An accepted packet, then another state signed under the same seq, which must be dropped.
    /// The first one's ack is already taken, so any ack for the seq now is the second's.
    fn same_seq(&mut self, check: &'static str) -> Outcome {
        let current = self.current;
        let refresh = self.phone.input(current);
        self.accepted(check, refresh, current.masked())?;
        let input = Input {
            pad: PadState {
                buttons: button::DPAD_LEFT | button::DPAD_RIGHT,
                ..PadState::NEUTRAL
            },
            ..refresh
        };
        let packet = self.phone.packet(&input);
        self.clear_ack_limiter();
        self.send(check, input.seq, &packet)?;
        let got = self
            .phone
            .ack_for(input.seq, NO_ACK_WAIT)
            .map_err(net(check))?;
        if got.is_some() {
            return fail(check, format!("got {got:?}"));
        }
        self.pad_is(check, current.masked())?;
        self.pass(check);
        Ok(())
    }

    // --- watchdog ----------------------------------------------------------------------------

    fn watchdog(&mut self) -> Outcome {
        let w = PadState {
            buttons: button::X | button::LEFT_SHOULDER,
            left_trigger: 200,
            right_x: 4321,
            ..PadState::NEUTRAL
        };
        self.accept(check::WATCHDOG_HOLD, w)?;
        let last = self.last_send;

        let hold = last + Duration::from_millis(1200);
        if let Some(wait) = hold.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
        if self.observable {
            self.pad_is(check::WATCHDOG_HOLD, w)?;
            self.pass(check::WATCHDOG_HOLD);
            let deadline = last + Duration::from_millis(2250);
            loop {
                let pad = self.pc.pad();
                if pad == Some(PadState::NEUTRAL) {
                    self.report.watchdog_release = Some(last.elapsed());
                    break;
                }
                if Instant::now() >= deadline {
                    return fail(check::WATCHDOG_RELEASE, format!("pad {pad:?} 2.25 s on"));
                }
                thread::sleep(Duration::from_millis(5));
            }
            self.pass(check::WATCHDOG_RELEASE);
        } else {
            self.skip(check::WATCHDOG_HOLD);
            self.skip(check::WATCHDOG_RELEASE);
            thread::sleep(Duration::from_millis(1050));
        }

        let port = self.phone.pc().port();
        match &self.beacon {
            Some(socket) => {
                let heard = wait_beacon(socket, port, Duration::from_millis(2500))
                    .map_err(net(check::BEACON_RESUMED))?;
                if heard.is_none() {
                    return fail(check::BEACON_RESUMED, "no beacon 2.5 s after the release");
                }
                self.pass(check::BEACON_RESUMED);
            }
            None => self.skip(check::BEACON_RESUMED),
        }

        let x = PadState {
            buttons: button::DPAD_DOWN,
            left_x: -1,
            ..PadState::NEUTRAL
        };
        self.accept(check::AFTER_WATCHDOG, x)?;
        self.pass(check::AFTER_WATCHDOG);
        self.current = x;
        Ok(())
    }

    // --- release -----------------------------------------------------------------------------

    fn release(&mut self) -> Outcome {
        let pad = PadState {
            buttons: button::A,
            left_trigger: 9,
            ..PadState::NEUTRAL
        };
        let input = Input {
            pad_connected: false,
            ..self.phone.input(pad)
        };
        self.accepted(check::NO_PAD, input, PadState::NEUTRAL)?;
        self.pass(check::NO_PAD);

        self.accept(check::FINAL_NEUTRAL, pad)?;
        let input = Input::neutral(self.phone.next_seq());
        self.phone.set_next_seq(input.seq.wrapping_add(1));
        self.accepted(check::FINAL_NEUTRAL, input, PadState::NEUTRAL)?;
        self.pass(check::FINAL_NEUTRAL);
        Ok(())
    }
}

// --- a receiver in another process -----------------------------------------------------------

/// A [`PcUnderTest`] for a receiver in another process, driven over a line protocol on TCP.
/// Each request is one line, each answer one line of JSON:
///
/// | Request | Answer |
/// | --- | --- |
/// | `codes` | the prompts' codes, oldest first: `[260329]` |
/// | `allow` | `"ok"` once Allow is clicked on the open prompt |
/// | `pad` | `[buttons, left_x, left_y, right_x, right_y, left_trigger, right_trigger]`, or `null` |
pub struct RemoteReceiver {
    input: SocketAddr,
    control: BufReader<TcpStream>,
    beacon: Option<UdpSocket>,
    pad_live: bool,
}

impl RemoteReceiver {
    pub fn connect(
        input: SocketAddr,
        control: SocketAddr,
        beacon: Option<UdpSocket>,
        pad_live: bool,
    ) -> io::Result<Self> {
        let stream = TcpStream::connect(control)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        Ok(Self {
            input,
            control: BufReader::new(stream),
            beacon,
            pad_live,
        })
    }

    fn ask(&mut self, request: &str) -> serde_json::Value {
        let stream = self.control.get_mut();
        writeln!(stream, "{request}").expect("the control channel takes a request");
        let mut line = String::new();
        self.control
            .read_line(&mut line)
            .expect("the control channel answers");
        serde_json::from_str(&line).expect("the control channel answers JSON")
    }
}

impl PcUnderTest for RemoteReceiver {
    fn input_addr(&self) -> SocketAddr {
        self.input
    }

    fn beacon_socket(&mut self) -> Option<UdpSocket> {
        self.beacon.take()
    }

    fn prompt_codes(&mut self) -> Vec<u32> {
        serde_json::from_value(self.ask("codes")).expect("a list of codes")
    }

    fn allow(&mut self) {
        self.ask("allow");
    }

    fn pad(&mut self) -> Option<PadState> {
        let pad: Option<(u16, i16, i16, i16, i16, u8, u8)> =
            serde_json::from_value(self.ask("pad")).expect("a pad state or null");
        pad.map(
            |(buttons, left_x, left_y, right_x, right_y, lt, rt)| PadState {
                buttons,
                left_x,
                left_y,
                right_x,
                right_y,
                left_trigger: lt,
                right_trigger: rt,
            },
        )
    }

    fn pad_live(&self) -> bool {
        self.pad_live
    }
}
