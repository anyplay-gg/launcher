//! Surfaces and compound widgets built from the components (docs/DESIGN.md section 5): the game
//! tile base, info cards (the "facts" line of the hero), panel rows with all their controls, the
//! side panel, full-screen panels, the toast and scroll-to-focus.

use egui::text::LayoutJob;
use egui::{
    Align, Align2, Area, Color32, Context, CursorIcon, Id, Layout, Order, Painter, Rect, Response,
    Role, Sense, TextEdit, TextureId, Ui, UiBuilder, Vec2, WidgetInfo, pos2, vec2,
};

use crate::ui::anim::{self, AnimExt, Motion, lerp};
use crate::ui::components::{
    self, Force, PRESS_SCALE, corner, key_cap, paint_capsule, paint_hairline, paint_progress,
    paint_shadow, scale_rect,
};
use crate::ui::glass::{GlassStyle, paint_glass};
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input;
use crate::ui::text;
use crate::ui::theme::{self, black, color, fade, mix, radius, shadows, size, ty, white};

// ---------------------------------------------------------------------------------------------
// Game tile (5.1)
// ---------------------------------------------------------------------------------------------

/// What a game tile shows on top of its icon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TileOverlay {
    /// Installed: nothing.
    None,
    Locked,
    Unreadable,
    Queued,
    /// Overall unlock progress 0..=1.
    Installing(f32),
    Failed,
    Starting,
    Running,
}

/// A decoded icon uploaded as an egui texture; `aspect` is width / height of the source image.
#[derive(Clone, Copy, Debug)]
pub struct TileImage {
    pub texture: TextureId,
    pub aspect: f32,
}

/// Everything a tile needs to be painted.
#[derive(Clone, Copy, Debug)]
pub struct TileStyle {
    pub image: Option<TileImage>,
    pub overlay: TileOverlay,
    /// Focus amount 0..=1: shadow grows from radius 10 to 22. The ring shows when `ring` is set.
    pub focus: f32,
    /// The focus ring is visible (focus on the tiles and no overlay open); also animated by `focus`.
    pub ring: bool,
    /// The pointer rests on the tile: a quieter ring than the focus ring (0..=1).
    pub hover: f32,
    pub accent: Color32,
    /// Overall opacity (tiles left of the selection fade out).
    pub opacity: f32,
    /// Clock for the spinner of the starting state.
    pub time: f64,
}

impl TileStyle {
    pub fn new(accent: Color32) -> Self {
        Self {
            image: None,
            overlay: TileOverlay::None,
            focus: 0.0,
            ring: false,
            hover: 0.0,
            accent,
            opacity: 1.0,
            time: 0.0,
        }
    }
}

/// Paints a square game tile into `rect` (already animated to its current position and size).
pub fn paint_tile(painter: &Painter, rect: Rect, style: &TileStyle) {
    let side = rect.width();
    let r = radius::tile(side);
    let alpha = style.opacity.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    if style.ring {
        components::paint_focus_glow(painter, rect, r, 3.0, style.focus * alpha);
    }
    // Focus is the full ring; the pointer alone draws it at 45 %.
    let ring_t = (if style.ring { style.focus } else { 0.0 }).max(0.45 * style.hover);
    paint_shadow(
        painter,
        rect,
        r,
        egui::epaint::Shadow {
            color: fade(shadows::tile(style.focus).color, alpha),
            ..shadows::tile(style.focus)
        },
    );
    painter.rect_filled(
        rect,
        corner(r),
        fade(color::surface_tile_placeholder(), alpha),
    );
    match style.image {
        Some(image) => {
            // Aspect-fill: crop the longer side around the centre.
            let uv = if image.aspect >= 1.0 {
                let w = 1.0 / image.aspect;
                Rect::from_min_max(pos2((1.0 - w) * 0.5, 0.0), pos2((1.0 + w) * 0.5, 1.0))
            } else {
                let h = image.aspect;
                Rect::from_min_max(pos2(0.0, (1.0 - h) * 0.5), pos2(1.0, (1.0 + h) * 0.5))
            };
            painter.add(
                egui::epaint::RectShape::filled(
                    rect,
                    corner(r),
                    Color32::WHITE.gamma_multiply(alpha),
                )
                .with_texture(image.texture, uv),
            );
        }
        None => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                Icon::GAME_CONTROLLER.glyph(),
                Icon::GAME_CONTROLLER.font(text::quantize_size(side * 0.32), true),
                fade(white(0.5), alpha),
            );
        }
    }
    paint_tile_overlay(painter, rect, r, style, alpha);
    if ring_t > 0.0 {
        components::paint_focus_stroke(painter, rect, r, 3.0, ring_t * alpha);
    }
}

