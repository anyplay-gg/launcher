//! The unlocking progress (docs/DESIGN.md 5.4): the five steps and their shares of one overall bar,
//! and the coalescing of progress events. The hero paints the bar itself (`screens::home`).
//!
//! The step enum mirrors the pipeline's (`Unwrap, Relink, Copy, Assemble, Finish`); the pipeline
//! step maps onto it with [`UnlockStep::from_core`] once the pipeline types are on the host's side.

use crate::ui::i18n::{Key, Language};

/// One step of the unlock: the pipeline's own enum (`Unwrap, Relink, Copy, Assemble, Finish`;
/// the original calls Assemble "bundle").
pub use anyps5_core::unlock::Step as UnlockStep;

/// The steps in order.
pub const STEPS: [UnlockStep; 5] = [
    UnlockStep::Unwrap,
    UnlockStep::Relink,
    UnlockStep::Copy,
    UnlockStep::Assemble,
    UnlockStep::Finish,
];

/// Where a step starts on the overall bar.
pub const fn step_start(step: UnlockStep) -> f32 {
    match step {
        UnlockStep::Unwrap => 0.0,
        UnlockStep::Relink => 0.12,
        UnlockStep::Copy => 0.62,
        UnlockStep::Assemble => 0.88,
        UnlockStep::Finish => 0.96,
    }
}

/// How much of the overall bar a step covers.
pub const fn step_length(step: UnlockStep) -> f32 {
    match step {
        UnlockStep::Unwrap => 0.12,
        UnlockStep::Relink => 0.50,
        UnlockStep::Copy => 0.26,
        UnlockStep::Assemble => 0.08,
        UnlockStep::Finish => 0.04,
    }
}

/// The text key of a step's title.
pub const fn step_key(step: UnlockStep) -> Key {
    match step {
        UnlockStep::Unwrap => Key::StepUnwrap,
        UnlockStep::Relink => Key::StepRelink,
        UnlockStep::Copy => Key::StepCopy,
        UnlockStep::Assemble => Key::StepBundle,
        UnlockStep::Finish => Key::StepFinish,
    }
}

pub fn step_title(step: UnlockStep, language: Language) -> &'static str {
    language.text(step_key(step))
}

/// `start + length * fraction`
pub fn step_overall(step: UnlockStep, fraction: f32) -> f32 {
    step_start(step) + step_length(step) * fraction.clamp(0.0, 1.0)
}

/// Where an unlock stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnlockProgress {
    pub step: UnlockStep,
    /// Progress inside the step, 0..=1.
    pub fraction: f32,
}

impl UnlockProgress {
    pub fn new(step: UnlockStep, fraction: f32) -> Self {
        Self { step, fraction }
    }

    pub fn overall(&self) -> f32 {
        step_overall(self.step, self.fraction)
    }

    /// `Int(overall * 100)`: the percentage the hero shows.
    pub fn percent(&self) -> u32 {
        (self.overall() * 100.0) as u32
    }

    /// The relinker runs for a while without reporting: the bar sweeps instead.
    pub fn indeterminate(&self) -> bool {
        self.step == UnlockStep::Relink && self.fraction == 0.0
    }

    /// Progress events are coalesced: the bar updates when the step changes, the fraction moved by
    /// at least 0.005, or the step is done.
    pub fn should_replace(&self, next: &UnlockProgress) -> bool {
        next.step != self.step
            || (next.fraction - self.fraction).abs() >= 0.005
            || next.fraction >= 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_steps_cover_the_whole_bar_without_gaps() {
        let mut at = 0.0_f32;
        for step in STEPS {
            assert!(
                (step_start(step) - at).abs() < 1e-6,
                "{step:?} starts at {at}"
            );
            at += step_length(step);
        }
        assert!((at - 1.0).abs() < 1e-6);
    }

    #[test]
    fn overall_is_start_plus_length_times_fraction() {
        assert_eq!(step_overall(UnlockStep::Copy, 0.0), 0.62);
        assert!((step_overall(UnlockStep::Copy, 0.4) - 0.724).abs() < 1e-6);
        assert!((step_overall(UnlockStep::Finish, 1.0) - 1.0).abs() < 1e-6);
        assert_eq!(step_overall(UnlockStep::Relink, 2.0), 0.62, "clamped");
    }

    #[test]
    fn the_screenshot_value_is_39_percent() {
        // launcher-unlocking.jpg shows the relink step ("Converting") at 39 %.
        let p = UnlockProgress::new(UnlockStep::Relink, 0.55);
        assert_eq!(p.percent(), 39);
    }

    #[test]
    fn percent_truncates_like_int() {
        assert_eq!(UnlockProgress::new(UnlockStep::Unwrap, 0.99).percent(), 11);
        assert_eq!(UnlockProgress::new(UnlockStep::Finish, 1.0).percent(), 100);
    }

    #[test]
    fn only_the_relink_start_sweeps() {
        assert!(UnlockProgress::new(UnlockStep::Relink, 0.0).indeterminate());
        assert!(!UnlockProgress::new(UnlockStep::Relink, 0.1).indeterminate());
        assert!(!UnlockProgress::new(UnlockStep::Unwrap, 0.0).indeterminate());
    }

    #[test]
    fn events_are_coalesced() {
        let a = UnlockProgress::new(UnlockStep::Copy, 0.10);
        assert!(!a.should_replace(&UnlockProgress::new(UnlockStep::Copy, 0.103)));
        assert!(a.should_replace(&UnlockProgress::new(UnlockStep::Copy, 0.11)));
        assert!(a.should_replace(&UnlockProgress::new(UnlockStep::Assemble, 0.10)));
        assert!(a.should_replace(&UnlockProgress::new(UnlockStep::Copy, 1.0)));
    }

    #[test]
    fn titles_exist_in_both_languages() {
        for step in STEPS {
            for language in Language::ALL {
                assert!(!step_title(step, language).is_empty());
            }
        }
        let system = if cfg!(windows) {
            "Windows"
        } else if cfg!(target_os = "macos") {
            "macOS"
        } else {
            "Linux"
        };
        assert_eq!(
            step_title(UnlockStep::Relink, Language::English),
            format!("Converting for {system}")
        );
    }
}
