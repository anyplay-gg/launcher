//! The interface cues (docs/DESIGN.md section 7.3): soft, warm tones in the middle register
//! with rounded attacks and a little room. Each cue renders once into a stereo buffer at the
//! output rate, normalised to its own peak level.

use super::synth::{Lowpass, Reverb, Shape, db_to_gain, shimmer, tone};

/// The interface cues.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cue {
    /// Any selection or focus change, value change, binding captured.
    Move,
    /// Something opened, an action ran, advance in onboarding.
    Select,
    /// Closing, leaving, cancelling.
    Back,
    /// Unlock finished, onboarding finished, trophy banner.
    Unlocked,
    /// Something failed (a warning toast).
    Error,
    /// The launcher opened: a slow swell under the intro.
    Boot,
    /// A call rings: two soft stacked fifths, repeated by the caller of the cue.
    Ring,
    /// A direction had nowhere to go: the end of a row or a list. Barely there.
    Bump,
}

impl Cue {
    pub const ALL: [Cue; 8] = [
        Cue::Move,
        Cue::Select,
        Cue::Back,
        Cue::Unlocked,
        Cue::Error,
        Cue::Boot,
        Cue::Ring,
        Cue::Bump,
    ];
}

/// How a cue is mixed: peak level, reverb send and decay, lowpass, and the dry length in
/// seconds (long enough for every tone to fall by 80 dB).
struct Mix {
    peak_db: f32,
    send: f32,
    rt60: f32,
    cutoff: f32,
    length: f32,
}

type Draw = fn(&mut [f32], f32);

const fn shape(attack: f32, decay: f32) -> Shape {
    Shape { attack, decay }
}

/// A sine with an almost equally loud octave and a faint fifth: round and soft.
const SOFT: &[(f32, f32)] = &[(1.0, 1.0), (2.0, 0.8), (1.5, 0.09)];
/// A little fuller: octave and a trace of the third harmonic.
const ROUND: &[(f32, f32)] = &[(1.0, 1.0), (2.0, 0.45), (3.0, 0.08)];
/// Nearly a pure sine.
const PURE: &[(f32, f32)] = &[(1.0, 1.0), (2.0, 0.25)];

const F3: f32 = 174.61;
const A3: f32 = 220.0;
const B_FLAT3: f32 = 233.08;
const C4: f32 = 261.63;
const E4: f32 = 329.63;
const F4: f32 = 349.23;
const G4: f32 = 392.0;
const C5: f32 = 523.25;
const F5: f32 = 698.46;
const G5: f32 = 783.99;
const C6: f32 = 1046.5;

