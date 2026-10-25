//! Shared widgets (docs/DESIGN.md section 5 and 2.4): console buttons, icon buttons, switch,
//! progress lines, keycaps and the hint bar.
//!
//! All widgets are drawn with the painter and driven by the *model*: the screen passes
//! `focused` from its focus model (`ui::focus`); egui's own keyboard focus is not used. Each widget
//! animates its focus with `Motion::Focus` and shows hover (mouse only) and press (0.97) states.
//! A disabled widget is drawn at 40 % and takes no input.

use egui::emath::GuiRounding;
use egui::epaint::{Mesh, RectShape, Shadow};
use egui::{
    Align2, Color32, CornerRadius, Painter, Pos2, Rect, Response, Role, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2, WidgetInfo, pos2, vec2,
};

use crate::ui::anim::{self, AnimExt, Motion, lerp};
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{self, Device};
use crate::ui::text;
use crate::ui::theme::{TextStyle, black, color, fade, mix, radius, shadows, size, ty, white};

/// Forces a visual state so galleries and tests can show hover or press without a pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Force {
    Hover,
    Press,
}

/// The press scale of buttons (a mouse affordance added on Windows, DESIGN.md 9.2).
pub const PRESS_SCALE: f32 = 0.97;

/// Rectangle scaled about its centre.
pub fn scale_rect(rect: Rect, factor: f32) -> Rect {
    Rect::from_center_size(rect.center(), rect.size() * factor)
}

/// `rect` with its edges on physical pixels, so a scaled element never rests on a fractional edge.
pub fn snap_rect(painter: &Painter, rect: Rect) -> Rect {
    rect.round_to_pixels(painter.ctx().pixels_per_point())
}

/// One physical pixel in dp: the width of a crisp hairline.
pub fn hairline_width(painter: &Painter) -> f32 {
    1.0 / painter.ctx().pixels_per_point()
}

/// A stroke width rounded to whole physical pixels (at least one).
pub fn snap_width(painter: &Painter, width: f32) -> f32 {
    let ppp = painter.ctx().pixels_per_point();
    (width * ppp).round().max(1.0) / ppp
}

/// Converts a radius in dp to egui's integer corner radius.
pub fn corner(radius: f32) -> CornerRadius {
    CornerRadius::same(radius.round().clamp(0.0, 255.0) as u8)
}

/// Paints a shadow behind `rect`.
pub fn paint_shadow(painter: &Painter, rect: Rect, radius: f32, shadow: Shadow) {
    if shadow.color.a() > 0 {
        painter.add(shadow.as_shape(rect, corner(radius)));
    }
}

/// The glow of a focused element (white 0.45, SwiftUI radius 10) around its focus ring. Paint it
/// **before** the element so the element covers the glow's interior; then paint the element and
/// [`paint_focus_stroke`] on top.
pub fn paint_focus_glow(painter: &Painter, rect: Rect, radius: f32, width: f32, t: f32) {
    let t = t.clamp(0.0, 1.0);
    if t > 0.001 {
        paint_shadow(
            painter,
            rect.expand(width * 1.6),
            radius + width * 1.6,
            shadows::focus_glow(t),
        );
    }
}

/// The white ring of a focused element (`focusRing`): a stroke of `width` drawn outside the
/// element, outset by `width * 1.6` with the radius grown by the same amount so the ring stays
/// concentric with the element. `t` is the focus amount; paint after the element.
pub fn paint_focus_stroke(painter: &Painter, rect: Rect, radius: f32, width: f32, t: f32) {
    let t = t.clamp(0.0, 1.0);
    if t > 0.001 {
        // An edge bump leans the ring toward the wall (ui::nudge); only a ring in focus follows.
        let rect = rect.translate(crate::ui::nudge::offset(painter.ctx()) * t);
        let width = snap_width(painter, width);
        // A hairline of shade in the gap keeps the ring apart from a light fill and one outside it keeps
        // it visible on a light picture.
        painter.add(RectShape::stroke(
            snap_rect(painter, rect.expand(1.0)),
            corner(radius + 1.0),
            Stroke::new(snap_width(painter, 1.0), black(0.30 * t)),
            StrokeKind::Inside,
        ));
        let outer = width * 1.6 + 1.0;
        painter.add(RectShape::stroke(
            snap_rect(painter, rect.expand(outer)),
            corner(radius + outer),
            Stroke::new(snap_width(painter, 1.0), black(0.35 * t)),
            StrokeKind::Inside,
        ));
        painter.add(RectShape::stroke(
            snap_rect(painter, rect.expand(width * 1.6)),
            corner(radius + width * 1.6),
            Stroke::new(width, white(0.95 * t)),
            StrokeKind::Inside,
        ));
    }
}

/// 1 px hairline drawn inside the shape (SwiftUI `strokeBorder`).
pub fn paint_hairline(painter: &Painter, rect: Rect, radius: f32, color: Color32) {
    if color.a() > 0 {
        painter.add(RectShape::stroke(
            rect,
            corner(radius),
            Stroke::new(hairline_width(painter), color),
            StrokeKind::Inside,
        ));
    }
}

