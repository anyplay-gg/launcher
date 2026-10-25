//! The edge bump (docs/DESIGN.md 6.7): when a direction has nowhere to go, the focus ring leans a
//! few dp toward the wall and springs back. It is one spring pair with a velocity kick, run on
//! `Motion::Focus`, so it needs no curve of its own, keeps the speed of a second bump and moves
//! only a transform. [`Nudge`] lives in the shell; [`publish`] hands its offset to the painters
//! through the egui context and [`offset`] reads it where the ring is drawn.

use egui::{Context, Vec2, vec2};

use crate::ui::anim::{Motion, Spring};
use crate::ui::input::Command;

/// Speed given to the ring, in dp/s. On `Motion::Focus` this peaks at about 3 dp after 55 ms and
/// is back inside a pixel in a quarter of a second.
pub const KICK_SPEED: f32 = 140.0;

/// The ring's offset from its element, a damped spring pair.
#[derive(Clone, Copy, Debug, Default)]
pub struct Nudge {
    x: Spring,
    y: Spring,
}

impl Nudge {
    /// Leans toward `direction`: a speed, not a jump, so a second bump on the way back adds up
    /// smoothly instead of snapping.
    pub fn kick(&mut self, direction: Command) {
        let (dx, dy) = match direction {
            Command::Left => (-1.0, 0.0),
            Command::Right => (1.0, 0.0),
            Command::Up => (0.0, -1.0),
            Command::Down => (0.0, 1.0),
            _ => return,
        };
        self.x.velocity += dx * KICK_SPEED;
        self.y.velocity += dy * KICK_SPEED;
    }

    /// Advances by `dt` seconds and returns the offset in dp.
    pub fn step(&mut self, dt: f32) -> Vec2 {
        let params = Motion::FOCUS;
        params.step(&mut self.x, 0.0, dt);
        params.step(&mut self.y, 0.0, dt);
        vec2(self.x.value, self.y.value)
    }

    pub fn is_settled(&self) -> bool {
        self.x.is_settled(0.0) && self.y.is_settled(0.0)
    }
}

fn id() -> egui::Id {
    egui::Id::unique("anyps5-launcher/nudge")
}

/// Stores this frame's offset for the painters.
pub fn publish(ctx: &Context, offset: Vec2) {
    ctx.data_mut(|d| d.insert_temp(id(), offset));
}

/// How far the focus ring leans this frame (zero at rest).
pub fn offset(ctx: &Context) -> Vec2 {
    ctx.data(|d| d.get_temp(id())).unwrap_or(Vec2::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(nudge: &mut Nudge) -> Vec<Vec2> {
        (0..60).map(|_| nudge.step(1.0 / 240.0)).collect()
    }

    #[test]
    fn a_kick_peaks_near_three_dp_and_comes_back_without_ringing() {
        let mut nudge = Nudge::default();
        nudge.kick(Command::Right);
        let path = run(&mut nudge);
        let peak = path.iter().map(|v| v.x).fold(0.0, f32::max);
        assert!((2.5..=3.5).contains(&peak), "peak {peak}");
        let at_peak = path.iter().position(|v| v.x == peak).unwrap() as f32 / 240.0;
        assert!((0.04..=0.07).contains(&at_peak), "peak after {at_peak} s");
        // The undershoot on the way back is invisible (under a quarter of a dp).
        let low = path.iter().map(|v| v.x).fold(0.0, f32::min);
        assert!(low > -0.25, "low {low}");
        assert!(path.iter().all(|v| v.y == 0.0));
        // Settled inside a quarter of a second.
        assert!(path[59].x.abs() < 0.35, "{}", path[59].x);
        for _ in 0..240 {
            nudge.step(1.0 / 240.0);
        }
        assert!(nudge.is_settled());
    }

    #[test]
    fn the_direction_decides_the_axis_and_sign() {
        let lean = |command| {
            let mut nudge = Nudge::default();
            nudge.kick(command);
            nudge.step(0.05)
        };
        let left = lean(Command::Left);
        assert!(left.x < 0.0 && left.y == 0.0);
        let up = lean(Command::Up);
        assert!(up.y < 0.0 && up.x == 0.0);
        let down = lean(Command::Down);
        assert!(down.y > 0.0 && down.x == 0.0);
        let mut nudge = Nudge::default();
        nudge.kick(Command::Select);
        assert!(nudge.is_settled(), "only directions lean");
    }

    #[test]
    fn the_offset_goes_through_the_context() {
        let ctx = Context::default();
        assert_eq!(offset(&ctx), Vec2::ZERO);
        publish(&ctx, vec2(2.0, -1.0));
        assert_eq!(offset(&ctx), vec2(2.0, -1.0));
    }
}
