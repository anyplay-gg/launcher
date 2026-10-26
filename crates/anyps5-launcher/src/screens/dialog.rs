//! The confirmation card (docs/DESIGN.md 5.9.1): a question that stands alone, a card in the middle
//! of a dimmed page, like a console's dialog. Uninstall, restoring a backup, closing the running
//! game, the missing toolkit release and every question of the online area share it, so they look
//! and answer the same way.
//!
//! The card holds a title, the sentence that says what the answer does and one to three buttons.
//! The buttons come from the same [`Row`]s the panels already build (a command row is a button), in
//! the order SYSTEM.md 3 gives: the primary or destructive answer first, `Cancel` last. The host
//! opens the focus on the safe answer. Left and Up go to the previous button, Right and Down to
//! the next; Select runs the focused one; Back cancels. A click on the dimmed page does nothing,
//! so nothing is answered by accident.

use std::sync::Arc;

use egui::{Area, Context, Galley, Id, Order, Rect, UiBuilder, pos2, vec2};

use crate::screens::rows::{self, Driven, ListEvent, Row};
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::components::{ConsoleButton, ControlSize, Variant, paint_scrim, paint_shadow};
use crate::ui::focus::{PanelEffect, PanelNav};
use crate::ui::glass::{GlassStyle, paint_glass};
use crate::ui::i18n::Language;
use crate::ui::input::Command;
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{self, TextStyle, color, radius, shadows, size, space, ty};

/// The card's width; it narrows to the screen minus a margin on a small window.
const CARD_WIDTH: f32 = 560.0;
/// How dark the page behind the card is at rest.
const SCRIM: f32 = 0.55;
/// The room between the card's edge and its content.
const CARD_PADDING: f32 = space::XL;
/// The card rises this far while it opens (a transform, not a reflow); none under "Reduced".
const RISE: f32 = 12.0;
/// The title is a panel title; the sentence is the question's body size with room between lines.
const TITLE: TextStyle = ty::PANEL_TITLE;
const MESSAGE: TextStyle = ty::QUESTION;
const MESSAGE_LEADING: f32 = 1.45;
const NOTE: TextStyle = ty::NOTE;

/// Row ids that answer "no": they are secondary and never the primary action.
const SAFE_ANSWERS: [&str; 3] = ["cancel", "confirmCancel", "keep"];

/// What a card shows.
pub struct Card<'a> {
    /// The animation identity of the card.
    pub id: &'static str,
    pub title: &'a str,
    pub message: Option<&'a str>,
    pub language: Language,
    /// The focused button (an index into the rows), from the host's navigation.
    pub focus: usize,
    /// Shows a hover or press state without a pointer (galleries).
    pub force: Option<(usize, crate::ui::components::Force)>,
}

/// The buttons of a card: its command rows. The first answer that is neither destructive nor a
/// refusal is the primary one, and only when no answer is destructive.
fn kinds(rows: &[Row]) -> Vec<Variant> {
    let danger = rows.iter().any(|r| r.destructive);
    let mut primary_given = danger;
    rows.iter()
        .map(|r| {
            if !primary_given && !r.destructive && !SAFE_ANSWERS.contains(&r.id) {
                primary_given = true;
                Variant::Primary
            } else {
                Variant::Secondary
            }
        })
        .collect()
}

/// The width a button needs for its label (and glyph).
fn needed(painter: &egui::Painter, row: &Row) -> f32 {
    let large = ControlSize::Large;
    let glyph = match row.icon {
        Some(_) => large.glyph(true) + large.gap(),
        None => 0.0,
    };
    large.text_pad() * 2.0 + glyph + text::width(painter, large.style(), &row.title)
}

fn message_galley(painter: &egui::Painter, text: &str, width: f32) -> Arc<Galley> {
    text::layout_font_pretty(
        painter,
        MESSAGE.font(),
        MESSAGE.size * MESSAGE_LEADING,
        MESSAGE.tracking,
        text,
        color::text_label(),
        width,
    )
}

/// Moves the focus between the buttons and hands everything else to the rows' own navigation
/// (Select runs a button, Back closes the card).
pub fn drive(shell: &mut Shell, nav: &mut PanelNav, rows: &[Row], commands: &[Command]) -> Driven {
    let mut out = Driven::default();
    for &command in commands {
        let last = rows.len().saturating_sub(1);
        nav.row = nav.row.min(last);
        let target = match command {
            Command::Left | Command::Up => Some(nav.row.checked_sub(1)),
            Command::Right | Command::Down => Some((nav.row < last).then_some(nav.row + 1)),
            _ => None,
        };
        match target {
            Some(Some(row)) => {
                nav.row = row;
                shell.play(Cue::Move);
            }
            Some(None) => shell.play(Cue::Bump),
            None => {
                let driven = rows::drive(shell, nav, rows, &[command]);
                out.changes.extend(driven.changes);
                out.close |= driven.close;
            }
        }
    }
    out
}

