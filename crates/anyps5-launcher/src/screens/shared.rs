//! Helpers shared by the home screen, the onboarding and the host: the screen context, state
//! labels, wrapped and centred text, gradients and the glass cards over the artwork.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::text::LayoutJob;
use egui::{
    Align, Align2, Color32, Context, Layout, Painter, Pos2, Rect, Shape, Ui, UiBuilder, pos2, vec2,
};

use crate::app::model::{GameState, Model};
use crate::app::textures::Textures;
use crate::ui::Shell;
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::theme::{TextStyle, color};

/// What a screen needs to draw and to act.
pub struct ScreenCx<'a> {
    pub model: &'a mut Model,
    pub shell: &'a Shell,
    pub textures: &'a mut Textures,
    pub ctx: &'a Context,
}

impl ScreenCx<'_> {
    pub fn language(&self) -> Language {
        self.model.language()
    }

    pub fn accent(&self) -> Color32 {
        self.shell.accent_color()
    }
}

/// Runs `add` in a child ui laid out left to right and centred vertically inside `rect`.
pub fn place<R>(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            add(ui)
        },
    )
    .inner
}

/// Glyph, tint and text of a game's state (DESIGN.md 5.1), for the label beside the tile, the
/// status card and the library grid.
pub struct StateLabel {
    pub icon: Icon,
    pub fill: bool,
    pub tint: Color32,
    pub text: String,
}

pub fn state_label(state: &GameState, language: Language, accent: Color32) -> StateLabel {
    let (icon, fill, tint, text) = match state {
        GameState::Locked => (
            Icon::LOCK,
            true,
            color::state_neutral(),
            language.text(Key::Locked).to_owned(),
        ),
        GameState::Unreadable => (
            Icon::X_CIRCLE,
            true,
            color::error_state(),
            language.text(Key::UnreadableDump).to_owned(),
        ),
        GameState::Queued => (
            Icon::CLOCK,
            false,
            color::state_neutral(),
            language.text(Key::Queued).to_owned(),
        ),
        GameState::Installing { step, .. } => (
            Icon::ARROW_CIRCLE_DOWN,
            true,
            accent,
            language.text(step.title_key()).to_owned(),
        ),
        GameState::Preparing { paused, .. } => (
            Icon::ARROW_CIRCLE_DOWN,
            false,
            if *paused {
                color::state_neutral()
            } else {
                accent
            },
            crate::screens::prepare_strings::state_label(*paused, language).to_owned(),
        ),
        GameState::Failed(_) => (
            Icon::WARNING,
            true,
            color::error_state(),
            language.text(Key::Failed).to_owned(),
        ),
        GameState::Installed { outdated: true } => (
            Icon::ARROWS_CLOCKWISE,
            false,
            color::update_available(),
            language.text(Key::UpdateAvailable).to_owned(),
        ),
        GameState::Installed { outdated: false } => (
            Icon::CHECK_CIRCLE,
            true,
            color::state_neutral(),
            language.text(Key::Ready).to_owned(),
        ),
        GameState::Starting => (
            Icon::HOURGLASS,
            false,
            accent,
            language.text(Key::Starting).to_owned(),
        ),
        GameState::Running => (
            Icon::PLAY_CIRCLE,
            true,
            accent,
            language.text(Key::Running).to_owned(),
        ),
    };
    StateLabel {
        icon,
        fill,
        tint,
        text,
    }
}

/// Gap between the glyph and the text of a label.
const LABEL_GAP: f32 = 6.0;

