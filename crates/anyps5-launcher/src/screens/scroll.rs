//! Scroll-to-focus (docs/DESIGN.md animation 31): a list scrolls so that the focused item sits in
//! the middle, with the focus spring (`scrollTo(anchor: .center)` inside `Motion.focus`). The mouse
//! wheel still scrolls freely; the next focus change takes over again.
//!
//! Usage per frame: [`Follow::begin`] before the scroll area (it yields the offset to apply), draw
//! the items, then [`Follow::end`] with where they ended up.

use egui::{Context, Id};

use crate::ui::anim::{AnimExt, Motion};

/// Scroll offset that centres an item, never past the ends.
pub fn centred_offset(top: f32, height: f32, viewport: f32, content: f32) -> f32 {
    let max = (content - viewport).max(0.0);
    (top + height * 0.5 - viewport * 0.5).clamp(0.0, max)
}

/// Scroll offset that centres two items and what lies between them, or puts the first at the
/// top when together they are taller than the viewport; never past the ends.
pub fn span_offset(first: (f32, f32), second: (f32, f32), viewport: f32, content: f32) -> f32 {
    let top = first.0.min(second.0);
    let bottom = (first.0 + first.1).max(second.0 + second.1);
    if bottom - top <= viewport {
        centred_offset(top, bottom - top, viewport, content)
    } else {
        first.0.clamp(0.0, (content - viewport).max(0.0))
    }
}

/// What the follower remembers between frames.
#[derive(Clone, Default)]
struct Memory {
    /// `(top, height)` of every item in content coordinates, from the last frame.
    items: Vec<(f32, f32)>,
    content_height: f32,
    viewport_height: f32,
    focus: Option<usize>,
    /// The item kept in view with the focused one ([`Follow::begin_with`]).
    with: Option<usize>,
    /// The offset egui used last frame.
    offset: f32,
    /// The offset this follower last asked for.
    set: f32,
    session: u32,
    following: bool,
}

/// One frame of following.
pub struct Follow {
    memory_id: Id,
    memory: Memory,
}

impl Follow {
    /// Starts the frame. Returns the follower and the offset to give the scroll area (`None` leaves
    /// the area alone, so the wheel works).
    pub fn begin(ctx: &Context, id: Id, focus: Option<usize>) -> (Follow, Option<f32>) {
        Follow::begin_with(ctx, id, focus, None)
    }

    /// [`Follow::begin`] with an item that belongs to the focused one (a switch and the code it
    /// shows): the two are centred together, or the focused one goes to the top when they do not
    /// fit. A new `with` starts a follow even when the focus stays.
    pub fn begin_with(
        ctx: &Context,
        id: Id,
        focus: Option<usize>,
        with: Option<usize>,
    ) -> (Follow, Option<f32>) {
        let memory_id = id.with("follow-memory");
        let mut mem: Memory = ctx.data(|d| d.get_temp(memory_id)).unwrap_or_default();
        let new_with = with.is_some() && with != mem.with;
        if focus != mem.focus || new_with {
            // A wheel scroll since the last follow leaves the spring's value stale: start a new one
            // from where the list really is.
            let dirty = (mem.offset - mem.set).abs() > 1.0;
            if dirty || mem.items.is_empty() {
                mem.session += 1;
            }
            mem.following = focus.is_some();
            mem.focus = focus;
        }
        mem.with = with;
        let mut offset = None;
        if mem.following
            && let Some(&(top, height)) = focus.and_then(|f| mem.items.get(f))
        {
            let target = match with.and_then(|w| mem.items.get(w)) {
                Some(&(with_top, with_height)) => span_offset(
                    (top, height),
                    (with_top, with_height),
                    mem.viewport_height,
                    mem.content_height,
                ),
                None => centred_offset(top, height, mem.viewport_height, mem.content_height),
            };
            let value = ctx.animate_from_to(
                (id, "follow", mem.session),
                mem.offset,
                target,
                Motion::Focus,
            );
            mem.set = value;
            offset = Some(value);
            if (value - target).abs() < 0.5 {
                mem.following = false;
            }
        }
        (
            Follow {
                memory_id,
                memory: mem,
            },
            offset,
        )
    }

    /// Ends the frame with the geometry the items were drawn at.
    pub fn end(
        mut self,
        ctx: &Context,
        items: Vec<(f32, f32)>,
        content_height: f32,
        viewport_height: f32,
        offset: f32,
    ) {
        self.memory.items = items;
        self.memory.content_height = content_height;
        self.memory.viewport_height = viewport_height;
        self.memory.offset = offset;
        ctx.data_mut(|d| d.insert_temp(self.memory_id, self.memory));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_item_is_centred_but_never_past_the_ends() {
        assert_eq!(centred_offset(0.0, 58.0, 400.0, 1000.0), 0.0);
        assert_eq!(centred_offset(500.0, 58.0, 400.0, 1000.0), 329.0);
        assert_eq!(centred_offset(980.0, 58.0, 400.0, 1000.0), 600.0);
        assert_eq!(
            centred_offset(10.0, 58.0, 400.0, 300.0),
            0.0,
            "short content"
        );
    }

    #[test]
    fn an_item_to_keep_in_view_starts_a_follow_with_the_focus_unchanged() {
        // A switch turned on with the pad: the focus stays on it, and the code comes below it.
        let ctx = Context::default();
        let id = Id::unique("list");
        let items = vec![(0.0, 58.0), (900.0, 58.0), (980.0, 240.0)];
        let (follow, _) = Follow::begin(&ctx, id, Some(1));
        follow.end(&ctx, items.clone(), 2000.0, 400.0, 0.0);
        let (follow, offset) = Follow::begin(&ctx, id, Some(1));
        follow.end(&ctx, items.clone(), 2000.0, 400.0, 0.0);
        assert!(offset.is_some(), "the first focus follows");
        // Settled where the switch is centred, as egui would leave it.
        let settled = centred_offset(900.0, 58.0, 400.0, 2000.0);
        let mut memory: Memory = ctx.data(|d| d.get_temp(id.with("follow-memory")).unwrap());
        memory.following = false;
        memory.offset = settled;
        memory.set = settled;
        ctx.data_mut(|d| d.insert_temp(id.with("follow-memory"), memory));
        let (follow, offset) = Follow::begin(&ctx, id, Some(1));
        follow.end(&ctx, items.clone(), 2000.0, 400.0, settled);
        assert_eq!(offset, None, "nothing new: the wheel is free");
        let (follow, offset) = Follow::begin_with(&ctx, id, Some(1), Some(2));
        follow.end(&ctx, items.clone(), 2000.0, 400.0, settled);
        assert!(offset.is_some(), "the code to keep in view starts a follow");
        let (follow, offset) = Follow::begin_with(&ctx, id, Some(1), Some(2));
        follow.end(&ctx, items, 2000.0, 400.0, settled);
        assert!(offset.is_some(), "it goes on until it gets there");
    }

    #[test]
    fn two_items_are_centred_together_or_the_first_goes_to_the_top() {
        // A switch at 500 and its code 80 dp below it, 240 tall: the pair is centred.
        assert_eq!(
            span_offset((500.0, 58.0), (580.0, 240.0), 400.0, 2000.0),
            460.0
        );
        // Together taller than the list: the switch at the top.
        assert_eq!(
            span_offset((500.0, 58.0), (580.0, 400.0), 400.0, 2000.0),
            500.0
        );
        // Never past the end.
        assert_eq!(
            span_offset((500.0, 58.0), (580.0, 240.0), 400.0, 700.0),
            300.0
        );
    }
}
