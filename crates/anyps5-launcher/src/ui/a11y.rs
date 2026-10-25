//! Screen-reader focus follows the focus ring.
//!
//! The launcher's model decides what is focused (keyboard, pad and pointer all feed it) and the
//! widgets draw the ring from it; egui's own keyboard focus is not used. AccessKit, though, reports
//! the focus egui holds, so without help a screen reader would announce nothing, or something
//! else, while the ring sits on a button. [`follow`] gives egui's focus to the widget the model has
//! focused, once per frame, and takes it back when the ring moves on.
//!
//! Three rules keep this from fighting egui:
//! - egui's focus is only taken when nothing, or this module's own earlier choice, holds it. A text
//!   field being edited, or a control the player reached with Tab, keeps it ([`foreign_focus`]).
//! - Tab and the arrows must not move egui's focus away from the ring: the widget's event filter
//!   claims them (the model reads the arrows itself). Tab is the model's too: `Input::poll` turns
//!   it into `InputEvent::Tab` and [`consume_tab`] takes it out of egui's input, so Tab and
//!   Shift+Tab walk the ring in reading order (home: top bar, tiles, actions, question; a panel:
//!   down and up; the onboarding: right and left). While a call card or strip is on screen
//!   (`Input::tab_for_egui`) Tab stays with egui, which focuses their buttons; that focus then
//!   owns the keys until Enter or Escape lets it go.
//! - Enter and Space activate a focused widget in egui; the model already turns them into "select",
//!   so [`consume_activation`] removes them from egui's input once the model has taken them.

use egui::{Context, EventFilter, Id, Key};

/// The widget whose egui focus this module set.
fn owned_id() -> Id {
    Id::unique("a11y-owned-focus")
}

fn owned(ctx: &Context) -> Option<Id> {
    ctx.data(|d| d.get_temp::<Id>(owned_id()))
}

/// Makes egui's focus (what AccessKit announces) follow the model: call for every widget that
/// can hold the ring, with whether the model has it there.
pub fn follow(ctx: &Context, id: Id, focused: bool) {
    let mine = owned(ctx);
    if focused {
        let held = ctx.memory(|m| m.focused());
        if held == Some(id) {
            // Already there; keep the keys the model reads out of egui's focus movement.
            if mine == Some(id) {
                ctx.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        id,
                        EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            escape: true,
                        },
                    );
                });
            }
        } else if held.is_none() || held == mine {
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.data_mut(|d| d.insert_temp(owned_id(), id));
        }
    } else if mine == Some(id) && ctx.memory(|m| m.has_focus(id)) {
        ctx.memory_mut(|m| m.surrender_focus(id));
        ctx.data_mut(|d| d.remove::<Id>(owned_id()));
    }
}

/// True while egui's focus is held by something this module did not choose: a text field, or a
/// control reached with Tab. The model's keys then belong to that widget.
pub fn foreign_focus(ctx: &Context) -> bool {
    match ctx.memory(|m| m.focused()) {
        None => false,
        Some(id) => owned(ctx) != Some(id),
    }
}

/// Takes Tab out of egui's input once the model has taken it as a move along the reading order,
/// so egui does not also move its own focus off the ring.
pub fn consume_tab(ctx: &Context) {
    ctx.input_mut(|i| {
        i.events.retain(|e| {
            !matches!(
                e,
                egui::Event::Key {
                    key: Key::Tab,
                    pressed: true,
                    ..
                }
            )
        });
    });
}

/// Takes Enter and Space out of egui's input when the model has handled them (or ignores them on
/// purpose, with a dialog open or a key being captured), so a followed widget is not also clicked by
/// egui's own activation. Nothing happens while egui's focus is foreign: a text field needs its Enter.
pub fn consume_activation(ctx: &Context) {
    if foreign_focus(ctx) {
        return;
    }
    ctx.input_mut(|i| {
        i.events.retain(|e| {
            !matches!(
                e,
                egui::Event::Key {
                    key: Key::Enter | Key::Space,
                    pressed: true,
                    ..
                }
            )
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Rect, Sense, pos2};

    /// One egui pass in which each `(id, focused)` is a focusable widget drawn in that order.
    fn pass(ctx: &Context, stops: &[(Id, bool)]) {
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for (n, (id, focused)) in stops.iter().enumerate() {
                let at = pos2(10.0, 10.0 + 30.0 * n as f32);
                ui.interact(
                    Rect::from_min_size(at, egui::vec2(20.0, 20.0)),
                    *id,
                    Sense::click(),
                );
                follow(ui.ctx(), *id, *focused);
            }
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn egui_focus_follows_the_ring_and_leaves_with_it() {
        let ctx = Context::default();
        let (a, b) = (Id::unique("a"), Id::unique("b"));
        pass(&ctx, &[(a, true), (b, false)]);
        assert_eq!(ctx.memory(|m| m.focused()), Some(a));
        assert!(!foreign_focus(&ctx));
        // The ring moves down to b, and back up to a, in either drawing order.
        pass(&ctx, &[(a, false), (b, true)]);
        assert_eq!(ctx.memory(|m| m.focused()), Some(b));
        pass(&ctx, &[(a, true), (b, false)]);
        assert_eq!(ctx.memory(|m| m.focused()), Some(a));
        // No ring, no focus.
        pass(&ctx, &[(a, false), (b, false)]);
        assert_eq!(ctx.memory(|m| m.focused()), None);
    }

    #[test]
    fn tab_does_not_move_the_focus_off_the_ring() {
        let ctx = Context::default();
        let (a, b) = (Id::unique("a"), Id::unique("b"));
        // The filter is set from the second pass on, once the widget had the focus a frame before.
        pass(&ctx, &[(a, true), (b, false)]);
        pass(&ctx, &[(a, true), (b, false)]);
        let tab = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let out = ctx.run_ui(tab, |ui| {
            for (n, (id, focused)) in [(a, true), (b, false)].into_iter().enumerate() {
                let at = pos2(10.0, 10.0 + 30.0 * n as f32);
                ui.interact(
                    Rect::from_min_size(at, egui::vec2(20.0, 20.0)),
                    id,
                    Sense::click(),
                );
                follow(ui.ctx(), id, focused);
            }
        });
        out.drop_without_applying_deltas();
        assert_eq!(ctx.memory(|m| m.focused()), Some(a));
        assert!(!foreign_focus(&ctx));
    }

    #[test]
    fn a_foreign_focus_is_not_taken() {
        let ctx = Context::default();
        let (field, button) = (Id::unique("field"), Id::unique("button"));
        pass(&ctx, &[(field, false), (button, false)]);
        ctx.memory_mut(|m| m.request_focus(field));
        assert!(foreign_focus(&ctx));
        pass(&ctx, &[(field, false), (button, true)]);
        assert_eq!(ctx.memory(|m| m.focused()), Some(field));
    }
}