/// A chevron of `size` dp drawn as a line, centred on `centre`. The font's caret is hairline thin
/// next to the text weight of a row; this one is 1.75 dp wide and rests on whole device pixels.
pub fn paint_caret(painter: &Painter, centre: Pos2, right: bool, size: f32, color: Color32) {
    let ppp = painter.ctx().pixels_per_point();
    let centre = centre.round_to_pixels(ppp);
    let (h, w) = (size * 0.34, size * 0.2);
    let dir = if right { 1.0 } else { -1.0 };
    // The mass of a chevron sits on its open side; shift toward the tip to centre it optically.
    let shift = dir * w * 0.33;
    let points = vec![
        centre + vec2(shift - dir * w, -h),
        centre + vec2(shift + dir * w, 0.0),
        centre + vec2(shift - dir * w, h),
    ];
    painter.add(Shape::line(
        points,
        Stroke::new(snap_width(painter, 1.75), color),
    ));
}

/// A spinning three-quarter arc with round ends (the original's small `ProgressView`).
pub fn paint_spinner_arc(
    painter: &Painter,
    centre: Pos2,
    radius: f32,
    time: f64,
    width: f32,
    color: Color32,
) {
    const POINTS: usize = 36;
    let angle = (time * std::f64::consts::TAU * 1.1) as f32;
    let points: Vec<Pos2> = (0..=POINTS)
        .map(|i| {
            let a = angle + i as f32 / POINTS as f32 * std::f32::consts::PI * 1.5;
            centre + radius * vec2(a.cos(), a.sin())
        })
        .collect();
    let (first, last) = (points[0], points[POINTS]);
    painter.add(Shape::line(points, Stroke::new(width, color)));
    painter.circle_filled(first, width * 0.5, color);
    painter.circle_filled(last, width * 0.5, color);
}
/// Fills a capsule, or a circle when the rectangle is (nearly) square. Every fully round fill goes
/// through here, never through `rect_filled` with a half-size radius on a square.
///
/// Why a circle is drawn as a circle: egui builds a rounded rectangle from four arcs and drops the
/// duplicated vertex where two arcs meet only when the side is within `f32::EPSILON * size` of
/// `2 * radius`. A snapped button at x = 1400 dp is not that exact (width and height differ by a few
/// 1e-5 from rounding at that magnitude), so the 3 and 9 o'clock vertices stay doubled, their feather
/// normals degenerate, and a translucent fill shows a lighter seam from the edge to the centre.
/// `circle_filled` builds its path from the centre and has no such case.
pub fn paint_capsule(painter: &Painter, rect: Rect, color: Color32) {
    let (w, h) = (rect.width(), rect.height());
    if (w - h).abs() < 0.5 {
        painter.circle_filled(rect.center(), 0.5 * w.min(h), color);
    } else {
        // egui clamps the radius to half the shorter side: a true capsule at any height.
        painter.rect_filled(rect, CornerRadius::same(255), color);
    }
}

// ---------------------------------------------------------------------------------------------
// Shared interaction/animation state
// ---------------------------------------------------------------------------------------------

/// What a widget needs to draw itself this frame.
pub struct Interaction {
    pub response: Response,
    /// The widget rectangle at rest (before scale effects).
    pub rect: Rect,
    /// Focus amount 0..=1 (spring).
    pub focus: f32,
    /// Hover amount 0..=1 (ease, 120 ms): mouse only.
    pub hover: f32,
    /// Press amount 0..=1.
    pub press: f32,
    pub hovered: bool,
    pub enabled: bool,
}

/// Allocates `size`, reads the pointer state and runs the focus, hover and press animations. The
/// allocation never changes with focus: a focused widget does not push its neighbours (DESIGN.md
/// 3.9); focus is a ring drawn outside the widget.
pub fn interact(
    ui: &mut Ui,
    size: Vec2,
    focused: bool,
    enabled: bool,
    force: Option<Force>,
) -> Interaction {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let id = ui.next_auto_id();
    let focus = ui.ctx().focus_t((id, "focus"), focused && enabled);
    let (rect, response) = ui.allocate_exact_size(size, sense);
    debug_assert_eq!(response.id, id);
    crate::ui::a11y::follow(ui.ctx(), id, focused && enabled);
    // The pointer only counts while it is the device in use: with a gamepad, a resting mouse must
    // not leave a second control lit beside the focused one.
    let hovered = enabled
        && (input::hovered(ui.ctx(), &response)
            || force == Some(Force::Hover)
            || force == Some(Force::Press));
    // A select from a key or the pad holds the focused control pressed for a moment, like a click.
    let selected = focused && crate::ui::press::active(ui.ctx());
    let pressed = enabled
        && (response.is_pointer_button_down_on() || force == Some(Force::Press) || selected);
    let ctx = ui.ctx();
    let hover = ctx.animate_to(
        (response.id, "hover"),
        if hovered { 1.0 } else { 0.0 },
        Motion::Hover,
    );
    let press = ctx.animate_to(
        (response.id, "press"),
        if pressed { 1.0 } else { 0.0 },
        Motion::Press,
    );
    if hovered && enabled {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    Interaction {
        response,
        rect,
        focus,
        hover,
        press,
        hovered,
        enabled,
    }
}

impl Interaction {
    /// The press scale, for widgets that shrink under the pointer.
    pub fn press_scale(&self) -> f32 {
        lerp(1.0, PRESS_SCALE, self.press)
    }

    /// The widget's rectangle as it is painted: pressed in, on whole pixels.
    pub fn painted(&self, painter: &Painter) -> Rect {
        snap_rect(painter, scale_rect(self.rect, self.press_scale()))
    }

    /// Multiplier for the whole widget when disabled.
    pub fn alpha(&self) -> f32 {
        if self.enabled { 1.0 } else { 0.4 }
    }
}

// ---------------------------------------------------------------------------------------------
// The control family (DESIGN.md 3.9)
// ---------------------------------------------------------------------------------------------

/// Which button of the family: one primary per view, everything else secondary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// Solid ink on a dark label: the one action the view is about (Play, Accept).
    Primary,
    /// Translucent with a hairline: everything else.
    Secondary,
}

