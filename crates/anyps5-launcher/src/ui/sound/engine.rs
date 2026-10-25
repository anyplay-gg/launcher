//! The mixer on the audio thread (docs/DESIGN.md section 7.3): cue voices plus the home music,
//! with the music fading in and out on request and dipping under every cue. The launcher talks
//! to it only through [`Command`]s, so nothing on the UI thread ever waits for audio.

use std::num::NonZero;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use rodio::Source;

use super::cues::{Cue, render};
use super::music::Music;
use super::synth::db_to_gain;

pub enum Command {
    Cue(Cue),
    /// Fade the home music in (`true`) or out.
    Music(bool),
}

/// Cues playing at once; a cue beyond that fades the oldest out over 5 ms.
const VOICES: usize = 8;
/// Hover ticks sounding at once: a sweep over tiles is a light ratchet, never a pile of ticks.
pub const HOVER_VOICES: usize = 2;
/// Commands are read every this many frames (about 1.3 ms at 48 kHz).
const POLL: u32 = 64;
const MUSIC_IN_SECONDS: f32 = 2.0;
const MUSIC_OUT_SECONDS: f32 = 0.8;

struct Voice {
    cue: Cue,
    samples: Arc<[f32]>,
    frame: usize,
    gain: f32,
    /// Per-frame gain step while it is being cut.
    fade: f32,
}

impl Voice {
    fn next(&mut self) -> (f32, f32) {
        let i = self.frame * 2;
        if i + 1 >= self.samples.len() {
            return (0.0, 0.0);
        }
        self.frame += 1;
        self.gain = (self.gain - self.fade).max(0.0);
        (self.samples[i] * self.gain, self.samples[i + 1] * self.gain)
    }

    fn done(&self) -> bool {
        self.frame * 2 + 1 >= self.samples.len() || self.gain == 0.0
    }
}

/// The music's level: a position moving linearly between 0 and 1, heard squared.
struct Fader {
    position: f32,
    target: f32,
    up: f32,
    down: f32,
}

impl Fader {
    fn new(rate: f32) -> Self {
        Self {
            position: 0.0,
            target: 0.0,
            up: 1.0 / (MUSIC_IN_SECONDS * rate),
            down: 1.0 / (MUSIC_OUT_SECONDS * rate),
        }
    }

    fn next(&mut self) -> f32 {
        if self.position < self.target {
            self.position = (self.position + self.up).min(self.target);
        } else if self.position > self.target {
            self.position = (self.position - self.down).max(self.target);
        }
        self.position * self.position
    }

    fn silent(&self) -> bool {
        self.position == 0.0 && self.target == 0.0
    }
}

/// Lowers the music under a cue: down in 15 ms, held, back up over 400 ms.
struct Duck {
    gain: f32,
    floor: f32,
    hold: u32,
    attack: f32,
    release: f32,
    rate: f32,
}

impl Duck {
    fn new(rate: f32) -> Self {
        Self {
            gain: 1.0,
            floor: 1.0,
            hold: 0,
            attack: 1.0 - (-1.0 / (0.015 * rate)).exp(),
            release: 1.0 - (-1.0 / (0.4 * rate)).exp(),
            rate,
        }
    }

    fn trigger(&mut self, cue: Cue) {
        let (depth_db, hold) = match cue {
            Cue::Move => (1.5, 0.12),
            // A pointer sweeping a row must not make the music pump.
            Cue::Hover => return,
            Cue::Select | Cue::Back | Cue::Error => (3.0, 0.25),
            Cue::Unlocked => (6.0, 1.2),
            Cue::Boot => (6.0, 1.5),
            Cue::Ring => (3.0, 0.6),
            Cue::Bump => (0.0, 0.0),
            Cue::On | Cue::Off => (1.5, 0.12),
            Cue::Open | Cue::Close => (3.0, 0.25),
            Cue::Notify | Cue::Message | Cue::Invite => (3.0, 0.5),
        };
        self.floor = self.floor.min(db_to_gain(-depth_db));
        self.hold = self.hold.max((hold * self.rate) as u32);
    }

    fn next(&mut self) -> f32 {
        let target = if self.hold > 0 {
            self.hold -= 1;
            self.floor
        } else {
            self.floor = 1.0;
            1.0
        };
        let k = if target < self.gain {
            self.attack
        } else {
            self.release
        };
        self.gain += (target - self.gain) * k;
        self.gain
    }
}