/// Paints `icon text` with the left edge at `pos.x` and the vertical centre at `pos.y`; wraps the
/// text to `max_width` in at most `max_rows` rows, everything faded by `alpha`. Returns the
/// rectangle used.
pub fn paint_label(
    painter: &Painter,
    pos: Pos2,
    label: &StateLabel,
    style: TextStyle,
    max_width: f32,
    max_rows: usize,
    alpha: f32,
) -> Rect {
    let tint = crate::ui::theme::fade(label.tint, alpha);
    let glyph = style.size;
    let galley = wrapped(
        painter,
        style,
        &label.text,
        tint,
        max_width - glyph - LABEL_GAP,
        max_rows,
    );
    let height = galley.size().y.max(style.line_height());
    let top = pos.y - height * 0.5;
    painter.text(
        pos2(pos.x + glyph * 0.5, top + style.line_height() * 0.5),
        Align2::CENTER_CENTER,
        label.icon.glyph(),
        label.icon.font(glyph, label.fill),
        tint,
    );
    let text_pos = pos2(pos.x + glyph + LABEL_GAP, top);
    let size = galley.size();
    painter.galley(text_pos, galley, tint);
    Rect::from_min_size(pos2(pos.x, top), vec2(glyph + LABEL_GAP + size.x, height))
}

/// Lays text out in `width` with at most `max_rows` rows (the rest is cut with an ellipsis).
pub fn wrapped(
    painter: &Painter,
    style: TextStyle,
    text: &str,
    color: Color32,
    width: f32,
    max_rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), style.font(), color, width.max(1.0));
    job.wrap.max_rows = max_rows;
    for section in &mut job.sections {
        section.format.line_height = Some(style.line_height());
        section.format.extra_letter_spacing = style.tracking;
    }
    painter.layout_job(job)
}

/// A circle filled with a diagonal gradient from the top left to the bottom right. A raw mesh
/// gets no anti-aliasing from egui, so the edge is feathered by hand over one physical pixel,
/// like egui's own shapes.
pub fn paint_gradient_circle(
    painter: &Painter,
    centre: Pos2,
    radius: f32,
    from: Color32,
    to: Color32,
) {
    const SEGMENTS: u32 = 96;
    let feather = 1.0 / painter.ctx().pixels_per_point();
    let tint = |p: Pos2| {
        let d = (p - centre) / radius;
        crate::ui::theme::mix(from, to, ((d.x + d.y) * 0.5 * 0.5 + 0.5).clamp(0.0, 1.0))
    };
    let mut mesh = Mesh::default();
    mesh.vertices.push(Vertex {
        pos: centre,
        uv: WHITE_UV,
        color: tint(centre),
    });
    // Per segment an inner vertex in full colour and an outer one, a pixel further out,
    // fully transparent.
    for i in 0..SEGMENTS {
        let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        let direction = vec2(a.cos(), a.sin());
        let inner = centre + (radius - feather * 0.5) * direction;
        mesh.vertices.push(Vertex {
            pos: inner,
            uv: WHITE_UV,
            color: tint(inner),
        });
        mesh.vertices.push(Vertex {
            pos: centre + (radius + feather * 0.5) * direction,
            uv: WHITE_UV,
            color: Color32::TRANSPARENT,
        });
    }
    for i in 0..SEGMENTS {
        let j = (i + 1) % SEGMENTS;
        let (inner_i, outer_i, inner_j, outer_j) = (1 + 2 * i, 2 + 2 * i, 1 + 2 * j, 2 + 2 * j);
        mesh.indices.extend_from_slice(&[0, inner_i, inner_j]);
        mesh.indices
            .extend_from_slice(&[inner_i, outer_i, outer_j, inner_i, outer_j, inner_j]);
    }
    painter.add(Shape::mesh(mesh));
}

/// A small spinning arc (the original's `ProgressView().controlSize(.small)`).
pub fn paint_spinner(painter: &Painter, centre: Pos2, radius: f32, time: f64, color: Color32) {
    crate::ui::components::paint_spinner_arc(painter, centre, radius, time, 2.0, color);
}

// ---------------------------------------------------------------------------------------------
// Glass over the game artwork
// ---------------------------------------------------------------------------------------------

