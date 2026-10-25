//! The Play Together card (docs/DESIGN.md 17, "Play Together"): "Mira is playing Minecraft. Join?"
//! and, in the same place, why a join or a hosting did not go ahead.
//!
//! One glass card at the top centre, below the toast and below a call's card or strip, on its own
//! layer like those. It comes in with the overlay spring (fade and a short slide down) and leaves the
//! same way. It holds at most one question: the offer has "Join" as its one primary action and a
//! quiet "Not now"; a blocker says what is the case in a headline and what to do under it, and has
//! "Close" (and, for the two it can mend, the remedy as the primary action). Nothing on it moves
//! while it stays. The pad reaches the same actions through the Online panel (an invitation's Join,
//! a friend's Join); the card is for the pointer and Tab, and each button shows a focus ring when it
//! has the keyboard.

use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{
    Align2, Area, Color32, Context, Galley, Id, Order, Painter, Rect, Sense, Ui, UiBuilder, pos2,
    vec2,
};

use crate::app::model::Model;
use crate::app::together::{self, Action, Card, Fix};
use crate::screens::calls;
use crate::screens::shared::paint_gradient_circle;
use crate::screens::together_strings::{TogetherText as T, blocker_text};
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::components::{ConsoleButton, paint_shadow};
use crate::ui::glass::{GlassStyle, paint_glass};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::text;
use crate::ui::theme::{self, TextStyle, Weight, fade, radius, shadows, size, ty, white};

const TOP: f32 = size::TOAST_TOP + 72.0;
const CARD_WIDTH: f32 = 420.0;
const PAD: f32 = 24.0;
const LEAD: f32 = 56.0;
const TEXT_GAP: f32 = 16.0;
/// Between the lines of text and the buttons.
const BODY_GAP: f32 = 20.0;
const GAP: f32 = size::CONTROL_GAP;
/// The headline is a semibold 16, the line under it a regular 14 with the room a few lines need.
const HEADLINE: TextStyle = TextStyle::new(16.0, Weight::SemiBold);
const HEADLINE_LEADING: f32 = 1.3;
const LINE_LEADING: f32 = 1.45;
const HEADLINE_ROWS: usize = 3;
const LINE_ROWS: usize = 3;

/// State kept between frames: what the card showed last, so it can fade out with its content.
#[derive(Debug, Default)]
pub struct TogetherUi {
    last: Option<Card>,
}

impl TogetherUi {
    /// Reads the model, paints the card and carries out what the player pressed.
    pub fn show(&mut self, ctx: &Context, model: &mut Model, shell: &mut Shell) {
        let language = model.language();
        let card = together::card(model);
        if card.is_some() {
            self.last = card.clone();
        }
        let shown = ctx.animate_to(
            "together-card",
            f32::from(u8::from(card.is_some())),
            Motion::Overlay,
        );
        // Below a ringing call or the call strip, which hang at the same place.
        let below = if calls::card_data(&model.online, 0).is_some() {
            calls::CARD_HEIGHT + GAP
        } else if calls::strip_data(&model.online).is_some() {
            calls::STRIP_HEIGHT + GAP
        } else {
            0.0
        };
        if let Some(data) = card.as_ref().or(self.last.as_ref())
            && let Some(action) = paint(
                ctx,
                shell.accent_color(),
                data,
                language,
                shown,
                card.is_some(),
                below,
            )
        {
            together::apply(model, action);
        }
        if shown <= 0.003 && card.is_none() {
            self.last = None;
        }
    }
}

/// What the card says and offers.
struct View {
    lead: Lead,
    headline: String,
    line: String,
    /// The quiet answer, when there are two.
    quiet: Option<(String, Action)>,
    primary: (String, Action),
}

enum Lead {
    /// The friend whose session it is: their initial.
    Friend(String),
    Warning,
}

