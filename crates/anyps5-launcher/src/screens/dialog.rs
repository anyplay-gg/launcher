//! The confirmation card (docs/DESIGN.md 5.9.1): a question that stands alone, a card in the middle
//! of a dimmed page, like a console's dialog. Uninstall, restoring a backup, closing the running
//! game, the missing toolkit release, deleting a capture and every question of the online area
//! share it, so they look and answer the same way.
//!
//! The card holds a title (the question), the sentence that says what the answer does and one to
//! three buttons. The buttons come from the same [`Row`]s the panels already build (a command row
//! is a button), in the order SYSTEM.md 3 gives: the primary or destructive answer first, the
//! refusal (`Cancel`) last. The host opens the focus on the safe answer. Left and Up go to the
//! previous button, Right and Down to the next; Select runs the focused one; Back cancels. A click
//! on the dimmed page does nothing, so nothing is answered by accident.

use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{Area, Context, Galley, Id, Order, Rect, UiBuilder, pos2, vec2};

use crate::screens::clamped;
use crate::screens::rows::{self, Driven, ListEvent, Row};
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::components::{
    ConsoleButton, ControlSize, Force, Variant, paint_scrim, paint_shadow,
};
use crate::ui::focus::{PanelEffect, PanelNav};
use crate::ui::glass::{GlassStyle, paint_glass};
use crate::ui::i18n::Language;
use crate::ui::icons::Icon;
use crate::ui::input::Command;
use crate::ui::pageblur;
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
/// A question is short; these rows are a bound for a very long name or a language that runs long.
const TITLE_ROWS: usize = 3;
const MESSAGE_ROWS: usize = 8;

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
    pub force: Option<(usize, Force)>,
}

/// One button of the card, taken from a command row.
#[derive(Clone, Debug, PartialEq)]
struct Button {
    title: String,
    icon: Option<Icon>,
    destructive: bool,
    refusal: bool,
    /// A state the button carries (a download under way).
    detail: Option<String>,
}

/// What the card needs to draw itself, kept while it is open so that its exit still shows it.
#[derive(Clone, Debug)]
struct Snapshot {
    title: String,
    message: Option<String>,
    buttons: Vec<Button>,
    focus: usize,
}

impl Snapshot {
    fn of(card: &Card<'_>, rows: &[Row]) -> Snapshot {
        Snapshot {
            title: card.title.to_owned(),
            message: card.message.map(str::to_owned),
            buttons: rows
                .iter()
                .map(|r| Button {
                    title: r.title.clone(),
                    icon: r.icon,
                    destructive: r.destructive,
                    refusal: r.refusal,
                    detail: r.detail.clone(),
                })
                .collect(),
            focus: card.focus,
        }
    }
}

/// The look of each button: the first answer that is neither destructive nor a refusal is the
/// primary one, and only when no answer is destructive.
fn kinds(buttons: &[Button]) -> Vec<Variant> {
    let mut primary_given = buttons.iter().any(|b| b.destructive);
    buttons
        .iter()
        .map(|b| {
            if !primary_given && !b.destructive && !b.refusal {
                primary_given = true;
                Variant::Primary
            } else {
                Variant::Secondary
            }
        })
        .collect()
}

/// The width a button needs for its label (and glyph).
fn needed(painter: &egui::Painter, button: &Button) -> f32 {
    let large = ControlSize::Large;
    let glyph = match button.icon {
        Some(_) => large.glyph(true) + large.gap(),
        None => 0.0,
    };
    large.text_pad() * 2.0 + glyph + text::width(painter, large.style(), &button.title)
}

/// How the buttons stand: side by side, all one width, when the widest fits that many times with a
/// gap between; else one under another across the card.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Arrangement {
    in_a_row: bool,
    /// The width of each button.
    width: f32,
}

