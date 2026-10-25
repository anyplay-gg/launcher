//! Game artwork over the backdrop (docs/DESIGN.md 5.2): aspect-fill, the slow drift (scale 1.01 to
//! 1.07 about the top-right corner) and the two scrims that darken the artwork toward the left and
//! the bottom where the text sits.
//!
//! The screen uploads `pic0.png` as an egui texture (decoded off the UI thread to at most
//! `max(window w, h) * 1.6` px) and calls [`paint_artwork`] once for the current artwork and once
//! for the previous one while cross-fading (`Motion::Fade`, easeInOut 0.55 s). Panels blur the theme
//! backdrop only, not the artwork (a difference from the original, noted in docs/DESIGN.md).

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, Painter, Rect, Shape, TextureId, pos2};

use crate::ui::anim::{self, lerp};
use crate::ui::theme::black;

/// Scale at the start and end of the drift.
pub const DRIFT_SCALE: (f32, f32) = (1.01, 1.07);
/// One way of the ping-pong drift, in seconds.
pub const DRIFT_PERIOD: f64 = 24.0;

/// Zoom of the drift at clock time `time` (static at the start value when `reduced`).
pub fn drift_zoom(time: f64, reduced: bool) -> f32 {
    if reduced {
        DRIFT_SCALE.0
    } else {
        lerp(
            DRIFT_SCALE.0,
            DRIFT_SCALE.1,
            anim::drift(time, DRIFT_PERIOD),
        )
    }
}

/// The part of the image to show: aspect-fill of a `rect_aspect` rectangle, zoomed by `zoom` about
/// the top-right corner.
pub fn artwork_uv(image_aspect: f32, rect_aspect: f32, zoom: f32) -> Rect {
    let (w, h) = if image_aspect > rect_aspect {
        (rect_aspect / image_aspect, 1.0)
    } else {
        (1.0, image_aspect / rect_aspect)
    };
    // Aspect-fill is centred.
    let min_x = (1.0 - w) * 0.5;
    let min_y = (1.0 - h) * 0.5;
    let (max_x, top) = (min_x + w, min_y);
    let zoom = zoom.max(1.0);
    let (zw, zh) = (w / zoom, h / zoom);
    Rect::from_min_max(pos2(max_x - zw, top), pos2(max_x, top + zh))
}

/// Paints the artwork into `rect` at opacity `alpha`, with the scrims.
/// `time` is the shared animation clock (`ctx.clock()`), `reduced` stops the drift.
pub fn paint_artwork(
    painter: &Painter,
    rect: Rect,
    texture: TextureId,
    image_aspect: f32,
    time: f64,
    reduced: bool,
    alpha: f32,
) {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    let uv = artwork_uv(
        image_aspect,
        rect.width() / rect.height().max(1.0),
        drift_zoom(time, reduced),
    );
    let mut mesh = Mesh::with_texture(texture);
    mesh.add_rect_with_uv(rect, uv, Color32::WHITE.gamma_multiply(alpha));
    painter.add(Shape::mesh(mesh));
    paint_scrims(painter, rect, alpha);
}

/// Horizontal scrim (black 0.78 at 0, 0.35 at 0.5, clear at 0.85) and vertical scrim (black 0.55 at
/// 0, clear at 0.3 and 0.55, black 0.85 at 1), both scaled by `alpha`.
pub fn paint_scrims(painter: &Painter, rect: Rect, alpha: f32) {
    painter.add(Shape::mesh(gradient(rect, &SCRIM_HORIZONTAL, true, alpha)));
    painter.add(Shape::mesh(gradient(rect, &SCRIM_VERTICAL, false, alpha)));
}

const SCRIM_HORIZONTAL: [(f32, f32); 4] = [(0.0, 0.78), (0.5, 0.35), (0.85, 0.0), (1.0, 0.0)];
const SCRIM_VERTICAL: [(f32, f32); 4] = [(0.0, 0.55), (0.3, 0.0), (0.55, 0.0), (1.0, 0.85)];

/// The two scrims at a point of the artwork (`u` and `v` in 0..=1 across it), as one black alpha.
pub fn scrim_alpha(u: f32, v: f32) -> f32 {
    let at = |stops: &[(f32, f32)], t: f32| {
        stops
            .windows(2)
            .find(|w| t <= w[1].0)
            .map_or(stops[stops.len() - 1].1, |w| {
                let k = ((t - w[0].0) / (w[1].0 - w[0].0)).max(0.0);
                lerp(w[0].1, w[1].1, k)
            })
    };
    1.0 - (1.0 - at(&SCRIM_HORIZONTAL, u)) * (1.0 - at(&SCRIM_VERTICAL, v))
}

/// A black gradient with `(position 0..1, alpha)` stops along one axis.
fn gradient(rect: Rect, stops: &[(f32, f32)], along_x: bool, alpha: f32) -> Mesh {
    let mut mesh = Mesh::default();
    for &(t, a) in stops {
        let colour = black(a * alpha);
        let (p0, p1) = if along_x {
            let x = lerp(rect.min.x, rect.max.x, t);
            (pos2(x, rect.min.y), pos2(x, rect.max.y))
        } else {
            let y = lerp(rect.min.y, rect.max.y, t);
            (pos2(rect.min.x, y), pos2(rect.max.x, y))
        };
        for pos in [p0, p1] {
            mesh.vertices.push(Vertex {
                pos,
                uv: WHITE_UV,
                color: colour,
            });
        }
    }
    for i in 0..(stops.len() as u32 - 1) {
        let a = i * 2;
        mesh.indices
            .extend_from_slice(&[a, a + 1, a + 2, a + 1, a + 3, a + 2]);
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_image_is_cropped_horizontally_about_the_centre() {
        // 21:9 artwork in a 16:9 window: 16/9 / (21/9) = 0.762 of the width is visible.
        let uv = artwork_uv(21.0 / 9.0, 16.0 / 9.0, 1.0);
        assert!((uv.width() - 0.762).abs() < 1e-3);
        assert!((uv.height() - 1.0).abs() < 1e-6);
        assert!((uv.center().x - 0.5).abs() < 1e-6);
    }

    #[test]
    fn tall_image_is_cropped_vertically() {
        let uv = artwork_uv(1.0, 16.0 / 9.0, 1.0);
        assert!((uv.width() - 1.0).abs() < 1e-6);
        assert!((uv.height() - 9.0 / 16.0).abs() < 1e-5);
    }

    #[test]
    fn zoom_keeps_the_top_right_corner_fixed() {
        let base = artwork_uv(16.0 / 9.0, 16.0 / 9.0, 1.0);
        let zoomed = artwork_uv(16.0 / 9.0, 16.0 / 9.0, 1.07);
        assert!((zoomed.max.x - base.max.x).abs() < 1e-6);
        assert!((zoomed.min.y - base.min.y).abs() < 1e-6);
        assert!((zoomed.width() - base.width() / 1.07).abs() < 1e-6);
    }

    #[test]
    fn drift_ping_pongs_and_stops_when_reduced() {
        assert!((drift_zoom(0.0, false) - 1.01).abs() < 1e-5);
        assert!((drift_zoom(24.0, false) - 1.07).abs() < 1e-5);
        assert!((drift_zoom(48.0, false) - 1.01).abs() < 1e-4);
        assert_eq!(drift_zoom(24.0, true), 1.01);
    }
}