fn view(card: &Card, language: Language) -> View {
    match card {
        Card::Offer {
            key,
            offer,
            invitation,
        } => {
            let line = match offer.places {
                Some((taken, total)) => T::OfferPlaces.format(
                    language,
                    &[Arg::Int(i64::from(taken)), Arg::Int(i64::from(total))],
                ),
                None => T::OfferNoPlaces.text(language).to_owned(),
            };
            View {
                lead: Lead::Friend(offer.host.clone()),
                headline: T::OfferHeadline
                    .format(language, &[Arg::Str(&offer.host), Arg::Str(&offer.title)]),
                line,
                quiet: Some((
                    T::NotNow.text(language).to_owned(),
                    Action::Dismiss {
                        key: key.clone(),
                        invitation: invitation.clone(),
                    },
                )),
                primary: (
                    T::Join.text(language).to_owned(),
                    Action::Join(offer.clone()),
                ),
            }
        }
        Card::Blocked(blocker) => {
            let (headline, line) = blocker_text(blocker, language);
            match Fix::of(blocker) {
                Some(fix) => View {
                    lead: Lead::Warning,
                    headline,
                    line,
                    quiet: Some((T::Close.text(language).to_owned(), Action::Close)),
                    primary: (
                        match fix {
                            Fix::SignIn => T::SignIn,
                            Fix::SwitchOn => T::SwitchOn,
                        }
                        .text(language)
                        .to_owned(),
                        Action::Fix(fix),
                    ),
                },
                None => View {
                    lead: Lead::Warning,
                    headline,
                    line,
                    quiet: None,
                    primary: (T::Close.text(language).to_owned(), Action::Close),
                },
            }
        }
    }
}

/// Text in a role with its own leading, cut after `rows` rows.
fn lay(
    painter: &Painter,
    style: TextStyle,
    leading: f32,
    text: &str,
    color: Color32,
    width: f32,
    rows: usize,
) -> Arc<Galley> {
    let mut job = LayoutJob::simple(text.to_owned(), style.font(), color, width.max(1.0));
    job.wrap.max_rows = rows;
    for section in &mut job.sections {
        section.format.line_height = Some(style.size * leading);
        section.format.extra_letter_spacing = style.tracking;
    }
    painter.layout_job(job)
}

/// The slide and fade: the card drops in from 24 dp above and leaves the same way.
fn entrance(shown: f32) -> (f32, f32) {
    let v = shown.clamp(0.0, 1.0);
    (v, -(1.0 - v) * 24.0)
}

/// A button's focus from the keyboard: the id it will get, looked up in egui's memory.
fn keyboard_focus(ui: &Ui) -> bool {
    let id = ui.next_auto_id();
    ui.ctx().memory(|m| m.has_focus(id))
}