fn arrange(wants: &[f32], inner: f32, gap: f32) -> Arrangement {
    let count = wants.len().max(1) as f32;
    let widest = wants.iter().copied().fold(0.0, f32::max);
    let gaps = gap * (count - 1.0);
    if widest * count + gaps <= inner {
        Arrangement {
            in_a_row: true,
            width: (inner - gaps) / count,
        }
    } else {
        Arrangement {
            in_a_row: false,
            width: inner,
        }
    }
}

/// What a command does to the focused button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Move(usize),
    /// Past the first or last button.
    Bump,
    /// Not a direction: the rows' own navigation takes it (Select, Back).
    Pass,
}

fn step(row: usize, len: usize, command: Command) -> Step {
    let last = len.saturating_sub(1);
    let row = row.min(last);
    match command {
        Command::Left | Command::Up => row.checked_sub(1).map_or(Step::Bump, Step::Move),
        Command::Right | Command::Down if row < last => Step::Move(row + 1),
        Command::Right | Command::Down => Step::Bump,
        _ => Step::Pass,
    }
}

/// The width of the card's content on this screen.
pub fn inner_width(ctx: &Context) -> f32 {
    card_width(ctx.content_rect().width()) - 2.0 * CARD_PADDING
}

fn card_width(screen_width: f32) -> f32 {
    CARD_WIDTH.min(screen_width - 2.0 * space::XL)
}

fn title_galley(painter: &egui::Painter, title: &str, width: f32) -> Arc<Galley> {
    clamped(painter, TITLE, title, color::text(), width, TITLE_ROWS)
}

fn message_galley(painter: &egui::Painter, message: &str, width: f32) -> Arc<Galley> {
    let galley = text::layout_font_pretty(
        painter,
        MESSAGE.font(),
        MESSAGE.size * MESSAGE_LEADING,
        MESSAGE.tracking,
        message,
        color::text_label(),
        width,
    );
    if galley.rows.len() <= MESSAGE_ROWS {
        return galley;
    }
    let mut job = LayoutJob::simple(
        message.to_owned(),
        MESSAGE.font(),
        color::text_label(),
        width,
    );
    job.wrap.max_rows = MESSAGE_ROWS;
    job.sections[0].format.line_height = Some(MESSAGE.size * MESSAGE_LEADING);
    painter.layout_job(job)
}

/// Moves the focus between the buttons and hands everything else to the rows' own navigation
/// (Select runs a button, Back closes the card).
pub fn drive(shell: &mut Shell, nav: &mut PanelNav, rows: &[Row], commands: &[Command]) -> Driven {
    let mut out = Driven::default();
    for &command in commands {
        match step(nav.row, rows.len(), command) {
            Step::Move(row) => {
                nav.row = row;
                shell.play(Cue::Move);
            }
            Step::Bump => shell.play(Cue::Bump),
            Step::Pass => {
                nav.row = nav.row.min(rows.len().saturating_sub(1));
                let driven = rows::drive(shell, nav, rows, &[command]);
                out.changes.extend(driven.changes);
                out.close |= driven.close;
            }
        }
    }
    out
}

/// Draws the card over the dimmed page, or its exit. Returns the mouse's events as the rows' list
/// would (a click focuses and runs a button); `None` once it is fully closed. While it is closing
/// the buttons stay on the card but no longer answer.
pub fn draw(ctx: &Context, open: bool, card: &Card<'_>, rows: &[Row]) -> Option<Vec<ListEvent>> {
    let id = Id::unique(("dialog", card.id));
    let snapshot = Snapshot::of(card, rows);
    if open {
        ctx.data_mut(|d| d.insert_temp(id, snapshot.clone()));
    }
    paint(ctx, id, open, &snapshot, card.force)
}

/// Lets a card that its host no longer draws fade out, from what it showed last. Call it on the
/// frames the question is not open; it draws nothing once the card is gone.
pub fn close(ctx: &Context, card_id: &'static str) {
    let id = Id::unique(("dialog", card_id));
    if let Some(snapshot) = ctx.data(|d| d.get_temp::<Snapshot>(id)) {
        paint(ctx, id, false, &snapshot, None);
    }
}

