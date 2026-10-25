//! When the pointer's tick plays (docs/DESIGN.md section 7.3, "Hover"): the decision as plain
//! data in, one bool out, so the rules are tested without a window or a device.
//!
//! The tick marks the pointer *arriving* on something it can use, so it needs a new interactive
//! element under a pointer that is really being moved by the player. Everything else stays
//! silent: resting inside an element, empty areas, content sliding under a resting pointer
//! (scrolling, a panel opening), a pressed button, a pad in use, a game in front, and the beat
//! after another cue. A minimum gap turns a fast sweep into a light ratchet.

/// The least time between two hover ticks, in seconds. A sweep over tiles 40 ms apart sounds
/// every other tile, about 16 ticks a second at the most.
pub const MIN_INTERVAL: f64 = 0.06;
/// Silence after any other cue (a focus move, a click's answer), so the two never stack.
pub const AFTER_CUE: f64 = 0.09;
/// How long after the last pointer movement an arrival still counts as the player's doing
/// (egui hit-tests against the previous frame, so the arrival can trail the movement).
pub const POINTER_ACTIVE: f64 = 0.15;
/// Quiet needed after the last scroll before arrivals count again.
pub const SCROLL_QUIET: f64 = 0.25;

/// What one frame knows.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    /// The frame's time in seconds.
    pub now: f64,
    /// The interactive element under the pointer, while the pointer is the device in use.
    pub hovered: Option<u64>,
    /// The pointer moved in this frame.
    pub pointer_moved: bool,
    /// A pointer button is held (a press, a drag).
    pub pointer_down: bool,
    /// A click completed in this frame.
    pub clicked: bool,
    /// The wheel or a scroll animation moved content in this frame.
    pub scrolling: bool,
    /// Another cue played in this frame (a focus move from keys, a click's answer).
    pub other_cue: bool,
    /// Interface Sounds on, no game starting or running, the launcher window focused.
    pub allowed: bool,
}

/// Remembers the element under the pointer and the times that matter.
#[derive(Clone, Debug)]
pub struct HoverGate {
    current: Option<u64>,
    last_tick: f64,
    last_cue: f64,
    last_moved: f64,
    last_scroll: f64,
}

impl Default for HoverGate {
    fn default() -> Self {
        let never = f64::NEG_INFINITY;
        Self {
            current: None,
            last_tick: never,
            last_cue: never,
            last_moved: never,
            last_scroll: never,
        }
    }
}

