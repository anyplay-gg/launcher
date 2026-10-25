//! Motion (docs/DESIGN.md section 2): a spring solver matching SwiftUI's `.spring(response:,
//! dampingFraction:)`, cubic-bezier easings, reduced-motion handling and a small retargetable,
//! time-based API for screens.
//!
//! ```ignore
//! let t = ctx.focus_t(("tile", id), focused);                 // 0..=1, Motion::Focus spring
//! let x = ctx.animate_to(("row", id, "x"), target_x, Motion::Layout);
//! ```
//!
//! Every animated property is `lerp(from, to, value)`; the value carries its velocity over when
//! the target changes, so retargeting mid-flight never jumps (the spring is interruptible).

use std::collections::HashMap;
use std::hash::Hash;

use egui::{Context, Id};

/// Animations count as finished inside this distance of the target and below this speed.
const SETTLE_DISTANCE: f32 = 0.001;
const SETTLE_SPEED: f32 = 0.01;
/// A stalled frame must not catapult a spring.
const MAX_DT: f32 = 1.0 / 20.0;

// ---------------------------------------------------------------------------------------------
// Springs (2.1)
// ---------------------------------------------------------------------------------------------

/// SwiftUI spring parameters: `response` is the period of the undamped oscillation in seconds,
/// `damping` the damping fraction (1.0 = critically damped).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringParams {
    pub response: f32,
    pub damping: f32,
}

impl SpringParams {
    pub const fn new(response: f32, damping: f32) -> Self {
        Self { response, damping }
    }

    /// Natural frequency `omega = 2 pi / response`.
    pub fn omega(&self) -> f32 {
        std::f32::consts::TAU / self.response
    }

    /// Stiffness of the unit-mass spring: `k = omega^2`.
    pub fn stiffness(&self) -> f32 {
        self.omega() * self.omega()
    }

    /// Damping coefficient of the unit-mass spring: `c = 2 z omega`.
    pub fn damping_coefficient(&self) -> f32 {
        2.0 * self.damping * self.omega()
    }

    /// Overshoot of a step response as a fraction of the step (0 when critically damped).
    pub fn overshoot(&self) -> f32 {
        if self.damping >= 1.0 {
            0.0
        } else {
            (-std::f32::consts::PI * self.damping / (1.0 - self.damping * self.damping).sqrt())
                .exp()
        }
    }

    /// Advances `spring` toward `target` by `dt` seconds. Exact for the linear ODE
    /// `x'' = -k (x - target) - c x'`, so it is frame-rate independent and stable at any `dt`.
    pub fn step(&self, spring: &mut Spring, target: f32, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let w = self.omega() as f64;
        let z = self.damping as f64;
        let d0 = (spring.value - target) as f64;
        let v0 = spring.velocity as f64;
        let t = dt as f64;
        let (d, v) = if z < 1.0 {
            let wd = w * (1.0 - z * z).sqrt();
            let e = (-z * w * t).exp();
            let (sin, cos) = (wd * t).sin_cos();
            let b = (v0 + z * w * d0) / wd;
            let d = e * (d0 * cos + b * sin);
            let v = e * ((wd * b - z * w * d0) * cos - (wd * d0 + z * w * b) * sin);
            (d, v)
        } else {
            // Critically damped (the design never uses an overdamped spring; treat z > 1 alike).
            let e = (-w * t).exp();
            let c = v0 + w * d0;
            (e * (d0 + c * t), e * (v0 - w * c * t))
        };
        spring.value = target + d as f32;
        spring.velocity = v as f32;
        if (spring.value - target).abs() < SETTLE_DISTANCE && spring.velocity.abs() < SETTLE_SPEED {
            spring.value = target;
            spring.velocity = 0.0;
        }
    }
}

/// An animated scalar with velocity.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
}

impl Spring {
    pub const fn at(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
        }
    }

    pub fn is_settled(&self, target: f32) -> bool {
        (self.value - target).abs() < SETTLE_DISTANCE && self.velocity.abs() < SETTLE_SPEED
    }
}

// ---------------------------------------------------------------------------------------------
// Easing (2.2)
// ---------------------------------------------------------------------------------------------

