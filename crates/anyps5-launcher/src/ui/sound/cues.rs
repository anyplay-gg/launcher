//! The interface cues (docs/DESIGN.md section 7.3): soft, warm tones in the middle register
//! with rounded attacks and a little room. Each cue renders once into a stereo buffer at the
//! output rate, normalised to its own peak level.

use super::synth::{Lowpass, Reverb, Shape, db_to_gain, shimmer, tone};

/// The interface cues.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cue {
    /// Any selection or focus change, value change, binding captured.
    Move,
    /// The pointer entered a new control, tile or row: the Move tick, a touch quieter.
    Hover,
    /// An action ran, a choice was made, advance in onboarding.
    Select,
    /// Leaving a level, cancelling, a step back.
    Back,
    /// A switch turned on: two quick notes up a fourth.
    On,
    /// A switch turned off: the same two notes, down.
    Off,
    /// A screen or panel opened: a rising fifth, F4 under C5.
    Open,
    /// A screen or panel closed: the fifth falling.
    Close,
    /// Something happened that was not asked for (a toast, a friend request): a soft chime.
    Notify,
    /// A message arrived: two short notes up a fourth, lower than the chime.
    Message,
    /// An invitation arrived: F4, C5, F5 one after the other, slowly.
    Invite,
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
    pub const ALL: [Cue; 16] = [
        Cue::Move,
        Cue::Hover,
        Cue::Select,
        Cue::Back,
        Cue::On,
        Cue::Off,
        Cue::Open,
        Cue::Close,
        Cue::Notify,
        Cue::Message,
        Cue::Invite,
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

/// The navigation tick: the same warmth with a quieter octave, so a run of ticks stays round.
const TICK: &[(f32, f32)] = &[(1.0, 1.0), (2.0, 0.55), (1.5, 0.06)];
/// A little fuller: octave and a trace of the third harmonic.
const ROUND: &[(f32, f32)] = &[(1.0, 1.0), (2.0, 0.45), (3.0, 0.08)];
/// Nearly a pure sine.
const PURE: &[(f32, f32)] = &[(1.0, 1.0), (2.0, 0.25)];

const F3: f32 = 174.61;
const A3: f32 = 220.0;
const B_FLAT3: f32 = 233.08;
const C4: f32 = 261.63;
const A4: f32 = 440.0;
const D5: f32 = 587.33;
const E4: f32 = 329.63;
const F4: f32 = 349.23;
const G4: f32 = 392.0;
const C5: f32 = 523.25;
const F5: f32 = 698.46;
const G5: f32 = 783.99;
const C6: f32 = 1046.5;

/// The shared navigation tick at `peak_db`: see [`Cue::Move`].
fn tick(peak_db: f32) -> (Mix, Draw) {
    (
        Mix {
            peak_db,
            send: 0.04,
            rt60: 0.3,
            cutoff: 5000.0,
            length: 0.25,
        },
        |o, r| tone(o, r, 0.0, F4, 1.0, shape(0.007, 0.026), TICK, 0.0),
    )
}

fn recipe(cue: Cue) -> (Mix, Draw) {
    match cue {
        // The navigation tick: one soft tone at F4 with a quieter octave, a 7 ms rounded attack,
        // gone within about 120 ms and nearly dry, so a run of them stays clean. Focus moves
        // use it as is; the pointer's `Hover` is the same tick 3 dB lower.
        Cue::Move => tick(-22.0),
        Cue::Hover => tick(-25.0),
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
        // A switch: C5 then F5, 55 ms apart, short and nearly dry; the off switch plays them down.
        Cue::On => (
            Mix {
                peak_db: -18.0,
                send: 0.08,
                rt60: 0.5,
                cutoff: 7000.0,
                length: 0.5,
            },
            |o, r| {
                tone(o, r, 0.0, C5, 1.0, shape(0.008, 0.05), TICK, 0.0);
                tone(o, r, 0.055, F5, 0.9, shape(0.008, 0.07), TICK, 0.0);
            },
        ),
        Cue::Off => (
            Mix {
                peak_db: -19.0,
                send: 0.08,
                rt60: 0.5,
                cutoff: 6000.0,
                length: 0.5,
            },
            |o, r| {
                tone(o, r, 0.0, F5, 0.9, shape(0.008, 0.05), TICK, 0.0);
                tone(o, r, 0.055, C5, 1.0, shape(0.008, 0.07), TICK, 0.0);
            },
        ),
        // A screen opens: F4 with C5 a breath later, a rounded attack and some room.
        Cue::Open => (
            Mix {
                peak_db: -16.0,
                send: 0.18,
                rt60: 1.0,
                cutoff: 6500.0,
                length: 1.0,
            },
            |o, r| {
                tone(o, r, 0.0, F4, 1.0, shape(0.018, 0.14), ROUND, 0.0);
                tone(o, r, 0.028, C5, 0.7, shape(0.018, 0.14), ROUND, 0.0);
            },
        ),
        // ... and closes: the same two notes the other way round, a little quieter.
        Cue::Close => (
            Mix {
                peak_db: -17.0,
                send: 0.16,
                rt60: 0.9,
                cutoff: 5500.0,
                length: 0.9,
            },
            |o, r| {
                tone(o, r, 0.0, C5, 0.7, shape(0.014, 0.11), ROUND, 0.0);
                tone(o, r, 0.028, F4, 1.0, shape(0.014, 0.11), ROUND, 0.0);
            },
        ),
        // A chime: G5 and C6, the C a moment later, a long soft tail.
        Cue::Notify => (
            Mix {
                peak_db: -16.0,
                send: 0.25,
                rt60: 1.2,
                cutoff: 9000.0,
                length: 1.4,
            },
            |o, r| {
                tone(o, r, 0.0, G5, 1.0, shape(0.015, 0.20), PURE, 0.0);
                tone(o, r, 0.04, C6, 0.6, shape(0.015, 0.20), PURE, 0.0);
            },
        ),
        // A message: A4 then D5, 90 ms apart, short.
        Cue::Message => (
            Mix {
                peak_db: -17.0,
                send: 0.12,
                rt60: 0.7,
                cutoff: 7000.0,
                length: 0.8,
            },
            |o, r| {
                tone(o, r, 0.0, A4, 1.0, shape(0.012, 0.07), TICK, 0.0);
                tone(o, r, 0.09, D5, 0.9, shape(0.012, 0.09), TICK, 0.0);
            },
        ),
        // An invitation: F4, C5, F5 as a slow arpeggio, 70 ms apart.
        Cue::Invite => (
            Mix {
                peak_db: -15.0,
                send: 0.22,
                rt60: 1.2,
                cutoff: 8000.0,
                length: 1.6,
            },
            |o, r| {
                for (k, (freq, amp)) in [F4, C5, F5].into_iter().zip([1.0, 0.85, 0.7]).enumerate() {
                    tone(
                        o,
                        r,
                        k as f32 * 0.07,
                        freq,
                        amp,
                        shape(0.02, 0.16),
                        PURE,
                        0.0,
                    );
                }
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
        // A low, dull knock: F3 with a faint octave, cut off at 1.6 kHz, gone in 80 ms. The
        // quietest cue; it says "that is the end" without being a note.
        Cue::Bump => (
            Mix {
                peak_db: -23.0,
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

    const TICKS: [(&str, Cue); 2] = [("move", Cue::Move), ("hover", Cue::Hover)];

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

    /// Mono mix of a rendered cue.
    fn mono(out: &[f32]) -> Vec<f32> {
        out.chunks(2).map(|f| 0.5 * (f[0] + f[1])).collect()
    }

    /// Writes 16-bit stereo PCM; a listening aid for `ANYPLAY_WRITE_TICK`.
    fn wav(path: &std::path::Path, out: &[f32], rate: u32) {
        let data: Vec<u8> = out
            .iter()
            .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes())
            .collect();
        let mut file = Vec::new();
        file.extend(b"RIFF");
        file.extend((36 + data.len() as u32).to_le_bytes());
        file.extend(b"WAVEfmt ");
        file.extend(16u32.to_le_bytes());
        file.extend(1u16.to_le_bytes());
        file.extend(2u16.to_le_bytes());
        file.extend(rate.to_le_bytes());
        file.extend((rate * 4).to_le_bytes());
        file.extend(4u16.to_le_bytes());
        file.extend(16u16.to_le_bytes());
        file.extend(b"data");
        file.extend((data.len() as u32).to_le_bytes());
        file.extend(data);
        std::fs::write(path, file).expect("write wav");
    }

    struct Metrics {
        peak_db: f32,
        /// Milliseconds until the last sample above -60 dBFS.
        length_ms: f32,
        /// |mean| relative to the peak.
        dc: f32,
        /// RMS frequency from the first difference: a brightness measure.
        rms_hz: f32,
    }

    fn metrics(out: &[f32], rate: u32) -> Metrics {
        let m = mono(out);
        let peak = m.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        let floor = db_to_gain(-60.0);
        let last = m.iter().rposition(|s| s.abs() > floor).unwrap_or(0);
        let mean = m.iter().sum::<f32>() / m.len() as f32;
        let energy: f32 = m.iter().map(|s| s * s).sum();
        let diff: f32 = m.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
        let ratio = (diff / energy).sqrt();
        Metrics {
            peak_db: 20.0 * peak.log10(),
            length_ms: last as f32 * 1000.0 / rate as f32,
            dc: mean.abs() / peak,
            rms_hz: 2.0 * (ratio / 2.0).asin() * rate as f32 / std::f32::consts::TAU,
        }
    }

    #[test]
    #[ignore = "a listening aid: ANYPLAY_WRITE_TICK=<dir> cargo test write_the_tick -- --ignored --nocapture"]
    fn write_the_tick() {
        let Ok(dir) = std::env::var("ANYPLAY_WRITE_TICK") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        for (name, cue) in TICKS {
            let out = render(cue, 48_000);
            let m = metrics(&out, 48_000);
            println!(
                "{name}: peak {:.1} dBFS, {:.0} ms to -60 dBFS, dc {:.5}, rms {:.0} Hz",
                m.peak_db, m.length_ms, m.dc, m.rms_hz
            );
            wav(&dir.join(format!("{name}.wav")), &out, 48_000);
        }
    }

    #[test]
    fn the_hover_tick_is_short_quiet_clean_and_the_focus_tick_alike() {
        const RATE: u32 = 48_000;
        let hover = render(Cue::Hover, RATE);
        let focus = render(Cue::Move, RATE);
        let (h, f) = (metrics(&hover, RATE), metrics(&focus, RATE));
        // The same voice, a touch quieter.
        assert!(
            (f.peak_db - h.peak_db - 3.0).abs() < 0.1,
            "{} {}",
            f.peak_db,
            h.peak_db
        );
        assert!((h.peak_db + 25.0).abs() < 0.1, "peak {}", h.peak_db);
        assert!((f.rms_hz - h.rms_hz).abs() < 1.0);
        // Short: over (below -60 dBFS) within 140 ms, but a real tick of at least 60 ms.
        assert!((60.0..140.0).contains(&h.length_ms), "{} ms", h.length_ms);
        assert!(f.length_ms < 140.0, "{} ms", f.length_ms);
        // Warm, not bright: the rms frequency stays in the low middle register.
        assert!((300.0..600.0).contains(&h.rms_hz), "{} Hz", h.rms_hz);
        // No DC offset, no clipping, a silent first and last sample.
        assert!(h.dc < 1e-3 && f.dc < 1e-3, "dc {} {}", h.dc, f.dc);
        for out in [&hover, &focus] {
            assert!(out.iter().all(|s| s.abs() < db_to_gain(-20.0)));
            assert_eq!((out[0], out[out.len() - 1]), (0.0, 0.0));
        }
        // The attack is rounded: the first 3 ms stay under a third of the peak (no click), the
        // peak comes within 12 ms.
        let m = mono(&hover);
        let peak = m.iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!(
            m[..(0.003 * RATE as f32) as usize]
                .iter()
                .all(|s| s.abs() < peak / 3.0)
        );
        let at = m.iter().position(|s| s.abs() > peak * 0.98).unwrap();
        assert!(at < (0.012 * RATE as f32) as usize, "peak at sample {at}");
    }

    fn peak_db(cue: Cue) -> f32 {
        let out = render(cue, 48_000);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        20.0 * peak.log10()
    }

    /// Loudness over the first 300 ms, the part a cue is heard in (RMS in dBFS).
    fn early_rms_db(cue: Cue) -> f32 {
        let out = render(cue, 48_000);
        let n = (0.3 * 48_000.0) as usize * 2;
        let part = &out[..n.min(out.len())];
        let mean = part.iter().map(|s| s * s).sum::<f32>() / part.len() as f32;
        10.0 * mean.log10()
    }

    #[test]
    fn the_sound_map_is_balanced() {
        // The three ticks are the quietest cues and a run of them stays soft.
        let ticks = [Cue::Hover, Cue::Move, Cue::Bump];
        let loudest_tick = ticks.iter().map(|&c| peak_db(c)).fold(f32::MIN, f32::max);
        for cue in Cue::ALL {
            if !ticks.contains(&cue) {
                assert!(
                    peak_db(cue) > loudest_tick,
                    "{cue:?} should be louder than a tick"
                );
            }
        }
        // Routine feedback sits under the confirmation, and the confirmation under the trophy.
        for quiet in [Cue::On, Cue::Off, Cue::Message] {
            assert!(peak_db(quiet) < peak_db(Cue::Select), "{quiet:?}");
        }
        assert!(peak_db(Cue::Select) < peak_db(Cue::Unlocked));
        // Nothing is shouting: every cue is at least 8 dB under full scale, and the early
        // loudness of a cue stays within 16 dB of any other (no cue vanishes next to another).
        let loudness: Vec<f32> = Cue::ALL
            .iter()
            .filter(|&&c| c != Cue::Boot)
            .map(|&c| early_rms_db(c))
            .collect();
        let (low, high) = loudness
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(
            high - low < 16.0,
            "early loudness spans {:.1} dB",
            high - low
        );
    }

    /// Writes every cue to `$ANYPS5_CUE_DIR/<name>.wav` (16-bit stereo, 48 kHz) to listen to:
    /// `ANYPS5_CUE_DIR=... cargo test render_the_cues_to_wav -- --ignored`.
    #[test]
    #[ignore = "writes files; run by hand to listen"]
    fn render_the_cues_to_wav() {
        let Some(dir) = std::env::var_os("ANYPS5_CUE_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for cue in Cue::ALL {
            let samples = render(cue, 48_000);
            let mut wav = Vec::with_capacity(44 + samples.len() * 2);
            let data_len = (samples.len() * 2) as u32;
            wav.extend_from_slice(b"RIFF");
            wav.extend_from_slice(&(36 + data_len).to_le_bytes());
            wav.extend_from_slice(b"WAVEfmt ");
            wav.extend_from_slice(&16u32.to_le_bytes());
            wav.extend_from_slice(&1u16.to_le_bytes());
            wav.extend_from_slice(&2u16.to_le_bytes());
            wav.extend_from_slice(&48_000u32.to_le_bytes());
            wav.extend_from_slice(&(48_000u32 * 4).to_le_bytes());
            wav.extend_from_slice(&4u16.to_le_bytes());
            wav.extend_from_slice(&16u16.to_le_bytes());
            wav.extend_from_slice(b"data");
            wav.extend_from_slice(&data_len.to_le_bytes());
            for s in &samples {
                wav.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
            }
            let name = format!("{cue:?}").to_lowercase();
            std::fs::write(dir.join(format!("{name}.wav")), wav).unwrap();
            println!(
                "{name}: peak {:.1} dBFS, early RMS {:.1} dBFS, {:.2} s",
                peak_db(cue),
                early_rms_db(cue),
                samples.len() as f32 / 96_000.0
            );
        }
    }
}