fn paint(
    ctx: &Context,
    id: Id,
    open: bool,
    card: &Snapshot,
    force: Option<(usize, Force)>,
) -> Option<Vec<ListEvent>> {
    let v = ctx.animate_from_to(
        (id, "open"),
        0.0,
        if open { 1.0 } else { 0.0 },
        Motion::Overlay,
    );
    if v <= 0.001 && !open {
        pageblur::release(ctx, id);
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
            // Covers the screen: the dimmed page takes the clicks and does nothing with them, so
            // nothing is answered by accident (and nothing behind it is hit).
            ui.expand_to_include_rect(screen);
            let painter = ui.painter().clone();
            // Text carries explicit colours, so only the painter's opacity fades it with the card.
            let mut text_painter = painter.clone();
            text_painter.set_opacity(v);
            // The flat dim holds until the blurred page arrives (a frame or two) and stays under
            // High Contrast; the blur then fades in over it.
            paint_scrim(&painter, screen, SCRIM * v);
            pageblur::paint(ctx, &painter, screen, id, open, v);

            let width = card_width(screen.width());
            let inner = width - 2.0 * CARD_PADDING;
            let title = title_galley(&painter, &card.title, inner);
            let message = card
                .message
                .as_deref()
                .map(|m| message_galley(&painter, m, inner));
            // A button that carries a state (a download under way) says it in a quiet line.
            let note = card
                .buttons
                .iter()
                .find_map(|b| b.detail.as_deref())
                .map(|n| text::layout(&painter, NOTE, n, color::text_detail(), inner));
            let button_height = ControlSize::Large.height();
            let gap = size::CONTROL_GAP;
            let wants: Vec<f32> = card.buttons.iter().map(|b| needed(&painter, b)).collect();
            let arrangement = arrange(&wants, inner, gap);
            let count = wants.len().max(1) as f32;
            let buttons_height = if arrangement.in_a_row {
                button_height
            } else {
                button_height * count + gap * (count - 1.0)
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
            // The card is a translucent pane over the blurred page; until that page has been
            // captured (and under High Contrast) it is the usual glass panel.
            if !pageblur::paint_card(ctx, &painter, rect, radius::LARGE, id, v) {
                let mut style = GlassStyle::glass(radius::LARGE, 0.6);
                style.black *= v;
                style.alpha = v;
                paint_glass(ctx, &painter, rect, style);
            }

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
            text_painter.galley(pos2(x, y), title, color::text());
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
                text_painter.galley(pos2(x, y), message, color::text_label());
                y += h;
            }
            if let Some(note) = note {
                y += space::S;
                let h = note.size().y;
                text_painter.galley(pos2(x, y), note, color::text_detail());
                y += h;
            }
            y += space::XL;

            let variants = kinds(&card.buttons);
            let mut at = pos2(x, y);
            let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
            child.set_opacity(v);
            for (i, button) in card.buttons.iter().enumerate() {
                let slot = Rect::from_min_size(at, vec2(arrangement.width, button_height));
                let mut cell = child.new_child(
                    UiBuilder::new()
                        .max_rect(slot)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                let force = force.filter(|(f, _)| *f == i).map(|(_, f)| f);
                let mut widget = ConsoleButton::new(&button.title)
                    .variant(variants[i])
                    .destructive(button.destructive)
                    .focused(open && card.focus == i)
                    .min_width(arrangement.width)
                    .force(force);
                if let Some(icon) = button.icon {
                    widget = widget.icon(icon);
                }
                // Drawn while the card leaves too; only the answer is gated.
                let clicked = widget.show(&mut cell).clicked();
                if open && clicked {
                    events.push(ListEvent::Focus(i));
                    events.push(ListEvent::Effect {
                        row: i,
                        effect: PanelEffect::Activate { row: i },
                    });
                }
                if arrangement.in_a_row {
                    at.x += arrangement.width + gap;
                } else {
                    at.y += button_height + gap;
                }
            }
        });
    Some(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::focus::PanelEffect;

    fn button(title: &str) -> Button {
        Button {
            title: title.to_owned(),
            icon: None,
            destructive: false,
            refusal: false,
            detail: None,
        }
    }

    fn danger(title: &str) -> Button {
        Button {
            destructive: true,
            ..button(title)
        }
    }

    fn no(title: &str) -> Button {
        Button {
            refusal: true,
            ..button(title)
        }
    }

    #[test]
    fn a_destructive_answer_leaves_no_primary_button() {
        assert_eq!(
            kinds(&[danger("Remove"), no("Cancel")]),
            [Variant::Secondary, Variant::Secondary]
        );
    }

    #[test]
    fn the_first_answer_that_is_not_a_refusal_is_the_one_primary() {
        assert_eq!(
            kinds(&[button("Download"), button("Follow"), no("Cancel")]),
            [Variant::Primary, Variant::Secondary, Variant::Secondary]
        );
        // A refusal in front never takes it.
        assert_eq!(
            kinds(&[no("Keep Playing"), button("Restore")]),
            [Variant::Secondary, Variant::Primary]
        );
    }

    #[test]
    fn the_buttons_share_a_row_at_one_width_while_they_fit() {
        let a = arrange(&[120.0, 200.0], 412.0, 12.0);
        assert!(a.in_a_row);
        assert_eq!(a.width, 200.0);
        // Equal widths: the widest needs 260, so two of them and the gap no longer fit in 500.
        let a = arrange(&[120.0, 260.0], 500.0, 12.0);
        assert!(!a.in_a_row);
        assert_eq!(a.width, 500.0);
    }

    #[test]
    fn three_long_answers_stack_across_the_card() {
        let a = arrange(&[210.0, 230.0, 120.0], 512.0, 12.0);
        assert!(!a.in_a_row);
        assert_eq!(a.width, 512.0);
    }

    #[test]
    fn directions_walk_the_buttons_and_bump_at_the_ends() {
        assert_eq!(step(1, 2, Command::Left), Step::Move(0));
        assert_eq!(step(0, 2, Command::Left), Step::Bump);
        assert_eq!(step(0, 2, Command::Up), Step::Bump);
        assert_eq!(step(0, 3, Command::Right), Step::Move(1));
        assert_eq!(step(1, 3, Command::Down), Step::Move(2));
        assert_eq!(step(2, 3, Command::Right), Step::Bump);
        // A focus left past the end is pulled back first.
        assert_eq!(step(9, 2, Command::Left), Step::Move(0));
    }

    #[test]
    fn select_and_back_belong_to_the_rows() {
        assert_eq!(step(0, 2, Command::Select), Step::Pass);
        assert_eq!(step(1, 2, Command::Back), Step::Pass);
        let rows = [Row::command("yes", "Yes"), Row::command("cancel", "Cancel")];
        let kinds = rows::kinds(&rows);
        let mut nav = PanelNav::single(1);
        assert_eq!(
            nav.command(Command::Back, &kinds).effect,
            PanelEffect::Close
        );
        assert_eq!(
            nav.command(Command::Select, &kinds).effect,
            PanelEffect::Activate { row: 1 }
        );
    }

    #[test]
    fn a_very_long_title_and_sentence_are_cut_at_three_and_eight_rows() {
        let ctx = Context::default();
        crate::ui::theme::install(&ctx);
        let long = "A very long name that keeps going and going ".repeat(60);
        let mut rows = (0, 0);
        // Fonts arrive with the second pass.
        for _ in 0..2 {
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let painter = ui.painter().clone();
                rows = (
                    title_galley(&painter, &long, 300.0).rows.len(),
                    message_galley(&painter, &long, 300.0).rows.len(),
                );
            });
            output.drop_without_applying_deltas();
        }
        assert_eq!(rows, (TITLE_ROWS, MESSAGE_ROWS));
    }

    #[test]
    fn a_card_is_narrower_than_a_small_window() {
        assert_eq!(card_width(1600.0), CARD_WIDTH);
        assert_eq!(card_width(400.0), 352.0);
    }
}