fn recipe(cue: Cue) -> (Mix, Draw) {
    match cue {
        // One soft tone at F4, gone after about 150 ms; the quietest cue.
        Cue::Move => (
            Mix {
                peak_db: -22.0,
                send: 0.10,
                rt60: 0.7,
                cutoff: 8000.0,
                length: 0.4,
            },
            |o, r| tone(o, r, 0.0, F4, 1.0, shape(0.012, 0.038), SOFT, 0.0),
        ),
        // An open fifth, C5 and G5, the G a moment later.
        Cue::Select => (
            Mix {
                peak_db: -13.0,
                send: 0.15,
                rt60: 0.9,
                cutoff: 8000.0,
                length: 1.2,
            },
            |o, r| {
                tone(o, r, 0.0, C5, 1.0, shape(0.010, 0.12), ROUND, 0.0);
                tone(o, r, 0.012, G5, 0.7, shape(0.010, 0.12), ROUND, 0.0);
            },
        ),
        // A low open fifth, A3 and E4.
        Cue::Back => (
            Mix {
                peak_db: -15.0,
                send: 0.12,
                rt60: 0.8,
                cutoff: 8000.0,
                length: 0.9,
            },
            |o, r| {
                tone(o, r, 0.0, A3, 1.0, shape(0.010, 0.09), ROUND, 0.0);
                tone(o, r, 0.0, E4, 0.7, shape(0.010, 0.09), ROUND, 0.0);
            },
        ),
        // Two close low tones, A3 and B flat 3, rubbing softly.
        Cue::Error => (
            Mix {
                peak_db: -15.0,
                send: 0.06,
                rt60: 0.8,
                cutoff: 8000.0,
                length: 0.9,
            },
            |o, r| {
                tone(o, r, 0.0, A3, 1.0, shape(0.012, 0.09), PURE, 0.0);
                tone(o, r, 0.0, B_FLAT3, 0.9, shape(0.012, 0.09), PURE, 0.0);
            },
        ),
        // A low, dull knock: F3 with almost no overtones, cut off at 1.6 kHz, gone in 80 ms. The
        // quietest cue; it says "that is the end" without being a note.
        Cue::Bump => (
            Mix {
                peak_db: -27.0,
                send: 0.04,
                rt60: 0.4,
                cutoff: 1600.0,
                length: 0.3,
            },
            |o, r| tone(o, r, 0.0, F3, 1.0, shape(0.010, 0.028), PURE, 0.0),
        ),
        // A call: F4 with C5 over it, then the same a step later and softer, slow attacks, a
        // little room. Warm and low, so it asks without alarming.
        Cue::Ring => (
            Mix {
                peak_db: -14.0,
                send: 0.22,
                rt60: 1.4,
                cutoff: 6500.0,
                length: 1.3,
            },
            |o, r| {
                for (at, gain) in [(0.0, 1.0), (0.3, 0.7)] {
                    tone(o, r, at, F4, gain, shape(0.04, 0.2), PURE, 0.0);
                    tone(o, r, at + 0.01, C5, gain * 0.7, shape(0.04, 0.2), PURE, 0.0);
                }
            },
        ),
        // The trophy chord's family an octave lower and slower: F3 C4 G4 C5 F5 swelling in
        // 70 ms apart over 0.35 s each, shimmering, with a late glitter; under the launch intro.
        Cue::Boot => (
            Mix {
                peak_db: -12.0,
                send: 0.35,
                rt60: 2.2,
                cutoff: 7000.0,
                length: 7.0,
            },
            |o, r| {
                for (k, (freq, amp)) in [F3, C4, G4, C5, F5]
                    .into_iter()
                    .zip([0.9, 0.8, 0.7, 0.55, 0.45])
                    .enumerate()
                {
                    tone(
                        o,
                        r,
                        k as f32 * 0.07,
                        freq,
                        amp,
                        shape(0.35, 0.65),
                        PURE,
                        5.0,
                    );
                }
                shimmer(o, r, 0.3, 0.1, 0.25, 9);
            },
        ),
        // Stacked fifths F4 C5 G5 C6 entering 15 ms apart, slowly shimmering, with a faint
        // glitter on top.
        Cue::Unlocked => (
            Mix {
                peak_db: -9.0,
                send: 0.2,
                rt60: 1.2,
                cutoff: 9000.0,
                length: 2.6,
            },
            |o, r| {
                for (k, (freq, amp)) in [F4, C5, G5, C6]
                    .into_iter()
                    .zip([1.0, 0.85, 0.7, 0.55])
                    .enumerate()
                {
                    tone(
                        o,
                        r,
                        k as f32 * 0.015,
                        freq,
                        amp,
                        shape(0.025, 0.25),
                        PURE,
                        5.0,
                    );
                }
                shimmer(o, r, 0.01, 0.12, 0.12, 5);
            },
        ),
    }
}

/// Renders a cue as interleaved stereo at `rate`, peak-normalised, with its reverb tail.
pub fn render(cue: Cue, rate: u32) -> Vec<f32> {
    let rate = rate as f32;
    let (mix, draw) = recipe(cue);
    let mut dry = vec![0.0; (mix.length * rate) as usize];
    draw(&mut dry, rate);
    let mut lowpass = Lowpass::new(mix.cutoff, rate);
    let mut reverb = Reverb::new(rate, mix.rt60, 0.5);
    let frames = dry.len() + (mix.rt60 * rate) as usize;
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let x = lowpass.process(dry.get(i).copied().unwrap_or(0.0));
        let (wet_l, wet_r) = reverb.process(x * mix.send);
        out.push(x + wet_l);
        out.push(x + wet_r);
    }
    let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 0.0 {
        let gain = db_to_gain(mix.peak_db) / peak;
        out.iter_mut().for_each(|s| *s *= gain);
    }
    trim(&mut out, rate);
    out
}

/// Cuts the tail where it stays below -72 dBFS and fades the last 15 ms.
fn trim(out: &mut Vec<f32>, rate: f32) {
    let floor = db_to_gain(-72.0);
    let last = out
        .iter()
        .rposition(|s| s.abs() > floor)
        .map_or(0, |i| i / 2 + 1);
    out.truncate(last * 2);
    let fade = ((0.015 * rate) as usize).min(last);
    for i in 0..fade {
        let gain = i as f32 / fade as f32;
        let frame = last - 1 - i;
        out[frame * 2] *= gain;
        out[frame * 2 + 1] *= gain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cue_is_soft_bounded_and_ends_silent() {
        for cue in Cue::ALL {
            let out = render(cue, 48_000);
            assert!(out.len() > 2_000, "{cue:?} too short");
            assert_eq!(out[0], 0.0, "{cue:?} starts with a click");
            assert_eq!(out[out.len() - 1], 0.0, "{cue:?} ends with a click");
            let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak <= db_to_gain(-8.9), "{cue:?} peak {peak}");
            // No sample-to-sample jump a soft tone could not make (clicks show up here).
            let jump = out
                .chunks(2)
                .collect::<Vec<_>>()
                .windows(2)
                .map(|w| (w[1][0] - w[0][0]).abs())
                .fold(0.0f32, f32::max);
            assert!(jump < peak * 0.5, "{cue:?} jump {jump} peak {peak}");
        }
    }
}
