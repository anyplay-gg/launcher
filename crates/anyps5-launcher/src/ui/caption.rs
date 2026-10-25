//! The window's own caption (docs/DESIGN.md 9.2). The launcher runs without the Windows title bar,
//! as most launchers do:
//!
//! * a drag strip along the top edge moves the window through the system's move loop, so Snap
//!   keeps working; a double click maximises or restores;
//! * three caption buttons sit in the top-right corner (minimise, maximise or restore, close),
//!   hidden while a controller is in use unless the pointer comes near them;
//! * while the window is neither maximised nor fullscreen, a thin band along every edge resizes it.
//!
//! Fullscreen keeps only the buttons, shown while the pointer is near the top-right corner; there
//! the middle one leaves fullscreen. Sizes are the Windows 11 ones in points at zoom 1: the caption keeps
//! that size while the UI grows with a large window, and shrinks with the UI in a small one, so it
//! never reaches into the home screen's top bar (36 dp down).

use egui::viewport::ResizeDirection;
use egui::{
    Area, Context, CornerRadius, CursorIcon, Id, Order, Painter, PointerButton, Rect, Response,
    Sense, Stroke, ViewportCommand, pos2, vec2,
};

use crate::ui::input::{self, Device};
use crate::ui::theme::{color, fade, mix, size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Button {
    Minimize,
    Maximize,
    Close,
}

/// Draws the caption and runs what it is used for. Call once per frame, above the screens and
/// panels and below the intro curtain. Returns the fullscreen state the player switched to, for
/// the host to apply and remember.
pub fn show(ctx: &Context) -> Option<bool> {
    let (fullscreen, maximized) = ctx.input(|i| {
        let viewport = i.viewport();
        (
            viewport.fullscreen.unwrap_or(false),
            viewport.maximized.unwrap_or(false),
        )
    });
    let screen = ctx.viewport_rect();
    let unit = 1.0 / ctx.zoom_factor().max(1.0);
    let button = vec2(size::CAPTION_BUTTON.0, size::CAPTION_BUTTON.1) * unit;
    let buttons = Rect::from_min_max(
        pos2(screen.right() - 3.0 * button.x, screen.top()),
        pos2(screen.right(), screen.top() + button.y),
    );

    if fullscreen {
        // Only the buttons, and only for a pointer in the corner.
        let corner = buttons.expand2(vec2(button.x, button.y));
        let near = ctx
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| corner.contains(p));
        let visible = ctx.animate_bool_with_time(Id::unique("caption-visible"), near, 0.2);
        let clicked = (visible > 0.0)
            .then(|| caption_buttons(ctx, buttons, button.x, true, visible, unit))
            .flatten();
        return match clicked {
            Some(Button::Maximize) => Some(false),
            Some(other) => {
                run(ctx, other, true);
                None
            }
            None => None,
        };
    }

    let strip = Rect::from_min_max(screen.left_top(), pos2(buttons.left(), buttons.bottom()));
    let response = region(ctx, "caption-strip", strip, Sense::click_and_drag());
    if response.double_clicked() {
        ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if response.drag_started_by(PointerButton::Primary) && primary_down(ctx) {
        // Only while the button is still down: a slow frame can report a long press as a drag
        // after the release, and a move loop started then would swallow the next click.
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
    }

    let near = ctx
        .input(|i| i.pointer.hover_pos())
        .is_some_and(|p| p.y < screen.top() + 2.0 * button.y);
    let visible = ctx.animate_bool_with_time(
        Id::unique("caption-visible"),
        input::device(ctx) == Device::Keyboard || near,
        0.2,
    );
    if visible > 0.0
        && let Some(which) = caption_buttons(ctx, buttons, button.x, maximized, visible, unit)
    {
        run(ctx, which, maximized);
    }

    if !maximized {
        resize_edges(ctx, screen, size::RESIZE_EDGE * unit);
    }
    None
}

/// What a caption button does in a window (`maximized`: the middle one restores).
fn run(ctx: &Context, which: Button, maximized: bool) {
    match which {
        Button::Minimize => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
        Button::Maximize => ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized)),
        Button::Close => ctx.send_viewport_cmd(ViewportCommand::Close),
    }
}

/// A fixed area over `rect` that senses `sense`, kept above the other foreground layers.
fn region(ctx: &Context, id: &str, rect: Rect, sense: Sense) -> Response {
    let area = Area::new(Id::unique(id))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain(false)
        .show(ctx, |ui| ui.allocate_rect(rect, sense));
    ctx.move_to_top(area.response.layer_id);
    area.inner
}

