//! Alerts that must not chatter (docs/DESIGN.md 7.6): a burst of messages or notices sounds once.
//!
//! `Notify` and `Message` are not answers to something the player did, so a flood of them (a
//! group chat, three requests at sign-in) would ring over and over. The first one sounds; the
//! next ones inside [`GAP`] stay silent, their toasts and badges still show. A dropped cue does
//! not extend the quiet, so a steady trickle still chimes once per [`GAP`]. Cues the player
//! caused, invitations, errors and rings are never held back.

use super::Cue;

/// The quiet after an alert, in seconds.
pub const GAP: f64 = 2.5;

/// Remembers when the last alert sounded.
#[derive(Clone, Copy, Debug, Default)]
pub struct AlertGate {
    last: Option<f64>,
}

impl AlertGate {
    /// Whether `cue` may sound at `now` (seconds). Every other cue passes untouched.
    pub fn allow(&mut self, cue: Cue, now: f64) -> bool {
        if !matches!(cue, Cue::Notify | Cue::Message) {
            return true;
        }
        match self.last {
            Some(last) if now - last < GAP => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_burst_of_messages_sounds_once() {
        let mut gate = AlertGate::default();
        assert!(gate.allow(Cue::Message, 10.0));
        assert!(!gate.allow(Cue::Message, 10.2));
        assert!(!gate.allow(Cue::Notify, 11.0));
        assert!(!gate.allow(Cue::Message, 12.4));
    }

    #[test]
    fn the_quiet_ends_and_a_dropped_cue_does_not_extend_it() {
        let mut gate = AlertGate::default();
        assert!(gate.allow(Cue::Notify, 0.0));
        assert!(!gate.allow(Cue::Notify, 2.0));
        assert!(gate.allow(Cue::Message, 2.6));
        assert!(!gate.allow(Cue::Message, 4.0));
    }

    #[test]
    fn other_cues_are_never_held_back() {
        let mut gate = AlertGate::default();
        assert!(gate.allow(Cue::Message, 0.0));
        for cue in [
            Cue::Select,
            Cue::Error,
            Cue::Invite,
            Cue::Ring,
            Cue::Unlocked,
        ] {
            assert!(gate.allow(cue, 0.1), "{cue:?}");
        }
    }
}
