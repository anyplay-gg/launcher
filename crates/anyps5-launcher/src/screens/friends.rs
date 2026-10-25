//! The Friends screen (docs/DESIGN.md 17, "Friends screen"): a full screen for everything social,
//! in the family of the profile screen. A header says who you are and how the connection is; a
//! rail on the left names the sections (Friends, Party, Messages, Activity, Settings) and leads on
//! to Your Profile; the page beside it is the page of the section, built from the rows of
//! [`online`](crate::screens::online), the same ones the panel it replaced showed in a drawer.
//!
//! Before sign-in there is no rail: the page stands alone in the middle (sign in, the cloud, the
//! browser). The screen draws the frame and moves the focus between the rail and the page
//! ([`nav`]); the page and its rows are the panel's.

mod nav;
mod paint;

use anyps5_core::online::api::AccountId;
use egui::{Align, Context, Layout, Rect, UiBuilder, pos2, vec2};

use crate::screens::friends_strings::FriendsText as FX;
use crate::screens::online::{
    OnlineAction, OnlineData, OnlinePanel, Page, Section, monogram_colour, status_line,
};
use crate::screens::online_feed_strings::FeedText as F;
use crate::screens::online_social_strings::SocialText as S;
use crate::screens::online_strings::OnlineText as T;
use crate::screens::right_aligned_hints;
use crate::ui::Shell;
use crate::ui::chrome::{RowAvatar, fullscreen_panel};
use crate::ui::components::{ConsoleButton, ControlSize};
use crate::ui::glass::GlassStyle;
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};
use crate::ui::sound::Cue;
use crate::ui::theme::pad;
use nav::{Focus, Step, Zone};

/// The entries of the rail: the sections, then Your Profile.
const SECTIONS: usize = Section::ALL.len();
const RAIL: usize = SECTIONS + 1;
/// The entry that opens the player's own profile.
const PROFILE: usize = SECTIONS;

/// The screen: the panel with the pages, and where the focus stands around it.
#[derive(Default)]
pub struct FriendsScreen {
    panel: OnlinePanel,
    zone: Option<Zone>,
    rail: usize,
    /// Back was pressed with the mouse: the page takes it with the next frame's commands.
    queued: Vec<InputEvent>,
}

impl FriendsScreen {
    fn focus(&self) -> Focus {
        Focus {
            zone: self.zone.unwrap_or(Zone::Page),
            rail: self.rail,
        }
    }

    /// Opens on the page that fits the state: the game's dialog first, then sign-in or the friends.
    pub fn open(&mut self, data: &OnlineData<'_>) {
        self.panel.open(data);
        self.land(data);
    }

    /// Opens again on the page it was on: the screen was left for a profile, and Back from there
    /// leads here.
    pub fn resume(&mut self, data: &OnlineData<'_>) {
        self.panel.resume(data);
        self.land(data);
    }

    /// Shows `page` (a profile sent the player to one; previews jump straight to one).
    pub fn show_page(&mut self, page: Page, data: &OnlineData<'_>) {
        self.panel.show_page(page, data);
        self.land(data);
    }

    /// The focus starts on the page, with the rail on its section.
    fn land(&mut self, _data: &OnlineData<'_>) {
        self.zone = Some(Zone::Page);
        self.queued.clear();
        if let Some(section) = self.panel.section() {
            self.rail = index_of(section);
        }
    }

    /// Slides out.
    pub fn close(&mut self) {
        self.panel.close();
    }

    /// Leaves at once, without the fade: another screen takes over.
    pub fn dismiss(&mut self) {
        self.panel.dismiss();
    }

    pub fn is_active(&self) -> bool {
        self.panel.is_active()
    }

    pub fn is_open(&self) -> bool {
        self.panel.is_open()
    }

    /// The page shows games, avatars or trophy names of this PC: the host builds a
    /// [`Local`](crate::screens::online::Local) for it every frame.
    pub fn wants_local(&self) -> bool {
        self.panel.wants_local()
    }

    pub fn set_local(&mut self, local: crate::screens::online::Local) {
        self.panel.set_local(local);
    }

    /// Puts the focus of the activity page on a card, or on one of its reactions (previews).
    pub fn set_feed_focus(&mut self, item: usize, slot: Option<usize>) {
        self.panel.set_feed_focus(item, slot);
    }

    /// Waits for the key of the talk-key row (previews).
    pub fn begin_capture(&mut self) {
        self.panel.begin_capture();
    }