/// The three heights of the family. All are capsules; a glyph alone makes a circle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ControlSize {
    /// 54: the action row of the game page and dialogs.
    #[default]
    Large,
    /// 44: the top bar.
    Medium,
    /// 32: an answer that sits in a line of text.
    Small,
}

impl ControlSize {
    pub const fn height(self) -> f32 {
        match self {
            ControlSize::Large => size::CONTROL_L,
            ControlSize::Medium => size::CONTROL_M,
            ControlSize::Small => size::CONTROL_S,
        }
    }
    /// Space between the edge and the content of a button with text.
    pub const fn text_pad(self) -> f32 {
        match self {
            ControlSize::Large => 30.0,
            ControlSize::Medium => 20.0,
            ControlSize::Small => 14.0,
        }
    }
    /// The glyph: next to text, and on its own.
    pub const fn glyph(self, with_text: bool) -> f32 {
        match (self, with_text) {
            (ControlSize::Large, true) => size::BUTTON_ICON,
            (ControlSize::Large, false) => size::BUTTON_ICON_ONLY,
            (ControlSize::Medium, true) => 18.0,
            (ControlSize::Medium, false) => size::ICON_BUTTON_GLYPH,
            (ControlSize::Small, _) => size::CHIP_ICON,
        }
    }
    /// Between the glyph and the text.
    pub const fn gap(self) -> f32 {
        match self {
            ControlSize::Large => size::BUTTON_GAP,
            ControlSize::Medium => 8.0,
            ControlSize::Small => size::CHIP_GAP,
        }
    }
    pub const fn style(self) -> TextStyle {
        match self {
            ControlSize::Large => ty::BUTTON,
            ControlSize::Medium => ty::BUTTON_MEDIUM,
            ControlSize::Small => ty::CHIP,
        }
    }
}

/// Paints the body of a control (capsule fill, hairline, focus ring) for the animation state in
/// `it` into `rect` (already pressed in and on whole pixels) and returns the ink to draw on it.
///
/// - rest, hover (fill and hairline ease in over 120 ms), pressed (darker, 0.97 scale in `rect`),
///   focused (ring outside, fill one step lighter), disabled (40 %);
/// - no shadow and no glow: a translucent fill over a shadow shows a seam.
pub fn paint_control(
    painter: &Painter,
    rect: Rect,
    variant: Variant,
    it: &Interaction,
    destructive: bool,
) -> Color32 {
    let a = it.alpha();
    let radius = (rect.height() * 0.5).round();
    let lift = it.hover.max(it.focus);
    let (fill, ink) = match variant {
        Variant::Primary => (
            mix(
                mix(color::primary(), color::primary_hover(), lift),
                color::primary_pressed(),
                it.press,
            ),
            color::on_primary(),
        ),
        Variant::Secondary => {
            let rest = mix(color::surface(), color::surface_hover(), it.hover);
            let rest = mix(rest, color::surface_focus(), it.focus);
            (
                mix(rest, color::surface_pressed(), it.press),
                if destructive {
                    color::destructive_icon()
                } else {
                    Color32::WHITE
                },
            )
        }
    };
    paint_capsule(painter, rect, fade(fill, a));
    if variant == Variant::Secondary {
        paint_hairline(
            painter,
            rect,
            radius,
            fade(
                mix(color::hairline_control(), color::hairline_hover(), lift),
                a,
            ),
        );
    }
    // The primary has no hairline to brighten: under the pointer it draws the ring at 45 %, like a tile.
    let ring = if variant == Variant::Primary {
        it.focus.max(0.45 * it.hover)
    } else {
        it.focus
    };
    paint_focus_ring(painter, rect, radius, ring * a);
    ink
}

/// The focus ring of a control: white, 2 dp, outside the shape with a 1.2 dp gap. `t` is the focus
/// amount; paint after the element.
pub fn paint_focus_ring(painter: &Painter, rect: Rect, radius: f32, t: f32) {
    paint_focus_stroke(painter, rect, radius, size::FOCUS_RING, t);
}