fn paint_tile_overlay(painter: &Painter, rect: Rect, r: f32, style: &TileStyle, alpha: f32) {
    let side = rect.width();
    let dim = |amount: f32| painter.rect_filled(rect, corner(r), fade(black(amount), alpha));
    let badge = |icon: Icon| {
        dim(0.42);
        let glyph = (side * 0.15).max(14.0);
        let boxed = glyph + side * 0.16;
        let centre = pos2(rect.max.x - boxed * 0.5, rect.max.y - boxed * 0.5);
        painter.circle_filled(
            centre,
            (glyph + side * 0.09) * 0.5,
            fade(black(0.55), alpha),
        );
        painter.text(
            centre,
            Align2::CENTER_CENTER,
            icon.glyph(),
            icon.font(text::quantize_size(glyph), icon != Icon::X_CIRCLE),
            fade(Color32::WHITE, alpha),
        );
    };
    match style.overlay {
        TileOverlay::None => {}
        TileOverlay::Locked => badge(Icon::LOCK),
        TileOverlay::Unreadable => badge(Icon::X_CIRCLE),
        TileOverlay::Failed => badge(Icon::WARNING),
        TileOverlay::Queued => {
            dim(0.42);
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                Icon::CLOCK.glyph(),
                Icon::CLOCK.font(text::quantize_size(side * 0.18), false),
                fade(white(0.85), alpha),
            );
        }
        TileOverlay::Installing(fraction) => {
            dim(0.30);
            let pad = side * 0.07;
            let h = size::tile_progress_height(side);
            let bar = Rect::from_min_size(
                pos2(rect.min.x + pad, rect.max.y - pad - h),
                vec2(side - 2.0 * pad, h),
            );
            paint_progress(painter, bar, fraction, fade(style.accent, alpha));
        }
        TileOverlay::Starting => {
            dim(0.42);
            let radius = side * 0.07;
            components::paint_spinner_arc(
                painter,
                rect.center(),
                radius,
                style.time,
                2.0,
                fade(Color32::WHITE, alpha),
            );
        }
        TileOverlay::Running => {
            let d = side * 0.09;
            let centre = pos2(
                rect.max.x - side * 0.07 - d * 0.5,
                rect.min.y + side * 0.07 + d * 0.5,
            );
            let dot = Rect::from_center_size(centre, Vec2::splat(d));
            paint_shadow(
                painter,
                dot,
                d * 0.5,
                theme::shadow(fade(style.accent, alpha), 6.0, 0.0),
            );
            painter.circle_filled(centre, d * 0.5, fade(style.accent, alpha));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Info cards: the facts line of the hero (5.3)
// ---------------------------------------------------------------------------------------------

/// One fact: a glyph and overline (`Status`, `Size`, ...) over a value.
#[derive(Clone, Debug)]
pub struct Fact<'a> {
    pub icon: Icon,
    pub title: &'a str,
    pub value: &'a str,
    /// Colour of the value (default white), e.g. the state tint.
    pub value_color: Option<Color32>,
}

/// One 236 x 96 glass card.
pub fn info_card(ui: &mut Ui, fact: &Fact<'_>, language: Language) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(size::INFO_CARD.0, size::INFO_CARD.1), Sense::hover());
    let name = format!("{}: {}", fact.title, fact.value);
    response.widget_info(|| WidgetInfo::labeled(Role::Label, true, name.as_str()));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        paint_glass(ui.ctx(), painter, rect, GlassStyle::card());
        let pad = size::INFO_CARD_PADDING;
        let overline_y = rect.min.y + pad;
        let icon_size = 12.0;
        painter.text(
            pos2(
                rect.min.x + pad,
                overline_y + ty::INFO_OVERLINE.line_height() * 0.5,
            ),
            Align2::LEFT_CENTER,
            fact.icon.glyph(),
            fact.icon.font(icon_size, false),
            color::text_overline(),
        );
        text::paint(
            painter,
            pos2(rect.min.x + pad + icon_size + 6.0, overline_y),
            Align2::LEFT_TOP,
            ty::INFO_OVERLINE,
            &text::to_upper(fact.title, language),
            color::text_overline(),
        );
        let value_y = overline_y + ty::INFO_OVERLINE.line_height() + 10.0;
        let max = size::INFO_CARD.0 - 2.0 * pad;
        let shown = text::middle_truncate(painter, ty::INFO_VALUE, fact.value, max);
        text::paint(
            painter,
            pos2(rect.min.x + pad, value_y),
            Align2::LEFT_TOP,
            ty::INFO_VALUE,
            &shown,
            fact.value_color.unwrap_or(Color32::WHITE),
        );
    }
    response
}