fn caption_buttons(
    ctx: &Context,
    rect: Rect,
    width: f32,
    maximized: bool,
    visible: f32,
    unit: f32,
) -> Option<Button> {
    let area = Area::new(Id::unique("caption-buttons"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .movable(false)
        .constrain(false)
        .show(ctx, |ui| {
            let mut clicked = None;
            for (index, which) in [Button::Minimize, Button::Maximize, Button::Close]
                .into_iter()
                .enumerate()
            {
                let left = rect.left() + index as f32 * width;
                let cell =
                    Rect::from_min_max(pos2(left, rect.top()), pos2(left + width, rect.bottom()));
                let response = ui.allocate_rect(cell, Sense::click());
                let hover = ctx.animate_bool_with_time(
                    Id::unique(("caption-hover", index)),
                    crate::ui::input::hovered(ctx, &response),
                    0.1,
                );
                paint_button(ui.painter(), cell, which, maximized, hover, visible, unit);
                if response.clicked() {
                    clicked = Some(which);
                }
            }
            clicked
        });
    ctx.move_to_top(area.response.layer_id);
    area.inner
}

/// One button: its hover fill, then the glyph (white once hovered).
fn paint_button(
    painter: &Painter,
    cell: Rect,
    which: Button,
    maximized: bool,
    hover: f32,
    visible: f32,
    unit: f32,
) {
    let fill = match which {
        Button::Close => color::caption_close(),
        _ => color::caption_hover(),
    };
    if hover > 0.0 {
        painter.rect_filled(cell, CornerRadius::ZERO, fade(fill, hover * visible));
    }
    let glyph = mix(color::caption_glyph(), color::text(), hover);
    let stroke = Stroke::new(unit, fade(glyph, visible));
    let half = size::CAPTION_GLYPH * unit / 2.0;
    let c = cell.center();
    let c = pos2(
        painter.round_to_pixel_center(c.x),
        painter.round_to_pixel_center(c.y),
    );
    match which {
        Button::Minimize => {
            painter.line_segment([pos2(c.x - half, c.y), pos2(c.x + half, c.y)], stroke);
        }
        Button::Maximize if maximized => {
            // Restore: a square with a second one peeking out behind its top-right corner.
            let step = 2.0 * unit;
            let front = Rect::from_min_max(
                pos2(c.x - half, c.y - half + step),
                pos2(c.x + half - step, c.y + half),
            );
            painter.rect_stroke(
                front,
                CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line_segment(
                [
                    pos2(front.left() + step, front.top() - step),
                    pos2(c.x + half, front.top() - step),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    pos2(c.x + half, front.top() - step),
                    pos2(c.x + half, front.bottom() - step),
                ],
                stroke,
            );
        }
        Button::Maximize => {
            let square = Rect::from_center_size(c, vec2(2.0 * half, 2.0 * half));
            painter.rect_stroke(
                square,
                CornerRadius::same(1),
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Button::Close => {
            painter.line_segment(
                [pos2(c.x - half, c.y - half), pos2(c.x + half, c.y + half)],
                stroke,
            );
            painter.line_segment(
                [pos2(c.x - half, c.y + half), pos2(c.x + half, c.y - half)],
                stroke,
            );
        }
    }
}

/// The bands along the edges that resize the window, corners first so they win.
fn resize_edges(ctx: &Context, screen: Rect, edge: f32) {
    let corner = 2.0 * edge;
    let (l, r, t, b) = (screen.left(), screen.right(), screen.top(), screen.bottom());
    let bands = [
        (
            "resize-nw",
            Rect::from_min_max(pos2(l, t), pos2(l + corner, t + corner)),
            ResizeDirection::NorthWest,
        ),
        (
            "resize-ne",
            Rect::from_min_max(pos2(r - corner, t), pos2(r, t + corner)),
            ResizeDirection::NorthEast,
        ),
        (
            "resize-sw",
            Rect::from_min_max(pos2(l, b - corner), pos2(l + corner, b)),
            ResizeDirection::SouthWest,
        ),
        (
            "resize-se",
            Rect::from_min_max(pos2(r - corner, b - corner), pos2(r, b)),
            ResizeDirection::SouthEast,
        ),
        (
            "resize-n",
            Rect::from_min_max(pos2(l + corner, t), pos2(r - corner, t + edge)),
            ResizeDirection::North,
        ),
        (
            "resize-s",
            Rect::from_min_max(pos2(l + corner, b - edge), pos2(r - corner, b)),
            ResizeDirection::South,
        ),
        (
            "resize-w",
            Rect::from_min_max(pos2(l, t + corner), pos2(l + edge, b - corner)),
            ResizeDirection::West,
        ),
        (
            "resize-e",
            Rect::from_min_max(pos2(r - edge, t + corner), pos2(r, b - corner)),
            ResizeDirection::East,
        ),
    ];
    // Shown in reverse so the corners end up on top of the sides they overlap.
    for (id, rect, direction) in bands.into_iter().rev() {
        let response = region(ctx, id, rect, Sense::drag());
        if response.hovered() {
            ctx.set_cursor_icon(cursor(direction));
            // On the press, not after a drag threshold: the system takes the pointer from here.
            if ctx.input(|i| i.pointer.primary_pressed()) && primary_down(ctx) {
                ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
            }
        }
    }
}

fn primary_down(ctx: &Context) -> bool {
    ctx.input(|i| i.pointer.primary_down())
}

fn cursor(direction: ResizeDirection) -> CursorIcon {
    match direction {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    }
}