/// A last guard against overlapping cues clipping: linear up to 0.9, then bent towards 1.
fn limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= 0.9 {
        x
    } else {
        x.signum() * (0.9 + 0.1 * ((a - 0.9) / 0.1).tanh())
    }
}

/// The endless stereo source handed to rodio's mixer. It ends when the launcher drops its
/// command sender.
pub struct Engine {
    rate: u32,
    commands: Receiver<Command>,
    cues: Vec<Arc<[f32]>>,
    voices: Vec<Voice>,
    music: Music,
    fader: Fader,
    duck: Duck,
    poll: u32,
    right: Option<f32>,
    ended: bool,
}

impl Engine {
    /// Renders every cue at `rate`; takes a moment, so build it off the UI thread.
    pub fn new(rate: u32, commands: Receiver<Command>, seed: u32) -> Self {
        Self {
            rate,
            commands,
            cues: Cue::ALL
                .iter()
                .map(|&cue| render(cue, rate).into())
                .collect(),
            voices: Vec::with_capacity(VOICES + 4),
            music: Music::new(rate, seed),
            fader: Fader::new(rate as f32),
            duck: Duck::new(rate as f32),
            poll: 0,
            right: None,
            ended: false,
        }
    }

    fn receive(&mut self) {
        loop {
            match self.commands.try_recv() {
                Ok(Command::Cue(cue)) => self.start(cue),
                Ok(Command::Music(on)) => self.fader.target = if on { 1.0 } else { 0.0 },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.ended = true;
                    break;
                }
            }
        }
        self.voices.retain(|v| !v.done());
    }

    fn start(&mut self, cue: Cue) {
        let playing = self.voices.iter().filter(|v| v.fade == 0.0).count();
        if playing >= VOICES
            && let Some(oldest) = self.voices.iter_mut().find(|v| v.fade == 0.0)
        {
            oldest.fade = 1.0 / (0.005 * self.rate as f32);
        }
        if cue == Cue::Hover {
            let sounding = self
                .voices
                .iter()
                .filter(|v| v.cue == Cue::Hover && v.fade == 0.0)
                .count();
            if sounding >= HOVER_VOICES
                && let Some(oldest) = self
                    .voices
                    .iter_mut()
                    .find(|v| v.cue == Cue::Hover && v.fade == 0.0)
            {
                oldest.fade = 1.0 / (0.005 * self.rate as f32);
            }
        }
        let index = Cue::ALL.iter().position(|&c| c == cue).unwrap_or(0);
        self.voices.push(Voice {
            cue,
            samples: self.cues[index].clone(),
            frame: 0,
            gain: 1.0,
            fade: 0.0,
        });
        self.duck.trigger(cue);
    }

    fn frame(&mut self) -> (f32, f32) {
        let (mut left, mut right) = (0.0, 0.0);
        for voice in &mut self.voices {
            let (l, r) = voice.next();
            left += l;
            right += r;
        }
        let duck = self.duck.next();
        if !self.fader.silent() {
            let level = self.fader.next() * duck;
            let (l, r) = self.music.next();
            left += l * level;
            right += r * level;
        }
        (limit(left), limit(right))
    }
}

impl Iterator for Engine {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(right) = self.right.take() {
            return Some(right);
        }
        if self.poll == 0 {
            self.receive();
            self.poll = POLL;
        }
        if self.ended {
            return None;
        }
        self.poll -= 1;
        let (left, right) = self.frame();
        self.right = Some(right);
        Some(left)
    }
}