/// White laid over a white-based `rest` colour by `amount`: the composite of the two layers as one
/// colour, so a hover or focus step never paints two translucent shapes with a seam between them.
pub fn lighten(rest: Color32, amount: f32) -> Color32 {
    let a = rest.a() as f32 / 255.0;
    white(1.0 - (1.0 - a) * (1.0 - amount.clamp(0.0, 1.0)))
}

/// How much white a container gains: under the pointer, with the focus, and pressed.
pub const LIFT_HOVER: f32 = 0.05;
pub const LIFT_FOCUS: f32 = 0.09;
pub const LIFT_PRESS: f32 = 0.06;

/// Fill and focus ring of a container that can be focused (a row, a card, a cell): `rest` is its
/// white-based fill at rest. Hover lifts it by 5 %, focus by 9 % and draws the ring (`ring` dp wide,
/// 0 for none); the press adds 6 %. The ring is outside the shape, so nothing grows or reflows.
#[allow(clippy::too_many_arguments)]
pub fn paint_surface(
    painter: &Painter,
    rect: Rect,
    radius: f32,
    rest: Color32,
    hover: f32,
    focus: f32,
    press: f32,
    ring: f32,
) {
    let amount = (LIFT_HOVER * hover).max(LIFT_FOCUS * focus) + LIFT_PRESS * press;
    if rest.a() > 0 || amount > 0.0 {
        painter.rect_filled(rect, corner(radius), lighten(rest, amount));
    }
    if ring > 0.0 {
        paint_focus_stroke(painter, rect, radius, ring, focus);
    }
}

/// One button of the family. Replaces the console button, the chip and the top-bar icon button.
pub struct ConsoleButton<'a> {
    title: &'a str,
    icon: Option<Icon>,
    icon_fill: bool,
    focused: bool,
    variant: Variant,
    size: ControlSize,
    destructive: bool,
    tabular: bool,
    min_width: f32,
    enabled: bool,
    force: Option<Force>,
    /// Accessible name when the button shows no text.
    label: Option<String>,
}

impl<'a> ConsoleButton<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            label: None,
            icon: None,
            icon_fill: false,
            focused: false,
            variant: Variant::Secondary,
            size: ControlSize::Large,
            destructive: false,
            tabular: false,
            min_width: 0.0,
            enabled: true,
            force: None,
        }
    }

    /// A button with only a glyph: a circle. `label` is its accessible name.
    pub fn icon_only(icon: Icon, label: impl Into<String>) -> ConsoleButton<'static> {
        let mut button = ConsoleButton::new("").icon(icon);
        button.label = Some(label.into());
        button
    }

    /// The name assistive technology reads when it differs from the visible text.
    pub fn accessible_name(mut self, name: impl Into<String>) -> Self {
        self.label = Some(name.into());
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Use the fill weight of the glyph (play, lock, trophy, ...).
    pub fn icon_fill(mut self, fill: bool) -> Self {
        self.icon_fill = fill;
        self
    }
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
    /// The primary action of the view: ink fill, dark label. One per view.
    pub fn prominent(mut self, prominent: bool) -> Self {
        self.variant = if prominent {
            Variant::Primary
        } else {
            Variant::Secondary
        };
        self
    }
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }
    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }
    /// Digits of equal width, for a count that changes.
    pub fn tabular(mut self, tabular: bool) -> Self {
        self.tabular = tabular;
        self
    }
    pub fn min_width(mut self, width: f32) -> Self {
        self.min_width = width;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn force(mut self, force: Option<Force>) -> Self {
        self.force = force;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let painter = ui.painter().clone();
        let style = self.size.style();
        let has_text = !self.title.is_empty();
        let height = self.size.height();
        let icon_width = match self.icon {
            Some(_) => self.size.glyph(has_text),
            None => 0.0,
        };
        let gap = if self.icon.is_some() && has_text {
            self.size.gap()
        } else {
            0.0
        };
        let text_width = match (has_text, self.tabular) {
            (false, _) => 0.0,
            (true, true) => text::tabular_width(&painter, style, self.title),
            (true, false) => text::width(&painter, style, self.title),
        };
        let content = icon_width + gap + text_width;
        let width = if has_text {
            (self.size.text_pad() * 2.0 + content).max(self.min_width)
        } else {
            height.max(self.min_width)
        };

        let it = interact(
            ui,
            vec2(width, height.max(style.line_height())),
            self.focused,
            self.enabled,
            self.force,
        );
        let enabled = self.enabled;
        let name = match (&self.label, has_text) {
            (Some(label), _) => label.as_str(),
            (None, true) => self.title,
            (None, false) => "",
        };
        it.response
            .widget_info(|| WidgetInfo::labeled(Role::Button, enabled, name));
        if !ui.is_rect_visible(it.rect) {
            return it.response;
        }
        let rect = it.painted(&painter);
        let ink = paint_control(&painter, rect, self.variant, &it, self.destructive);
        let ink = fade(ink, it.alpha());

        // Content, centred.
        let mut x = rect.center().x - content * 0.5;
        if let Some(icon) = self.icon {
            let centre = pos2(
                x + icon_width * 0.5 + icon.optical_dx(icon_width),
                rect.center().y,
            );
            if icon == Icon::DOTS_THREE && !has_text {
                // The font's dots are hairline-thin at button size: three round dots instead.
                let k = height / size::CONTROL_L;
                for d in [-1.0, 0.0, 1.0] {
                    painter.circle_filled(centre + vec2(d * 7.0 * k, 0.0), 2.2 * k, ink);
                }
            } else if icon == Icon::CARET_LEFT || icon == Icon::CARET_RIGHT {
                paint_caret(&painter, centre, icon == Icon::CARET_RIGHT, icon_width, ink);
            } else {
                painter.text(
                    centre,
                    Align2::CENTER_CENTER,
                    icon.glyph(),
                    icon.font(text::quantize_size(icon_width), self.icon_fill),
                    ink,
                );
            }
            x += icon_width + gap;
        }
        if has_text {
            if self.tabular {
                let top = pos2(x, rect.center().y - style.line_height() * 0.5);
                text::paint_tabular(&painter, top, style, self.title, ink);
            } else {
                text::paint(
                    &painter,
                    pos2(x, rect.center().y),
                    Align2::LEFT_CENTER,
                    style,
                    self.title,
                    ink,
                );
            }
        }
        it.response
    }
}