/// A row of info cards with the 14 dp gap.
pub fn facts_line(ui: &mut Ui, facts: &[Fact<'_>], language: Language) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = size::INFO_CARD_GAP;
        for fact in facts {
            info_card(ui, fact, language);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Panel rows (5.8)
// ---------------------------------------------------------------------------------------------

/// One key cap of a keys row.
#[derive(Clone, Debug)]
pub struct KeyCapSpec {
    pub label: String,
    /// Keyboard keys fill at 0.12, mouse/wheel sources at 0.06.
    pub keyboard: bool,
}

/// The value control on the right of a row.
pub enum RowControl<'a> {
    /// `<  value  >`: chevrons step, the row advances (wrapping).
    Choice {
        value: &'a str,
        can_decrement: bool,
        can_increment: bool,
    },
    Toggle {
        on: bool,
        accent: Color32,
    },
    /// `-  value  +` with tabular figures.
    Stepper {
        value: &'a str,
        can_decrement: bool,
        can_increment: bool,
    },
    /// Trailing chevron: the row opens a picker or a panel.
    Action,
    /// No control: the row runs at once.
    Command,
    /// Static text (15 medium, 0.7); without a value the title is a note: a dim paragraph.
    Info {
        value: Option<&'a str>,
    },
    /// A section caption over the rows that follow.
    Caption,
    /// Right-aligned text field, 200 wide, edits live.
    Text {
        value: &'a mut String,
    },
    /// Key caps, or the pulsing capture prompt.
    Keys {
        caps: &'a [KeyCapSpec],
        capturing: bool,
        none_label: &'a str,
        press_label: &'a str,
    },
    /// A few small buttons at the row's end, side by side (answer a request: Accept, Decline).
    /// `selected` is the one the keyboard and the pad would press.
    Buttons {
        labels: &'a [String],
        selected: usize,
    },
}

/// A settings row: glyph, title, detail and a value control.
pub struct PanelRow<'a> {
    pub icon: Option<Icon>,
    pub title: &'a str,
    pub detail: Option<&'a str>,
    pub control: RowControl<'a>,
    pub focused: bool,
    /// Danger: the row removes or ends something (the danger ink).
    pub destructive: bool,
    /// Caution: a note or row that says something may not work (the amber ink). Never danger.
    pub warning: bool,
    /// A controller face glyph painted before the title (`✕ Cross`).
    pub pad_glyph: Option<components::PadGlyph>,
    pub force: Option<Force>,
    /// A note directly under the row above: it keeps no air over its text.
    pub attached: bool,
    /// A text field that takes the room the title leaves, written from the left: a message.
    pub wide: bool,
    /// Language of the accessible names of the chevrons.
    pub language: Language,
}

/// What happened in a row this frame.
pub struct RowOutput {
    /// Response of the whole row (click = the row's primary activation).
    pub response: Response,
    /// A chevron or the minus/plus was clicked: -1 or +1.
    pub step: Option<i8>,
    /// The switch was clicked.
    pub toggled: bool,
    /// The text field changed.
    pub text_changed: bool,
    /// Enter ended the text field's editing.
    pub submitted: bool,
    /// One of a row's buttons was clicked: its index.
    pub pressed: Option<usize>,
}

/// The buttons at a row's end: 32 dp high, 12 dp apart, their text 14 dp in from the edge.
const BUTTON_HEIGHT: f32 = 32.0;
const BUTTON_GAP: f32 = 12.0;
const BUTTON_PAD: f32 = 14.0;

/// Chevrons rest quiet on a card and come up on the focused row.
const CHEVRON_OPACITY: f32 = 0.45;
const CHEVRON_OPACITY_FOCUS: f32 = 0.85;
const CHEVRON_OPACITY_END: f32 = 0.25;

impl<'a> PanelRow<'a> {
    pub fn new(title: &'a str, control: RowControl<'a>) -> Self {
        Self {
            icon: None,
            title,
            detail: None,
            control,
            focused: false,
            destructive: false,
            warning: false,
            pad_glyph: None,
            force: None,
            attached: false,
            wide: false,
            language: Language::English,
        }
    }
    pub fn attached(mut self, attached: bool) -> Self {
        self.attached = attached;
        self
    }
    pub fn wide(mut self, wide: bool) -> Self {
        self.wide = wide;
        self
    }
    pub fn pad_glyph(mut self, glyph: components::PadGlyph) -> Self {
        self.pad_glyph = Some(glyph);
        self
    }
    pub fn force(mut self, force: Option<Force>) -> Self {
        self.force = force;
        self
    }
    pub fn language(mut self, language: Language) -> Self {
        self.language = language;
        self
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn detail(mut self, detail: &'a str) -> Self {
        self.detail = Some(detail);
        self
    }
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }
    pub fn warning(mut self, warning: bool) -> Self {
        self.warning = warning;
        self
    }

    fn focusable(&self) -> bool {
        !matches!(self.control, RowControl::Info { .. } | RowControl::Caption)
    }

    /// Notes and captions: bare text on the panel, no card, as high as their text needs.
    fn show_text_block(self, ui: &mut Ui) -> RowOutput {
        let painter = ui.painter().clone();
        let caption = matches!(self.control, RowControl::Caption);
        let width = ui.available_width();
        let inner = width - 2.0 * size::ROW_PADDING_X;
        let (style, quiet, space) = if caption {
            (ty::SECTION_CAPTION, color::text_caption(), (14.0, 6.0))
        } else {
            (
                ty::NOTE,
                color::text_note(),
                (if self.attached { 0.0 } else { 4.0 }, 4.0),
            )
        };
        // A warning note has the ink of the warning line under Play on the home screen.
        let ink = if self.warning && !caption {
            color::warning_text()
        } else {
            quiet
        };
        let galley = text::layout(&painter, style, self.title, ink, inner);
        let height = space.0 + galley.size().y + space.1;
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        response.widget_info(|| self.widget_info());
        if ui.is_rect_visible(rect) {
            painter.galley(
                pos2(rect.min.x + size::ROW_PADDING_X, rect.min.y + space.0),
                galley,
                ink,
            );
        }
        RowOutput {
            response,
            step: None,
            toggled: false,
            text_changed: false,
            submitted: false,
            pressed: None,
        }
    }

    /// The row as one accessibility node: a switch for toggles, a label for static rows, a button
    /// (with its current value) for everything else.
    fn widget_info(&self) -> WidgetInfo {
        match &self.control {
            RowControl::Caption => WidgetInfo::labeled(Role::Label, true, self.title),
            RowControl::Info { value } => {
                let mut name = match value {
                    Some(v) => format!("{}: {v}", self.title),
                    None => self.title.to_owned(),
                };
                if let Some(detail) = self.detail {
                    name = format!("{name}, {detail}");
                }
                WidgetInfo::labeled(Role::Label, true, name)
            }
            RowControl::Toggle { on, .. } => {
                WidgetInfo::selected(Role::Switch, true, *on, self.title)
            }
            RowControl::Choice { value, .. } | RowControl::Stepper { value, .. } => {
                let mut info = WidgetInfo::labeled(Role::Button, true, self.title);
                info.current_text_value = Some((*value).to_owned());
                info
            }
            RowControl::Keys {
                caps,
                capturing,
                press_label,
                ..
            } => {
                let mut info = WidgetInfo::labeled(Role::Button, true, self.title);
                info.current_text_value = Some(if *capturing {
                    (*press_label).to_owned()
                } else {
                    caps.iter()
                        .map(|c| c.label.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                });
                info
            }
            RowControl::Buttons { labels, .. } => {
                // The answers are read after the name: "Vale, Accept, Decline".
                let mut info = WidgetInfo::labeled(Role::Button, true, self.title);
                info.current_text_value = Some(labels.join(", "));
                info
            }
            RowControl::Action | RowControl::Command | RowControl::Text { .. } => {
                // What the row says under its title is read after its name.
                let mut info = WidgetInfo::labeled(Role::Button, true, self.title);
                info.current_text_value = self.detail.map(str::to_owned);
                info
            }
        }
    }

    /// Draws the row across the available width.
    pub fn show(mut self, ui: &mut Ui) -> RowOutput {
        let painter = ui.painter().clone();
        let ctx = ui.ctx().clone();
        if matches!(
            self.control,
            RowControl::Caption | RowControl::Info { value: None }
        ) {
            return self.show_text_block(ui);
        }
        let focusable = self.focusable();
        let width = ui.available_width();
        let pad = size::ROW_PADDING_X;

        // Value width decides how much room the text column has.
        let value_width = self.value_width(&painter, width);
        // A pad glyph without an icon stands in the icon column, so every title starts on one edge.
        let icon_block = if self.icon.is_some() || self.pad_glyph.is_some() {
            size::ROW_ICON_BOX + 14.0
        } else {
            0.0
        };
        // `✕ Cross` with an icon as well: the glyph goes between icon and title.
        let glyph_block = if self.pad_glyph.is_some() && self.icon.is_some() {
            24.0
        } else {
            0.0
        };
        let text_width =
            (width - 2.0 * pad - icon_block - glyph_block - value_width - 12.0).max(40.0);
        // A wide field shows its title as the hint inside the field, not beside it.
        let title_text = if self.wide && matches!(self.control, RowControl::Text { .. }) {
            ""
        } else {
            self.title
        };
        let title_style = ty::ROW_TITLE;
        let title_galley = {
            let shrink = {
                let natural = text::layout(
                    &painter,
                    title_style,
                    title_text,
                    Color32::WHITE,
                    text_width,
                );
                if natural.rows.len() > 2 { 0.85 } else { 1.0 }
            };
            let style = text::scaled(title_style, shrink);
            let mut job = LayoutJob::simple(
                title_text.to_owned(),
                style.font(),
                Color32::PLACEHOLDER,
                text_width,
            );
            job.wrap.max_rows = 2;
            job.sections[0].format.line_height = Some(style.line_height());
            painter.layout_job(job)
        };
        let detail_galley = self.detail.map(|detail| {
            let mut job = LayoutJob::simple(
                detail.to_owned(),
                ty::DETAIL.font(),
                Color32::PLACEHOLDER,
                text_width,
            );
            job.wrap.max_rows = 2;
            // Words wrap whole (the original); only a single long word, such as a path, breaks.
            job.wrap.break_anywhere = false;
            job.sections[0].format.line_height = Some(ty::DETAIL.line_height());
            painter.layout_job(job)
        });
        let text_height =
            title_galley.size().y + detail_galley.as_ref().map_or(0.0, |d| 3.0 + d.size().y);
        let height = size::ROW_MIN_HEIGHT.max(text_height + 20.0);

        let sense = if focusable && !matches!(self.control, RowControl::Text { .. }) {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), sense);
        response.widget_info(|| self.widget_info());
        crate::ui::a11y::follow(
            &ctx,
            response.id,
            self.focused && focusable && sense == Sense::click(),
        );
        let hovered = focusable && (input::hovered(&ctx, &response) || self.force.is_some());
        if hovered {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        let t = ctx.focus_t((response.id, "focus"), self.focused && focusable);
        let mut out = RowOutput {
            response,
            step: None,
            toggled: false,
            text_changed: false,
            submitted: false,
            pressed: None,
        };
        if !ui.is_rect_visible(rect) {
            return out;
        }

        // The row: hover lifts it, focus lifts it more and draws the ring (DESIGN.md 3.9). Nothing grows.
        let bg_rect = rect;
        let rest = if focusable {
            color::surface_row()
        } else {
            Color32::TRANSPARENT
        };
        let hover_t = ctx.animate_to(
            (out.response.id, "hover"),
            f32::from(hovered),
            Motion::Hover,
        );
        let press_t = ctx.animate_to(
            (out.response.id, "press"),
            f32::from(
                focusable
                    && (out.response.is_pointer_button_down_on()
                        || self.force == Some(Force::Press)),
            ),
            Motion::Press,
        );
        components::paint_surface(
            &painter,
            bg_rect,
            radius::ROW,
            rest,
            hover_t,
            t,
            press_t,
            size::FOCUS_RING,
        );

        let fg = Color32::WHITE;
        let dim = |factor: f32| fade(fg, factor);
        if self.wide && matches!(self.control, RowControl::Text { .. }) {
            // A field that fills the row would pass for one more row: its outline says it takes text.
            painter.rect_stroke(
                bg_rect,
                corner(radius::ROW),
                egui::Stroke::new(1.0, dim(0.22)),
                egui::StrokeKind::Inside,
            );
        }
        let (destructive, warning) = (self.destructive, self.warning);
        let mut x = rect.min.x + pad;
        if let Some(icon) = self.icon {
            let tint = if destructive {
                color::destructive_icon()
            } else if warning {
                color::warning_text()
            } else {
                dim(0.85)
            };
            painter.text(
                pos2(x + size::ROW_ICON_BOX * 0.5, rect.center().y),
                Align2::CENTER_CENTER,
                icon.glyph(),
                icon.font(size::ROW_ICON, false),
                tint,
            );
            x += icon_block;
        } else if let Some(glyph) = self.pad_glyph {
            glyph.paint(
                &painter,
                pos2(x + size::ROW_ICON_BOX * 0.5, rect.center().y),
                16.0,
                dim(0.85),
            );
            x += icon_block;
        }
        let top = rect.center().y - text_height * 0.5;
        let title_color = if destructive {
            color::destructive_text()
        } else if warning {
            color::warning_text()
        } else if focusable {
            fg
        } else {
            color::text_inert()
        };
        let title_h = title_galley.size().y;
        if let (Some(glyph), Some(_)) = (self.pad_glyph, self.icon) {
            let first_line = title_galley.rows.first().map_or(title_h, |r| r.height());
            glyph.paint(
                &painter,
                pos2(x + 8.0, top + first_line * 0.5),
                16.0,
                title_color,
            );
            x += glyph_block;
        }
        painter.galley(pos2(x, top), title_galley, title_color);
        if let Some(detail) = detail_galley {
            painter.galley(
                pos2(x, top + title_h + 3.0),
                detail,
                color::text_row_detail(),
            );
        }

        // Value control.
        let value_rect = Rect::from_min_max(
            pos2(rect.max.x - pad - value_width, rect.min.y),
            pos2(rect.max.x - pad, rect.max.y),
        );
        self.paint_value(ui, &ctx, value_rect, fg, t, &mut out);
        out
    }

    fn value_width(&self, painter: &Painter, row_width: f32) -> f32 {
        let chevron = size::CHEVRON_BOX;
        match &self.control {
            RowControl::Choice { value, .. } => {
                let w = text::width(painter, ty::CHOICE_VALUE, value)
                    .clamp(size::CHOICE_VALUE_WIDTH.0, size::CHOICE_VALUE_WIDTH.1);
                chevron * 2.0 + w + 24.0
            }
            RowControl::Stepper { .. } => chevron * 2.0 + size::STEPPER_VALUE_WIDTH + 24.0,
            RowControl::Toggle { .. } => size::SWITCH.0,
            RowControl::Action => 14.0,
            RowControl::Caption | RowControl::Command => 0.0,
            RowControl::Buttons { labels, .. } => {
                labels
                    .iter()
                    .map(|l| text::width(painter, ty::CHOICE_VALUE, l) + 2.0 * BUTTON_PAD)
                    .sum::<f32>()
                    + BUTTON_GAP * labels.len().saturating_sub(1) as f32
            }
            // A long value must not squeeze the title column: at most 40 % of the row.
            RowControl::Info { value } => value.map_or(0.0, |v| {
                text::width(painter, ty::ROW_INFO, v).min(row_width * 0.4)
            }),
            RowControl::Text { .. } if self.wide => {
                // The field takes the whole row; the title is its hint while it is empty.
                let icon = if self.icon.is_some() || self.pad_glyph.is_some() {
                    size::ROW_ICON_BOX + 14.0
                } else {
                    0.0
                };
                (row_width - 2.0 * size::ROW_PADDING_X - icon).max(size::TEXT_FIELD_WIDTH)
            }
            RowControl::Text { .. } => size::TEXT_FIELD_WIDTH,
            RowControl::Keys {
                caps,
                capturing,
                none_label,
                press_label,
            } => {
                if *capturing {
                    text::width(
                        painter,
                        text::scaled(ty::CHOICE_VALUE, 15.0 / 16.0),
                        press_label,
                    )
                } else if caps.is_empty() {
                    text::width(painter, ty::ROW_INFO, none_label)
                } else {
                    caps.iter()
                        .map(|c| {
                            (text::width(painter, ty::KEY_CAP, &c.label)
                                + 2.0 * size::KEYCAP_PADDING_X)
                                .max(size::KEYCAP_MIN.0)
                        })
                        .sum::<f32>()
                        + size::KEYCAP_GAP * (caps.len() as f32 - 1.0)
                }
            }
        }
    }

    fn paint_value(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        rect: Rect,
        fg: Color32,
        t: f32,
        out: &mut RowOutput,
    ) {
        let painter = ui.painter().clone();
        let chevron_box = size::CHEVRON_BOX;
        let row_id = out.response.id;
        let title = self.title;
        let wide = self.wide;
        let language = self.language;
        let chevron = |ui: &mut Ui, at: Rect, icon: Icon, enabled: bool, step: i8, glyph: f32| {
            let r = ui.interact(
                at,
                row_id.with(("chevron", step)),
                if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            let name = format!(
                "{title}: {}",
                language.text(if step < 0 {
                    Key::DirectionLeft
                } else {
                    Key::DirectionRight
                })
            );
            r.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, name.as_str()));
            let hover = enabled && input::hovered(ctx, &r);
            let press = enabled && r.is_pointer_button_down_on();
            if hover {
                // A soft disc behind the glyph: the mouse affordance of the otherwise bare chevron.
                let disc = Rect::from_center_size(at.center(), Vec2::splat(chevron_box));
                painter.circle_filled(
                    disc.center(),
                    chevron_box * 0.5 * if press { PRESS_SCALE } else { 1.0 },
                    fade(fg, 0.10),
                );
            }
            let opacity = if !enabled {
                CHEVRON_OPACITY_END
            } else if hover {
                1.0
            } else {
                lerp(CHEVRON_OPACITY, CHEVRON_OPACITY_FOCUS, t)
            };
            if icon == Icon::CARET_LEFT || icon == Icon::CARET_RIGHT {
                components::paint_caret(
                    &painter,
                    at.center(),
                    icon == Icon::CARET_RIGHT,
                    glyph,
                    fade(fg, opacity),
                );
            } else {
                // The stepper's minus and plus stay glyphs.
                painter.text(
                    at.center(),
                    Align2::CENTER_CENTER,
                    icon.glyph(),
                    icon.font(glyph, false),
                    fade(fg, opacity),
                );
            }
            if enabled && input::hovered(ctx, &r) {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            if enabled && r.clicked() {
                Some(step)
            } else {
                None
            }
        };
        match &mut self.control {
            RowControl::Choice {
                value,
                can_decrement,
                can_increment,
            } => {
                let value_w = rect.width() - 2.0 * chevron_box - 24.0;
                let left = Rect::from_min_size(rect.left_top(), vec2(chevron_box, rect.height()));
                let right = Rect::from_min_size(
                    pos2(rect.max.x - chevron_box, rect.min.y),
                    vec2(chevron_box, rect.height()),
                );
                let style = text::scaled(ty::CHOICE_VALUE, 1.0);
                let shrink = text::shrink_factor(&painter, style, value, value_w, 0.7);
                text::paint(
                    &painter,
                    pos2(
                        rect.min.x + chevron_box + 12.0 + value_w * 0.5,
                        rect.center().y,
                    ),
                    Align2::CENTER_CENTER,
                    text::scaled(style, shrink),
                    value,
                    fg,
                );
                let a = chevron(
                    ui,
                    left,
                    Icon::CARET_LEFT,
                    *can_decrement,
                    -1,
                    size::CHEVRON_GLYPH,
                );
                let b = chevron(
                    ui,
                    right,
                    Icon::CARET_RIGHT,
                    *can_increment,
                    1,
                    size::CHEVRON_GLYPH,
                );
                out.step = a.or(b);
            }
            RowControl::Stepper {
                value,
                can_decrement,
                can_increment,
            } => {
                let left = Rect::from_min_size(rect.left_top(), vec2(chevron_box, rect.height()));
                let right = Rect::from_min_size(
                    pos2(rect.max.x - chevron_box, rect.min.y),
                    vec2(chevron_box, rect.height()),
                );
                let cell = Rect::from_min_max(
                    pos2(rect.min.x + chevron_box + 12.0, rect.min.y),
                    pos2(rect.max.x - chevron_box - 12.0, rect.max.y),
                );
                text::paint_tabular_aligned(
                    &painter,
                    cell,
                    Align::Center,
                    ty::CHOICE_VALUE,
                    value,
                    fg,
                );
                let a = chevron(
                    ui,
                    left,
                    Icon::MINUS,
                    *can_decrement,
                    -1,
                    size::CHEVRON_GLYPH,
                );
                let b = chevron(
                    ui,
                    right,
                    Icon::PLUS,
                    *can_increment,
                    1,
                    size::CHEVRON_GLYPH,
                );
                out.step = a.or(b);
            }
            RowControl::Toggle { on, accent } => {
                let switch_rect = Rect::from_center_size(
                    pos2(rect.max.x - size::SWITCH.0 * 0.5, rect.center().y),
                    vec2(size::SWITCH.0, size::SWITCH.1),
                );
                let mut child = ui.new_child(UiBuilder::new().max_rect(switch_rect));
                let switch = components::Switch::new(*on, *accent)
                    .focus_amount(t)
                    .announce(false)
                    .label(self.title)
                    .show(&mut child);
                out.toggled = switch.clicked();
            }
            RowControl::Buttons { labels, selected } => {
                // Right to left, so the first label ends up leftmost and the group ends on the row's edge.
                let mut right = rect.max.x;
                for (index, label) in labels.iter().enumerate().rev() {
                    let w = text::width(&painter, ty::CHOICE_VALUE, label) + 2.0 * BUTTON_PAD;
                    let at = Rect::from_center_size(
                        pos2(right - w * 0.5, rect.center().y),
                        vec2(w, BUTTON_HEIGHT),
                    );
                    right -= w + BUTTON_GAP;
                    let r = ui.interact(at, row_id.with(("button", index)), Sense::click());
                    r.widget_info(|| WidgetInfo::labeled(Role::Button, true, label.as_str()));
                    let hover = input::hovered(ctx, &r);
                    if hover {
                        ctx.set_cursor_icon(CursorIcon::PointingHand);
                    }
                    let pressed = r.is_pointer_button_down_on();
                    let shown = at.shrink(if pressed { 1.0 } else { 0.0 });
                    // The row rests dark: its buttons are quiet chips. With the focus on the row, the
                    // button the keys would press becomes the primary one (ink fill, dark label); the
                    // other stays a chip.
                    let picked = index == *selected;
                    let chip = white(if hover { 0.20 } else { 0.14 });
                    let fill = mix(chip, color::primary(), if picked { t } else { 0.0 });
                    components::paint_capsule(&painter, shown, fill);
                    let ink = mix(fg, color::on_primary(), if picked { t } else { 0.0 });
                    text::paint(
                        &painter,
                        shown.center(),
                        Align2::CENTER_CENTER,
                        ty::CHOICE_VALUE,
                        label,
                        ink,
                    );
                    if r.clicked() {
                        out.pressed = Some(index);
                    }
                }
            }
            RowControl::Action => {
                painter.text(
                    pos2(rect.max.x - 7.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    Icon::CARET_RIGHT.glyph(),
                    Icon::CARET_RIGHT.font(14.0, false),
                    fade(fg, 0.6),
                );
            }
            RowControl::Caption | RowControl::Command => {}
            RowControl::Info { value } => {
                if let Some(value) = value {
                    text::paint(
                        &painter,
                        pos2(rect.max.x, rect.center().y),
                        Align2::RIGHT_CENTER,
                        ty::ROW_INFO,
                        value,
                        fade(fg, 0.7),
                    );
                }
            }
            RowControl::Text { value } => {
                let field = Rect::from_center_size(rect.center(), vec2(rect.width(), 30.0));
                let (align, layout) = if wide {
                    (Align2::LEFT_CENTER, Layout::left_to_right(Align::Center))
                } else {
                    (Align2::RIGHT_CENTER, Layout::right_to_left(Align::Center))
                };
                let edit = TextEdit::singleline(*value)
                    .font(ty::CHOICE_VALUE.font())
                    .hint_text(
                        egui::RichText::new(if wide { title } else { "" })
                            .font(ty::CHOICE_VALUE.font()),
                    )
                    .align(align)
                    .frame(egui::Frame::NONE)
                    .desired_width(field.width())
                    .text_color(fg)
                    .margin(egui::Margin::ZERO);
                let mut child = ui.new_child(UiBuilder::new().max_rect(field).layout(layout));
                // The hint takes the row's ink, a little dimmer, on the light focused row too.
                child.visuals_mut().weak_text_color = Some(fade(fg, 0.62));
                // A keyboard or pad "select" on the row asks for the caret (see `screens::rows`).
                let edit_id = row_id.with("text-edit");
                let want_focus = ctx
                    .data_mut(|d| d.remove_temp::<bool>(Id::unique("panel-text-focus")))
                    .unwrap_or(false);
                if want_focus {
                    ctx.memory_mut(|m| m.request_focus(edit_id));
                }
                let response = child.add(edit.id(edit_id));
                out.text_changed = response.changed();
                out.submitted =
                    response.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter));
            }
            RowControl::Keys {
                caps,
                capturing,
                none_label,
                press_label,
            } => {
                if *capturing {
                    let pulse = anim::pulse_opacity(ctx.clock());
                    ctx.request_repaint();
                    text::paint(
                        &painter,
                        pos2(rect.max.x, rect.center().y),
                        Align2::RIGHT_CENTER,
                        text::scaled(ty::CHOICE_VALUE, 15.0 / 16.0),
                        press_label,
                        fade(fg, pulse),
                    );
                } else if caps.is_empty() {
                    text::paint(
                        &painter,
                        pos2(rect.max.x, rect.center().y),
                        Align2::RIGHT_CENTER,
                        ty::ROW_INFO,
                        none_label,
                        fade(fg, 0.5),
                    );
                } else {
                    let mut child = ui.new_child(
                        UiBuilder::new()
                            .max_rect(rect)
                            .layout(Layout::left_to_right(Align::Center)),
                    );
                    child.spacing_mut().item_spacing.x = size::KEYCAP_GAP;
                    for cap in caps.iter() {
                        key_cap(&mut child, &cap.label, cap.keyboard, t);
                    }
                }
            }
        }
    }
}

/// Spring-animated scroll offset that keeps the focused row centred (`scrollTo(anchor: .center)`
/// inside `Motion.focus`). Feed the result to `ScrollArea::vertical_scroll_offset`.
pub fn centered_scroll_offset(
    ctx: &Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    row_top: f32,
    row_height: f32,
    viewport_height: f32,
    content_height: f32,
) -> f32 {
    let max = (content_height - viewport_height).max(0.0);
    let target = (row_top + row_height * 0.5 - viewport_height * 0.5).clamp(0.0, max);
    ctx.animate_to(id, target, Motion::Focus)
}

// ---------------------------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------------------------

/// Result of drawing the side panel.
pub struct SidePanelOutput {
    /// The dimmed area left of the panel was clicked: close the panel.
    pub scrim_clicked: bool,
    /// Rect of the panel contents (inside the 36 dp padding).
    pub content: Rect,
}

/// The game-settings family of panels: slides in from the trailing edge (spring `Motion::Overlay`
/// on `open`), dims the rest with black 0.35 and shows the title (30 light, 2 lines) and an optional
/// subtitle (15, white 0.6). `add_contents` receives the content `Ui` below the title.
pub fn side_panel<R>(
    ctx: &Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    open: bool,
    title: &str,
    subtitle: Option<&str>,
    question: bool,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Option<(SidePanelOutput, R)> {
    let id = Id::unique(id);
    let v = ctx.animate_to((id, "open"), if open { 1.0 } else { 0.0 }, Motion::Overlay);
    if v <= 0.001 && !open {
        return None;
    }
    let screen = ctx.content_rect();
    let width = size::SIDE_PANEL_WIDTH;
    let offset = (1.0 - v) * width;
    let panel = Rect::from_min_max(
        pos2(screen.max.x - width + offset, screen.min.y),
        pos2(screen.max.x + offset, screen.max.y),
    );
    let mut scrim_clicked = false;
    let mut result = None;
    Area::new(id.with("area"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            // The area covers the screen, so egui finds it under the pointer: the list scrolls
            // with the wheel and nothing behind the scrim reacts.
            ui.expand_to_include_rect(screen);
            let painter = ui.painter().clone();
            // Scrim over the rest of the screen.
            let scrim = Rect::from_min_max(screen.min, pos2(panel.min.x, screen.max.y));
            painter.rect_filled(
                screen,
                0.0,
                fade(color::scrim_side_panel(), v.clamp(0.0, 1.0)),
            );
            let scrim_response = ui.interact(scrim, id.with("scrim"), Sense::click());
            scrim_clicked = scrim_response.clicked();
            paint_glass(ctx, &painter, panel, GlassStyle::side_panel());
            let edge = Rect::from_min_size(
                panel.min,
                vec2(components::hairline_width(&painter), panel.height()),
            );
            painter.rect_filled(
                components::snap_rect(&painter, edge),
                0.0,
                color::hairline_panel_edge(),
            );
            let pad = size::SIDE_PANEL_PADDING;
            let inner = panel.shrink(pad);
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(inner)
                    .layout(Layout::top_down(Align::Min)),
            );
            let title_galley = text::layout(
                &painter,
                ty::SIDE_PANEL_TITLE,
                title,
                Color32::WHITE,
                inner.width(),
            );
            child.add_space(0.0);
            let title_h = title_galley.size().y;
            let top = child.cursor().min.y;
            // Title and subtitle start on the edge of the row cards below them.
            let edge = inner.min.x + size::LIST_INSET;
            painter.galley(pos2(edge, top), title_galley, Color32::WHITE);
            child.add_space(title_h + 8.0);
            if let Some(subtitle) = subtitle {
                let top = child.cursor().min.y;
                // Wraps (the confirmation sentence is long, and longer in Turkish).
                // A confirmation's question is what the player decides on: body size and ink, with
                // room between the lines. Any other subtitle is a quiet line.
                let (style, ink, leading) = if question {
                    (ty::QUESTION, color::text_label(), 1.45)
                } else {
                    (
                        theme::TextStyle::new(15.0, theme::Weight::Regular),
                        color::text_detail(),
                        1.2,
                    )
                };
                let galley = text::layout_font(
                    &painter,
                    style.font(),
                    style.size * leading,
                    style.tracking,
                    subtitle,
                    ink,
                    inner.width(),
                );
                let height = galley.size().y;
                painter.galley(pos2(edge, top), galley, ink);
                child.add_space(height + 8.0);
            }
            child.add_space(18.0);
            let content = child.available_rect_before_wrap();
            result = Some((
                SidePanelOutput {
                    scrim_clicked: false,
                    content,
                },
                add_contents(&mut child),
            ));
        });
    result.map(|(mut output, r)| {
        output.scrim_clicked = scrim_clicked;
        (output, r)
    })
}

/// Full-screen overlay panels (settings, library, trophies, log): glass plus black, opacity and
/// scale 1.03 -> 1 on `Motion::Overlay`. Returns the animated openness; draw contents with `add`.
pub fn fullscreen_panel<R>(
    ctx: &Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    open: bool,
    style: GlassStyle,
    add_contents: impl FnOnce(&mut Ui, f32) -> R,
) -> Option<R> {
    let id = Id::unique(id);
    let v = ctx.animate_to((id, "open"), if open { 1.0 } else { 0.0 }, Motion::Overlay);
    if v <= 0.001 && !open {
        return None;
    }
    let screen = ctx.content_rect();
    let mut result = None;
    Area::new(id.with("area"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            // Covers the screen so the panel's lists get the wheel (see the side panel).
            ui.expand_to_include_rect(screen);
            let painter = ui.painter().clone();
            let faded = GlassStyle {
                black: style.black * v.clamp(0.0, 1.0),
                overlay: fade(style.overlay, v),
                material_mix: lerp(0.0, style.material_mix, v.clamp(0.0, 1.0)),
                ..style
            };
            paint_glass(ctx, &painter, screen, faded);
            let scale = lerp(1.03, 1.0, v);
            let area = scale_rect(screen, scale);
            let mut child = ui.new_child(UiBuilder::new().max_rect(area));
            child.set_opacity(v.clamp(0.0, 1.0));
            result = Some(add_contents(&mut child, v));
        });
    result
}

/// The toast: a glass pill at the top centre. `v` is the openness from `Motion::Overlay`
/// (spring in/out, offset -(1 - v) * 80); the caller keeps it up for 3.2 s.
pub fn toast(
    ctx: &Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    icon: Icon,
    message: &str,
    v: f32,
) {
    if v <= 0.001 {
        return;
    }
    let screen = ctx.content_rect();
    Area::new(Id::unique(id))
        .order(Order::Tooltip)
        .fixed_pos(screen.min)
        .interactable(false)
        .show(ctx, |ui| {
            let painter = ui.painter().clone();
            let max_text = (screen.width() * 0.6).min(760.0);
            let galley = {
                let mut job = LayoutJob::simple(
                    message.to_owned(),
                    ty::TOAST.font(),
                    Color32::PLACEHOLDER,
                    max_text,
                );
                job.wrap.max_rows = 2;
                job.sections[0].format.line_height = Some(ty::TOAST.line_height());
                painter.layout_job(job)
            };
            let glyph = 17.0;
            let (pad_x, pad_y, gap) = (22.0, 14.0, 10.0);
            let w = pad_x * 2.0 + glyph + gap + galley.size().x;
            let h = (pad_y * 2.0 + galley.size().y).max(radius::TOAST * 2.0);
            let top = size::TOAST_TOP - (1.0 - v) * 80.0;
            let rect = Rect::from_min_size(
                pos2(screen.center().x - w * 0.5, screen.min.y + top),
                vec2(w, h),
            );
            let a = v.clamp(0.0, 1.0);
            let status = ui.interact(rect, Id::unique(("toast-status", message)), Sense::hover());
            status.widget_info(|| WidgetInfo::labeled(Role::Status, true, message));
            paint_shadow(
                &painter,
                rect,
                radius::TOAST,
                egui::epaint::Shadow {
                    // Squared, so the shadow never shows through the fading glass.
                    color: fade(shadows::toast().color, a * a),
                    ..shadows::toast()
                },
            );
            let mut style = GlassStyle::toast();
            style.black *= a;
            paint_glass(ctx, &painter, rect, style);
            painter.text(
                pos2(rect.min.x + pad_x + glyph * 0.5, rect.center().y),
                Align2::CENTER_CENTER,
                icon.glyph(),
                icon.font(glyph, true),
                fade(Color32::WHITE, a),
            );
            painter.galley(
                pos2(
                    rect.min.x + pad_x + glyph + gap,
                    rect.center().y - galley.size().y * 0.5,
                ),
                galley,
                fade(Color32::WHITE, a),
            );
        });
}

/// A frosted card with a hairline, used by onboarding and diagnosis boxes: glass at `rect`.
pub fn glass_card(ctx: &Context, painter: &Painter, rect: Rect, style: GlassStyle) {
    paint_glass(ctx, painter, rect, style);
}

/// Language card of the onboarding (280 x 150, radius 20): `TR` / `EN` tag over the name.
pub fn language_card(
    ui: &mut Ui,
    language: Language,
    focused: bool,
    force: Option<Force>,
) -> Response {
    let it = components::interact(
        ui,
        vec2(size::LANGUAGE_CARD.0, size::LANGUAGE_CARD.1),
        focused,
        true,
        force,
    );
    let name = language.display_name();
    it.response
        .widget_info(|| WidgetInfo::labeled(Role::Button, true, name));
    if !ui.is_rect_visible(it.rect) {
        return it.response;
    }
    let painter = ui.painter();
    let s = 1.0;
    let rect = it.painted(painter);
    let r = radius::LANGUAGE_CARD;
    components::paint_surface(
        painter,
        rect,
        r,
        color::surface(),
        it.hover,
        it.focus,
        it.press,
        3.0,
    );
    paint_hairline(painter, rect, r, color::hairline_card());
    let ink = Color32::WHITE;
    let pad = 26.0 * s;
    let tag_style = theme::TextStyle::new(15.0, theme::Weight::Bold);
    let name_style = text::scaled(ty::LANGUAGE_NAME, s);
    let name_h = name_style.line_height();
    text::paint(
        painter,
        pos2(rect.min.x + pad, rect.max.y - pad - name_h - 20.0 * s),
        Align2::LEFT_TOP,
        text::scaled(tag_style, s),
        language.tag(),
        fade(ink, 0.6),
    );
    text::paint(
        painter,
        pos2(rect.min.x + pad, rect.max.y - pad),
        Align2::LEFT_BOTTOM,
        name_style,
        name,
        ink,
    );
    it.response
}

/// Capsule dots of the onboarding: current 46 x 5, others 18 x 5; white 0.95 up to the current
/// step, else 0.22; widths animate with `Motion::Layout`.
pub fn onboarding_dots(ui: &mut Ui, steps: usize, current: usize) {
    let ctx = ui.ctx().clone();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for step in 0..steps {
            let w = ctx.animate_to(
                ("onboarding-dot", step),
                if step == current { 46.0 } else { 18.0 },
                Motion::Layout,
            );
            let (rect, _) = ui.allocate_exact_size(vec2(w, 5.0), Sense::hover());
            let c = if step <= current {
                white(0.95)
            } else {
                white(0.22)
            };
            paint_capsule(ui.painter(), rect, c);
        }
    });
}
