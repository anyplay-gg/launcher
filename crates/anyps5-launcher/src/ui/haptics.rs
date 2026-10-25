//! Controller vibration (docs/DESIGN.md 6.8): short, soft pulses that answer the actions of 7.3.
//!
//! The pad is driven through `gilrs-core` directly, not through gilrs' force-feedback server: that
//! server works in 50 ms ticks, which makes a 15 ms tap impossible and delays every start by up to
//! a tick. Here a pattern is a list of motor states with their times; a thread of its own sets
//! each one at its moment (the thread asks Windows for a 1 ms timer) and always ends on a stop.
//!
//! * [`Level`] is the "Controller Vibration" setting: off, light, medium (default) or strong.
//! * [`pattern`] is the table: which pulses an action makes. Nothing in it knows about devices.
//! * [`Haptics`] is the handle the shell keeps. It never vibrates while a game has the pad (the
//!   shell asks), never without force feedback (such a pad is skipped), and every pattern, every
//!   interruption, a disconnect and a normal end of the program leave both motors at zero (the
//!   handle joins its thread when it is dropped). A crash is the one exception: on Windows the pad
//!   keeps the last pulse, at most 70 ms long, until the system resets it.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::ui::sound::Cue;

/// The "Controller Vibration" setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Level {
    Off,
    Light,
    #[default]
    Medium,
    Strong,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Off, Level::Light, Level::Medium, Level::Strong];

    /// How much of every pulse's strength reaches the motors.
    pub fn gain(self) -> f32 {
        match self {
            Level::Off => 0.0,
            Level::Light => 0.7,
            Level::Medium => 1.0,
            Level::Strong => 1.5,
        }
    }
}

/// One pulse of a pattern: both motors at a strength from 0 to 1 for a while.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pulse {
    /// Milliseconds after the start of the pattern.
    pub at: u16,
    /// How long it lasts, in milliseconds.
    pub ms: u16,
    /// The light, high motor (the right one on an Xbox pad).
    pub weak: f32,
    /// The heavy, low motor (the left one).
    pub strong: f32,
}

const fn pulse(at: u16, ms: u16, weak: f32, strong: f32) -> Pulse {
    Pulse {
        at,
        ms,
        weak,
        strong,
    }
}

/// What an action feels like, in the order of the sound map. Moving the focus has no pulse: it
/// happens dozens of times a minute, and a motor that answers every one would be noise.
pub fn pattern(cue: Cue) -> &'static [Pulse] {
    // Very light: one short tap of the light motor.
    const SELECT: [Pulse; 1] = [pulse(0, 24, 0.32, 0.0)];
    // Lighter still.
    const BACK: [Pulse; 1] = [pulse(0, 18, 0.22, 0.0)];
    // Two short, firmer taps: "no".
    const ERROR: [Pulse; 2] = [pulse(0, 30, 0.42, 0.14), pulse(90, 30, 0.42, 0.14)];
    // Barely there.
    const BUMP: [Pulse; 1] = [pulse(0, 14, 0.16, 0.0)];
    // A trophy: three taps that swell, the last one carried by the heavy motor.
    const TROPHY: [Pulse; 3] = [
        pulse(0, 24, 0.24, 0.0),
        pulse(70, 30, 0.30, 0.12),
        pulse(150, 70, 0.34, 0.26),
    ];
    // A call: a heavy beat, then a light one, softly.
    const RING: [Pulse; 2] = [pulse(0, 60, 0.0, 0.24), pulse(110, 40, 0.30, 0.0)];
    match cue {
        Cue::Select => &SELECT,
        Cue::Back => &BACK,
        Cue::Error => &ERROR,
        Cue::Bump => &BUMP,
        Cue::Unlocked => &TROPHY,
        Cue::Ring => &RING,
        Cue::Move | Cue::Hover | Cue::Boot => &[],
    }
}

/// The weakest strength a pad's motors show at all (an estimate; to be tuned on hardware). Every
/// pulse is lifted to it *before* the level scales it, so Light stays below Medium for every cue.
pub const FLOOR: f32 = 0.14;

