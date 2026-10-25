//! The press of a key or a pad button (docs/DESIGN.md 3.9 and 6.8): a mouse click shows the
//! control pressed in (`Motion::Press`, scale 0.97) for as long as the button is down; a select
//! from a pad or a key has no "down" to show, so it holds the focused control pressed for
//! [`HOLD`] and lets go. The same easing, the same scale, the same frame as the cue.

use egui::Context;

use crate::ui::anim::AnimExt;

/// How long a select holds the focused control pressed, in seconds. Two frames at 60 Hz plus a
/// little: long enough for the eased scale (0.10 s) to start down and be seen, short enough that
/// the release is part of the same gesture.
pub const HOLD: f64 = 0.09;

fn id() -> egui::Id {
    egui::Id::unique("anyps5-launcher/press")
}

/// A select arrived: the focused control looks pressed for [`HOLD`].
pub fn trigger(ctx: &Context) {
    let until = ctx.clock() + HOLD;
    ctx.data_mut(|d| d.insert_temp(id(), until));
    ctx.request_repaint_after(std::time::Duration::from_secs_f64(HOLD));
}

/// Whether a select is being held right now.
pub fn active(ctx: &Context) -> bool {
    let until: f64 = ctx.data(|d| d.get_temp(id())).unwrap_or(0.0);
    ctx.clock() < until
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_select_holds_for_ninety_milliseconds_and_lets_go() {
        let ctx = Context::default();
        assert!(!active(&ctx));
        // The clock of a bare context starts at 0.
        trigger(&ctx);
        assert!(active(&ctx));
        let until: f64 = ctx.data(|d| d.get_temp(id())).unwrap();
        assert!((until - ctx.clock() - HOLD).abs() < 1e-9);
    }
}