    /// Puts the focus on the `nth` row with this id of the page (previews).
    pub fn focus_row(
        &mut self,
        id: crate::screens::rows::RowId,
        nth: usize,
        data: &OnlineData<'_>,
    ) {
        self.panel.focus_row(id, nth, data);
    }

    /// Puts the focus on the rail (previews).
    pub fn focus_rail(&mut self) {
        self.zone = Some(Zone::Rail);
    }

    pub fn page(&self) -> &Page {
        self.panel.page()
    }

    /// Runs one frame: the commands, the frame around the page, the page.
    pub fn show(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &OnlineData<'_>,
    ) -> Vec<OnlineAction> {
        let mut actions = Vec::new();
        if !self.panel.is_active() {
            let idle = self.panel.update(shell, ctx, events, data);
            actions.extend(idle);
            return actions;
        }
        let open = self.panel.is_open();
        let section = self.panel.section();
        let has_rail = data.online.is_signed_in() && section.is_some();
        if !has_rail {
            self.zone = Some(Zone::Page);
        }
        if self.focus().zone == Zone::Page
            && let Some(section) = section
        {
            self.rail = index_of(section);
        }

        let mut pass: Vec<InputEvent> = Vec::new();
        if open {
            let mut all = std::mem::take(&mut self.queued);
            all.extend(events.iter().cloned());
            for event in all {
                match event {
                    InputEvent::Command(command) if has_rail && !self.panel.capturing() => {
                        let keeps_left = !self.panel.left_leaves(data);
                        match nav::step(self.focus(), command, RAIL, SECTIONS, keeps_left) {
                            Step::Stay => {}
                            Step::Moved(focus) => {
                                shell.play(Cue::Move);
                                let moved = focus.rail != self.rail;
                                self.zone = Some(focus.zone);
                                self.rail = focus.rail;
                                // Walking the rail shows the section it stops on.
                                if moved && focus.zone == Zone::Rail && focus.rail < SECTIONS {
                                    self.panel.go(Section::ALL[focus.rail].root(), data);
                                }
                            }
                            Step::Press(entry) => {
                                shell.play(Cue::Select);
                                if entry == PROFILE
                                    && let Some(me) = data.online.account.as_ref()
                                {
                                    actions.push(OnlineAction::OpenProfile(me.id));
                                }
                            }
                            Step::Page(command) => pass.push(InputEvent::Command(command)),
                            Step::Leave => {
                                shell.play(Cue::Back);
                                actions.push(OnlineAction::Close);
                                self.panel.close();
                            }
                        }
                    }
                    other => pass.push(other),
                }
            }
        } else {
            self.queued.clear();
        }
        // The rail has the focus: the page takes the other events (a captured key) but no
        // command, and draws without a focused row.
        let page_focus = self.focus().zone == Zone::Page;
        actions.extend(self.panel.update(shell, ctx, &pass, data));

        let language = data.language;
        let rail_focus = (open && self.focus().zone == Zone::Rail).then_some(self.rail);
        let mut clicked_rail = None;
        let mut back_clicked = false;
        let mut page_actions = Vec::new();
        let drawn = fullscreen_panel(
            ctx,
            "friends-screen",
            open,
            GlassStyle::fullscreen_panel(),
            |ui, _| {
                let screen = ui.max_rect();
                let area = screen.shrink2(vec2(pad::TROPHIES.0, pad::TROPHIES.1));
                right_aligned_hints(
                    ui,
                    Rect::from_min_size(area.min, vec2(area.width(), 26.0)),
                    language,
                );
                let mut back = ui.new_child(
                    UiBuilder::new()
                        .max_rect(Rect::from_min_size(area.min, vec2(200.0, 32.0)))
                        .layout(Layout::left_to_right(Align::Center)),
                );
                if ConsoleButton::new(language.text(Key::Back))
                    .icon(Icon::CARET_LEFT)
                    .size(ControlSize::Small)
                    .show(&mut back)
                    .clicked()
                {
                    back_clicked = true;
                }
                let mut top = area.min.y + 32.0 + 28.0;
                let (page_left, page_width) = if has_rail {
                    let me = data.online.account.as_ref();
                    let handle = me.map_or("", |a| a.handle.as_str());
                    let status = status_line(data);
                    let name = format!("{handle}, {status}");
                    let hash = me
                        .and_then(|a| data.online.avatars.get(&a.id))
                        .and_then(|h| h.as_deref());
                    let avatar = RowAvatar {
                        texture: self.panel.avatar_texture(hash),
                        colour: monogram_colour(me.map_or(AccountId(0), |a| a.id)),
                        initial: handle.chars().next(),
                        dot: None,
                        away: false,
                    };
                    paint::header(
                        ui,
                        pos2(area.min.x, top),
                        area.width(),
                        &paint::Header {
                            handle,
                            status: &status,
                            connected: data.online.is_online() && !data.online.appears_offline(),
                            avatar,
                            accent: data.accent,
                            name: &name,
                        },
                    );
                    top += paint::HEADER_HEIGHT + 40.0;
                    let entries = rail_entries(data, section, language);
                    clicked_rail = paint::rail(ui, pos2(area.min.x, top), &entries, rail_focus);
                    let left = area.min.x + paint::RAIL_WIDTH + paint::RAIL_GAP;
                    (left, (area.max.x - left).min(paint::PAGE_MAX))
                } else {
                    // Signing in: the page stands in the middle.
                    let width = area.width().min(640.0);
                    (area.center().x - width * 0.5, width)
                };
                let (title, note) = self.panel.heading(data);
                let note = note.as_deref();
                let mut head = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_size(
                    pos2(page_left, top),
                    vec2(page_width, 80.0),
                )));
                let used = paint::page_title(
                    &mut head,
                    pos2(page_left + crate::ui::theme::size::LIST_INSET, top),
                    page_width - 2.0 * crate::ui::theme::size::LIST_INSET,
                    &title,
                    note,
                );
                let list_top = top + used + 22.0;
                let list = Rect::from_min_max(
                    pos2(page_left, list_top),
                    pos2(page_left + page_width, screen.max.y),
                );
                let mut child = ui.new_child(UiBuilder::new().max_rect(list));
                page_actions = self.panel.draw(shell, ctx, &mut child, data, page_focus);
            },
        );
        if drawn.is_none() {
            self.panel.finished();
        }
        actions.extend(page_actions);
        if open {
            if let Some(entry) = clicked_rail {
                shell.play(Cue::Select);
                if entry == PROFILE {
                    if let Some(me) = data.online.account.as_ref() {
                        actions.push(OnlineAction::OpenProfile(me.id));
                    }
                } else {
                    self.rail = entry;
                    self.zone = Some(Zone::Page);
                    self.panel.go(Section::ALL[entry].root(), data);
                }
            }
            if back_clicked {
                shell.play(Cue::Back);
                self.queued.push(InputEvent::Command(Command::Back));
                ctx.request_repaint();
            }
        }
        actions.extend(self.panel.end_frame(shell, data));
        actions
    }
}

