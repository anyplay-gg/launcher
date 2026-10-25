//! The trophy banner (docs/DESIGN.md 5.14 and 9.4): a card that drops in at the top of the screen
//! when a game earns a trophy, holds for five seconds and rises away again. Banners are queued,
//! one at a time.
//!
//! * [`BannerQueue`] is the timeline (slide spring, icon pop, hold, ease-in out, 0.35 s between
//!   banners). It is plain state advanced with the clock, so it is tested without a window.
//! * [`paint_card`] draws one card at any moment of the timeline.
//! * [`BannerHost`] shows the queue in its own always-on-top, click-through viewport so the card
//!   also appears over a windowed or borderless game; where such a window cannot be shown the
//!   host still runs the queue and the trophy list updates.

use std::collections::VecDeque;

use egui::epaint::{PathShape, PathStroke, Shadow};
use egui::{
    Align2, Color32, Context, Pos2, Rect, Shape, Ui, ViewportBuilder, ViewportClass, ViewportId,
    pos2, vec2,
};

use crate::screens::trophies::IconCache;
use crate::ui::Shell;
use crate::ui::anim::{Bezier, Curve, Motion, Tracker, lerp};
use crate::ui::components::corner;
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{self, Grade, color, fade, mix, radius, ty, white};

/// The banner window, in points (not scaled with the launcher window).
pub const WINDOW: (f32, f32) = (560.0, 150.0);
/// The card inside it.
pub const CARD_WIDTH: f32 = 480.0;
/// Resting offset of the card from the top of the window.
pub const REST_OFFSET: f32 = 24.0;
/// How long the card holds once it has arrived, from its spawn.
pub const HOLD: f32 = 5.0;
/// Rising away.
pub const OUT_DURATION: f32 = 0.3;
/// From the start of the exit to the next banner.
pub const NEXT_AFTER: f32 = 0.35;
/// The icon pops in this long after the card starts to slide.
pub const POP_DELAY: f32 = 0.18;

/// A trophy to announce.
#[derive(Clone, Debug)]
pub struct BannerTrophy {
    pub id: String,
    /// Empty shows "Hidden Trophy".
    pub name: String,
    pub detail: String,
    pub grade: Grade,
    /// The trophy's PNG, if the package has one.
    pub icon: Option<Vec<u8>>,
}

impl BannerTrophy {
    pub fn from_core(trophy: &anyps5_core::trophies::Trophy) -> Self {
        Self {
            id: trophy.id(),
            name: trophy.name.clone(),
            detail: trophy.detail.clone(),
            grade: crate::screens::convert::grade(trophy.grade),
            icon: trophy.icon.clone(),
        }
    }
}

/// One moment of the timeline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BannerLook {
    /// Slide amount: 0 = away above the window, 1 = at rest (may overshoot).
    pub slide: f32,
    /// Icon pop: 0.6 scale to 1.
    pub pop: f32,
}

impl BannerLook {
    /// Card offset from the top of the window.
    pub fn offset_y(&self) -> f32 {
        lerp(-WINDOW.1, REST_OFFSET, self.slide)
    }

    pub fn opacity(&self) -> f32 {
        self.slide.clamp(0.0, 1.0)
    }

    pub fn icon_scale(&self) -> f32 {
        lerp(0.6, 1.0, self.pop)
    }

    pub fn glow_opacity(&self) -> f32 {
        self.pop.clamp(0.0, 1.0)
    }

    /// The card at rest (previews).
    pub const SETTLED: BannerLook = BannerLook {
        slide: 1.0,
        pop: 1.0,
    };
}

struct Showing {
    trophy: BannerTrophy,
    age: f32,
    slide: Tracker,
    pop: Tracker,
    leaving: bool,
}

/// What the host draws this frame.
pub struct BannerFrame<'a> {
    pub trophy: &'a BannerTrophy,
    pub look: BannerLook,
    /// The first frame of this banner: play the cue.
    pub spawned: bool,
}

/// The queue and the timeline of the banner.
#[derive(Default)]
pub struct BannerQueue {
    queue: VecDeque<BannerTrophy>,
    current: Option<Showing>,
    last: Option<f64>,
}

