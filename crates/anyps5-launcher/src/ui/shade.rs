//! The shades under the home screen's text (docs/DESIGN.md 3.10).
//!
//! White text has to read on whatever the artwork or the theme puts behind it, and a pale picture
//! under a full-width ramp turns grey like fog. So the shade is not a ramp over the whole lower
//! half: it is a soft-edged patch, as dark as the text needs on a plateau under that text and
//! easing to nothing around it. Three of them: the top bar's band, the hero's patch (state, facts)
//! and the hint bar's patch. The strengths are chosen with `ui::contrast`, which measures the
//! worst backdrops (a pure white picture, Sunset's lit lower band) against the ink laid on them.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Painter, Pos2, Rect, Shape, Vec2, pos2, vec2};

use crate::ui::theme::black;

/// Darkness of the band under the top bar at its top edge; it ends 150 dp lower.
pub const SHADE_TOP: f32 = 0.45;
pub const TOP_HEIGHT: f32 = 150.0;
/// Darkness on the plateau under the hero's state line and facts.
pub const SHADE_HERO: f32 = 0.46;
/// Darkness on the plateau under the hint bar.
pub const SHADE_HINTS: f32 = 0.28;

/// A patch of black: `peak` on a plateau, easing to nothing over `feather` (across, up and down)
/// beyond it. The ease follows the distance from the plateau, so its contours are round at the
/// corners and no straight edge shows. A plateau that reaches past the screen's edge has no fade
/// there.
#[derive(Clone, Copy, Debug)]
pub struct Patch {
    pub plateau: Rect,
    pub feather: Vec2,
    pub peak: f32,
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Patch {
    /// Black alpha at `p`.
    pub fn alpha(&self, p: Pos2) -> f32 {
        let dx = (self.plateau.min.x - p.x)
            .max(p.x - self.plateau.max.x)
            .max(0.0);
        let dy = (self.plateau.min.y - p.y)
            .max(p.y - self.plateau.max.y)
            .max(0.0);
        let d = (dx / self.feather.x).hypot(dy / self.feather.y);
        self.peak * (1.0 - smoothstep(d))
    }

    /// The area the patch touches at all.
    pub fn reach(&self) -> Rect {
        self.plateau.expand2(self.feather)
    }
}

/// The hero's patch for a screen: from the left edge to 880 dp, from 400 dp above the bottom down
/// (the state line, the title, the buttons, the glibc line and the facts, which can run to 880).
pub fn hero(screen: Rect) -> Patch {
    let far = 400.0;
    Patch {
        plateau: Rect::from_min_max(
            pos2(screen.min.x - far, screen.max.y - 400.0),
            pos2(screen.min.x + 880.0, screen.max.y + far),
        ),
        feather: vec2(360.0, 260.0),
        peak: SHADE_HERO,
    }
}

/// The hint bar's patch: the right 520 dp, the bottom 110 dp.
pub fn hints(screen: Rect) -> Patch {
    let far = 400.0;
    Patch {
        plateau: Rect::from_min_max(
            pos2(screen.max.x - 520.0, screen.max.y - 110.0),
            pos2(screen.max.x + far, screen.max.y + far),
        ),
        feather: vec2(300.0, 220.0),
        peak: SHADE_HINTS,
    }
}

/// Black alpha of the top band at `y` below the top edge.
pub fn top_alpha(y: f32) -> f32 {
    SHADE_TOP * (1.0 - (y / TOP_HEIGHT).clamp(0.0, 1.0))
}

/// Everything the home screen lays under its text, at `p`: the three shades laid over each other.
pub fn alpha_at(screen: Rect, p: Pos2, with_hero: bool) -> f32 {
    let mut keep = 1.0 - top_alpha(p.y - screen.min.y);
    if with_hero {
        keep *= 1.0 - hero(screen).alpha(p);
        keep *= 1.0 - hints(screen).alpha(p);
    }
    1.0 - keep
}

/// Paints the shades under the home screen: the top band, and (with games) the hero's and the hint
/// bar's patches.
pub fn paint(painter: &Painter, screen: Rect, with_hero: bool) {
    let mut mesh = Mesh::default();
    for (y, a) in [(0.0, SHADE_TOP), (TOP_HEIGHT, 0.0)] {
        for x in [screen.min.x, screen.max.x] {
            mesh.vertices.push(Vertex {
                pos: pos2(x, screen.min.y + y),
                uv: WHITE_UV,
                color: black(a),
            });
        }
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);
    painter.add(Shape::mesh(mesh));
    if with_hero {
        paint_patch(painter, screen, hero(screen));
        paint_patch(painter, screen, hints(screen));
    }
}

/// A grid over the patch's reach, clipped to the screen, each vertex at the patch's alpha.
fn paint_patch(painter: &Painter, screen: Rect, patch: Patch) {
    const N: usize = 20;
    let area = patch.reach().intersect(screen);
    if area.width() <= 0.0 || area.height() <= 0.0 {
        return;
    }
    // Lines are spread over the area but also land on the plateau's edges, where the slope changes.
    let lines = |lo: f32, hi: f32, edges: [f32; 2]| {
        let mut v: Vec<f32> = (0..=N)
            .map(|i| lo + (hi - lo) * i as f32 / N as f32)
            .collect();
        v.extend(edges.into_iter().filter(|e| *e > lo && *e < hi));
        v.sort_by(f32::total_cmp);
        v
    };
    let xs = lines(
        area.min.x,
        area.max.x,
        [patch.plateau.min.x, patch.plateau.max.x],
    );
    let ys = lines(
        area.min.y,
        area.max.y,
        [patch.plateau.min.y, patch.plateau.max.y],
    );
    let mut mesh = Mesh::default();
    for &y in &ys {
        for &x in &xs {
            mesh.vertices.push(Vertex {
                pos: pos2(x, y),
                uv: WHITE_UV,
                color: black(patch.alpha(pos2(x, y))),
            });
        }
    }
    let w = xs.len() as u32;
    for j in 0..ys.len() as u32 - 1 {
        for i in 0..w - 1 {
            let a = j * w + i;
            mesh.indices
                .extend_from_slice(&[a, a + 1, a + w, a + 1, a + w + 1, a + w]);
        }
    }
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 900.0))
    }

    #[test]
    fn the_patches_are_full_on_the_plateau_and_gone_far_away() {
        let s = screen();
        assert!((hero(s).alpha(pos2(300.0, 760.0)) - SHADE_HERO).abs() < 1e-6);
        assert!((hints(s).alpha(pos2(1400.0, 862.0)) - SHADE_HINTS).abs() < 1e-6);
        // Over the tile row, and in the middle of the screen, nothing is laid.
        assert_eq!(hero(s).alpha(pos2(300.0, 100.0)), 0.0);
        assert_eq!(hero(s).alpha(pos2(1300.0, 760.0)), 0.0);
        assert_eq!(hints(s).alpha(pos2(600.0, 862.0)), 0.0);
    }

    #[test]
    fn the_patches_reach_the_screen_edge_without_a_fade_there() {
        let s = screen();
        assert!((hero(s).alpha(pos2(0.0, 899.0)) - SHADE_HERO).abs() < 1e-6);
        assert!((hints(s).alpha(pos2(1599.0, 899.0)) - SHADE_HINTS).abs() < 1e-6);
    }

    #[test]
    fn the_top_band_fades_over_its_height() {
        assert!((top_alpha(0.0) - SHADE_TOP).abs() < 1e-6);
        assert_eq!(top_alpha(TOP_HEIGHT), 0.0);
        assert_eq!(top_alpha(400.0), 0.0);
    }
}