/// One change of the motors: at `at` after the start, both set to these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub at: Duration,
    pub strong: u16,
    pub weak: u16,
}

fn magnitude(strength: f32, gain: f32) -> u16 {
    if strength <= 0.0 || gain <= 0.0 {
        return 0;
    }
    let value = (strength.max(FLOOR) * gain).clamp(0.0, 1.0);
    (value * f32::from(u16::MAX)).round() as u16
}

/// The motor states of a pattern at `level`: each pulse starts and then stops, ending on zero.
pub fn plan(pattern: &[Pulse], level: Level) -> Vec<Step> {
    let gain = level.gain();
    if gain <= 0.0 {
        return Vec::new();
    }
    let mut steps = Vec::with_capacity(pattern.len() * 2);
    for p in pattern {
        steps.push(Step {
            at: Duration::from_millis(u64::from(p.at)),
            strong: magnitude(p.strong, gain),
            weak: magnitude(p.weak, gain),
        });
        steps.push(Step {
            at: Duration::from_millis(u64::from(p.at) + u64::from(p.ms)),
            strong: 0,
            weak: 0,
        });
    }
    steps
}

enum Message {
    Play(Vec<Step>),
    Stop,
}

/// What a silent handle was asked to do (tests read it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Play(Cue),
    Stop,
}

/// The vibration thread and its mailbox.
struct Worker {
    mail: Sender<Message>,
    thread: std::thread::JoinHandle<()>,
}

/// Starts to try this many times before giving up on a backend that never works.
const SPAWN_TRIES: u8 = 3;

/// The shell's handle on the vibration thread, started when the pad is first used.
pub struct Haptics {
    /// The "Controller Vibration" setting.
    pub level: Level,
    /// Tests and the gallery never touch a device: they only note what was asked.
    silent: bool,
    worker: Option<Worker>,
    tries: u8,
    /// What a silent handle was asked to do.
    pub asked: Vec<Asked>,
}

impl Haptics {
    pub fn new() -> Self {
        Self {
            level: Level::default(),
            silent: false,
            worker: None,
            tries: 0,
            asked: Vec::new(),
        }
    }

    pub fn silent() -> Self {
        let mut haptics = Self::new();
        haptics.silent = true;
        haptics
    }

    /// Starts the thread (the backend takes a moment to find the pads) so that the first pulse is
    /// not late. Call when the pad is first used.
    pub fn warm(&mut self) {
        if !self.silent && self.level != Level::Off {
            self.worker();
        }
    }

    /// Makes the pulses of `cue`, replacing whatever is still running.
    pub fn play(&mut self, cue: Cue) {
        let steps = plan(pattern(cue), self.level);
        if steps.is_empty() {
            return;
        }
        if self.silent {
            self.asked.push(Asked::Play(cue));
            return;
        }
        self.send(Message::Play(steps));
    }

    /// Stops at once (a game took the pad, the setting went off).
    pub fn stop(&mut self) {
        if self.silent {
            self.asked.push(Asked::Stop);
        } else if self.worker.is_some() {
            // Nothing runs when no thread was ever started.
            self.send(Message::Stop);
        }
    }

    fn worker(&mut self) -> Option<&Worker> {
        if self.worker.is_none() && self.tries < SPAWN_TRIES {
            self.tries += 1;
            let (mail, messages) = mpsc::channel();
            match std::thread::Builder::new()
                .name("haptics".into())
                .spawn(move || run(messages))
            {
                Ok(thread) => self.worker = Some(Worker { mail, thread }),
                Err(err) => tracing::warn!("cannot start the vibration thread: {err}"),
            }
        }
        self.worker.as_ref()
    }

    fn send(&mut self, message: Message) {
        let delivered = self
            .worker()
            .is_some_and(|worker| worker.mail.send(message).is_ok());
        // A thread that ended (no backend, a panic) is dropped and tried again, a few times.
        if !delivered {
            self.worker = None;
        }
    }
}

impl Default for Haptics {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Haptics {
    /// Ends the thread and waits for it: its pads are set to zero as it goes.
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.mail.send(Message::Stop);
            drop(worker.mail);
            let _ = worker.thread.join();
        }
    }
}