impl Source for Engine {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        NonZero::new(2).unwrap_or(NonZero::<u16>::MIN)
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        NonZero::new(self.rate).unwrap_or(NonZero::<u32>::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    const RATE: u32 = 24_000;

    fn take(engine: &mut Engine, seconds: f32) -> Vec<f32> {
        (0..(seconds * RATE as f32) as usize * 2)
            .map(|_| engine.next().unwrap())
            .collect()
    }

    fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0f32, |m, s| m.max(s.abs()))
    }

    #[test]
    fn a_cue_plays_once_and_then_it_is_quiet() {
        let (tx, rx) = channel();
        let mut engine = Engine::new(RATE, rx, 1);
        assert_eq!(peak(&take(&mut engine, 0.1)), 0.0);
        tx.send(Command::Cue(Cue::Select)).unwrap();
        assert!(peak(&take(&mut engine, 0.3)) > db_to_gain(-16.0));
        take(&mut engine, 2.0);
        assert_eq!(peak(&take(&mut engine, 0.2)), 0.0);
    }

    #[test]
    fn music_fades_in_and_out_to_silence() {
        let (tx, rx) = channel();
        let mut engine = Engine::new(RATE, rx, 1);
        tx.send(Command::Music(true)).unwrap();
        take(&mut engine, 8.0);
        assert!(peak(&take(&mut engine, 2.0)) > db_to_gain(-40.0));
        tx.send(Command::Music(false)).unwrap();
        take(&mut engine, MUSIC_OUT_SECONDS + 0.05);
        assert_eq!(peak(&take(&mut engine, 0.5)), 0.0);
        assert!(engine.fader.silent());
    }

    #[test]
    fn the_music_dips_under_a_cue_and_recovers() {
        let mut duck = Duck::new(RATE as f32);
        duck.trigger(Cue::Unlocked);
        let low = (0..(0.2 * RATE as f32) as usize)
            .map(|_| duck.next())
            .last()
            .unwrap();
        assert!((low - db_to_gain(-6.0)).abs() < 0.01, "{low}");
        let back = (0..(3 * RATE) as usize)
            .map(|_| duck.next())
            .last()
            .unwrap();
        assert!(back > 0.99, "{back}");
    }

    #[test]
    fn too_many_cues_cut_the_oldest_softly() {
        let (tx, rx) = channel();
        let mut engine = Engine::new(RATE, rx, 1);
        for _ in 0..VOICES + 3 {
            tx.send(Command::Cue(Cue::Unlocked)).unwrap();
        }
        take(&mut engine, 0.05);
        assert!(engine.voices.iter().filter(|v| v.fade == 0.0).count() <= VOICES);
        take(&mut engine, 0.05);
        assert!(engine.voices.len() <= VOICES);
    }

    #[test]
    fn a_sweep_of_hover_ticks_keeps_two_voices_and_stays_clean() {
        let (tx, rx) = channel();
        let mut engine = Engine::new(RATE, rx, 1);
        let one = {
            tx.send(Command::Cue(Cue::Hover)).unwrap();
            peak(&take(&mut engine, 0.3))
        };
        take(&mut engine, 0.5);
        // Twelve ticks 20 ms apart: far closer than the gate ever allows.
        let mut sweep = Vec::new();
        for _ in 0..12 {
            tx.send(Command::Cue(Cue::Hover)).unwrap();
            sweep.extend(take(&mut engine, 0.02));
            let sounding = engine
                .voices
                .iter()
                .filter(|v| v.cue == Cue::Hover && v.fade == 0.0)
                .count();
            assert!(sounding <= HOVER_VOICES, "{sounding} voices");
        }
        let top = peak(&sweep);
        assert!(top < one * 2.0 && top < db_to_gain(-16.0), "{top} vs {one}");
        // The cut-off voices fade in 5 ms: no sample jump a soft tone could not make.
        let jump = sweep
            .chunks(2)
            .collect::<Vec<_>>()
            .windows(2)
            .map(|w| (w[1][0] - w[0][0]).abs())
            .fold(0.0f32, f32::max);
        assert!(jump < top * 0.35, "jump {jump} peak {top}");
    }

    #[test]
    fn hover_ticks_do_not_dip_the_music() {
        let mut duck = Duck::new(RATE as f32);
        duck.trigger(Cue::Hover);
        assert!((0..RATE).all(|_| duck.next() == 1.0));
    }
    #[test]
    fn dropping_the_sender_ends_the_source() {
        let (tx, rx) = channel();
        let mut engine = Engine::new(RATE, rx, 1);
        drop(tx);
        assert_eq!(engine.next(), None);
    }

    #[test]
    fn limiter_is_transparent_below_its_knee() {
        assert_eq!(limit(0.5), 0.5);
        assert!(limit(0.95) < 0.95);
        assert!(limit(3.0) <= 1.0 && limit(-3.0) >= -1.0);
    }
}