/// A button with only a glyph in the top bar: a 44 dp circle (library, settings).
pub struct IconButton {
    icon: Icon,
    label: String,
    fill: bool,
    focused: bool,
    enabled: bool,
    force: Option<Force>,
}

impl IconButton {
    /// `label` is the accessible name (the glyph carries no text).
    pub fn new(icon: Icon, label: impl Into<String>) -> Self {
        Self {
            icon,
            label: label.into(),
            fill: false,
            focused: false,
            enabled: true,
            force: None,
        }
    }
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
    pub fn fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn force(mut self, force: Option<Force>) -> Self {
        self.force = force;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        ConsoleButton::icon_only(self.icon, self.label)
            .size(ControlSize::Medium)
            .icon_fill(self.fill)
            .focused(self.focused)
            .enabled(self.enabled)
            .force(self.force)
            .show(ui)
    }
}

// ---------------------------------------------------------------------------------------------
// Switch
// ---------------------------------------------------------------------------------------------

/// The 50 x 28 on/off switch. The caller flips its state when the response is clicked.
pub struct Switch {
    on: bool,
    accent: Color32,
    /// Focus amount of the row the switch sits in (0..=1): the off track steps up to white 0.30.
    focus: f32,
    label: String,
    enabled: bool,
    /// Expose the switch to assistive technology (off when the row already does).
    announce: bool,
}

impl Switch {
    pub fn new(on: bool, accent: Color32) -> Self {
        Self {
            on,
            accent,
            focus: 0.0,
            label: String::new(),
            enabled: true,
            announce: true,
        }
    }
    /// The row is focused: the off track steps up to white 0.30.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focus = if focused { 1.0 } else { 0.0 };
        self
    }
    /// Animated focus amount of the surrounding row (0..=1).
    pub fn focus_amount(mut self, focus: f32) -> Self {
        self.focus = focus.clamp(0.0, 1.0);
        self
    }
    /// Accessible name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }
    /// A disabled switch is drawn at 40 % and takes no input.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    /// Whether the switch is its own accessibility node (default). A row that already announces
    /// its toggle state turns it off so screen readers do not read it twice.
    pub fn announce(mut self, announce: bool) -> Self {
        self.announce = announce;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(vec2(size::SWITCH.0, size::SWITCH.1), sense);
        let (on, label, enabled) = (self.on, self.label.as_str(), self.enabled);
        if self.announce {
            response.widget_info(|| WidgetInfo::selected(Role::Switch, enabled, on, label));
            // A switch of its own is what the ring sits on; inside a row the row follows.
            crate::ui::a11y::follow(ui.ctx(), response.id, self.focus >= 1.0 && enabled);
        }
        let hovered = self.enabled && input::hovered(ui.ctx(), &response);
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let t = ui.ctx().flag_t((response.id, "on"), self.on, Motion::Focus);
        let rest = if hovered {
            white(0.24)
        } else {
            color::switch_track_off()
        };
        let off = mix(rest, color::switch_track_off_on_focus(), self.focus);
        let a = if self.enabled { 1.0 } else { 0.4 };
        let painter = ui.painter();
        paint_capsule(painter, rect, fade(mix(off, self.accent, t), a));
        let knob = size::SWITCH.1 - 2.0 * size::SWITCH_INSET;
        let travel = size::SWITCH.0 - 2.0 * size::SWITCH_INSET - knob;
        let centre = pos2(
            rect.min.x + size::SWITCH_INSET + knob * 0.5 + travel * t,
            rect.center().y,
        );
        let knob_rect = Rect::from_center_size(centre, Vec2::splat(knob));
        if self.enabled {
            paint_shadow(painter, knob_rect, knob * 0.5, shadows::switch_knob());
        }
        // White on the off track; on the accent the knob turns dark, because white on mint or gold
        // is 1.5:1 and the knob is what shows the state (DESIGN.md 3.10).
        let knob_ink = mix(Color32::WHITE, color::on_primary(), t);
        painter.circle_filled(centre, knob * 0.5, fade(knob_ink, a));
        response
    }
}