/// Windows rounds waits to its timer tick (15.6 ms) unless asked for 1 ms; a 14 ms pulse needs it.
struct FineTimer;

impl FineTimer {
    fn begin() -> Self {
        #[cfg(windows)]
        // SAFETY: a plain call into winmm, paired with `timeEndPeriod` in `drop`.
        unsafe {
            windows_sys::Win32::Media::timeBeginPeriod(1);
        }
        FineTimer
    }
}

impl Drop for FineTimer {
    fn drop(&mut self) {
        #[cfg(windows)]
        // SAFETY: the matching call of `begin`.
        unsafe {
            windows_sys::Win32::Media::timeEndPeriod(1);
        }
    }
}

/// The pads the thread drives. Dropping it leaves both motors at zero, whatever ended the thread.
struct Pads {
    backend: gilrs_core::Gilrs,
    devices: Vec<gilrs_core::FfDevice>,
    stale: bool,
    lit: bool,
}

impl Pads {
    fn refresh(&mut self) {
        self.devices.clear();
        for id in 0..self.backend.last_gamepad_hint() {
            if let Some(pad) = self.backend.gamepad(id)
                && pad.is_connected()
                && pad.is_ff_supported()
                && let Some(device) = pad.ff_device()
            {
                self.devices.push(device);
            }
        }
        self.stale = false;
    }

    /// Reads the backend's events (so its queue stays empty) and notes plug-ins and removals.
    fn drain(&mut self) {
        while let Some(event) = self.backend.next_event() {
            if matches!(
                event.event,
                gilrs_core::EventType::Connected | gilrs_core::EventType::Disconnected
            ) {
                self.stale = true;
            }
        }
    }

    fn set(&mut self, strong: u16, weak: u16) {
        if self.stale {
            self.refresh();
        }
        // The pad's own timer (Linux) ends a pulse that never gets its stop.
        for device in &mut self.devices {
            device.set_ff_state(strong, weak, Duration::from_millis(200));
        }
        self.lit = strong > 0 || weak > 0;
    }
}

impl Pads {
    /// Both motors to zero, if anything of ours is running (a game's own rumble is left alone).
    fn silence(&mut self) {
        if self.lit {
            self.set(0, 0);
        }
    }
}

impl Drop for Pads {
    fn drop(&mut self) {
        self.silence();
    }
}

/// What is running: when it began, its steps, and the next one to apply.
struct Running {
    began: Instant,
    steps: Vec<Step>,
    next: usize,
}

