//! The profile screen (docs/DESIGN.md 17, "Profile screen"): a full-screen page for a player's
//! profile, the player's own or another's. A header with the avatar, the name, how you know each
//! other and a line about them; a few figures on the right; then up to three quiet lists: games,
//! recent trophies and recent activity. Nothing sits in a box at rest: a row is text on the page,
//! and only the focused row (or the one under the pointer) becomes a surface.
//!
//! What is shown is decided in [`view`], which is plain data; this module lays it out and paints
//! it, moves the focus ([`nav`]) and reports what a press asks for.

mod layout;
mod nav;
mod paint;
mod view;

pub use view::{Inputs, OwnGame, OwnLibrary, OwnTrophy, Press, parse_stamp, view};

use anyps5_core::online::api::AccountId;
use egui::{Color32, Context};

use crate::screens::scroll::Follow;
use crate::screens::side::commands;
use crate::ui::Shell;
use crate::ui::chrome::fullscreen_panel;
use crate::ui::glass::GlassStyle;
use crate::ui::input::{Command, InputEvent};
use crate::ui::sound::Cue;

/// What the host has to do after a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileAction {
    /// Back or options: leave the screen.
    Close,
    /// A button or a row was pressed.
    Press(Press),
}

/// The screen: whether it is open, where the focus stands, and the geometry of the last frame.
#[derive(Default)]
pub struct ProfileScreen {
    open: bool,
    closing: bool,
    account: Option<AccountId>,
    /// The focused slot: buttons first, then the rows of the columns.
    focus: usize,
    /// Where the slots were drawn last frame.
    slots: Vec<nav::Slot>,
    /// What pressing each slot does, as of the last frame.
    presses: Vec<Press>,
}

impl ProfileScreen {
    /// Opens the screen on `account`.
    pub fn open(&mut self, account: AccountId) {
        if self.account != Some(account) || !self.open {
            self.focus = 0;
            self.slots.clear();
            self.presses.clear();
        }
        self.account = Some(account);
        self.open = true;
        self.closing = false;
    }

    /// Opens the screen again on `account` after a comparison: the focus stays on its row.
    pub fn reopen(&mut self, account: AccountId) {
        self.account = Some(account);
        self.open = true;
        self.closing = false;
    }

    pub fn close(&mut self) {
        if self.open {
            self.open = false;
            self.closing = true;
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// True while the screen is open or still fading out; the host keeps calling `show`.
    pub fn is_active(&self) -> bool {
        self.open || self.closing
    }

    pub fn account(&self) -> Option<AccountId> {
        self.account
    }

    /// Puts the focus on a slot (the gallery shows a focused row).
    pub fn set_focus(&mut self, slot: usize) {
        self.focus = slot;
    }

    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        inputs: &Inputs<'_>,
        accent: Color32,
    ) -> Vec<ProfileAction> {
        let mut actions = Vec::new();
        if !self.is_active() {
            return actions;
        }
        let model = view(inputs);
        if self.open {
            for command in commands(events) {
                self.command(shell, command, &mut actions);
            }
        }
        let open = self.open;
        let language = inputs.language;
        let focus = self.focus.min(self.slots.len().saturating_sub(1));
        let (follow, scroll_to) = Follow::begin(
            ctx,
            egui::Id::unique("profile-screen-follow"),
            open.then_some(focus).filter(|_| !self.slots.is_empty()),
        );
        let mut clicked = None;
        let mut frame = paint::Drawn::default();
        let drawn = fullscreen_panel(
            ctx,
            "profile-screen",
            open,
            GlassStyle::fullscreen_panel(),
            |ui, _| {
                paint::draw(
                    ui,
                    &model,
                    paint::Settings {
                        language,
                        accent,
                        focus: open.then_some(focus),
                        scroll_to,
                    },
                    &mut frame,
                    &mut clicked,
                )
            },
        );
        match drawn {
            None => self.closing = false,
            Some(()) => {
                follow.end(
                    ctx,
                    frame.geometry.clone(),
                    frame.content_height,
                    frame.viewport_height,
                    frame.offset,
                );
                self.slots = frame.slots;
                self.presses = frame.presses;
            }
        }
        if open && let Some(slot) = clicked {
            if slot != self.focus {
                self.focus = slot;
            }
            self.press(shell, slot, &mut actions);
        }
        actions
    }

    fn command(&mut self, shell: &mut Shell, command: Command, actions: &mut Vec<ProfileAction>) {
        match command {
            Command::Back | Command::Options => {
                shell.play(Cue::Close);
                actions.push(ProfileAction::Close);
            }
            Command::Select => self.press(shell, self.focus, actions),
            Command::Up | Command::Down | Command::Left | Command::Right => {
                if let Some(next) = nav::step(self.focus, command, &self.slots) {
                    self.focus = next;
                    shell.play(Cue::Move);
                }
            }
        }
    }

    fn press(&mut self, shell: &mut Shell, slot: usize, actions: &mut Vec<ProfileAction>) {
        match self.presses.get(slot) {
            Some(Press::None) | None => {}
            Some(press) => {
                shell.play(Cue::Select);
                actions.push(ProfileAction::Press(press.clone()));
            }
        }
    }
}