// ---------------------------------------------------------------------------------------------
// Progress
// ---------------------------------------------------------------------------------------------

/// A thin capsule progress bar; the fill width eases with easeOut 0.35 s per change.
pub struct ProgressLine {
    fraction: f32,
    color: Color32,
    height: f32,
    width: Option<f32>,
    label: String,
}

impl ProgressLine {
    pub fn new(fraction: f32, color: Color32) -> Self {
        Self {
            fraction,
            color,
            height: size::PROGRESS_HEIGHT,
            width: None,
            label: String::new(),
        }
    }
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
    /// Fixed width; otherwise the bar fills the available width.
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    /// Accessible name (the current step, for example).
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let (rect, response) = ui.allocate_exact_size(vec2(width, self.height), Sense::hover());
        let fraction = self.fraction.clamp(0.0, 1.0);
        response.widget_info(|| {
            let mut info = WidgetInfo::labeled(Role::ProgressIndicator, true, self.label.as_str());
            info.value = Some(f64::from(fraction));
            info
        });
        if ui.is_rect_visible(rect) {
            let eased = ui
                .ctx()
                .animate_to((response.id, "fill"), fraction, Motion::Progress);
            paint_progress(ui.painter(), rect, eased, self.color);
        }
        response
    }
}

/// Track plus fill (with the accent glow) for an already eased fraction.
pub fn paint_progress(painter: &Painter, rect: Rect, fraction: f32, accent: Color32) {
    paint_capsule(painter, rect, color::track());
    let fill_width = (rect.width() * fraction.clamp(0.0, 1.0)).max(rect.height());
    let fill = Rect::from_min_size(rect.min, vec2(fill_width, rect.height()));
    paint_shadow(
        painter,
        fill,
        rect.height() * 0.5,
        shadows::progress_glow(accent),
    );
    paint_capsule(painter, fill, accent);
}

/// An endless sweep for steps of unknown length: a 35 %-wide gradient (clear, accent, clear)
/// slides from -0.4 w to 1.05 w every 1.3 s, clipped to the capsule.
pub struct IndeterminateLine {
    color: Color32,
    height: f32,
    width: Option<f32>,
}

impl IndeterminateLine {
    pub fn new(color: Color32) -> Self {
        Self {
            color,
            height: size::PROGRESS_HEIGHT,
            width: None,
        }
    }
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let width = self.width.unwrap_or_else(|| ui.available_width());
        let (rect, response) = ui.allocate_exact_size(vec2(width, self.height), Sense::hover());
        response.widget_info(|| WidgetInfo::new(Role::ProgressIndicator));
        if ui.is_rect_visible(rect) {
            let time = ui.ctx().clock();
            paint_indeterminate(ui.painter(), rect, time, self.color);
            ui.ctx().request_repaint();
        }
        response
    }
}

/// Paints the indeterminate bar for clock time `time`.
pub fn paint_indeterminate(painter: &Painter, rect: Rect, time: f64, accent: Color32) {
    paint_capsule(painter, rect, color::track());
    let phase = anim::indeterminate_phase(time);
    let w = rect.width();
    let x0 = rect.min.x + w * phase;
    let (x1, mid) = (x0 + w * 0.35, x0 + w * 0.175);
    let (lo, hi) = (x0.max(rect.min.x), x1.min(rect.max.x));
    if hi <= lo {
        return;
    }
    let accent = accent.to_srgba_unmultiplied();
    let accent = Color32::from_rgba_unmultiplied(accent[0], accent[1], accent[2], 255);
    // Columns at the band's corners and along both capsule ends; the half height of the capsule
    // at a column is the band's extent there, so the band follows the rounded ends.
    let r = (rect.height() * 0.5).min(w * 0.5);
    let cy = rect.center().y;
    const CAP_STEPS: usize = 6;
    let mut xs = [0.0f32; 3 + 2 * (CAP_STEPS + 1)];
    xs[0] = lo;
    xs[1] = hi;
    xs[2] = mid.clamp(lo, hi);
    for k in 0..=CAP_STEPS {
        let d = r * k as f32 / CAP_STEPS as f32;
        xs[3 + 2 * k] = (rect.min.x + d).clamp(lo, hi);
        xs[4 + 2 * k] = (rect.max.x - d).clamp(lo, hi);
    }
    xs.sort_by(f32::total_cmp);
    let feather = hairline_width(painter);
    let mut mesh = Mesh::default();
    let mut last = f32::NEG_INFINITY;
    for &x in &xs {
        if x - last < 0.01 {
            continue;
        }
        last = x;
        let into_cap = (rect.min.x + r - x).max(x - (rect.max.x - r)).max(0.0);
        let half = (r * r - into_cap * into_cap).max(0.0).sqrt();
        let profile = if x <= mid {
            (x - x0) / (mid - x0)
        } else {
            (x1 - x) / (x1 - mid)
        };
        let solid = accent.gamma_multiply(profile.clamp(0.0, 1.0));
        let (inner, outer) = ((half - feather * 0.5).max(0.0), half + feather * 0.5);
        let base = mesh.vertices.len() as u32;
        for (dy, c) in [
            (-outer, Color32::TRANSPARENT),
            (-inner, solid),
            (inner, solid),
            (outer, Color32::TRANSPARENT),
        ] {
            mesh.colored_vertex(pos2(x, cy + dy), c);
        }
        if base > 0 {
            for row in 0..3 {
                let (a, b) = (base - 4 + row, base + row);
                mesh.add_triangle(a, a + 1, b);
                mesh.add_triangle(a + 1, b + 1, b);
            }
        }
    }
    painter.add(Shape::mesh(mesh));
}