fn run(messages: Receiver<Message>) {
    let _timer = FineTimer::begin();
    let backend = match gilrs_core::Gilrs::new() {
        Ok(backend) => backend,
        Err(err) => {
            tracing::warn!("no controller vibration: {err}");
            return;
        }
    };
    let mut pads = Pads {
        backend,
        devices: Vec::new(),
        stale: true,
        lit: false,
    };
    let mut running: Option<Running> = None;
    loop {
        let wait = match &running {
            Some(run) => run.steps.get(run.next).map_or(Duration::ZERO, |step| {
                (run.began + step.at).saturating_duration_since(Instant::now())
            }),
            // Idle: wake now and then to read plug-ins and removals.
            None => Duration::from_millis(250),
        };
        let message = messages.recv_timeout(wait);
        // Plug-ins and removals first, so a pulse never goes to a pad that is gone.
        pads.drain();
        match message {
            Ok(Message::Play(steps)) => {
                pads.silence();
                running = Some(Running {
                    began: Instant::now(),
                    steps,
                    next: 0,
                });
            }
            Ok(Message::Stop) => {
                running = None;
                pads.silence();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if let Some(run) = &mut running {
            while let Some(step) = run.steps.get(run.next) {
                if Instant::now() < run.began + step.at {
                    break;
                }
                pads.set(step.strong, step.weak);
                run.next += 1;
            }
            if run.next >= run.steps.len() {
                running = None;
                pads.silence();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_cues() -> impl Iterator<Item = Cue> {
        Cue::ALL.into_iter()
    }

    #[test]
    fn off_makes_no_steps() {
        for cue in all_cues() {
            assert!(plan(pattern(cue), Level::Off).is_empty(), "{cue:?}");
        }
    }

    #[test]
    fn every_plan_ends_on_a_stop_and_its_pulses_never_overlap() {
        for cue in all_cues() {
            let pulses = pattern(cue);
            for pair in pulses.windows(2) {
                assert!(pair[0].at + pair[0].ms <= pair[1].at, "{cue:?} overlaps");
            }
            for level in [Level::Light, Level::Medium, Level::Strong] {
                let steps = plan(pulses, level);
                assert_eq!(steps.len(), pulses.len() * 2, "{cue:?}");
                let last = steps.last().map(|s| (s.strong, s.weak));
                assert_eq!(
                    last,
                    if pulses.is_empty() {
                        None
                    } else {
                        Some((0, 0))
                    }
                );
                assert!(
                    steps.windows(2).all(|w| w[0].at <= w[1].at),
                    "{cue:?} out of order"
                );
            }
        }
    }

    #[test]
    fn nothing_is_long_or_strong() {
        for cue in all_cues() {
            let pulses = pattern(cue);
            let end = pulses.last().map_or(0, |p| p.at + p.ms);
            assert!(end <= 260, "{cue:?} lasts {end} ms");
            for p in pulses {
                assert!(p.ms <= 70, "{cue:?} pulse of {} ms", p.ms);
                assert!(p.weak <= 0.45 && p.strong <= 0.30, "{cue:?} too strong");
            }
        }
    }

    #[test]
    fn the_sound_map_orders_the_strengths() {
        let peak = |cue| {
            pattern(cue)
                .iter()
                .map(|p| p.weak.max(p.strong))
                .fold(0.0, f32::max)
        };
        assert!(peak(Cue::Bump) < peak(Cue::Back), "bump barely there");
        assert!(
            peak(Cue::Back) < peak(Cue::Select),
            "back lighter than select"
        );
        assert!(peak(Cue::Select) < peak(Cue::Error), "an error is firmer");
        assert!(
            pattern(Cue::Move).is_empty(),
            "moving the focus is silent to the hand"
        );
        assert_eq!(pattern(Cue::Error).len(), 2, "two taps");
    }

    #[test]
    fn levels_scale_the_pulse_but_never_below_the_floor_or_above_full() {
        for cue in all_cues().filter(|&c| !pattern(c).is_empty()) {
            let peak = |level| {
                plan(pattern(cue), level)
                    .iter()
                    .map(|s| s.weak.max(s.strong))
                    .max()
                    .unwrap_or(0)
            };
            assert!(peak(Level::Light) < peak(Level::Medium), "{cue:?} light");
            assert!(peak(Level::Medium) < peak(Level::Strong), "{cue:?} strong");
        }
        // The floor lifts a weak pulse before the level scales it.
        let bump = plan(pattern(Cue::Bump), Level::Medium)[0].weak;
        assert!(f32::from(bump) / f32::from(u16::MAX) >= FLOOR - 1e-3);
        let error = plan(pattern(Cue::Error), Level::Strong)[0].weak;
        assert!(error < u16::MAX);
    }

    #[test]
    fn a_silent_handle_starts_no_thread_and_notes_what_it_was_asked() {
        let mut haptics = Haptics::silent();
        haptics.play(Cue::Select);
        haptics.play(Cue::Move);
        haptics.stop();
        assert!(haptics.worker.is_none());
        assert_eq!(haptics.asked, [Asked::Play(Cue::Select), Asked::Stop]);
    }

    #[test]
    fn off_asks_for_nothing_and_never_starts_a_thread() {
        let mut off = Haptics::new();
        off.level = Level::Off;
        off.play(Cue::Select);
        off.warm();
        off.stop();
        assert!(off.worker.is_none(), "off never touches a device");
        let mut silent = Haptics::silent();
        silent.level = Level::Off;
        silent.play(Cue::Error);
        assert!(silent.asked.is_empty());
    }
}