impl HoverGate {
    /// Feeds one frame; `true` means: play the hover tick now.
    pub fn update(&mut self, f: &Frame) -> bool {
        if f.pointer_moved {
            self.last_moved = f.now;
        }
        if f.scrolling {
            self.last_scroll = f.now;
        }
        if f.other_cue || f.clicked {
            self.last_cue = f.now;
        }
        // The element is remembered whether or not it ticks: one that arrived during a scroll or
        // inside the throttle gap is not announced late.
        let arrived = f.hovered.is_some() && f.hovered != self.current;
        self.current = f.hovered;
        if !arrived {
            return false;
        }
        let play = f.allowed
            && !f.pointer_down
            && f.now - self.last_moved <= POINTER_ACTIVE
            && f.now - self.last_scroll >= SCROLL_QUIET
            && f.now - self.last_tick >= MIN_INTERVAL
            && f.now - self.last_cue >= AFTER_CUE;
        if play {
            self.last_tick = f.now;
        }
        play
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame at `now` with the pointer moving over `hovered`, everything allowed.
    fn moving(now: f64, hovered: Option<u64>) -> Frame {
        Frame {
            now,
            hovered,
            pointer_moved: true,
            allowed: true,
            ..Frame::default()
        }
    }

    #[test]
    fn arriving_on_an_element_ticks_once() {
        let mut gate = HoverGate::default();
        assert!(!gate.update(&moving(1.0, None)), "an empty area is silent");
        assert!(gate.update(&moving(1.1, Some(7))));
        for k in 0..20 {
            assert!(
                !gate.update(&moving(1.2 + k as f64 * 0.016, Some(7))),
                "staying inside is silent"
            );
        }
    }

    #[test]
    fn leaving_and_coming_back_ticks_again() {
        let mut gate = HoverGate::default();
        assert!(gate.update(&moving(1.0, Some(1))));
        assert!(!gate.update(&moving(1.1, None)));
        assert!(gate.update(&moving(1.2, Some(1))));
    }

    #[test]
    fn a_sweep_is_a_light_ratchet_not_a_machine_gun() {
        let mut gate = HoverGate::default();
        // 40 ms per tile for two seconds: 50 arrivals.
        let ticks = (0..50)
            .filter(|&k| gate.update(&moving(1.0 + k as f64 * 0.04, Some(k + 1))))
            .count();
        assert_eq!(ticks, 25, "every second tile");
        // Never closer than the minimum gap, whatever the frame times.
        let mut gate = HoverGate::default();
        let mut last = f64::NEG_INFINITY;
        for k in 0..400 {
            let now = 5.0 + k as f64 * 0.007;
            if gate.update(&moving(now, Some(k + 1))) {
                assert!(now - last >= MIN_INTERVAL - 1e-9);
                last = now;
            }
        }
    }

    #[test]
    fn a_swallowed_arrival_is_not_announced_late() {
        let mut gate = HoverGate::default();
        assert!(gate.update(&moving(1.0, Some(1))));
        assert!(!gate.update(&moving(1.02, Some(2))), "inside the gap");
        assert!(
            !gate.update(&moving(1.2, Some(2))),
            "and still silent later"
        );
    }

    #[test]
    fn nothing_while_content_moves_under_a_resting_pointer() {
        let mut gate = HoverGate::default();
        assert!(gate.update(&moving(1.0, Some(1))));
        // The pointer rests; the layout slides another element under it.
        let rest = Frame {
            pointer_moved: false,
            ..moving(3.0, Some(2))
        };
        assert!(!gate.update(&rest));
    }

    #[test]
    fn nothing_while_scrolling_and_nothing_late_after_it() {
        let mut gate = HoverGate::default();
        let scroll = |now, hovered| Frame {
            scrolling: true,
            ..moving(now, hovered)
        };
        assert!(!gate.update(&scroll(1.0, Some(1))));
        assert!(!gate.update(&scroll(1.1, Some(2))));
        assert!(
            !gate.update(&moving(1.2, Some(3))),
            "the wheel just stopped"
        );
        assert!(!gate.update(&moving(1.5, Some(3))));
        assert!(gate.update(&moving(1.6, Some(4))), "scrolling is over");
    }

    #[test]
    fn nothing_while_a_button_is_held() {
        let mut gate = HoverGate::default();
        let drag = |now, hovered| Frame {
            pointer_down: true,
            ..moving(now, hovered)
        };
        assert!(!gate.update(&drag(1.0, Some(1))));
        assert!(!gate.update(&drag(1.2, Some(2))));
        assert!(gate.update(&moving(1.4, Some(3))));
    }

    #[test]
    fn it_does_not_double_with_another_cue() {
        let mut gate = HoverGate::default();
        // A key or a click moved the focus in this very frame.
        let focus = Frame {
            other_cue: true,
            ..moving(1.0, Some(1))
        };
        assert!(!gate.update(&focus));
        assert!(!gate.update(&moving(1.05, Some(2))), "the beat after it");
        assert!(gate.update(&moving(1.2, Some(3))));
        // A click that completes in the frame is an answer of its own.
        let click = Frame {
            clicked: true,
            ..moving(2.0, Some(3))
        };
        assert!(!gate.update(&click));
        assert!(!gate.update(&moving(2.05, Some(4))));
    }

    #[test]
    fn respects_the_switches_and_a_game_in_front() {
        let mut gate = HoverGate::default();
        let off = Frame {
            allowed: false,
            ..moving(1.0, Some(1))
        };
        assert!(!gate.update(&off), "sounds off or a game runs");
        assert!(gate.update(&moving(1.2, Some(2))));
    }

    #[test]
    fn switching_between_pad_and_mouse() {
        let mut gate = HoverGate::default();
        assert!(gate.update(&moving(1.0, Some(1))));
        // The pad takes over: the hover is gone, whatever the resting mouse is over.
        let pad = |now| Frame {
            pointer_moved: false,
            ..moving(now, None)
        };
        assert!(!gate.update(&pad(2.0)));
        assert!(!gate.update(&pad(3.0)));
        // The mouse moves again and lands on the same tile: it is an arrival.
        assert!(gate.update(&moving(4.0, Some(1))));
    }
}