/// A CSS-style cubic bezier easing curve through (0,0) and (1,1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bezier(pub f32, pub f32, pub f32, pub f32);

impl Bezier {
    /// SwiftUI `easeIn`.
    pub const EASE_IN: Bezier = Bezier(0.42, 0.0, 1.0, 1.0);
    /// SwiftUI `easeOut`.
    pub const EASE_OUT: Bezier = Bezier(0.0, 0.0, 0.58, 1.0);
    /// SwiftUI `easeInOut`.
    pub const EASE_IN_OUT: Bezier = Bezier(0.42, 0.0, 0.58, 1.0);
    pub const LINEAR: Bezier = Bezier(0.0, 0.0, 1.0, 1.0);

    /// Eased progress for linear progress `t` in 0..=1 (clamped).
    pub fn ease(&self, t: f32) -> f32 {
        cubic_bezier(self.0, self.1, self.2, self.3, t)
    }
}

/// Solves the bezier for `x = t` with Newton's method (bisection as the safety net) and returns y.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t == 0.0 || t == 1.0 {
        return t;
    }
    let (x1, y1, x2, y2, x) = (x1 as f64, y1 as f64, x2 as f64, y2 as f64, t as f64);
    let coeff = |a: f64, b: f64| (3.0 * a, 3.0 * (b - a) - 3.0 * a, 1.0 - 3.0 * b + 3.0 * a);
    let (cx, bx, ax) = coeff(x1, x2);
    let (cy, by, ay) = coeff(y1, y2);
    let sample_x = |s: f64| ((ax * s + bx) * s + cx) * s;
    let sample_y = |s: f64| ((ay * s + by) * s + cy) * s;
    let slope_x = |s: f64| (3.0 * ax * s + 2.0 * bx) * s + cx;
    let mut s = x;
    for _ in 0..8 {
        let err = sample_x(s) - x;
        if err.abs() < 1e-7 {
            return sample_y(s) as f32;
        }
        let slope = slope_x(s);
        if slope.abs() < 1e-6 {
            break;
        }
        s -= err / slope;
    }
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    s = x;
    for _ in 0..40 {
        let v = sample_x(s);
        if (v - x).abs() < 1e-7 {
            break;
        }
        if v < x {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) * 0.5;
    }
    sample_y(s) as f32
}

/// Linear interpolation.
pub fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

// ---------------------------------------------------------------------------------------------
// Motion presets (2.1 table, 2.3 reduced motion)
// ---------------------------------------------------------------------------------------------

/// A way to get from the old value to the new one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Spring(SpringParams),
    Ease { bezier: Bezier, duration: f32 },
}

/// The named animations of the design.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    /// Focus/hover feedback, switch, key scroll: response 0.32, damping 0.82.
    Focus,
    /// Pointer hover: easeOut 0.12 s, the same with reduced motion (a colour change only).
    Hover,
    /// A control pressed by the pointer: easeOut 0.10 s both ways, the same with reduced motion. A
    /// spring would lag behind the click; an ease retargets from the current value, so a quick
    /// click-and-release reverses mid-press instead of finishing it first.
    Press,
    /// Tile row, hero swap, onboarding steps, list reflow: 0.45 / 0.86.
    Layout,
    /// Panels, splash, toast: 0.42 / 0.90.
    Overlay,
    /// Backdrop cross-fade and onboarding finish: easeInOut 0.55 s (0.15 s when reduced).
    Fade,
    /// Trophy card slide-in: 0.55 / 0.78 (easeOut 0.25 s when reduced).
    BannerSlide,
    /// Trophy icon pop: 0.45 / 0.55. Not affected by reduced motion.
    BannerPop,
    /// Progress bar width: easeOut 0.35 s. Not affected by reduced motion.
    Progress,
}

impl Motion {
    pub const FOCUS: SpringParams = SpringParams::new(0.32, 0.82);
    pub const LAYOUT: SpringParams = SpringParams::new(0.45, 0.86);
    pub const OVERLAY: SpringParams = SpringParams::new(0.42, 0.90);
    pub const BANNER_SLIDE: SpringParams = SpringParams::new(0.55, 0.78);
    pub const BANNER_POP: SpringParams = SpringParams::new(0.45, 0.55);