/// Draws the card over the dimmed page, or its exit. Returns the mouse's events as the rows' list
/// would (a click focuses and runs a button); `None` once it is fully closed.
pub fn draw(ctx: &Context, open: bool, card: &Card<'_>, rows: &[Row]) -> Option<Vec<ListEvent>> {
    let id = Id::unique(("dialog", card.id));
    let v = ctx.animate_from_to(
        (id, "open"),
        0.0,
        if open { 1.0 } else { 0.0 },
        Motion::Overlay,
    );
    if v <= 0.001 && !open {
        return None;
    }
    let v = v.clamp(0.0, 1.0);
    let screen = ctx.content_rect();
    let mut events = Vec::new();
    Area::new(id.with("area"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .interactable(true)
        .show(ctx, |ui| {
            // Covers the screen: the dimmed page takes the clicks, so nothing behind it is hit.
            ui.expand_to_include_rect(screen);
            let painter = ui.painter().clone();
            paint_scrim(&painter, screen, SCRIM * v);

            let width = CARD_WIDTH.min(screen.width() - 2.0 * space::XL);
            let inner = width - 2.0 * CARD_PADDING;
            let title = text::layout(&painter, TITLE, card.title, color::text(), inner);
            let message = card.message.map(|m| message_galley(&painter, m, inner));
            // A button that carries a state (a download under way) says it in a quiet line.
            let note = rows
                .iter()
                .find_map(|r| r.detail.as_deref())
                .map(|n| text::layout(&painter, NOTE, n, color::text_detail(), inner));
            // One row of buttons when they fit with a gap between, else one under another.
            let buttons = ControlSize::Large.height();
            let gap = size::CONTROL_GAP;
            let wants: Vec<f32> = rows.iter().map(|r| needed(&painter, r)).collect();
            let count = wants.len().max(1) as f32;
            let in_a_row = wants.iter().sum::<f32>() + gap * (count - 1.0) <= inner;
            let buttons_height = if in_a_row {
                buttons
            } else {
                buttons * count + gap * (count - 1.0)
            };
            let mut height = CARD_PADDING * 2.0 + title.size().y + buttons_height + space::XL;
            if let Some(message) = &message {
                height += space::S + message.size().y;
            }
            if let Some(note) = &note {
                height += space::S + note.size().y;
            }
            let rise = if ctx.reduced_motion() {
                0.0
            } else {
                (1.0 - v) * RISE
            };
            let rect =
                Rect::from_center_size(screen.center() + vec2(0.0, rise), vec2(width, height));

            let faded = |c: egui::Color32| theme::fade(c, v);
            paint_shadow(
                &painter,
                rect,
                radius::LARGE,
                egui::epaint::Shadow {
                    color: faded(shadows::toast().color),
                    ..shadows::toast()
                },
            );
            let mut style = GlassStyle::glass(radius::LARGE, 0.6);
            style.black *= v;
            style.alpha = v;
            paint_glass(ctx, &painter, rect, style);

            let mut y = rect.min.y + CARD_PADDING;
            let x = rect.min.x + CARD_PADDING;
            let title_h = title.size().y;
            ui.interact(
                Rect::from_min_size(pos2(x, y), vec2(inner, title_h)),
                id.with("title"),
                egui::Sense::hover(),
            )
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::accesskit::Role::Heading, true, title.text())
            });
            painter.galley(pos2(x, y), title, faded(color::text()));
            y += title_h;
            if let Some(message) = message {
                y += space::S;
                let h = message.size().y;
                ui.interact(
                    Rect::from_min_size(pos2(x, y), vec2(inner, h)),
                    id.with("message"),
                    egui::Sense::hover(),
                )
                .widget_info(|| {
                    egui::WidgetInfo::labeled(egui::accesskit::Role::Label, true, message.text())
                });
                painter.galley(pos2(x, y), message, faded(color::text_label()));
                y += h;
            }
            if let Some(note) = note {
                y += space::S;
                let h = note.size().y;
                painter.galley(pos2(x, y), note, faded(color::text_detail()));
                y += h;
            }
            y += space::XL;

            let variants = kinds(rows);
            let extra = if in_a_row {
                (inner - wants.iter().sum::<f32>() - gap * (count - 1.0)) / count
            } else {
                0.0
            };
            let mut at = pos2(x, y);
            let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
            child.set_opacity(v);
            for (i, row) in rows.iter().enumerate() {
                let w = if in_a_row { wants[i] + extra } else { inner };
                let slot = Rect::from_min_size(at, vec2(w, buttons));
                let mut cell = child.new_child(
                    UiBuilder::new()
                        .max_rect(slot)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let force = card.force.filter(|(f, _)| *f == i).map(|(_, f)| f);
                let mut button = ConsoleButton::new(&row.title)
                    .variant(variants[i])
                    .destructive(row.destructive)
                    .focused(open && card.focus == i)
                    .min_width(w)
                    .force(force);
                if let Some(icon) = row.icon {
                    button = button.icon(icon);
                }
                if open && button.show(&mut cell).clicked() {
                    events.push(ListEvent::Focus(i));
                    events.push(ListEvent::Effect {
                        row: i,
                        effect: PanelEffect::Activate { row: i },
                    });
                }
                if in_a_row {
                    at.x += w + gap;
                } else {
                    at.y += buttons + gap;
                }
            }
        });
    Some(events)
}