// ---------------------------------------------------------------------------------------------
// Keycaps, hints, controller glyphs
// ---------------------------------------------------------------------------------------------

/// The controller face buttons, painted as vector shapes (the font glyphs are not relied on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadGlyph {
    Cross,
    Circle,
    Triangle,
    Square,
}

impl PadGlyph {
    /// The text characters the original uses for them.
    pub fn from_char(c: char) -> Option<PadGlyph> {
        match c {
            '✕' => Some(PadGlyph::Cross),
            '○' => Some(PadGlyph::Circle),
            '△' => Some(PadGlyph::Triangle),
            '□' => Some(PadGlyph::Square),
            _ => None,
        }
    }

    /// Paints the glyph centred at `centre`, fitting a `size` x `size` box, strokes of 1.5 dp.
    pub fn paint(self, painter: &Painter, centre: Pos2, size: f32, color: Color32) {
        let stroke = Stroke::new(1.5, color);
        let h = size * 0.5;
        match self {
            PadGlyph::Cross => {
                // Two diagonals cover less ink than a ring of the same width: draw them heavier.
                let stroke = Stroke::new(1.8, color);
                let d = h * 0.78;
                painter.line_segment([centre + vec2(-d, -d), centre + vec2(d, d)], stroke);
                painter.line_segment([centre + vec2(-d, d), centre + vec2(d, -d)], stroke);
            }
            PadGlyph::Circle => {
                painter.circle_stroke(centre, h * 0.85, stroke);
            }
            PadGlyph::Triangle => {
                let r = h * 0.95;
                // The bounding box of a triangle sits high; lower it so the mass is centred.
                let centre = centre + vec2(0.0, r * 0.12);
                let points = vec![
                    centre + vec2(0.0, -r),
                    centre + vec2(r * 0.9, r * 0.7),
                    centre + vec2(-r * 0.9, r * 0.7),
                ];
                painter.add(Shape::closed_line(points, stroke));
            }
            PadGlyph::Square => {
                let d = h * 0.75;
                painter.rect_stroke(
                    Rect::from_center_size(centre, Vec2::splat(d * 2.0)),
                    corner(1.0),
                    stroke,
                    StrokeKind::Middle,
                );
            }
        }
    }
}

/// Paints a key string such as `↩ / ✕` in a role: text runs as text, controller glyphs as shapes.
/// Returns the width used. `origin` is the left-centre of the string.
pub fn paint_key_string(
    painter: &Painter,
    origin: Pos2,
    style: TextStyle,
    keys: &str,
    color: Color32,
) -> f32 {
    let mut x = origin.x;
    let mut run = String::new();
    let flush = |run: &mut String, x: &mut f32| {
        if !run.is_empty() {
            let rect = text::paint(
                painter,
                pos2(*x, origin.y),
                Align2::LEFT_CENTER,
                style,
                run,
                color,
            );
            *x += rect.width();
            run.clear();
        }
    };
    for c in keys.chars() {
        if let Some(glyph) = PadGlyph::from_char(c) {
            flush(&mut run, &mut x);
            let side = style.size * 0.95;
            glyph.paint(painter, pos2(x + side * 0.5, origin.y), side, color);
            x += side;
        } else {
            run.push(c);
        }
    }
    flush(&mut run, &mut x);
    x - origin.x
}

/// Width of a key string (see [`paint_key_string`]).
pub fn key_string_width(painter: &Painter, style: TextStyle, keys: &str) -> f32 {
    let mut width = 0.0;
    let mut run = String::new();
    for c in keys.chars() {
        if PadGlyph::from_char(c).is_some() {
            if !run.is_empty() {
                width += text::width(painter, style, &run);
                run.clear();
            }
            width += style.size * 0.95;
        } else {
            run.push(c);
        }
    }
    if !run.is_empty() {
        width += text::width(painter, style, &run);
    }
    width
}

/// What a hint stands for. Its key follows the device in use (DESIGN.md 3.7): a keyboard shows
/// the key name, a gamepad the face button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HintAction {
    Select,
    Back,
    Options,
}

impl HintAction {
    fn key(self) -> &'static str {
        match self {
            HintAction::Select => "Enter",
            HintAction::Back => "Esc",
            HintAction::Options => "O",
        }
    }

    fn glyph(self) -> PadGlyph {
        match self {
            HintAction::Select => PadGlyph::Cross,
            HintAction::Back => PadGlyph::Circle,
            HintAction::Options => PadGlyph::Triangle,
        }
    }
}

/// One hint: the key of an action and what it does ("Enter Select", "○ Back").
pub struct Hint<'a> {
    pub action: HintAction,
    pub label: &'a str,
}