impl BannerQueue {
    /// Queues a banner; it shows when the ones before it are done.
    pub fn push(&mut self, trophy: BannerTrophy) {
        self.queue.push_back(trophy);
    }

    pub fn is_idle(&self) -> bool {
        self.current.is_none() && self.queue.is_empty()
    }

    pub fn pending(&self) -> usize {
        self.queue.len() + usize::from(self.current.is_some())
    }

    fn spawn(&mut self, reduced: bool) -> bool {
        let Some(trophy) = self.queue.pop_front() else {
            return false;
        };
        let slide_curve = Motion::BannerSlide.curve(reduced);
        let mut slide = Tracker::new(0.0);
        slide.retarget(1.0, slide_curve, 0.0);
        let mut pop = Tracker::new(0.0);
        pop.retarget(1.0, Motion::BannerPop.curve(false), POP_DELAY);
        self.current = Some(Showing {
            trophy,
            age: 0.0,
            slide,
            pop,
            leaving: false,
        });
        true
    }

    /// Advances to `now` (seconds, any clock) and returns the card to draw, if any.
    pub fn update(&mut self, now: f64, reduced: bool) -> Option<BannerFrame<'_>> {
        let dt = self
            .last
            .map_or(0.0, |last| (now - last).clamp(0.0, 0.1) as f32);
        self.last = Some(now);
        let mut spawned = false;
        if self.current.is_none() {
            spawned = self.spawn(reduced);
        }
        let mut finished = false;
        if let Some(showing) = self.current.as_mut() {
            if !spawned {
                showing.age += dt;
            }
            let slide_curve = Motion::BannerSlide.curve(reduced);
            let pop_curve = Motion::BannerPop.curve(false);
            if !showing.leaving && showing.age >= HOLD {
                showing.leaving = true;
                showing.slide.retarget(
                    0.0,
                    Curve::Ease {
                        bezier: Bezier::EASE_IN,
                        duration: OUT_DURATION,
                    },
                    0.0,
                );
            }
            let out_curve = Curve::Ease {
                bezier: Bezier::EASE_IN,
                duration: OUT_DURATION,
            };
            showing.slide.advance(
                if showing.leaving {
                    out_curve
                } else {
                    slide_curve
                },
                dt,
            );
            showing.pop.advance(pop_curve, dt);
            finished = showing.leaving && showing.age >= HOLD + NEXT_AFTER;
        }
        if finished {
            self.current = None;
            // The next banner starts at once.
            if self.spawn(reduced) {
                let showing = self.current.as_ref()?;
                return Some(BannerFrame {
                    trophy: &showing.trophy,
                    look: BannerLook {
                        slide: showing.slide.value(),
                        pop: showing.pop.value(),
                    },
                    spawned: true,
                });
            }
            return None;
        }
        self.current.as_ref().map(|showing| BannerFrame {
            trophy: &showing.trophy,
            look: BannerLook {
                slide: showing.slide.value(),
                pop: showing.pop.value(),
            },
            spawned,
        })
    }
}

// ---------------------------------------------------------------------------------------------
// The card
// ---------------------------------------------------------------------------------------------

/// Points of a rounded rectangle outline, clockwise from the top-left arc, for a gradient stroke.
fn rounded_outline(rect: Rect, r: f32) -> Vec<Pos2> {
    let steps = 10;
    let mut points = Vec::with_capacity(steps * 4 + 4);
    let corners = [
        (pos2(rect.max.x - r, rect.min.y + r), -90.0_f32),
        (pos2(rect.max.x - r, rect.max.y - r), 0.0),
        (pos2(rect.min.x + r, rect.max.y - r), 90.0),
        (pos2(rect.min.x + r, rect.min.y + r), 180.0),
    ];
    for (centre, start) in corners {
        for i in 0..=steps {
            let a = (start + 90.0 * i as f32 / steps as f32).to_radians();
            points.push(centre + vec2(a.cos(), a.sin()) * r);
        }
    }
    points
}