fn index_of(section: Section) -> usize {
    Section::ALL.iter().position(|s| *s == section).unwrap_or(0)
}

/// The entries of the rail with what waits in each.
fn rail_entries(
    data: &OnlineData<'_>,
    current: Option<Section>,
    language: Language,
) -> Vec<paint::Entry> {
    let online = data.online;
    let social = &online.social;
    let friends = (online.friends.incoming.len() + online.invitations.len()) as u32;
    let party = social.party_invitations.len() as u32;
    let messages = social.unread_total() as u32;
    let mut entries: Vec<paint::Entry> = Section::ALL
        .iter()
        .map(|section| {
            let (icon, label, waiting) = match section {
                Section::Friends => (Icon::USERS, T::Friends.text(language), friends),
                Section::Party => (Icon::GAME_CONTROLLER, S::Party.text(language), party),
                Section::Messages => (Icon::ENVELOPE_SIMPLE, S::Messages.text(language), messages),
                Section::Activity => (Icon::LIST, FX::Activity.text(language), 0),
                Section::Settings => (Icon::GEAR_SIX, FX::Settings.text(language), 0),
            };
            paint::Entry {
                icon,
                label: label.to_owned(),
                waiting,
                current: current == Some(*section),
                action: false,
                name: waiting_name(label, waiting, language),
            }
        })
        .collect();
    let profile = F::YourProfile.text(language);
    entries.push(paint::Entry {
        icon: Icon::USER_CIRCLE,
        label: profile.to_owned(),
        waiting: 0,
        current: false,
        action: true,
        name: profile.to_owned(),
    });
    entries
}

fn waiting_name(label: &str, waiting: u32, language: Language) -> String {
    if waiting == 0 {
        label.to_owned()
    } else {
        FX::WaitingName.format(language, &[Arg::Str(label), Arg::Int(i64::from(waiting))])
    }
}

#[cfg(test)]
mod tests;