/// The card. `live` is false while it fades out: it can no longer be pressed. `below` is the room
/// a call's card or strip takes at the same place.
pub(crate) fn paint(
    ctx: &Context,
    accent: Color32,
    card: &Card,
    language: Language,
    shown: f32,
    live: bool,
    below: f32,
) -> Option<Action> {
    let (a, slide) = entrance(shown);
    if a <= 0.003 {
        return None;
    }
    let view = view(card, language);
    let screen = ctx.content_rect();
    let measure = ctx.layer_painter(egui::LayerId::new(
        Order::Foreground,
        Id::unique("together-measure"),
    ));
    let text_width = CARD_WIDTH - 2.0 * PAD - LEAD - TEXT_GAP;
    let headline = lay(
        &measure,
        HEADLINE,
        HEADLINE_LEADING,
        &view.headline,
        Color32::WHITE,
        text_width,
        HEADLINE_ROWS,
    );
    let line = lay(
        &measure,
        ty::NOTE,
        LINE_LEADING,
        &view.line,
        Color32::WHITE,
        text_width,
        LINE_ROWS,
    );
    let text_height = headline.size().y + 4.0 + line.size().y;
    let body = text_height.max(LEAD);
    let height = PAD * 2.0 + body + BODY_GAP + size::BUTTON_MIN_HEIGHT;
    let rect = Rect::from_min_size(
        pos2(
            screen.center().x - CARD_WIDTH * 0.5,
            screen.min.y + TOP + below + slide,
        ),
        vec2(CARD_WIDTH, height),
    );
    let mut action = None;
    Area::new(Id::unique("together-card"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .interactable(live)
        .show(ctx, |ui| {
            let painter = ui.painter().clone();
            paint_shadow(
                &painter,
                rect,
                radius::TOAST * 0.5,
                egui::epaint::Shadow {
                    color: fade(shadows::toast().color, a * a),
                    ..shadows::toast()
                },
            );
            let mut style = GlassStyle::glass(radius::TOAST * 0.5, 0.6);
            style.black *= a;
            style.alpha = a;
            paint_glass(ctx, &painter, rect, style);

            // The lead: the friend's initial, or a quiet warning.
            let inner = rect.shrink(PAD);
            let centre = pos2(inner.min.x + LEAD * 0.5, inner.min.y + LEAD * 0.5);
            match &view.lead {
                Lead::Friend(handle) => {
                    paint_gradient_circle(
                        &painter,
                        centre,
                        LEAD * 0.5,
                        fade(accent, a),
                        fade(theme::fade(accent, 0.55), a),
                    );
                    let initial: String = handle
                        .chars()
                        .take(1)
                        .flat_map(char::to_uppercase)
                        .collect();
                    text::paint(
                        &painter,
                        centre,
                        Align2::CENTER_CENTER,
                        TextStyle::new(24.0, Weight::Bold),
                        &initial,
                        fade(Color32::WHITE, a),
                    );
                }
                Lead::Warning => {
                    painter.circle_filled(centre, LEAD * 0.5, fade(white(0.10), a));
                    painter.text(
                        centre,
                        Align2::CENTER_CENTER,
                        Icon::WARNING.glyph().to_string(),
                        Icon::WARNING.font(26.0, true),
                        fade(white(0.92), a),
                    );
                }
            }

            // The words, set against the top of the lead.
            let text_x = inner.min.x + LEAD + TEXT_GAP;
            let top = inner.min.y + (body - text_height) * 0.5;
            let line_top = top + headline.size().y + 4.0;
            painter.galley(pos2(text_x, top), headline, fade(Color32::WHITE, a));
            painter.galley(pos2(text_x, line_top), line, fade(white(0.7), a));
            let status = ui.interact(rect, Id::unique("together-card-status"), Sense::hover());
            let label = format!("{}. {}", view.headline, view.line);
            status.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Status, true, label.as_str())
            });

            // The answers: the quiet one first, the primary one at the trailing edge.
            let buttons = Rect::from_min_size(
                pos2(inner.min.x, inner.max.y - size::BUTTON_MIN_HEIGHT),
                vec2(inner.width(), size::BUTTON_MIN_HEIGHT),
            );
            let mut child = ui.new_child(
                UiBuilder::new()
                    .max_rect(buttons)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            child.spacing_mut().item_spacing.x = GAP;
            child.set_opacity(a);
            let each = if view.quiet.is_some() {
                (inner.width() - GAP) * 0.5
            } else {
                inner.width()
            };
            if let Some((label, quiet)) = &view.quiet {
                let focused = keyboard_focus(&child);
                if ConsoleButton::new(label)
                    .min_width(each)
                    .focused(focused)
                    .enabled(live)
                    .show(&mut child)
                    .clicked()
                {
                    action = Some(quiet.clone());
                }
            }
            let focused = keyboard_focus(&child);
            if ConsoleButton::new(&view.primary.0)
                .prominent(true)
                .min_width(each)
                .focused(focused)
                .enabled(live)
                .show(&mut child)
                .clicked()
            {
                action = Some(view.primary.1.clone());
            }
        });
    action
}
