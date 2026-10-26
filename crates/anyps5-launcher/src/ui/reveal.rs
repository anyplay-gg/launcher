//! The words a cut line lost (docs/DESIGN.md 3.13). A row, a tile or a strip that ends in an
//! ellipsis shows its full text in a glass chip beside it while the pointer is on it or the focus
//! has rested on it, so no value the launcher shortens is out of reach. The launcher has no
//! tooltips on a delay of its own: the chip is the one way, for the pointer and for keys and pad.

use egui::{Area, Context, Id, Order, Pos2, Rect, pos2, vec2};

use crate::ui::anim::{AnimExt, Motion};
use crate::ui::glass::{GlassStyle, paint_glass};
use crate::ui::text;
use crate::ui::theme::{color, fade, radius, space, ty};

/// How long the focus rests before the chip comes up, so a walk down a list shows none.
const REST: f64 = 0.5;
/// The widest the chip grows.
const MAX_WIDTH: f32 = 440.0;
/// The room the chip keeps from the window's edge.
const EDGE: f32 = space::L;

/// Whether a galley lost words to an ellipsis.
pub fn is_cut(galley: &egui::Galley) -> bool {
    galley.elided
}

/// Shows `full` beside `anchor` while `active` (the pointer is over it, or the focus rests on
/// it) and `cut` (the visible text lost words). Call it every frame for the thing in question;
/// it keeps its own rest timer under `id`.
pub fn full_text(ctx: &Context, id: Id, anchor: Rect, full: &str, active: bool, cut: bool) {
    let key = id.with("reveal-since");
    let now = ctx.input(|i| i.time);
    let wanted = active && cut && !full.is_empty();
    let since = if wanted {
        let since = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with::<f64>(key, || now));
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(REST));
        since
    } else {
        ctx.data_mut(|d| d.remove_temp::<f64>(key));
        now
    };
    let up = wanted && now - since >= REST;
    let a = ctx.animate_to((id, "reveal"), f32::from(up), Motion::Hover);
    if a <= 0.003 {
        return;
    }
    let screen = ctx.content_rect();
    Area::new(id.with("reveal-chip"))
        .order(Order::Tooltip)
        .fixed_pos(screen.min)
        .interactable(false)
        .show(ctx, |ui| {
            let painter = ui.painter().clone();
            let wrap = MAX_WIDTH.min(screen.width() - 2.0 * EDGE);
            let galley = text::layout(&painter, ty::NOTE, full, egui::Color32::PLACEHOLDER, wrap);
            let size = galley.size() + vec2(2.0 * space::L, 2.0 * space::M);
            let rect = Rect::from_min_size(place(screen, anchor, size), size);
            let mut style = GlassStyle::glass(radius::ROW, 0.7);
            style.black *= a;
            style.alpha = a;
            paint_glass(ctx, &painter, rect, style);
            painter.galley(
                pos2(rect.min.x + space::L, rect.min.y + space::M),
                galley,
                fade(color::text(), a),
            );
        });
}

/// Where a chip of `size` goes: under the anchor from its text's edge, over it where the screen
/// ends below, and always inside the window.
fn place(screen: Rect, anchor: Rect, size: egui::Vec2) -> Pos2 {
    let x = (anchor.min.x + space::XL).clamp(
        screen.min.x + EDGE,
        (screen.max.x - EDGE - size.x).max(screen.min.x + EDGE),
    );
    let below = anchor.max.y + space::XS;
    let y = if below + size.y + EDGE <= screen.max.y {
        below
    } else {
        (anchor.min.y - space::XS - size.y).max(screen.min.y + EDGE)
    };
    pos2(x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chip_stays_inside_the_window() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let size = vec2(300.0, 60.0);
        let low = Rect::from_min_size(pos2(1200.0, 780.0), vec2(60.0, 20.0));
        let at = place(screen, low, size);
        assert!(at.x + size.x <= screen.max.x);
        assert!(
            at.y + size.y <= low.min.y,
            "above the anchor where there is no room below"
        );
        let high = Rect::from_min_size(pos2(40.0, 100.0), vec2(300.0, 40.0));
        assert!(place(screen, high, size).y >= high.max.y);
    }
}