    /// The curve for this motion, honouring "Animations: Reduced".
    pub fn curve(self, reduced: bool) -> Curve {
        let ease_out = |duration| Curve::Ease {
            bezier: Bezier::EASE_OUT,
            duration,
        };
        match (self, reduced) {
            (Motion::Focus, false) => Curve::Spring(Self::FOCUS),
            (Motion::Focus, true) => ease_out(0.12),
            (Motion::Hover, _) => ease_out(0.12),
            (Motion::Press, _) => ease_out(0.10),
            (Motion::Layout, false) => Curve::Spring(Self::LAYOUT),
            (Motion::Layout, true) => ease_out(0.15),
            (Motion::Overlay, false) => Curve::Spring(Self::OVERLAY),
            (Motion::Overlay, true) => ease_out(0.15),
            (Motion::Fade, reduced) => Curve::Ease {
                bezier: Bezier::EASE_IN_OUT,
                duration: if reduced { 0.15 } else { 0.55 },
            },
            (Motion::BannerSlide, false) => Curve::Spring(Self::BANNER_SLIDE),
            (Motion::BannerSlide, true) => ease_out(0.25),
            (Motion::BannerPop, _) => Curve::Spring(Self::BANNER_POP),
            (Motion::Progress, _) => ease_out(0.35),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tracker: one animated value (pure, testable)
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Tween {
    from: f32,
    elapsed: f32,
    duration: f32,
    bezier: Bezier,
}

/// One retargetable animated value. Springs keep their velocity when the target changes; easings
/// restart from the current value.
#[derive(Clone, Copy, Debug)]
pub struct Tracker {
    spring: Spring,
    target: f32,
    tween: Option<Tween>,
    delay: f32,
}

impl Tracker {
    pub const fn new(value: f32) -> Self {
        Self {
            spring: Spring::at(value),
            target: value,
            tween: None,
            delay: 0.0,
        }
    }

    pub fn value(&self) -> f32 {
        self.spring.value
    }

    pub fn velocity(&self) -> f32 {
        self.spring.velocity
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn is_settled(&self) -> bool {
        self.delay <= 0.0 && self.tween.is_none() && self.spring.is_settled(self.target)
    }

    /// Sets a new target; `delay` seconds pass before the value starts to move.
    pub fn retarget(&mut self, target: f32, curve: Curve, delay: f32) {
        if target == self.target {
            return;
        }
        self.target = target;
        self.delay = delay.max(0.0);
        match curve {
            Curve::Spring(_) => {
                // A running tween hands over with the value it reached and no speed.
                if self.tween.take().is_some() {
                    self.spring.velocity = 0.0;
                }
            }
            Curve::Ease { bezier, duration } => {
                self.spring.velocity = 0.0;
                self.tween = Some(Tween {
                    from: self.spring.value,
                    elapsed: 0.0,
                    duration: duration.max(1e-3),
                    bezier,
                });
            }
        }
    }

    /// Advances by `dt` seconds and returns the new value.
    pub fn advance(&mut self, curve: Curve, dt: f32) -> f32 {
        let mut dt = dt.max(0.0);
        if self.delay > 0.0 {
            self.delay -= dt;
            if self.delay > 0.0 {
                return self.spring.value;
            }
            dt = -self.delay;
            self.delay = 0.0;
        }
        match (curve, self.tween.as_mut()) {
            (Curve::Ease { .. }, Some(tween)) => {
                tween.elapsed += dt;
                let progress = (tween.elapsed / tween.duration).min(1.0);
                self.spring.value = lerp(tween.from, self.target, tween.bezier.ease(progress));
                if progress >= 1.0 {
                    self.spring.value = self.target;
                    self.tween = None;
                }
            }
            (Curve::Spring(params), _) => {
                self.tween = None;
                // Sub-step long frames so a 50 ms hitch stays smooth.
                let mut left = dt;
                while left > 0.0 {
                    let slice = left.min(1.0 / 120.0);
                    params.step(&mut self.spring, self.target, slice);
                    left -= slice;
                }
            }
            (Curve::Ease { .. }, None) => {
                // Curve type changed (reduced motion toggled) or nothing to do: snap if settled.
                if !self.spring.is_settled(self.target) {
                    self.spring.value = self.target;
                    self.spring.velocity = 0.0;
                }
            }
        }
        self.spring.value
    }
}

// ---------------------------------------------------------------------------------------------
// egui integration
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Store {
    trackers: HashMap<Id, Slot>,
    reduced: bool,
    pruned_at: f64,
}

#[derive(Clone, Copy)]
struct Slot {
    tracker: Tracker,
    last_pass: u64,
    last_used: f64,
}

fn store_id() -> Id {
    Id::unique("anyps5-launcher/anim")
}

/// Animation entry points on [`egui::Context`]. Ids are anything hashable; combine the widget's
/// identity with a purpose, for example `("tile", game_id, "size")`.
pub trait AnimExt {
    /// Turns reduced motion on or off ("Animations: Reduced"). Call when the setting changes.
    fn set_reduced_motion(&self, reduced: bool);
    fn reduced_motion(&self) -> bool;
    /// Animated value that follows `target`; the first call starts at `target` (no entrance).
    fn animate_to(&self, id: impl Hash + std::fmt::Debug, target: f32, motion: Motion) -> f32;
    /// Like [`animate_to`](Self::animate_to) but the first call starts at `from`.
    fn animate_from_to(
        &self,
        id: impl Hash + std::fmt::Debug,
        from: f32,
        target: f32,
        motion: Motion,
    ) -> f32;
    /// Like [`animate_to`](Self::animate_to) with `delay` seconds before a new target takes effect.
    fn animate_to_delayed(
        &self,
        id: impl Hash + std::fmt::Debug,
        target: f32,
        motion: Motion,
        delay: f32,
    ) -> f32;
    /// 0..=1 follower of a boolean, with `Motion::Focus`: the standard focus/hover amount.
    fn focus_t(&self, id: impl Hash + std::fmt::Debug, on: bool) -> f32;
    /// 0..=1 follower of a boolean with a given motion.
    fn flag_t(&self, id: impl Hash + std::fmt::Debug, on: bool, motion: Motion) -> f32;
    /// The current value of an animation without retargeting it (`None` if never started).
    fn animation_value(&self, id: impl Hash + std::fmt::Debug) -> Option<f32>;
    /// Seconds since the program started, from egui's clock.
    fn clock(&self) -> f64;
}

impl AnimExt for Context {
    fn set_reduced_motion(&self, reduced: bool) {
        self.data_mut(|d| d.get_temp_mut_or_default::<Store>(store_id()).reduced = reduced);
        self.request_repaint();
    }

    fn reduced_motion(&self) -> bool {
        self.data_mut(|d| d.get_temp_mut_or_default::<Store>(store_id()).reduced)
    }

    fn animate_to(&self, id: impl Hash + std::fmt::Debug, target: f32, motion: Motion) -> f32 {
        self.animate_inner(Id::unique(id), None, target, motion, 0.0)
    }

    fn animate_from_to(
        &self,
        id: impl Hash + std::fmt::Debug,
        from: f32,
        target: f32,
        motion: Motion,
    ) -> f32 {
        self.animate_inner(Id::unique(id), Some(from), target, motion, 0.0)
    }

    fn animate_to_delayed(
        &self,
        id: impl Hash + std::fmt::Debug,
        target: f32,
        motion: Motion,
        delay: f32,
    ) -> f32 {
        self.animate_inner(Id::unique(id), None, target, motion, delay)
    }

    fn focus_t(&self, id: impl Hash + std::fmt::Debug, on: bool) -> f32 {
        self.animate_to(id, if on { 1.0 } else { 0.0 }, Motion::Focus)
    }

    fn flag_t(&self, id: impl Hash + std::fmt::Debug, on: bool, motion: Motion) -> f32 {
        self.animate_to(id, if on { 1.0 } else { 0.0 }, motion)
    }

    fn animation_value(&self, id: impl Hash + std::fmt::Debug) -> Option<f32> {
        let id = Id::unique(id);
        self.data_mut(|d| {
            d.get_temp_mut_or_default::<Store>(store_id())
                .trackers
                .get(&id)
                .map(|slot| slot.tracker.value())
        })
    }

    fn clock(&self) -> f64 {
        self.input(|i| i.time)
    }
}

trait AnimInner {
    fn animate_inner(
        &self,
        id: Id,
        from: Option<f32>,
        target: f32,
        motion: Motion,
        delay: f32,
    ) -> f32;
}

impl AnimInner for Context {
    fn animate_inner(
        &self,
        id: Id,
        from: Option<f32>,
        target: f32,
        motion: Motion,
        delay: f32,
    ) -> f32 {
        let (dt, time) = self.input(|i| (i.unstable_dt.min(MAX_DT), i.time));
        let pass = self.cumulative_pass_nr();
        let (value, settled) = self.data_mut(|d| {
            let store = d.get_temp_mut_or_default::<Store>(store_id());
            let curve = motion.curve(store.reduced);
            let slot = store.trackers.entry(id).or_insert_with(|| Slot {
                tracker: Tracker::new(from.unwrap_or(target)),
                last_pass: pass,
                last_used: time,
            });
            slot.tracker.retarget(target, curve, delay);
            slot.last_used = time;
            let value = if slot.last_pass == pass {
                slot.tracker.value()
            } else {
                slot.last_pass = pass;
                slot.tracker.advance(curve, dt)
            };
            let settled = slot.tracker.is_settled();
            // Forget animations nobody asked about for a while.
            if time - store.pruned_at > 10.0 {
                store.pruned_at = time;
                store.trackers.retain(|_, s| time - s.last_used < 10.0);
            }
            (value, settled)
        });
        if !settled {
            self.request_repaint();
        }
        value
    }
}

// ---------------------------------------------------------------------------------------------
// Endless animations (time-based, stateless)
// ---------------------------------------------------------------------------------------------

/// Eased ping-pong between 0 and 1: `each_way` seconds up, `each_way` seconds down.
pub fn ping_pong(time: f64, each_way: f64, bezier: Bezier) -> f32 {
    let phase = (time / each_way).rem_euclid(2.0);
    let t = if phase < 1.0 { phase } else { 2.0 - phase };
    bezier.ease(t as f32)
}

/// Eased sawtooth: 0 to 1 over `period` seconds, then jump back (no autoreverse).
pub fn sweep(time: f64, period: f64, bezier: Bezier) -> f32 {
    bezier.ease(((time / period).rem_euclid(1.0)) as f32)
}

/// The indeterminate progress sweep (2.4 #17): x position of the 35 %-wide gradient as a fraction
/// of the bar width, from -0.4 to 1.05 every 1.3 s.
pub fn indeterminate_phase(time: f64) -> f32 {
    lerp(-0.4, 1.05, sweep(time, 1.3, Bezier::EASE_IN_OUT))
}

/// The "Press a key..." pulse: opacity 1 <-> 0.45 with easeInOut 0.7 s each way.
pub fn pulse_opacity(time: f64) -> f32 {
    lerp(1.0, 0.45, ping_pong(time, 0.7, Bezier::EASE_IN_OUT))
}

/// Splash breathing: 0..1 eased, 1.6 s each way.
pub fn breathing(time: f64) -> f32 {
    ping_pong(time, 1.6, Bezier::EASE_IN_OUT)
}

/// Artwork drift: 0..1 eased, 24 s each way (28 s for the user picture).
pub fn drift(time: f64, each_way: f64) -> f32 {
    ping_pong(time, each_way, Bezier::EASE_IN_OUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seconds until a unit step response stays within 2 % of the target for good.
    fn settle_time(params: SpringParams) -> f32 {
        let mut spring = Spring::at(0.0);
        let dt = 1.0 / 1000.0;
        let mut last_outside = 0.0;
        for i in 0..3000 {
            params.step(&mut spring, 1.0, dt);
            if (spring.value - 1.0).abs() > 0.02 {
                last_outside = (i + 1) as f32 * dt;
            }
        }
        last_outside
    }

    fn peak(params: SpringParams) -> f32 {
        let mut spring = Spring::at(0.0);
        let mut peak = 0.0f32;
        for _ in 0..3000 {
            params.step(&mut spring, 1.0, 1.0 / 1000.0);
            peak = peak.max(spring.value);
        }
        peak - 1.0
    }

    #[test]
    fn stiffness_and_damping_match_the_conversion_table() {
        let table = [
            (Motion::FOCUS, 19.63, 385.5, 32.2),
            (Motion::LAYOUT, 13.96, 195.0, 24.0),
            (Motion::OVERLAY, 14.96, 223.8, 26.9),
            (Motion::BANNER_SLIDE, 11.42, 130.5, 17.8),
            (Motion::BANNER_POP, 13.96, 195.0, 15.4),
        ];
        for (p, omega, k, c) in table {
            assert!((p.omega() - omega).abs() < 0.02, "omega {p:?}");
            assert!(
                (p.stiffness() - k).abs() < 0.6,
                "k {p:?}: {}",
                p.stiffness()
            );
            assert!((p.damping_coefficient() - c).abs() < 0.1, "c {p:?}");
        }
    }

    #[test]
    fn overshoot_matches_the_table() {
        let table = [
            (Motion::FOCUS, 0.011),
            (Motion::LAYOUT, 0.005),
            (Motion::OVERLAY, 0.0015),
            (Motion::BANNER_SLIDE, 0.020),
            (Motion::BANNER_POP, 0.126),
        ];
        for (p, want) in table {
            assert!((peak(p) - want).abs() < 0.0015, "{p:?}: {}", peak(p));
            assert!((p.overshoot() - want).abs() < 0.0015, "{p:?} formula");
        }
    }

    #[test]
    fn settle_times_match_the_table() {
        let table = [
            (Motion::FOCUS, 0.25),
            (Motion::LAYOUT, 0.33),
            (Motion::OVERLAY, 0.30),
            (Motion::BANNER_SLIDE, 0.45),
            (Motion::BANNER_POP, 0.52),
        ];
        for (p, want) in table {
            // The table quotes the envelope bound ln(50) / (zeta omega); the exact response is
            // inside it (within 15 %: the oscillation amplitude is 1/sqrt(1 - zeta^2) times the envelope).
            let envelope = 50f32.ln() / (p.damping * p.omega());
            assert!((envelope - want).abs() < 0.03, "{p:?}: envelope {envelope}");
            let got = settle_time(p);
            assert!(
                got <= envelope * 1.15 && got > envelope * 0.5,
                "{p:?}: {got}"
            );
        }
    }

    #[test]
    fn critically_damped_never_overshoots() {
        assert!(peak(SpringParams::new(0.4, 1.0)) < 1e-4);
    }

    #[test]
    fn step_is_independent_of_frame_rate() {
        let params = Motion::LAYOUT;
        let run = |dt: f32| {
            let mut s = Spring::at(0.0);
            let mut t = 0.0;
            while t < 0.2 - 1e-6 {
                params.step(&mut s, 1.0, dt);
                t += dt;
            }
            s.value
        };
        assert!((run(1.0 / 60.0 / 3.0) - run(0.2 / 12.0)).abs() < 1e-4);
        assert!((run(0.01) - run(0.002)).abs() < 1e-4);
    }

    #[test]
    fn retargeting_keeps_velocity() {
        let mut s = Spring::at(0.0);
        for _ in 0..8 {
            Motion::FOCUS.step(&mut s, 1.0, 1.0 / 60.0);
        }
        let v = s.velocity;
        assert!(v > 0.5);
        // The same state, new target: value and velocity are unchanged at the instant of retarget.
        let mut tracker = Tracker::new(0.0);
        tracker.retarget(1.0, Curve::Spring(Motion::FOCUS), 0.0);
        for _ in 0..8 {
            tracker.advance(Curve::Spring(Motion::FOCUS), 1.0 / 60.0);
        }
        let (value, velocity) = (tracker.value(), tracker.velocity());
        tracker.retarget(0.0, Curve::Spring(Motion::FOCUS), 0.0);
        assert_eq!(tracker.value(), value);
        assert_eq!(tracker.velocity(), velocity);
    }

    #[test]
    fn bezier_endpoints_and_monotonic() {
        for b in [
            Bezier::EASE_IN,
            Bezier::EASE_OUT,
            Bezier::EASE_IN_OUT,
            Bezier::LINEAR,
        ] {
            assert_eq!(b.ease(0.0), 0.0);
            assert_eq!(b.ease(1.0), 1.0);
            let mut last = 0.0;
            for i in 1..=100 {
                let y = b.ease(i as f32 / 100.0);
                assert!(y >= last - 1e-5, "{b:?} not monotonic at {i}");
                last = y;
            }
        }
        // Reference values of the CSS ease-in-out curve.
        assert!((Bezier::EASE_IN_OUT.ease(0.5) - 0.5).abs() < 1e-4);
        assert!((Bezier::EASE_IN_OUT.ease(0.25) - 0.129).abs() < 0.01);
        assert!(Bezier::EASE_OUT.ease(0.5) > 0.5);
        assert!(Bezier::EASE_IN.ease(0.5) < 0.5);
        assert!((Bezier::LINEAR.ease(0.3) - 0.3).abs() < 1e-5);
    }

    #[test]
    fn a_press_is_a_short_ease_never_a_spring() {
        for reduced in [false, true] {
            assert!(matches!(
                Motion::Press.curve(reduced),
                Curve::Ease { duration, .. } if duration <= 0.12
            ));
        }
    }

    #[test]
    fn reduced_motion_swaps_the_curves() {
        assert!(matches!(Motion::Focus.curve(false), Curve::Spring(_)));
        assert_eq!(
            Motion::Focus.curve(true),
            Curve::Ease {
                bezier: Bezier::EASE_OUT,
                duration: 0.12
            }
        );
        assert_eq!(
            Motion::Fade.curve(false),
            Curve::Ease {
                bezier: Bezier::EASE_IN_OUT,
                duration: 0.55
            }
        );
        assert_eq!(
            Motion::Fade.curve(true),
            Curve::Ease {
                bezier: Bezier::EASE_IN_OUT,
                duration: 0.15
            }
        );
        // The banner pop ignores the setting.
        assert_eq!(
            Motion::BannerPop.curve(true),
            Motion::BannerPop.curve(false)
        );
    }

    #[test]
    fn tracker_reaches_its_target_and_settles() {
        for reduced in [false, true] {
            let curve = Motion::Layout.curve(reduced);
            let mut tracker = Tracker::new(0.0);
            tracker.retarget(10.0, curve, 0.0);
            assert!(!tracker.is_settled());
            for _ in 0..120 {
                tracker.advance(curve, 1.0 / 60.0);
            }
            assert_eq!(tracker.value(), 10.0);
            assert!(tracker.is_settled());
        }
    }

    #[test]
    fn delay_holds_the_value_back() {
        let curve = Motion::BannerPop.curve(false);
        let mut tracker = Tracker::new(0.6);
        tracker.retarget(1.0, curve, 0.18);
        for _ in 0..10 {
            tracker.advance(curve, 1.0 / 60.0);
        }
        assert_eq!(tracker.value(), 0.6, "still waiting after 0.167 s");
        for _ in 0..6 {
            tracker.advance(curve, 1.0 / 60.0);
        }
        assert!(tracker.value() > 0.6, "moving after the delay");
    }

    #[test]
    fn endless_animations_hit_their_extremes() {
        assert!((pulse_opacity(0.0) - 1.0).abs() < 1e-5);
        assert!((pulse_opacity(0.7) - 0.45).abs() < 1e-3);
        assert!((pulse_opacity(1.4) - 1.0).abs() < 1e-3);
        assert!((indeterminate_phase(0.0) + 0.4).abs() < 1e-5);
        assert!(indeterminate_phase(1.29) > 1.0);
        assert_eq!(drift(24.0, 24.0), 1.0);
    }
}