/// The artwork behind the home screen as the glass cards see it; the host writes it every frame
/// (`ctx.data`), the cards read it.
#[derive(Clone, Copy, Debug)]
pub struct ArtState {
    /// The frosted version of the artwork (`textures::MATERIAL`) and its aspect.
    pub material: egui::TextureId,
    pub aspect: f32,
    /// Opacity of the current artwork (cross-fade).
    pub alpha: f32,
    /// Drift zoom of this frame.
    pub zoom: f32,
}

/// Id the host stores the [`ArtState`] under.
pub fn art_state_id() -> egui::Id {
    egui::Id::unique("home-artwork-state")
}

/// Value of a piecewise linear scrim at `t` (stops are `(position, alpha)`).
fn scrim(stops: &[(f32, f32)], t: f32) -> f32 {
    for pair in stops.windows(2) {
        let ((t0, a0), (t1, a1)) = (pair[0], pair[1]);
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0).max(1e-6)).clamp(0.0, 1.0);
            return a0 + (a1 - a0) * f;
        }
    }
    stops.last().map_or(0.0, |s| s.1)
}

/// How much of the artwork the two scrims of DESIGN.md 5.2 let through at a point of the window
/// (`x` and `y` as fractions of the window).
pub fn artwork_transmission(x: f32, y: f32) -> f32 {
    let horizontal = [(0.0, 0.78), (0.5, 0.35), (0.85, 0.0), (1.0, 0.0)];
    let vertical = [(0.0, 0.55), (0.3, 0.0), (0.55, 0.0), (1.0, 0.85)];
    (1.0 - scrim(&horizontal, x)) * (1.0 - scrim(&vertical, y))
}

/// A glass card: the theme glass, and over it the frosted artwork the card sits on (the original's
/// material blurs everything behind it, artwork included).
pub fn paint_glass_card(
    ctx: &Context,
    painter: &Painter,
    rect: Rect,
    style: crate::ui::glass::GlassStyle,
) {
    crate::ui::glass::paint_glass(ctx, painter, rect, style);
    let Some(art) = ctx.data(|d| d.get_temp::<ArtState>(art_state_id())) else {
        return;
    };
    if art.alpha <= 0.003 {
        return;
    }
    let screen = ctx.content_rect();
    let window_uv = crate::ui::artwork::artwork_uv(
        art.aspect,
        screen.width() / screen.height().max(1.0),
        art.zoom,
    );
    let at = |p: Pos2| {
        pos2(
            window_uv.min.x + (p.x - screen.min.x) / screen.width() * window_uv.width(),
            window_uv.min.y + (p.y - screen.min.y) / screen.height() * window_uv.height(),
        )
    };
    let uv = Rect::from_min_max(at(rect.min), at(rect.max));
    let centre = rect.center();
    let transmission = artwork_transmission(
        (centre.x - screen.min.x) / screen.width(),
        (centre.y - screen.min.y) / screen.height(),
    );
    // The glass black of the card, then the scrims the artwork sits under.
    let level = (1.0 - style.black) * transmission;
    let a = art.alpha * painter.opacity();
    let g = level * a;
    let tint = Color32::from_rgba_premultiplied(
        (g * 255.0) as u8,
        (g * 255.0) as u8,
        (g * 255.0) as u8,
        (a * 255.0) as u8,
    );
    painter.add(
        egui::epaint::RectShape::filled(rect, style.radius, tint).with_texture(art.material, uv),
    );
    if style.stroke {
        crate::ui::components::paint_hairline(painter, rect, style.radius, color::hairline_glass());
    }
}

/// Like [`wrapped`] with every row centred (`multilineTextAlignment(.center)`); the galley is
/// centred on the x it is painted at.
pub fn wrapped_centered(
    painter: &Painter,
    style: TextStyle,
    text: &str,
    color: Color32,
    width: f32,
    max_rows: usize,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), style.font(), color, width.max(1.0));
    job.wrap.max_rows = max_rows;
    job.halign = Align::Center;
    for section in &mut job.sections {
        section.format.line_height = Some(style.line_height());
    }
    painter.layout_job(job)
}