/// Draws the card for `trophy` at `look` inside a banner window `window` (its top-left is the
/// origin of the offsets). `k` scales every length: 1 when the window is shown at 560 x 150 points.
/// `texture` is the trophy icon, if decoded.
pub fn paint_card(
    ui: &mut Ui,
    window: Rect,
    k: f32,
    trophy: &BannerTrophy,
    look: BannerLook,
    texture: Option<egui::TextureId>,
    language: Language,
) {
    let alpha = look.opacity();
    if alpha <= 0.001 {
        return;
    }
    let painter = ui.painter().clone();
    let grade = trophy.grade.color();
    let height = 14.0 * k * 2.0 + 64.0 * k;
    let card = Rect::from_min_size(
        pos2(
            window.center().x - CARD_WIDTH * k * 0.5,
            window.min.y + look.offset_y() * k,
        ),
        vec2(CARD_WIDTH * k, height),
    );
    let r = radius::BANNER * k;

    // Shadow: black 0.5, SwiftUI radius 18, y 8.
    let shadow = theme::shadows::banner();
    painter.add(
        Shadow {
            color: fade(shadow.color, alpha),
            offset: [0, (8.0 * k) as i8],
            blur: (36.0 * k) as u8,
            ..shadow
        }
        .as_shape(card, corner(r)),
    );
    painter.rect_filled(card, corner(r), fade(color::card_fill(), alpha));
    let outline = rounded_outline(card, r);
    let (top, bottom) = (card.min.y, card.max.y);
    painter.add(Shape::Path(PathShape::closed_line(
        outline,
        PathStroke::new_uv(1.0, move |_, p| {
            let t = ((p.y - top) / (bottom - top)).clamp(0.0, 1.0);
            fade(mix(fade(grade, 0.55), white(0.08), t), alpha)
        }),
    )));

    // Icon with its glow, pinned to the leading edge.
    let glow_centre = pos2(card.min.x + (14.0 + 32.0) * k, card.center().y);
    painter.add(
        Shadow {
            color: fade(grade, 0.35 * look.glow_opacity() * alpha),
            offset: [0, 0],
            blur: (28.0 * k) as u8,
            spread: 0,
        }
        .as_shape(
            Rect::from_center_size(glow_centre, vec2(64.0, 64.0) * k * 0.75),
            corner(16.0 * k),
        ),
    );
    let icon_side = 60.0 * k * look.icon_scale();
    let icon_rect = Rect::from_center_size(glow_centre, vec2(icon_side, icon_side));
    {
        let mut icon_ui = ui.new_child(egui::UiBuilder::new().max_rect(icon_rect));
        icon_ui.set_opacity(alpha);
        crate::screens::trophies::paint_trophy_icon(&mut icon_ui, icon_rect, texture, true);
    }

    // Label, name, detail.
    let x = card.min.x + (14.0 + 64.0 + 16.0) * k;
    let max_w = card.max.x - x - (14.0 + 26.0 + 6.0 + 12.0) * k;
    let label_style = text::scaled(ty::BANNER_LABEL, k);
    let name_style = text::scaled(ty::BANNER_NAME, k);
    let detail_style = text::scaled(ty::BANNER_DETAIL, k);
    let block = label_style.line_height()
        + 4.0 * k
        + name_style.line_height()
        + 4.0 * k
        + detail_style.line_height();
    let mut y = card.center().y - block * 0.5;
    // No cup before the label: the grade's trophy already stands on the right.
    text::paint(
        &painter,
        pos2(x, y),
        Align2::LEFT_TOP,
        label_style,
        language.text(Key::TrophyEarned),
        fade(grade, alpha),
    );
    y += label_style.line_height() + 4.0 * k;
    let name = if trophy.name.is_empty() {
        language.text(Key::TrophyHidden).to_owned()
    } else {
        trophy.name.clone()
    };
    let shown = text::middle_truncate(&painter, name_style, &name, max_w);
    text::paint(
        &painter,
        pos2(x, y),
        Align2::LEFT_TOP,
        name_style,
        &shown,
        fade(Color32::WHITE, alpha),
    );
    y += name_style.line_height() + 4.0 * k;
    let detail = text::middle_truncate(&painter, detail_style, &trophy.detail, max_w);
    text::paint(
        &painter,
        pos2(x, y),
        Align2::LEFT_TOP,
        detail_style,
        &detail,
        fade(color::text_detail(), alpha),
    );

    // The big trophy on the trailing edge, with a vertical gradient from the grade colour down.
    let big = Icon::TROPHY.font(26.0 * k, true);
    let centre = pos2(card.max.x - (14.0 + 6.0 + 13.0) * k, card.center().y);
    let bands = 6;
    let span = 26.0 * k;
    for band in 0..bands {
        let t0 = band as f32 / bands as f32;
        let clip = Rect::from_min_max(
            pos2(centre.x - span, centre.y - span * 0.5 + span * t0),
            pos2(
                centre.x + span,
                centre.y - span * 0.5 + span * (t0 + 1.0 / bands as f32),
            ),
        );
        let c = fade(
            grade,
            lerp(1.0, 0.6, (t0 + 0.5 / bands as f32).min(1.0)) * alpha,
        );
        painter.with_clip_rect(clip).text(
            centre,
            Align2::CENTER_CENTER,
            Icon::TROPHY.glyph(),
            big.clone(),
            c,
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------------------------

/// Id of the banner viewport.
fn viewport_id() -> ViewportId {
    ViewportId::from_hash_of("anyps5-trophy-banner")
}

/// The window the banner lives in: borderless, transparent, on top of other windows, never taking
/// focus or mouse, not in the taskbar. `monitor_width` is the primary display's width in points;
/// the window sits centred at its top.
pub fn viewport_builder(monitor_width: f32) -> ViewportBuilder {
    ViewportBuilder::default()
        .with_title("AnyPlay Launcher trophy")
        .with_inner_size([WINDOW.0, WINDOW.1])
        .with_position([(monitor_width - WINDOW.0) * 0.5, 0.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_resizable(false)
        .with_taskbar(false)
        .with_mouse_passthrough(true)
        .with_active(false)
}

/// Owns the queue and shows it in its own viewport.
#[derive(Default)]
pub struct BannerHost {
    pub queue: BannerQueue,
    icons: IconCache,
}

impl BannerHost {
    /// Queues a trophy.
    pub fn push(&mut self, trophy: BannerTrophy) {
        self.queue.push(trophy);
    }

    /// Advances the timeline and, while a banner shows, draws it in the banner viewport. Plays the
    /// `unlocked` cue when a banner starts (the shell mutes it when sounds are off). Call every
    /// frame; it requests repaints while something moves.
    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        reduced: bool,
        monitor_width: f32,
        language: Language,
    ) {
        let now = ctx.input(|i| i.time);
        let Some(frame) = self.queue.update(now, reduced) else {
            return;
        };
        ctx.request_repaint();
        if frame.spawned {
            shell.play(Cue::Unlocked);
        }
        let trophy = frame.trophy.clone();
        let look = frame.look;
        let texture = self.icons.get(
            ctx,
            &format!("banner/{}", trophy.id),
            trophy.icon.as_deref(),
            true,
        );
        let k = 1.0 / shell.scale().max(0.1);
        ctx.show_viewport_immediate(
            viewport_id(),
            viewport_builder(monitor_width),
            |ctx, class| {
                if class != ViewportClass::Immediate && class != ViewportClass::Root {
                    return;
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        let window = ui.max_rect();
                        paint_card(ui, window, k, &trophy, look, texture, language);
                    });
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trophy(id: &str) -> BannerTrophy {
        BannerTrophy {
            id: id.into(),
            name: format!("Trophy {id}"),
            detail: "Do the thing".into(),
            grade: Grade::Bronze,
            icon: None,
        }
    }

    /// Runs the queue at 60 fps and records `(time, id, look)` per frame.
    fn run(
        queue: &mut BannerQueue,
        seconds: f64,
        reduced: bool,
    ) -> Vec<(f64, String, BannerLook, bool)> {
        let mut frames = Vec::new();
        let mut t = 0.0;
        while t <= seconds {
            if let Some(frame) = queue.update(t, reduced) {
                frames.push((t, frame.trophy.id.clone(), frame.look, frame.spawned));
            }
            t += 1.0 / 60.0;
        }
        frames
    }

    #[test]
    fn a_banner_slides_in_holds_and_leaves() {
        let mut queue = BannerQueue::default();
        queue.push(trophy("a"));
        let frames = run(&mut queue, 6.0, false);
        let first = &frames[0];
        assert!(first.3, "the first frame is the spawn");
        assert!(first.2.slide < 0.05, "starts above the window");
        let rest = frames.iter().find(|f| f.0 > 1.5).unwrap();
        assert!(
            (rest.2.slide - 1.0).abs() < 0.02,
            "at rest after the spring"
        );
        assert!((rest.2.offset_y() - REST_OFFSET).abs() < 1.0);
        let at_4 = frames.iter().find(|f| f.0 > 4.0).unwrap();
        assert!((at_4.2.slide - 1.0).abs() < 0.02, "still holding at 4 s");
        assert!(queue.is_idle(), "gone after hold + exit");
        assert!(frames.last().unwrap().0 < HOLD as f64 + 0.5);
    }

    #[test]
    fn the_spring_overshoots_a_little_and_the_icon_pops_later() {
        let mut queue = BannerQueue::default();
        queue.push(trophy("a"));
        let frames = run(&mut queue, 1.5, false);
        let peak = frames.iter().map(|f| f.2.slide).fold(0.0, f32::max);
        assert!(peak > 1.0 && peak < 1.06, "2 % overshoot, got {peak}");
        let early = frames.iter().find(|f| f.0 >= 0.1).unwrap();
        assert!(early.2.pop < 0.01, "the pop waits 0.18 s");
        let pop_peak = frames.iter().map(|f| f.2.pop).fold(0.0, f32::max);
        assert!(pop_peak > 1.05, "the pop bounces, got {pop_peak}");
    }

    #[test]
    fn reduced_motion_slides_with_a_short_ease_without_overshoot() {
        let mut queue = BannerQueue::default();
        queue.push(trophy("a"));
        let frames = run(&mut queue, 1.0, true);
        let peak = frames.iter().map(|f| f.2.slide).fold(0.0, f32::max);
        assert!(peak <= 1.0001, "an ease never overshoots, got {peak}");
        let done = frames.iter().find(|f| f.0 > 0.3).unwrap();
        assert!((done.2.slide - 1.0).abs() < 0.01, "easeOut 0.25 s");
    }

    #[test]
    fn banners_are_strictly_queued() {
        let mut queue = BannerQueue::default();
        queue.push(trophy("a"));
        queue.push(trophy("b"));
        queue.push(trophy("c"));
        assert_eq!(queue.pending(), 3);
        let frames = run(&mut queue, 20.0, false);
        let mut order: Vec<&str> = Vec::new();
        for f in &frames {
            if order.last() != Some(&f.1.as_str()) {
                order.push(&f.1);
            }
        }
        assert_eq!(order, ["a", "b", "c"]);
        // No two banners at once: ids never interleave.
        let spawns: Vec<f64> = frames.iter().filter(|f| f.3).map(|f| f.0).collect();
        assert_eq!(spawns.len(), 3);
        assert!(
            spawns[1] - spawns[0] >= (HOLD + NEXT_AFTER) as f64 - 0.05,
            "{spawns:?}"
        );
        assert!(queue.is_idle());
    }

    #[test]
    fn a_banner_pushed_later_starts_when_idle() {
        let mut queue = BannerQueue::default();
        assert!(queue.update(0.0, false).is_none());
        queue.push(trophy("late"));
        let frame = queue.update(1.0, false).unwrap();
        assert!(frame.spawned);
        assert_eq!(frame.trophy.id, "late");
    }

    #[test]
    fn the_look_maps_to_the_numbers_of_the_design() {
        let away = BannerLook {
            slide: 0.0,
            pop: 0.0,
        };
        assert_eq!(away.offset_y(), -150.0);
        assert_eq!(away.opacity(), 0.0);
        assert_eq!(away.icon_scale(), 0.6);
        let settled = BannerLook::SETTLED;
        assert_eq!(settled.offset_y(), REST_OFFSET);
        assert_eq!(settled.opacity(), 1.0);
        assert_eq!(settled.icon_scale(), 1.0);
        assert_eq!(settled.glow_opacity(), 1.0);
    }
}