impl Hint<'_> {
    /// Width of the key: a circle for a face button, a keycap for a key name.
    fn cap_width(painter: &Painter, action: HintAction, device: Device) -> f32 {
        match device {
            Device::Gamepad => size::HINT_CAP,
            Device::Keyboard => (text::width(painter, ty::HINT_KEY, action.key())
                + 2.0 * size::HINT_CAP_PADDING)
                .max(size::HINT_CAP),
        }
    }

    pub fn width(&self, painter: &Painter, device: Device) -> f32 {
        Self::cap_width(painter, self.action, device)
            + size::HINT_GAP
            + text::width(painter, ty::HINT_LABEL, self.label)
    }

    pub fn show(&self, ui: &mut Ui) -> Response {
        let painter = ui.painter().clone();
        let device = input::device(ui.ctx());
        let cap_w = Self::cap_width(&painter, self.action, device);
        let label = text::layout(
            &painter,
            ty::HINT_LABEL,
            self.label,
            Color32::PLACEHOLDER,
            f32::INFINITY,
        );
        let height = size::HINT_CAP.max(label.size().y);
        let (rect, response) = ui.allocate_exact_size(
            vec2(cap_w + size::HINT_GAP + label.size().x, height),
            Sense::hover(),
        );
        if ui.is_rect_visible(rect) {
            let cap = Rect::from_min_size(
                pos2(rect.min.x, rect.center().y - size::HINT_CAP * 0.5),
                vec2(cap_w, size::HINT_CAP),
            );
            match device {
                Device::Gamepad => {
                    painter.circle_filled(cap.center(), size::HINT_CAP * 0.5, color::keycap());
                    self.action.glyph().paint(
                        &painter,
                        cap.center(),
                        size::HINT_GLYPH,
                        color::text_hint_key(),
                    );
                }
                Device::Keyboard => {
                    painter.rect_filled(cap, corner(radius::KEYCAP_HINT), color::keycap());
                    text::paint(
                        &painter,
                        cap.center(),
                        Align2::CENTER_CENTER,
                        ty::HINT_KEY,
                        self.action.key(),
                        color::text_hint_key(),
                    );
                }
            }
            painter.galley(
                pos2(
                    cap.max.x + size::HINT_GAP,
                    rect.center().y - label.size().y * 0.5,
                ),
                label,
                color::text_hint(),
            );
        }
        response
    }
}

fn hint_list(language: Language, options: bool) -> Vec<Hint<'static>> {
    let mut hints = vec![
        Hint {
            action: HintAction::Select,
            label: language.text(Key::HintSelect),
        },
        Hint {
            action: HintAction::Back,
            label: language.text(Key::HintBack),
        },
    ];
    if options {
        hints.push(Hint {
            action: HintAction::Options,
            label: language.text(Key::HintOptions),
        });
    }
    hints
}

/// The hint bar: Select, Back and, optionally, Options, in the keys of the device in use.
/// Moving needs no hint.
pub fn hint_bar(ui: &mut Ui, language: Language, options: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = size::HINT_BAR_GAP;
        for hint in hint_list(language, options) {
            hint.show(ui);
        }
    });
}

/// Width and height of [`hint_bar`], for screens that align it to the right.
pub fn hint_bar_size(painter: &Painter, language: Language, options: bool) -> (f32, f32) {
    let device = input::device(painter.ctx());
    let hints = hint_list(language, options);
    let width = hints.iter().map(|h| h.width(painter, device)).sum::<f32>()
        + size::HINT_BAR_GAP * hints.len().saturating_sub(1) as f32;
    (width, size::HINT_CAP.max(ty::HINT_LABEL.line_height()))
}

/// A key cap of the controls panel: 30 x 28 minimum, radius 7, 13.5 semibold, 1 px stroke at 0.18.
/// `keyboard` keys fill at 0.12, mouse and wheel sources at 0.06; `focus` (the row's focus amount) is kept for callers: the ink no longer inverts.
pub fn key_cap(ui: &mut Ui, label: &str, keyboard: bool, _focus: f32) -> Response {
    let painter = ui.painter().clone();
    let galley = text::layout(
        &painter,
        ty::KEY_CAP,
        label,
        Color32::PLACEHOLDER,
        f32::INFINITY,
    );
    let width = (galley.size().x + 2.0 * size::KEYCAP_PADDING_X).max(size::KEYCAP_MIN.0);
    let height = size::KEYCAP_MIN.1.max(galley.size().y);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    if ui.is_rect_visible(rect) {
        let ink = Color32::WHITE;
        let fill = fade(ink, if keyboard { 0.12 } else { 0.06 });
        painter.rect_filled(rect, corner(radius::KEYCAP), fill);
        paint_hairline(&painter, rect, radius::KEYCAP, fade(ink, 0.18));
        painter.galley(rect.center() - galley.size() * 0.5, galley, ink);
    }
    response
}

/// Convenience: fills `rect` with black alpha, used for scrims.
pub fn paint_scrim(painter: &Painter, rect: Rect, alpha: f32) {
    painter.rect_filled(rect, 0.0, black(alpha));
}
