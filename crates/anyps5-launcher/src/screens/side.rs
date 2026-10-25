//! The side panel family (docs/DESIGN.md 5.9): game settings, controls, advanced and the uninstall
//! confirmation are four panels on one frame. They share the slide-in, so switching from one to
//! another keeps the panel on screen and only swaps the content, as in the original.

use egui::{Context, Ui};

use crate::screens::rows::{self, ListEvent, ListOptions, Row};
use crate::ui::chrome;
use crate::ui::components::Force;
use crate::ui::focus::PanelNav;
use crate::ui::i18n::Language;
use crate::ui::input::{Command, InputEvent};

/// The animation identity all four panels share.
const SIDE_ID: &str = "game-side-panel";

/// Open and closing state plus the focus model of one side panel.
#[derive(Clone, Copy, Debug)]
pub struct SideState {
    open: bool,
    closing: bool,
    pub nav: PanelNav,
}

impl Default for SideState {
    fn default() -> Self {
        Self {
            open: false,
            closing: false,
            nav: PanelNav::single(0),
        }
    }
}

impl SideState {
    /// Opens at `row` (game settings 0, controls 1, advanced 1, confirm 0).
    pub fn open_at(&mut self, row: usize) {
        self.open = true;
        self.closing = false;
        self.nav = PanelNav::single(row);
    }

    /// Slides out.
    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.closing = true;
        }
    }

    /// Leaves without a slide: another panel of the family takes over the frame.
    pub fn dismiss(&mut self) {
        self.open = false;
        self.closing = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// True while the panel is open or still sliding out; the host keeps calling `show`.
    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    /// Called when the slide-out has finished.
    pub fn finished(&mut self) {
        self.closing = false;
    }
}

/// The commands among a frame's input events.
pub fn commands(events: &[InputEvent]) -> Vec<Command> {
    events
        .iter()
        .filter_map(|e| match e {
            InputEvent::Command(c) => Some(*c),
            _ => None,
        })
        .collect()
}

/// What a panel frame produced: actions for the host and whether the settings it edits changed
/// (the host saves them).
#[derive(Debug)]
pub struct Output<A> {
    pub actions: Vec<A>,
    pub changed: bool,
}

impl<A> Default for Output<A> {
    fn default() -> Self {
        Self {
            actions: Vec::new(),
            changed: false,
        }
    }
}

/// What a panel shows and how.
pub struct SideView<'a> {
    pub title: &'a str,
    pub subtitle: Option<&'a str>,
    pub list_id: &'static str,
    pub nav: &'a PanelNav,
    pub accent: egui::Color32,
    pub language: Language,
    pub force: Option<(usize, Force)>,
}

/// What came back from drawing.
pub struct SideDrawn {
    pub scrim_clicked: bool,
    pub events: Vec<ListEvent>,
}

/// Draws the panel (or its slide-out). `None` once it is fully closed.
pub fn draw(ctx: &Context, open: bool, view: &SideView<'_>, rows: &mut [Row]) -> Option<SideDrawn> {
    draw_with_header(ctx, open, view, rows, |_| {})
}

/// [`draw`] with something above the first row that scrolls with the rows: the profile page's
/// banner.
pub fn draw_with_header(
    ctx: &Context,
    open: bool,
    view: &SideView<'_>,
    rows: &mut [Row],
    header: impl FnOnce(&mut Ui),
) -> Option<SideDrawn> {
    let opts = ListOptions {
        accent: view.accent,
        language: view.language,
        focus: Some(view.nav.row),
        force: view.force,
    };
    chrome::side_panel(ctx, SIDE_ID, open, view.title, view.subtitle, |ui| {
        rows::list_with_header(ui, view.list_id, rows, &opts, header)
    })
    .map(|(output, events)| SideDrawn {
        scrim_clicked: output.scrim_clicked,
        events,
    })
}

/// The panel frame with contents of the page's own: the feed's cards. Returns whether the scrim
/// was clicked, and what `contents` returned; `None` once the panel is fully closed.
pub fn frame<R>(
    ctx: &Context,
    open: bool,
    title: &str,
    subtitle: Option<&str>,
    contents: impl FnOnce(&mut Ui) -> R,
) -> Option<(bool, R)> {
    chrome::side_panel(ctx, SIDE_ID, open, title, subtitle, contents)
        .map(|(output, r)| (output.scrim_clicked, r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_close_and_switch() {
        let mut s = SideState::default();
        assert!(!s.is_active());
        s.open_at(1);
        assert!(s.is_open() && s.nav.row == 1 && s.nav.in_rows);
        s.close();
        assert!(!s.is_open() && s.is_active(), "still sliding out");
        s.finished();
        assert!(!s.is_active());
        s.open_at(0);
        s.dismiss();
        assert!(!s.is_active(), "a switch does not slide");
    }

    #[test]
    fn only_commands_are_taken_from_events() {
        let events = [
            InputEvent::Scroll(Command::Left),
            InputEvent::Command(Command::Down),
            InputEvent::OpenSettings,
            InputEvent::Command(Command::Select),
        ];
        assert_eq!(commands(&events), vec![Command::Down, Command::Select]);
    }
}
