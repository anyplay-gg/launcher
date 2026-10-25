//! "Online": signing in to the AnyPlay network, friends, invitations and joining a friend's game.
//! One side panel on the game sheet's frame, with pages that swap in place. The panel only shows
//! and asks; the net service acts, and the host turns the panel's actions into commands for it.

mod feed;
mod local;
mod profile;
mod restriction;
pub mod screen;
mod social;
mod voice;

pub use feed::{ago_text, monogram_colour, paint_avatar, rarity_text, sentence, theme_grade};
pub use local::{Avatar, Local, OwnedGame};
pub use voice::talk_key;

use std::collections::BTreeSet;

use anyps5_core::online::api::{AccountId, Audience, Count, Reaction, ReportReason};
use anyps5_core::online::{
    self, Cloud, DeviceKind, Link, Online, Pending, Purpose, ScreenCommand, VoiceMode,
    VoiceProvider,
};
use egui::{Color32, Context, Ui};

use crate::screens::friends_strings::FriendsText as FX;
use crate::screens::online_call_strings::{self as call_text, CallText as CT};
use crate::screens::online_feed_strings::FeedText as F;
use crate::screens::online_strings::OnlineText as T;
use crate::screens::online_voice_strings::VoiceText as V;
use crate::screens::rows::{self, Change, Row, RowId};
use crate::screens::side::{self, SideState};
use crate::ui::Shell;
use crate::ui::chrome::RowAvatar;
use crate::ui::focus::RowKind;
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};
use crate::ui::sound::Cue;

/// What the panel asks the host to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnlineAction {
    /// Start the selected game with a Play Together session.
    PlayTogether,
    SetCloud(String),
    /// Back to the cloud AnyPlay runs.
    UseOfficialCloud,
    /// Sign in, or create an account, on the cloud's page in the system browser.
    BrowserSignIn {
        create: bool,
    },
    /// Open the sign-in page again, as the net service gave it.
    OpenSignInPage,
    /// Stop waiting for the browser.
    CancelSignIn,
    /// Start the net service again: the connection is out.
    Reconnect,
    /// Open the account web: e-mail, passkeys, devices and deleting the account.
    ManageAccount,
    /// Appeal the mute an action made.
    Appeal {
        action: u64,
        text: String,
    },
    AddFriend(String),
    Accept(AccountId),
    Decline(AccountId),
    RemoveFriend(AccountId),
    Block(AccountId),
    Unblock(AccountId),
    Report(AccountId, ReportReason),
    /// Join the session a friend plays in.
    Join(AccountId),
    /// Invite a friend to the player's session.
    Invite(AccountId),
    AnswerInvitation {
        id: String,
        accept: bool,
    },
    SignOut,
    /// The friends picked for the game's invitation dialog, or `None` when it was closed.
    Dialog(Option<Vec<AccountId>>),
    /// Keep every tunnel on a relay, or let them go direct again.
    RelayOnly(bool),
    /// Invite friends to the party; a party is made when the player is in none.
    PartyInvite(Vec<AccountId>),
    PartyAnswer {
        id: String,
        accept: bool,
    },
    PartyLeave,
    PartyKick(AccountId),
    PartyLeader(AccountId),
    /// Invite the party into the session the player is in.
    PartyMove(String),
    /// Open the conversation with a friend.
    OpenDirect(AccountId),
    /// A conversation page opened: fetch what it lacks.
    OpenThread(String),
    /// Read further back in a conversation.
    LoadEarlier(String),
    /// The text typed for a conversation and not sent.
    Draft {
        conversation: String,
        text: String,
    },
    SendMessage(String),
    /// The player looks at a conversation: its newest message counts as read.
    MarkRead(String),
    AcceptRequest(String),
    /// Leave a group, or decline a message request.
    LeaveConversation(String),
    /// Put a message's text on the clipboard.
    CopyText(String),
    /// Appear offline to every other player, or show again.
    AppearOffline(bool),
    /// Who sees the player's trophies.
    TrophyAudience(Audience),
    /// The Feed page opened or asks again: read its newest items.
    FeedRefresh,
    /// Read the next older items of the feed.
    FeedOlder,
    /// Set the player's reaction to a feed item, or take it back.
    React(u64, Option<Reaction>),
    /// Read the profile of this account (the profile screen asks again).
    Profile(AccountId),
    /// Open the profile screen on this account.
    OpenProfile(AccountId),
    /// Call a friend, or add the friend to the call the player leads.
    Call(AccountId),
    /// The player looked at a friend's missed calls.
    CallsSeen(AccountId),
    /// The leader sets the party's Discord link; empty removes it.
    PartyLink(String),
    /// Open a Discord link in the system browser.
    OpenDiscord(String),
    // Party voice
    Voice(bool),
    VoiceProvider(VoiceProvider),
    VoiceMode(VoiceMode),
    /// The key held to talk, by its egui name.
    VoiceKey(String),
    /// A microphone or speakers by name; `None` is the system's default.
    VoiceDevice(DeviceKind, Option<String>),
    RefreshVoiceDevices,
    PeerVolume(AccountId, u16),
    PeerMute(AccountId, bool),
    /// Share screen: start, stop, watch a member's stream, leave it.
    Screen(ScreenCommand),
    Close,
}

/// The pages of the panel.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Start,
    Cloud,
    /// Signing in in the browser.
    Browser,
    Home,
    Friend(AccountId),
    /// Every friend request that waits, each with its answers.
    Requests,
    Request(AccountId),
    Outgoing(AccountId),
    Blocked(AccountId),
    Invitation(String),
    AddFriend,
    Account,
    Report(AccountId),
    Picker,
    Party,
    PartyMember(AccountId),
    /// Pick friends to invite to the party.
    PartyPicker,
    PartyInvitation(String),
    Messages,
    Conversation(String),
    MessageRequest(String),
    /// One message in full: copy it, report it.
    Message(String, u64),
    /// Party voice: the switch, the mode, the talk key and the devices.
    Voice,
    /// The microphones or the speakers to choose from.
    VoiceDevices(DeviceKind),
    /// What limits the account, by the kind of the restriction (`muted`, `banned` ...).
    Restriction(String),
    /// What the player and their friends did.
    Feed,
}

/// The sections of the Friends screen: one entry of its rail each, with a page of its own to start
/// on. Every page belongs to one of them (or to none: signing in).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Friends,
    Party,
    Messages,
    Activity,
    Settings,
}

impl Section {
    pub const ALL: [Section; 5] = [
        Section::Friends,
        Section::Party,
        Section::Messages,
        Section::Activity,
        Section::Settings,
    ];

    /// The page a section starts on.
    pub fn root(self) -> Page {
        match self {
            Section::Friends => Page::Home,
            Section::Party => Page::Party,
            Section::Messages => Page::Messages,
            Section::Activity => Page::Feed,
            Section::Settings => Page::Account,
        }
    }
}

impl Page {
    /// The section a page belongs to; `None` for the pages before sign-in. A restriction page
    /// belongs to where it was opened from, which the panel knows (`OnlinePanel::section`).
    pub fn section(&self) -> Option<Section> {
        match self {
            Page::Start | Page::Cloud | Page::Browser => None,
            Page::Home
            | Page::Friend(_)
            | Page::Requests
            | Page::Request(_)
            | Page::Outgoing(_)
            | Page::Invitation(_)
            | Page::AddFriend
            | Page::Report(_)
            | Page::Picker => Some(Section::Friends),
            Page::Party
            | Page::PartyMember(_)
            | Page::PartyPicker
            | Page::PartyInvitation(_)
            | Page::Voice
            | Page::VoiceDevices(_) => Some(Section::Party),
            Page::Messages
            | Page::Conversation(_)
            | Page::MessageRequest(_)
            | Page::Message(..) => Some(Section::Messages),
            Page::Feed => Some(Section::Activity),
            Page::Account | Page::Blocked(_) | Page::Restriction(_) => Some(Section::Settings),
        }
    }

    /// The first page of a section: Back from it leaves the screen.
    pub fn is_root(&self) -> bool {
        matches!(
            self,
            Page::Start
                | Page::Home
                | Page::Picker
                | Page::Party
                | Page::Messages
                | Page::Feed
                | Page::Account
        )
    }
}

/// What the panel shows, borrowed for one frame.
pub struct OnlineData<'a> {
    pub online: &'a Online,
    /// The cloud in use: the one the player picked, else AnyPlay's own.
    pub cloud: Option<&'a str>,
    /// The player picked a cloud of their own.
    pub custom_cloud: bool,
    /// AnyPlay runs a cloud this build knows.
    pub official_cloud: bool,
    /// Tunnels to other players go only through a relay (`Preferences::online_relay_only`).
    pub relay_only: bool,
    pub language: Language,
    pub accent: Color32,
    /// The time now, in Unix seconds: the times of messages show their day when it is another.
    pub now: i64,
    /// The local UTC offset for the times of messages; `None` asks the system for each.
    pub utc_offset: Option<i32>,
    /// A game is running: it has the keyboard, so push to talk cannot reach the launcher.
    pub game_running: bool,
    /// The selected game can be played together and starts now: its name, for the party's
    /// Play Together row.
    pub play_together: Option<&'a str>,
}

/// The choices of "Who Sees Your Trophies", in the order the row shows them.
const AUDIENCES: [Audience; 3] = [Audience::Friends, Audience::Everyone, Audience::Nobody];

const REASONS: [ReportReason; 5] = [
    ReportReason::Cheating,
    ReportReason::Harassment,
    ReportReason::Spam,
    ReportReason::Name,
    ReportReason::Other,
];

/// The side panel and what is typed into it.
#[derive(Default)]
pub struct OnlinePanel {
    side: SideState,
    pub page: Page,
    pub cloud_text: String,
    pub friend_text: String,
    /// The appeal being written.
    appeal_text: String,
    reason: usize,
    picked: BTreeSet<AccountId>,
    /// How many messages the conversation page shows.
    shown: usize,
    /// Which answer of a request row the keys would press: 0 Accept, 1 Decline.
    request_answer: usize,
    /// Which quick action of a friend's row the keys would press.
    friend_action: Option<usize>,
    /// The page the player came from.
    previous: Option<Page>,
    /// Where Back from the report page leads, when it did not open from a friend.
    report_from: Option<Page>,
    /// The friend whose conversation was asked for; the page opens it once it is there.
    opening: Option<AccountId>,
    /// Actions that a page change asks for, handed out with the next frame's.
    queued: Vec<OnlineAction>,
    /// The row the focus stands on, for a page whose rows change under it.
    anchor: Option<social::Anchor>,
    /// The Discord link being written for the party.
    link_text: String,
    /// The talk key row waits for a key.
    capturing: bool,
    /// What the shell's key capture was last set to by this panel.
    capture_synced: bool,
    /// What the profile page and the feed take from this PC (see [`Local`]).
    local: Local,
    /// Where the focus stands in the feed's cards.
    feed_focus: feed::FeedFocus,
    /// The item the feed's focus is on, to keep it there when items come in above it.
    feed_anchor: Option<u64>,
}

impl OnlinePanel {
    /// Opens on the page that fits the state: the game's dialog first, then sign-in or the
    /// friends.
    pub fn open(&mut self, data: &OnlineData<'_>) {
        let page = if data.online.dialog.is_some() && data.online.is_signed_in() {
            Page::Picker
        } else {
            entry_page(data)
        };
        self.go(page, data);
    }

    pub fn close(&mut self) {
        self.side.close();
    }

    /// Opens again on the page it was on and the row it was on: the panel was left for the profile
    /// screen, and Back from there leads here.
    pub fn resume(&mut self, data: &OnlineData<'_>) {
        let row = self.side.nav.row;
        self.follow(data);
        self.side.open_at(row);
    }

    pub fn dismiss(&mut self) {
        self.side.dismiss();
    }

    pub fn is_active(&self) -> bool {
        self.side.is_active()
    }

    /// The section the page on screen belongs to. A restriction page belongs to the page it was
    /// opened from.
    pub fn section(&self) -> Option<Section> {
        match (&self.page, &self.previous) {
            (Page::Restriction(_), Some(from)) if from.section().is_some() => from.section(),
            (page, _) => page.section(),
        }
    }

    /// The page on screen.
    pub fn page(&self) -> &Page {
        &self.page
    }

    /// The title and the line under it of the page on screen.
    pub fn heading(&self, data: &OnlineData<'_>) -> (String, Option<String>) {
        let (title, subtitle, _) = self.rows(data);
        (title, subtitle)
    }

    /// Puts the focus on the `nth` row with this id (previews).
    pub fn focus_row(&mut self, id: RowId, nth: usize, data: &OnlineData<'_>) {
        let rows = self.rows(data).2;
        if let Some(row) = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.id == id)
            .nth(nth)
            .map(|(i, _)| i)
        {
            self.side.nav.row = row;
        }
    }

    /// The talk key row waits for a key.
    pub fn capturing(&self) -> bool {
        self.capturing
    }

    /// The picture of an avatar hash, once this PC has it.
    pub fn avatar_texture(&self, hash: Option<&str>) -> Option<egui::TextureId> {
        self.local.avatar(hash).map(|a| a.texture)
    }

    /// Whether Left on the focused row would only bump into its start: then the screen takes it
    /// to the rail. A row that steps (a choice, buttons) keeps it while it still has a step to go.
    pub fn left_leaves(&self, data: &OnlineData<'_>) -> bool {
        if self.capturing {
            return false;
        }
        if self.feed_is_list(data) {
            return self.feed_focus.slot.is_none();
        }
        let rows = self.rows(data).2;
        match rows.get(self.side.nav.row).map(|r| &r.control) {
            Some(
                rows::Control::Choice { selected, .. } | rows::Control::Buttons { selected, .. },
            ) => *selected == 0,
            Some(rows::Control::Stepper { value, min, .. }) => value <= min,
            Some(rows::Control::Toggle(_)) => true,
            _ => true,
        }
    }

    /// Back would leave the screen from this page.
    pub fn back_closes(&self, data: &OnlineData<'_>) -> bool {
        self.back_page(data).is_none()
    }

    pub fn is_open(&self) -> bool {
        self.side.is_open()
    }

    /// The page shows games, avatars or trophy names of this PC: the host builds a [`Local`] for
    /// it every frame.
    pub fn wants_local(&self) -> bool {
        self.side.is_active()
    }

    pub fn set_local(&mut self, local: Local) {
        self.local = local;
    }

    /// Puts the feed's focus on a card, or on one of its reactions (previews).
    pub fn set_feed_focus(&mut self, item: usize, slot: Option<usize>) {
        self.feed_focus = feed::FeedFocus { item, slot };
    }

    /// Shows `page` (previews jump straight to one).
    pub fn show_page(&mut self, page: Page, data: &OnlineData<'_>) {
        self.go(page, data);
    }

    /// Shows `page`, focused on its first row that takes focus.
    pub fn go(&mut self, page: Page, data: &OnlineData<'_>) {
        if page == Page::Cloud {
            self.cloud_text = data.cloud.unwrap_or_default().to_owned();
        }
        if page == Page::Picker {
            self.picked.clear();
        }
        if matches!(page, Page::Report(_)) {
            self.reason = 0;
        }
        if matches!(page, Page::Restriction(_)) && self.page != page {
            self.clear_appeal();
        }
        match &page {
            // Looking at a friend is seeing their missed calls.
            Page::Friend(id) if data.online.calls.missed_from(*id).is_some() => {
                self.queued.push(OnlineAction::CallsSeen(*id));
            }
            Page::Conversation(id) | Page::MessageRequest(id) => {
                if self.page != page && !matches!(self.page, Page::Message(..)) {
                    self.shown = social::SHOWN;
                }
                self.queued.push(OnlineAction::OpenThread(id.clone()));
            }
            Page::Feed => {
                // Coming back from a profile keeps the place in the feed.
                if self.page != Page::Feed {
                    self.feed_focus = feed::FeedFocus::default();
                    self.feed_anchor = None;
                }
                self.queued.push(OnlineAction::FeedRefresh);
                if let Some(me) = data.online.account.as_ref() {
                    self.queued.push(OnlineAction::Profile(me.id));
                }
            }
            _ => {}
        }
        // A message and its chat are one place: going between them is no step away from where
        // the chat was opened.
        let in_chat = |page: &Page| matches!(page, Page::Conversation(_) | Page::MessageRequest(_));
        let within_chat = (matches!(self.page, Page::Message(..)) && in_chat(&page))
            || (in_chat(&self.page) && matches!(page, Page::Message(..)));
        if self.page != page && !within_chat {
            self.previous = Some(std::mem::take(&mut self.page));
        }
        self.anchor = None;
        self.capturing = false;
        self.page = page;
        let rows = self.rows(data).2;
        // A page opens on its first row that takes focus, a conversation on its field. A request
        // opens on its first message: read before answering.
        let wanted = match self.page {
            Page::Conversation(_) => "compose",
            _ => "",
        };
        let first = rows::position(&rows, wanted).unwrap_or_else(|| {
            rows::kinds(&rows)
                .iter()
                .position(|kind| *kind != RowKind::Info)
                .unwrap_or(0)
        });
        self.side.open_at(first);
    }

    /// Follows the service: a finished sign-in leads to the friends, a sign-out back to the
    /// start, and a page whose friend or invitation is gone back to the list.
    fn follow(&mut self, data: &OnlineData<'_>) {
        let online = data.online;
        self.settle_focus(data);
        let wanted = match &self.page {
            Page::Restriction(_) => self.restriction_follow(data),
            Page::Start | Page::Browser if online.is_signed_in() => Some(Page::Home),
            Page::Start if online.pending == Some(Pending::Browser) => Some(Page::Browser),
            Page::Browser if online.pending.is_none() => Some(Page::Start),
            Page::Cloud | Page::Start | Page::Browser => None,
            _ if !online.is_signed_in() => Some(Page::Start),
            Page::Friend(id) if online.friend(*id).is_none() => Some(Page::Home),
            Page::Requests if online.friends.incoming.is_empty() => Some(Page::Home),
            Page::Request(id) if !online.friends.incoming.iter().any(|r| r.account.id == *id) => {
                Some(Page::Home)
            }
            Page::Outgoing(id) if !online.friends.outgoing.iter().any(|r| r.account.id == *id) => {
                Some(Page::Home)
            }
            Page::Blocked(id) if !online.friends.blocked.iter().any(|a| a.id == *id) => {
                Some(Page::Account)
            }
            Page::Invitation(id) if !online.invitations.iter().any(|i| &i.id == id) => {
                Some(Page::Home)
            }
            Page::Picker if online.dialog.is_none() => Some(Page::Home),
            _ => self.social_follow(data),
        };
        if let Some(page) = wanted {
            self.go(page, data);
        }
    }

    /// The page that Back leads to, or `None` to close the panel.
    fn back_page(&self, data: &OnlineData<'_>) -> Option<Page> {
        match self.page {
            Page::Start | Page::Home | Page::Picker | Page::Feed | Page::Account => None,
            Page::Cloud => Some(Page::Start),
            // Leaving the page leaves the sign-in waiting; Cancel stops it.
            Page::Browser => Some(entry_page(data)).filter(|p| *p != self.page),
            Page::Blocked(_) => Some(Page::Account),
            Page::Voice | Page::VoiceDevices(_) => self.voice_back(),
            Page::Restriction(_) => Some(self.restriction_back(data)),
            Page::Report(id) => Some(self.report_from.clone().unwrap_or(Page::Friend(id))),
            _ => self.social_back(data).unwrap_or(Some(Page::Home)),
        }
    }

    /// Runs the frame's commands on the page (the focus, the rows, Back) and follows the service.
    /// Call it before [`OnlinePanel::draw`]; `commands` are the commands the page may take.
    pub fn update(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        events: &[InputEvent],
        data: &OnlineData<'_>,
    ) -> Vec<OnlineAction> {
        let mut actions = Vec::new();
        if !self.side.is_open() {
            self.capturing = false;
            self.sync_capture(shell);
            return actions;
        }
        self.follow(data);
        if self.anchor.is_some() {
            let rows = self.rows(data).2;
            self.restore_anchor(data, &rows);
        }
        if let Page::Conversation(id) = &self.page
            && data.online.social.read_target(id).is_some()
        {
            actions.push(OnlineAction::MarkRead(id.clone()));
        }
        let capturing = self.capturing;
        if capturing {
            self.capture(events, &mut actions);
        }
        for command in side::commands(events) {
            if capturing {
                break;
            }
            if self.feed_is_list(data) {
                if self.feed_command(shell, command, data, &mut actions) {
                    self.step_back(shell, data, &mut actions);
                    break;
                }
                continue;
            }
            // Back goes up one page with the "back" cue; leaving the screen is answered by the
            // app, which knows whether it lands on Home ("close") or on another screen ("back").
            if matches!(command, Command::Back | Command::Options) {
                self.step_back(shell, data, &mut actions);
                break;
            }
            let rows = self.rows(data).2;
            let driven = rows::drive(shell, &mut self.side.nav, &rows, &[command]);
            for change in &driven.changes {
                self.handle(ctx, change, data, &mut actions);
            }
            if driven.close {
                self.back(data, &mut actions);
                break;
            }
        }
        actions
    }

    /// Draws the page's rows (or the feed's cards) into `ui` and applies what the mouse did.
    /// `focused` is false while the focus stands elsewhere on the screen (the rail).
    pub fn draw(
        &mut self,
        shell: &mut Shell,
        ctx: &Context,
        ui: &mut Ui,
        data: &OnlineData<'_>,
        focused: bool,
    ) -> Vec<OnlineAction> {
        let mut actions = Vec::new();
        let open = self.side.is_open();
        let (_, _, mut rows) = self.rows(data);
        let opts = rows::ListOptions {
            accent: data.accent,
            language: data.language,
            focus: focused.then_some(self.side.nav.row),
            force: None,
        };
        // Each kind of page keeps its own scroll: walking the rail never lands on an old offset.
        let list_id = ("online-rows", std::mem::discriminant(&self.page));
        let events = match self.page.clone() {
            Page::Feed if self.feed_is_list(data) => {
                self.feed_draw(ui, data, open && focused, &mut actions);
                Vec::new()
            }
            Page::Feed => {
                let (icon, title, note) = self.feed_state(data);
                let language = data.language;
                rows::list_with_header(ui, list_id, &mut rows, &opts, |ui| {
                    feed::paint_state(
                        ui,
                        icon,
                        title.text(language),
                        note.map(|n| n.text(language)),
                    );
                })
            }
            _ => rows::list(ui, list_id, &mut rows, &opts),
        };
        if open {
            let (changes, texts) = rows::apply_mouse(shell, &mut self.side.nav, &rows, events);
            for (row, value) in texts {
                self.typed(row, value, &mut actions);
            }
            for change in &changes {
                self.handle(ctx, change, data, &mut actions);
            }
        }
        actions
    }

    /// After the draw: where the focus stands now, what a page change asked for, the key capture.
    pub fn end_frame(&mut self, shell: &mut Shell, data: &OnlineData<'_>) -> Vec<OnlineAction> {
        self.anchor = if self.side.is_open() {
            let rows = self.rows(data).2;
            self.anchor_now(data, &rows)
        } else {
            None
        };
        let actions = std::mem::take(&mut self.queued);
        self.sync_capture(shell);
        actions
    }

    /// The panel has finished closing (the screen is gone).
    pub fn finished(&mut self) {
        self.side.finished();
    }

    /// Hands the keys to the talk key row while it waits, and gives them back after.
    fn sync_capture(&mut self, shell: &mut Shell) {
        if self.capturing != self.capture_synced {
            shell.input.capture_keys = self.capturing;
            self.capture_synced = self.capturing;
        }
    }

    /// One step back: up a page with the "back" cue, or out of the screen without one.
    fn step_back(
        &mut self,
        shell: &mut Shell,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) {
        if self.back_page(data).is_some() {
            shell.play(Cue::Back);
        }
        self.back(data, actions);
    }

    fn back(&mut self, data: &OnlineData<'_>, actions: &mut Vec<OnlineAction>) {
        match self.back_page(data) {
            Some(page) => self.go(page, data),
            None => self.close_panel(data, actions),
        }
    }

    fn close_panel(&mut self, data: &OnlineData<'_>, actions: &mut Vec<OnlineAction>) {
        if self.page == Page::Picker && data.online.dialog.is_some() {
            actions.push(OnlineAction::Dialog(None));
        }
        self.side.close();
        actions.push(OnlineAction::Close);
    }

    fn typed(&mut self, row: RowId, value: String, actions: &mut Vec<OnlineAction>) {
        let field = match row {
            "cloud" => &mut self.cloud_text,
            "friend" => &mut self.friend_text,
            "appeal" => &mut self.appeal_text,
            "discordLink" => &mut self.link_text,
            "compose" => {
                if let Page::Conversation(id) = &self.page {
                    actions.push(OnlineAction::Draft {
                        conversation: id.clone(),
                        text: value,
                    });
                }
                return;
            }
            _ => return,
        };
        *field = value;
    }

    /// The item of a list row: the focused row's place among the rows with the same id.
    fn nth(&self, data: &OnlineData<'_>, id: RowId) -> usize {
        let rows = self.rows(data).2;
        let first = rows::position(&rows, id).unwrap_or(0);
        self.side.nav.row.saturating_sub(first)
    }

    fn handle(
        &mut self,
        ctx: &Context,
        change: &Change,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) {
        let online = data.online;
        if self.voice_change(change, actions) {
            return;
        }
        match change {
            Change::Edit { .. } => {
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique("panel-text-focus"), true));
                ctx.request_repaint();
            }
            Change::Toggle {
                row: "relayOnly",
                on,
            } => actions.push(OnlineAction::RelayOnly(*on)),
            Change::Toggle {
                row: "appearOffline",
                on,
            } => actions.push(OnlineAction::AppearOffline(*on)),
            Change::Toggle {
                row: "pickParty",
                on,
            } => {
                self.social_toggle("pickParty", *on, data);
            }
            Change::Submit { row: "compose" } => {
                self.run("send", data, actions);
                // The field stays ready for the next message.
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique("panel-text-focus"), true));
                ctx.request_repaint();
            }
            Change::Submit { row: "friend" } => self.run("sendRequest", data, actions),
            Change::Toggle { row: "pick", on } => {
                let index = self.nth(data, "pick");
                if let Some(friend) = picker_friends(online).get(index) {
                    if *on {
                        self.picked.insert(friend.account.id);
                    } else {
                        self.picked.remove(&friend.account.id);
                    }
                }
            }
            Change::Choice {
                row: "reason",
                index,
            } => self.reason = *index,
            Change::Choice {
                row: "trophyAudience",
                index,
            } => {
                if let Some(audience) = AUDIENCES.get(*index) {
                    actions.push(OnlineAction::TrophyAudience(*audience));
                }
            }
            Change::Choice {
                row: "requestRow",
                index,
            } => self.request_answer = *index,
            Change::Press {
                row: "requestRow",
                index,
            } => {
                let item = self.nth(data, "requestRow");
                if let Some(request) = online.friends.incoming.get(item) {
                    let id = request.account.id;
                    match index {
                        0 => actions.push(OnlineAction::Accept(id)),
                        1 => actions.push(OnlineAction::Decline(id)),
                        // The row's body: the request's own page, where it can be blocked too.
                        _ => self.go(Page::Request(id), data),
                    }
                }
            }
            Change::Choice {
                row: "friendRow",
                index,
            } => self.friend_action = Some(*index),
            Change::Press {
                row: "friendRow",
                index,
            } => {
                let item = self.occurrence(data, "friendRow");
                if let Some(friend) = online.sorted_friends().get(item) {
                    let id = friend.account.id;
                    let acts = friend_actions(data, friend);
                    match acts.get(*index).copied().unwrap_or(FriendAct::Options) {
                        FriendAct::Join => actions.push(OnlineAction::Join(id)),
                        FriendAct::Message => self.message_friend(id, data, actions),
                        FriendAct::Invite => actions.push(OnlineAction::PartyInvite(vec![id])),
                        FriendAct::Profile => self.open_profile(id),
                        FriendAct::Options => self.go(Page::Friend(id), data),
                    }
                }
            }
            Change::Run { row } => self.run(row, data, actions),
            _ => {}
        }
    }

    fn run(&mut self, row: &str, data: &OnlineData<'_>, actions: &mut Vec<OnlineAction>) {
        let online = data.online;
        if self.profile_feed_run(row, actions)
            || self.social_run(row, data, actions)
            || self.restriction_run(row, data, actions)
            || self.voice_run(row, data, actions)
            || self.screen_run(row, data, actions)
        {
            return;
        }
        match (row, &self.page) {
            ("cloudPage", _) => self.go(Page::Cloud, data),
            ("useCloud", _) if online::is_cloud_address(&self.cloud_text) => {
                actions.push(OnlineAction::SetCloud(self.cloud_text.trim().into()));
                self.go(Page::Start, data);
            }
            ("useOfficial", _) => {
                actions.push(OnlineAction::UseOfficialCloud);
                self.go(Page::Start, data);
            }
            ("signIn", _) if data.cloud.is_some() => {
                actions.push(OnlineAction::BrowserSignIn { create: false });
            }
            ("createAccount", _) if data.cloud.is_some() => {
                actions.push(OnlineAction::BrowserSignIn { create: true });
            }
            ("cancelSignIn", Page::Browser) => {
                actions.push(OnlineAction::CancelSignIn);
                self.go(Page::Start, data);
            }
            ("openAgain", Page::Browser) => actions.push(OnlineAction::OpenSignInPage),
            ("manageAccount", Page::Account) => actions.push(OnlineAction::ManageAccount),
            ("invitation", _) => {
                let index = self.nth(data, "invitation");
                if let Some(invitation) = online.invitations.get(index) {
                    self.go(Page::Invitation(invitation.id.clone()), data);
                }
            }
            ("allRequests", _) => self.go(Page::Requests, data),
            ("sent", _) => {
                let index = self.nth(data, "sent");
                if let Some(request) = online.friends.outgoing.get(index) {
                    self.go(Page::Outgoing(request.account.id), data);
                }
            }
            ("blockedRow", _) => {
                let index = self.nth(data, "blockedRow");
                if let Some(account) = online.friends.blocked.get(index) {
                    self.go(Page::Blocked(account.id), data);
                }
            }
            ("addFriend", _) => {
                self.friend_text.clear();
                self.go(Page::AddFriend, data);
            }
            ("sendRequest", _) => {
                actions.push(OnlineAction::AddFriend(self.friend_text.trim().into()));
            }
            ("reconnect", _) => actions.push(OnlineAction::Reconnect),
            ("signOut", _) => actions.push(OnlineAction::SignOut),
            ("cancel", Page::Picker) => {
                actions.push(OnlineAction::Dialog(None));
                self.go(Page::Home, data);
            }
            ("accept", Page::Request(id)) => actions.push(OnlineAction::Accept(*id)),
            ("decline", Page::Request(id)) => actions.push(OnlineAction::Decline(*id)),
            ("block", Page::Request(id) | Page::Friend(id)) => {
                actions.push(OnlineAction::Block(*id));
            }
            ("withdraw", Page::Outgoing(id)) => actions.push(OnlineAction::Decline(*id)),
            ("unblock", Page::Blocked(id)) => actions.push(OnlineAction::Unblock(*id)),
            ("join", Page::Friend(id)) => actions.push(OnlineAction::Join(*id)),
            ("call", Page::Friend(id)) => actions.push(OnlineAction::Call(*id)),
            ("invite", Page::Friend(id)) => actions.push(OnlineAction::Invite(*id)),
            ("remove", Page::Friend(id)) => actions.push(OnlineAction::RemoveFriend(*id)),
            ("report", Page::Friend(id)) => {
                let id = *id;
                self.report_from = None;
                self.go(Page::Report(id), data);
            }
            ("sendReport", Page::Report(id)) => {
                actions.push(OnlineAction::Report(*id, REASONS[self.reason.min(4)]));
            }
            ("joinInvitation", Page::Invitation(id)) => {
                actions.push(OnlineAction::AnswerInvitation {
                    id: id.clone(),
                    accept: true,
                });
            }
            ("declineInvitation", Page::Invitation(id)) => {
                actions.push(OnlineAction::AnswerInvitation {
                    id: id.clone(),
                    accept: false,
                });
            }
            ("sendInvitations", Page::Picker) if !self.picked.is_empty() => {
                actions.push(OnlineAction::Dialog(Some(
                    self.picked.iter().copied().collect(),
                )));
                self.go(Page::Home, data);
            }
            _ => {}
        }
    }

    /// The page's title, subtitle and rows.
    pub fn rows(&self, data: &OnlineData<'_>) -> (String, Option<String>, Vec<Row>) {
        let language = data.language;
        let t = |text: T| text.text(language);
        let f = |text: T, args: &[Arg<'_>]| text.format(language, args);
        let online = data.online;
        let waiting = |test: fn(&Purpose) -> bool| online.is_waiting(test);
        let mut rows = Vec::new();
        let busy = |row: Row, test: fn(&Purpose) -> bool| {
            if waiting(test) {
                row.detail(t(T::Sending))
            } else {
                row
            }
        };
        let handle_of = |id: AccountId| {
            online
                .known(id)
                .map(|a| a.handle.clone())
                .unwrap_or_default()
        };
        let (title, subtitle) = match &self.page {
            Page::Start => {
                let mut sign_in = busy(
                    Row::command("signIn", t(T::SignIn)).icon(Icon::SIGN_IN),
                    |p| matches!(p, Purpose::BrowserSignIn { create: false } | Purpose::Hello),
                );
                let mut create = busy(
                    Row::command("createAccount", t(T::CreateAccount)).icon(Icon::USER_PLUS),
                    |p| matches!(p, Purpose::BrowserSignIn { create: true }),
                );
                if data.cloud.is_none() {
                    sign_in = sign_in.detail(t(T::NeedsCloud));
                    create = create.detail(t(T::NeedsCloud));
                }
                self.ban_rows(data, &mut rows);
                let banned = !rows.is_empty();
                if banned {
                    sign_in = sign_in.group_start();
                }
                rows.push(sign_in);
                rows.push(create);
                let shown = match data.cloud {
                    Some(_) if !data.custom_cloud => t(T::CloudOfficial),
                    Some(url) => url,
                    None => t(T::CloudNotSet),
                };
                rows.push(
                    Row::action("cloudPage", t(T::Cloud))
                        .icon(Icon::CLOUD)
                        .detail(shown)
                        .group_start(),
                );
                rows.push(Row::info("note", t(T::SignInNote), None).group_start());
                (t(T::Online).to_owned(), Some(t(T::Subtitle).to_owned()))
            }
            Page::Cloud => {
                rows.push(
                    Row::text("cloud", t(T::CloudAddress), self.cloud_text.clone())
                        .icon(Icon::CLOUD),
                );
                let mut use_cloud = Row::command("useCloud", t(T::CloudUse)).icon(Icon::CHECK);
                if !online::is_cloud_address(&self.cloud_text) {
                    use_cloud = use_cloud.detail(t(T::CloudInvalid));
                }
                rows.push(use_cloud);
                if data.custom_cloud && data.official_cloud {
                    rows.push(
                        Row::command("useOfficial", t(T::CloudUseOfficial))
                            .icon(Icon::ARROW_COUNTER_CLOCKWISE)
                            .group_start(),
                    );
                }
                rows.push(Row::info("hint", t(T::CloudHint), None).group_start());
                (t(T::Cloud).to_owned(), Some(t(T::CloudSubtitle).to_owned()))
            }
            Page::Browser => {
                rows.push(Row::command("openAgain", t(T::OpenAgain)).icon(Icon::GLOBE));
                rows.push(Row::command("cancelSignIn", language.text(Key::Cancel)).icon(Icon::X));
                rows.push(Row::info("note", t(T::BrowserNote), None).group_start());
                (
                    t(T::Online).to_owned(),
                    Some(t(T::ContinueInBrowser).to_owned()),
                )
            }
            Page::Home => {
                self.home_rows(data, &mut rows);
                (t(T::Friends).to_owned(), None)
            }
            Page::Requests => {
                self.request_rows(data, &mut rows, None);
                (
                    t(T::Requests).to_owned(),
                    Some(f(
                        T::RequestsWaiting,
                        &[Arg::Int(online.friends.incoming.len() as i64)],
                    )),
                )
            }
            Page::Friend(id) => {
                let friend = online.friend(*id);
                let presence = friend.map(|f| presence_text(f, language));
                let joinable = friend
                    .and_then(|f| f.presence.activity.as_ref())
                    .and_then(|a| a.session.as_ref())
                    .filter(|s| s.joinable);
                if let Some(session) = joinable {
                    rows.push(
                        Row::command("join", t(T::JoinGame))
                            .icon(Icon::GAME_CONTROLLER)
                            .detail(f(
                                T::JoinDetail,
                                &[
                                    Arg::Int(i64::from(session.members)),
                                    Arg::Int(i64::from(session.max_members)),
                                ],
                            )),
                    );
                }
                // A friend who plays a LAN game with no session: say why there is nothing to join.
                if joinable.is_none()
                    && let Some(f) = friend
                    && let Err(blocker @ anyps5_core::play_together::Blocker::NoSession { .. }) =
                        anyps5_core::play_together::Offer::of_friend(f)
                {
                    let (headline, line) =
                        super::together_strings::blocker_text(&blocker, language);
                    rows.push(Row::info("noSession", headline, None).detail(line));
                }
                let in_session = online
                    .session
                    .as_ref()
                    .is_some_and(|s| !s.members.iter().any(|m| m.account.id == *id));
                if in_session {
                    rows.push(
                        Row::command("invite", t(T::InviteToSession)).icon(Icon::PAPER_PLANE_RIGHT),
                    );
                }
                self.friend_social_rows(data, *id, &mut rows);
                rows.extend(call_row(data, *id));
                rows.push(
                    Row::action("viewProfile", F::ViewProfile.text(language))
                        .icon(Icon::USER_CIRCLE),
                );
                if let Some(card) = online.cards.get(id) {
                    rows.push(trophy_row(card.as_ref(), language).group_start());
                }
                rows.push(
                    Row::command("remove", t(T::RemoveFriend))
                        .icon(Icon::USER_MINUS)
                        .group_start(),
                );
                rows.push(
                    Row::command("block", t(T::Block))
                        .icon(Icon::PROHIBIT)
                        .destructive(),
                );
                rows.push(Row::action("report", t(T::Report)).icon(Icon::FLAG));
                (handle_of(*id), presence)
            }
            Page::Request(id) => {
                rows.push(Row::command("accept", t(T::Accept)).icon(Icon::CHECK));
                rows.push(Row::command("decline", t(T::Decline)).icon(Icon::X));
                rows.push(
                    Row::command("block", t(T::Block))
                        .icon(Icon::PROHIBIT)
                        .destructive()
                        .group_start(),
                );
                (handle_of(*id), Some(t(T::WantsFriend).to_owned()))
            }
            Page::Outgoing(id) => {
                rows.push(Row::command("withdraw", t(T::Withdraw)).icon(Icon::X));
                (handle_of(*id), Some(t(T::Waiting).to_owned()))
            }
            Page::Blocked(id) => {
                rows.push(Row::command("unblock", t(T::Unblock)).icon(Icon::CHECK));
                (handle_of(*id), Some(t(T::Blocked).to_owned()))
            }
            Page::Invitation(id) => {
                let invitation = online.invitations.iter().find(|i| &i.id == id);
                rows.push(
                    Row::command("joinInvitation", t(T::JoinGame)).icon(Icon::GAME_CONTROLLER),
                );
                rows.push(Row::command("declineInvitation", t(T::Decline)).icon(Icon::X));
                match invitation {
                    Some(invitation) => (
                        f(T::InvitedBy, &[Arg::Str(&invitation.from.handle)]),
                        Some(invitation.title.clone()),
                    ),
                    None => (t(T::Invitations).to_owned(), None),
                }
            }
            Page::AddFriend => {
                let now = u64::try_from(data.now).unwrap_or(0);
                if let Some(paused) = online
                    .restrictions
                    .blocking(online::Blocked::FriendRequest, now)
                {
                    // The request cannot go out: say so where it would be written.
                    rows.push(restriction::paused_row(
                        "pausedFriend",
                        online::Blocked::FriendRequest,
                        paused,
                        data,
                    ));
                } else {
                    rows.push(
                        Row::text("friend", t(T::Handle), self.friend_text.clone())
                            .icon(Icon::USER_CIRCLE),
                    );
                    rows.push(busy(
                        Row::command("sendRequest", t(T::SendRequest)).icon(Icon::USER_PLUS),
                        |p| matches!(p, Purpose::AddFriend(_)),
                    ));
                }
                rows.push(Row::info("hint", t(T::AddFriendHint), None).group_start());
                (
                    t(T::AddFriend).to_owned(),
                    Some(t(T::AddFriendSubtitle).to_owned()),
                )
            }
            Page::Account => {
                let account = online.account.as_ref();
                self.restriction_rows_for_page(data, &mut rows);
                let restricted = !rows.is_empty();
                let mut manage = busy(
                    Row::command("manageAccount", t(T::ManageAccount)).icon(Icon::GLOBE),
                    |p| matches!(p, Purpose::AccountPage),
                )
                .detail(t(T::ManageAccountDetail));
                if restricted {
                    manage = manage.group_start();
                }
                rows.push(manage);
                rows.push(Row::info(
                    "cloud",
                    t(T::Cloud),
                    data.cloud.map(str::to_owned),
                ));
                rows.push(appear_offline_row(data).group_start());
                rows.push(
                    Row::toggle("relayOnly", t(T::RelayOnly), data.relay_only)
                        .icon(Icon::EYE_SLASH)
                        .group_start(),
                );
                rows.push(Row::info("relayOnlyNote", t(T::RelayOnlyNote), None));
                let audience = online.privacy.map(|p| p.trophies).unwrap_or_default();
                let audiences = [T::AudienceFriends, T::AudienceEveryone, T::AudienceOnlyYou]
                    .iter()
                    .map(|a| t(*a).to_owned())
                    .collect();
                rows.push(
                    Row::choice(
                        "trophyAudience",
                        t(T::TrophyAudience),
                        audiences,
                        AUDIENCES.iter().position(|a| *a == audience).unwrap_or(0),
                    )
                    .icon(Icon::TROPHY)
                    .group_start(),
                );
                rows.push(Row::info(
                    "trophyAudienceNote",
                    t(T::TrophyAudienceNote),
                    None,
                ));
                if !online.friends.blocked.is_empty() {
                    rows.push(Row::caption("blockedCaption", t(T::Blocked)).group_start());
                    for blocked in &online.friends.blocked {
                        rows.push(
                            Row::action("blockedRow", blocked.handle.clone()).icon(Icon::PROHIBIT),
                        );
                    }
                }
                rows.push(
                    busy(
                        Row::command("signOut", t(T::SignOut)).icon(Icon::SIGN_OUT),
                        |p| matches!(p, Purpose::SignOut),
                    )
                    .group_start(),
                );
                (
                    FX::OnlineSettings.text(language).to_owned(),
                    account.map(|a| a.handle.clone()),
                )
            }
            Page::Report(id) => {
                let reasons = [
                    T::ReasonCheating,
                    T::ReasonHarassment,
                    T::ReasonSpam,
                    T::ReasonName,
                    T::ReasonOther,
                ]
                .iter()
                .map(|r| t(*r).to_owned())
                .collect();
                rows.push(Row::choice("reason", t(T::Reason), reasons, self.reason));
                rows.push(busy(
                    Row::command("sendReport", t(T::SendReport)).icon(Icon::FLAG),
                    |p| matches!(p, Purpose::Report(_)),
                ));
                (f(T::ReportTitle, &[Arg::Str(&handle_of(*id))]), None)
            }
            Page::Voice => {
                self.voice_page(data, &mut rows);
                (
                    V::Voice.text(language).to_owned(),
                    Some(V::VoiceSubtitle.text(language).to_owned()),
                )
            }
            Page::VoiceDevices(kind) => {
                self.device_page(data, &mut rows, *kind);
                let (title, subtitle) = match kind {
                    DeviceKind::Input => (V::Microphone, V::PickMicrophone),
                    DeviceKind::Output => (V::Speakers, V::PickSpeakers),
                };
                (
                    title.text(language).to_owned(),
                    Some(subtitle.text(language).to_owned()),
                )
            }
            Page::Party
            | Page::PartyMember(_)
            | Page::PartyPicker
            | Page::PartyInvitation(_)
            | Page::Messages
            | Page::Conversation(_)
            | Page::MessageRequest(_)
            | Page::Message(..) => self
                .social_rows(data, &mut rows)
                .unwrap_or_else(|| (String::new(), None)),
            Page::Restriction(_) => self
                .restriction_page_rows(data, &mut rows)
                .unwrap_or_else(|| (String::new(), None)),
            Page::Feed => {
                if !self.feed_is_list(data) {
                    self.feed_rows(data, &mut rows);
                }
                // The page is named like its rail entry.
                (
                    FX::Activity.text(language).to_owned(),
                    Some(F::FeedSubtitle.text(language).to_owned()),
                )
            }
            Page::Picker => {
                let dialog = online.dialog.as_ref();
                for friend in picker_friends(online) {
                    rows.push(Row::toggle(
                        "pick",
                        friend.account.handle.clone(),
                        self.picked.contains(&friend.account.id),
                    ));
                }
                if rows.is_empty() {
                    rows.push(Row::info("none", t(T::NoFriends), None));
                }
                rows.push(
                    Row::command("sendInvitations", t(T::SendInvitations))
                        .icon(Icon::PAPER_PLANE_RIGHT)
                        .group_start(),
                );
                rows.push(Row::command("cancel", language.text(Key::Cancel)).icon(Icon::X));
                let title = dialog.map(|d| d.title.as_str()).unwrap_or_default();
                let max = dialog.map(|d| i64::from(d.max)).unwrap_or(1);
                (
                    f(T::PickerTitle, &[Arg::Str(title)]),
                    Some(f(T::PickerSubtitle, &[Arg::Int(max)])),
                )
            }
        };
        (title, subtitle, rows)
    }

    /// The friend requests that wait, each with Accept and Decline at its end. With a `limit`, the
    /// rest hide behind "See All Requests".
    fn request_rows(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>, limit: Option<usize>) {
        let language = data.language;
        let t = |text: T| text.text(language);
        let incoming = &data.online.friends.incoming;
        if incoming.is_empty() {
            return;
        }
        if limit.is_some() {
            let mut caption = Row::caption("requestsCaption", t(T::Requests));
            if !rows.is_empty() {
                caption = caption.group_start();
            }
            rows.push(caption);
        }
        let answers = vec![t(T::Accept).to_owned(), t(T::Decline).to_owned()];
        for request in incoming.iter().take(limit.unwrap_or(usize::MAX)) {
            rows.push(
                Row::buttons(
                    "requestRow",
                    request.account.handle.clone(),
                    answers.clone(),
                    self.request_answer.min(1),
                )
                .avatar(self.avatar_of(data, &request.account, None))
                .detail(t(T::WantsFriend)),
            );
        }
        if let Some(limit) = limit
            && incoming.len() > limit
        {
            rows.push(
                Row::action("allRequests", t(T::SeeAllRequests))
                    .icon(Icon::USERS)
                    .detail(
                        T::RequestsWaiting.format(language, &[Arg::Int(incoming.len() as i64)]),
                    ),
            );
        }
    }

    fn home_rows(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>) {
        let language = data.language;
        let t = |text: T| text.text(language);
        let online = data.online;
        // The state of the connection comes first: a list that may be old says so, with the way
        // out beside it.
        match online.cloud {
            Some(Cloud::Online) => {}
            Some(Cloud::Connecting) | None => {
                rows.push(Row::info(
                    "connecting",
                    FX::ConnectingNote.text(language),
                    None,
                ));
            }
            Some(Cloud::Offline) => {
                rows.push(Row::info(
                    "offlineNote",
                    FX::OfflineNote.text(language),
                    None,
                ));
                rows.push(
                    Row::command("reconnect", FX::TryAgain.text(language))
                        .icon(Icon::ARROWS_CLOCKWISE),
                );
            }
        }
        // A limit on the account explains why an action will not work. Then what waits for an
        // answer: friend requests, then invitations.
        self.restriction_rows_for_page(data, rows);
        self.request_rows(data, rows, Some(MAX_INLINE_REQUESTS));
        if !online.invitations.is_empty() || !online.social.party_invitations.is_empty() {
            let mut caption = Row::caption("invitationsCaption", t(T::Invitations));
            caption.group_start = !rows.is_empty();
            rows.push(caption);
            for invitation in &online.invitations {
                rows.push(
                    Row::action(
                        "invitation",
                        T::InvitedBy.format(language, &[Arg::Str(&invitation.from.handle)]),
                    )
                    .icon(Icon::ENVELOPE_SIMPLE)
                    .detail(invitation.title.clone()),
                );
            }
            self.party_invitation_rows(data, rows);
        }
        // A stream on offer in the party: one line that leads to the party page.
        screen::home_row(data, rows);
        // Playing Together shows once someone else is in the session: alone it would list only you.
        let me = online.account.as_ref().map(|a| a.id);
        if let Some(session) = online
            .session
            .as_ref()
            .filter(|s| s.members.iter().any(|m| Some(m.account.id) != me))
        {
            let caption = super::together_strings::own_session_line(session, language)
                .unwrap_or_else(|| t(T::Session).to_owned());
            rows.push(Row::caption("sessionCaption", caption).group_start());
            for member in &session.members {
                let value = if Some(member.account.id) == me {
                    t(T::You)
                } else {
                    link_text(member.link, language)
                };
                rows.push(Row::info(
                    "member",
                    member.account.handle.clone(),
                    Some(value.to_owned()),
                ));
            }
        }
        let mut add = Row::command("addFriend", t(T::AddFriend)).icon(Icon::USER_PLUS);
        add.group_start = !rows.is_empty();
        rows.push(add);
        let friends = online.sorted_friends();
        if friends.is_empty() {
            rows.push(Row::info("noFriends", t(T::NoFriends), None).group_start());
        }
        let mut group = None;
        for friend in friends {
            let rank = friend_group(friend);
            if group != Some(rank) {
                group = Some(rank);
                let title = match rank {
                    FriendGroup::Playing => FX::PlayingNow,
                    FriendGroup::Online => FX::OnlineCaption,
                    FriendGroup::Offline => FX::OfflineCaption,
                };
                rows.push(Row::caption("friendsCaption", title.text(language)).group_start());
            }
            rows.push(self.friend_row(data, friend));
        }
        if !online.friends.outgoing.is_empty() {
            rows.push(Row::caption("sentCaption", t(T::Sent)).group_start());
            for request in &online.friends.outgoing {
                rows.push(
                    Row::action("sent", request.account.handle.clone())
                        .avatar(self.avatar_of(data, &request.account, None))
                        .detail(t(T::Waiting)),
                );
            }
        }
    }

    /// A friend's row: picture, name, what they do, and the quick actions that come up with the
    /// focus. The row's own press is the last one, Options: the friend's page.
    fn friend_row(&self, data: &OnlineData<'_>, friend: &online::api::Friend) -> Row {
        let language = data.language;
        let acts = friend_actions(data, friend);
        let labels: Vec<String> = acts.iter().map(|a| a.label(language).to_owned()).collect();
        // Without a choice yet the keys press Message, never a Join the player did not pick.
        let default = acts
            .iter()
            .position(|a| *a == FriendAct::Message)
            .unwrap_or(0);
        let selected = self
            .friend_action
            .unwrap_or(default)
            .min(labels.len().saturating_sub(1));
        Row::buttons("friendRow", friend.account.handle.clone(), labels, selected)
            .avatar(self.avatar_of(data, &friend.account, Some(&friend.presence)))
            .detail(friend_detail(data, friend))
            .reveal()
    }

    /// The picture of a person in a row. A friend's presence marks the picture: a dot while they
    /// are here, quieter while they are not.
    fn avatar_of(
        &self,
        data: &OnlineData<'_>,
        account: &online::api::Account,
        presence: Option<&online::api::Presence>,
    ) -> RowAvatar {
        let hash = data
            .online
            .avatars
            .get(&account.id)
            .and_then(|hash| hash.as_deref());
        RowAvatar {
            texture: self.local.avatar(hash).map(|a| a.texture),
            colour: monogram_colour(account.id),
            initial: account.handle.chars().next(),
            dot: presence.filter(|p| p.online).map(|_| data.accent),
            away: presence.is_some_and(|p| !p.online),
        }
    }

    /// How many rows with this id stand above the focused one: the place of the focused row among
    /// its kind, across the captions that split a list.
    fn occurrence(&self, data: &OnlineData<'_>, id: RowId) -> usize {
        let rows = self.rows(data).2;
        rows.iter()
            .take(self.side.nav.row)
            .filter(|row| row.id == id)
            .count()
    }
}

/// The quick actions of a friend's row, in the order of the buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FriendAct {
    Join,
    Message,
    Invite,
    Profile,
    Options,
}

impl FriendAct {
    fn label(self, language: Language) -> &'static str {
        match self {
            FriendAct::Join => FX::ActionJoin.text(language),
            FriendAct::Message => FX::ActionMessage.text(language),
            FriendAct::Invite => {
                crate::screens::online_social_strings::SocialText::InviteToParty.text(language)
            }
            FriendAct::Profile => F::ViewProfile.text(language),
            FriendAct::Options => FX::MoreAbout.text(language),
        }
    }
}

/// What a friend's row offers right now. Join only while they play in a session others may join;
/// the invitation only to a party the player may add to.
pub(super) fn friend_actions(
    data: &OnlineData<'_>,
    friend: &online::api::Friend,
) -> Vec<FriendAct> {
    let online = data.online;
    let social = &online.social;
    let mut acts = Vec::new();
    let joinable = friend
        .presence
        .activity
        .as_ref()
        .and_then(|a| a.session.as_ref())
        .is_some_and(|s| s.joinable);
    if joinable {
        acts.push(FriendAct::Join);
    }
    acts.push(FriendAct::Message);
    let me = online.account.as_ref().map(|a| a.id);
    let may_invite = social.party.is_none() || me.is_some_and(|me| social.leads(me));
    if may_invite && !social.in_party(friend.account.id) && social::free_places(online) > 0 {
        acts.push(FriendAct::Invite);
    }
    acts.push(FriendAct::Profile);
    acts.push(FriendAct::Options);
    acts
}

/// Where a friend stands in the list: playing, here, away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FriendGroup {
    Playing,
    Online,
    Offline,
}

fn friend_group(friend: &online::api::Friend) -> FriendGroup {
    match (&friend.presence.activity, friend.presence.online) {
        (Some(_), _) => FriendGroup::Playing,
        (None, true) => FriendGroup::Online,
        (None, false) => FriendGroup::Offline,
    }
}

/// The switch for appearing offline: it says what is true while it is on (friends see the player
/// as offline), the way every toggle here does.
pub(super) fn appear_offline_row(data: &OnlineData<'_>) -> Row {
    let language = data.language;
    Row::toggle(
        "appearOffline",
        T::AppearOffline.text(language),
        data.online.appears_offline(),
    )
    .icon(Icon::CIRCLE_DASHED)
    .detail(T::AppearOfflineDetail.text(language))
}

/// How many friend requests the friends list answers in place; the rest wait behind a row.
const MAX_INLINE_REQUESTS: usize = 5;

/// The line under the handle in the header of the Friends screen: how the connection is, and,
/// once it is up, how many friends are here.
pub fn status_line(data: &OnlineData<'_>) -> String {
    let (online, language) = (data.online, data.language);
    match online.cloud {
        Some(Cloud::Online) if online.appears_offline() => {
            T::AppearingOffline.text(language).to_owned()
        }
        Some(Cloud::Online) => {
            let status = T::StatusOnline.text(language);
            let count = match online.friends_online() {
                0 => FX::CountNone.text(language).to_owned(),
                1 => FX::CountOne.text(language).to_owned(),
                n => FX::CountMany.format(language, &[Arg::Int(n as i64)]),
            };
            format!("{status} · {count}")
        }
        Some(Cloud::Connecting) | None => T::StatusConnecting.text(language).to_owned(),
        Some(Cloud::Offline) => T::StatusOffline.text(language).to_owned(),
    }
}

/// Sign-in pages come first; signed in, the friends.
fn entry_page(data: &OnlineData<'_>) -> Page {
    let online = data.online;
    if online.is_signed_in() {
        Page::Home
    } else {
        match online.pending {
            Some(Pending::Browser) => Page::Browser,
            // The e-mail code pages are the cloud's web site now.
            Some(Pending::Code | Pending::Handle) | None => Page::Start,
        }
    }
}

/// The friends a game's invitation can go to: those who are online, sorted like the list.
fn picker_friends(online: &Online) -> Vec<&online::api::Friend> {
    online
        .sorted_friends()
        .into_iter()
        .filter(|f| f.presence.online)
        .collect()
}

/// What a friend is doing, in a few words.
/// A friend's trophies: the total, and the grades they have under it, highest first.
fn trophy_row(count: Option<&Count>, language: Language) -> Row {
    let title = language.text(Key::Trophies);
    let Some(count) = count else {
        let hidden = T::TrophiesHidden.text(language).to_owned();
        return Row::info("trophies", title, Some(hidden)).icon(Icon::TROPHY);
    };
    let grades = [
        (count.platinum, Key::GradePlatinum),
        (count.gold, Key::GradeGold),
        (count.silver, Key::GradeSilver),
        (count.bronze, Key::GradeBronze),
    ];
    let total: u32 = grades.iter().map(|(n, _)| n).sum();
    if total == 0 {
        let none = T::TrophiesNone.text(language).to_owned();
        return Row::info("trophies", title, Some(none)).icon(Icon::TROPHY);
    }
    let detail = grades
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, grade)| format!("{n} {}", language.text(*grade)))
        .collect::<Vec<_>>()
        .join(" · ");
    Row::info("trophies", title, Some(total.to_string()))
        .icon(Icon::TROPHY)
        .detail(detail)
}

/// The row that calls a friend, or adds them to the call the player leads. A friend who is offline
/// is not rung; a player who is in someone else's party cannot add anyone to it.
fn call_row(data: &OnlineData<'_>, id: AccountId) -> Option<Row> {
    let online = data.online;
    let friend = online.friend(id).filter(|f| f.presence.online)?;
    if online.calls.ringing.is_some() {
        return None;
    }
    let me = online.account.as_ref()?.id;
    let title = match &online.social.party {
        Some(party) if party.leader != me => return None,
        Some(party)
            if party
                .members
                .iter()
                .any(|m| m.account.id == friend.account.id) =>
        {
            return None;
        }
        Some(party) if party.members.len() > 1 => CT::AddToCall,
        _ => CT::CallFriend,
    };
    Some(
        Row::command("call", title.text(data.language))
            .icon(Icon::SPEAKER_HIGH)
            .detail(CT::CallFriendDetail.text(data.language)),
    )
}

/// The detail of a friend's row: a missed call until it is seen, else what they are doing.
fn friend_detail(data: &OnlineData<'_>, friend: &online::api::Friend) -> String {
    match data.online.calls.missed_from(friend.account.id) {
        Some(missed) => call_text::ago(
            u64::try_from(data.now)
                .unwrap_or(0)
                .saturating_sub(missed.at),
            data.language,
        ),
        None => presence_text(friend, data.language),
    }
}

pub fn presence_text(friend: &online::api::Friend, language: Language) -> String {
    match &friend.presence.activity {
        Some(activity) => {
            // A Play Together session reads "Playing Minecraft together (2/4)".
            let together = anyps5_core::play_together::presence(activity);
            if let Some(line) = super::together_strings::presence_line(&together, language) {
                return line;
            }
            let joinable = activity.session.as_ref().is_some_and(|s| s.joinable);
            let text = if joinable {
                T::PlayingJoinable
            } else {
                T::Playing
            };
            text.format(language, &[Arg::Str(&activity.title)])
        }
        None if friend.presence.online => T::FriendOnline.text(language).to_owned(),
        None => T::FriendOffline.text(language).to_owned(),
    }
}

fn link_text(link: Option<Link>, language: Language) -> &'static str {
    match link {
        Some(Link::Direct) => T::LinkDirect.text(language),
        Some(Link::Relay) => T::LinkRelay.text(language),
        _ => T::LinkConnecting.text(language),
    }
}

#[cfg(test)]
mod tests {
    use anyps5_core::online::api::{
        Account, Activity, Friend, FriendLists, Invitation, Presence, Request, SessionRef,
    };
    use anyps5_core::online::{SignedIn, Status};

    use crate::screens::rows::Control;

    use super::*;

    fn account(id: u64, handle: &str) -> Account {
        Account {
            id: AccountId(id),
            handle: handle.into(),
        }
    }

    fn signed_in() -> Online {
        let mut online = Online::default();
        online.status = Status::Running;
        online.cloud = Some(Cloud::Online);
        online.account = Some(SignedIn {
            id: AccountId(1),
            handle: "Alec".into(),
            email: "a@example.org".into(),
            appear_offline: false,
        });
        online.friends = FriendLists {
            friends: vec![Friend {
                account: account(2, "Lumen"),
                since: 0,
                presence: Presence {
                    online: true,
                    activity: Some(Activity {
                        title_id: "TEST00001".into(),
                        title: "Test Game".into(),
                        version: "01.000".into(),
                        session: Some(SessionRef {
                            kind: Default::default(),
                            id: "s1".into(),
                            joinable: true,
                            members: 1,
                            max_members: 4,
                        }),
                    }),
                    revision: 0,
                },
            }],
            incoming: vec![Request {
                account: account(3, "Kit"),
                since: 0,
            }],
            ..FriendLists::default()
        };
        online
    }

    fn data(online: &Online) -> OnlineData<'_> {
        OnlineData {
            online,
            cloud: Some("https://cloud.example.org"),
            custom_cloud: true,
            official_cloud: false,
            relay_only: false,
            language: Language::English,
            accent: Color32::WHITE,
            now: 1_792_670_400,
            utc_offset: Some(0),
            game_running: false,
            play_together: None,
        }
    }

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.id).collect()
    }

    #[test]
    fn before_sign_in_the_panel_offers_the_browser_and_the_cloud() {
        let online = Online::default();
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        assert_eq!(panel.page, Page::Start);
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Online");
        assert_eq!(
            subtitle.as_deref(),
            Some("Play with your friends over the internet")
        );
        assert_eq!(ids(&rows), ["signIn", "createAccount", "cloudPage", "note"]);
        assert_eq!(rows[2].detail.as_deref(), Some("https://cloud.example.org"));
        let mut actions = Vec::new();
        panel.run("signIn", &data(&online), &mut actions);
        panel.run("createAccount", &data(&online), &mut actions);
        assert_eq!(
            actions,
            [
                OnlineAction::BrowserSignIn { create: false },
                OnlineAction::BrowserSignIn { create: true }
            ]
        );
        // Without a cloud the commands say what to do first, and do nothing.
        let none = OnlineData {
            cloud: None,
            ..data(&online)
        };
        let rows = panel.rows(&none).2;
        assert_eq!(rows[0].detail.as_deref(), Some("Set up the cloud first."));
        let mut actions = Vec::new();
        panel.run("signIn", &none, &mut actions);
        assert!(actions.is_empty());
    }

    #[test]
    fn the_pages_follow_the_browser_sign_in() {
        let mut online = Online::default();
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        online.pending = Some(Pending::Browser);
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Browser);
        let (_, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(subtitle.as_deref(), Some("Continue in your browser"));
        assert_eq!(ids(&rows), ["openAgain", "cancelSignIn", "note"]);
        let mut actions = Vec::new();
        panel.run("openAgain", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::OpenSignInPage]);
        // Stopped or timed out: back to the start.
        online.pending = None;
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Start);
        online.pending = Some(Pending::Browser);
        panel.follow(&data(&online));
        let mut actions = Vec::new();
        panel.run("cancelSignIn", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::CancelSignIn]);
        assert_eq!(panel.page, Page::Start);
        // Finished in the browser: the friends.
        panel.go(Page::Browser, &data(&online));
        let online = signed_in();
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Home);
    }

    #[test]
    fn signed_in_the_friends_show_with_what_they_play() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Friends");
        // The header of the screen says who and how many; the page keeps no line of its own.
        assert_eq!(subtitle, None);
        assert_eq!(
            ids(&rows),
            [
                "requestsCaption",
                "requestRow",
                "addFriend",
                "friendsCaption",
                "friendRow",
            ]
        );
        assert_eq!(
            rows[4].detail.as_deref(),
            Some("Playing Test Game · You can join")
        );
        // The first row that takes focus, not a caption.
        assert_eq!(panel.side.nav.row, 1);
    }

    #[test]
    fn a_friends_row_offers_the_quick_actions_that_fit() {
        let mut online = signed_in();
        let labels = |online: &Online| {
            let mut panel = OnlinePanel::default();
            panel.open(&data(online));
            let rows = panel.rows(&data(online)).2;
            match &rows.iter().find(|r| r.id == "friendRow").unwrap().control {
                Control::Buttons { labels, .. } => labels.clone(),
                other => panic!("{other:?}"),
            }
        };
        // Joinable, and a party can be started: all five, Options last.
        assert_eq!(
            labels(&online),
            [
                "Join",
                "Message",
                "Invite to Party",
                "View Profile",
                "Options"
            ]
        );
        // Not joinable: no Join.
        online.friends.friends[0]
            .presence
            .activity
            .as_mut()
            .unwrap()
            .session = None;
        assert_eq!(
            labels(&online),
            ["Message", "Invite to Party", "View Profile", "Options"]
        );
        // Already in the party: no invitation.
        online.social.apply_party(Some(online::api::Party {
            id: "p".into(),
            leader: AccountId(1),
            members: [(1, "Alec"), (2, "Lumen")]
                .iter()
                .map(|(id, handle)| online::api::PartyMember {
                    account: account(*id, handle),
                    joined: 0,
                    endpoint: String::new(),
                    relay: None,
                    voice: false,
                })
                .collect(),
            max_members: 8,
            conversation: "c".into(),
            created: 0,
            revision: 1,
            discord: None,
        }));
        assert_eq!(labels(&online), ["Message", "View Profile", "Options"]);
    }

    #[test]
    fn a_friends_quick_actions_do_what_they_say() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        let focus = rows::position(&panel.rows(&data(&online)).2, "friendRow").unwrap();
        let press = |panel: &mut OnlinePanel, index: usize| {
            panel.side.nav.row = focus;
            let mut actions = Vec::new();
            panel.handle(
                &Context::default(),
                &Change::Press {
                    row: "friendRow",
                    index,
                },
                &data(&online),
                &mut actions,
            );
            actions
        };
        assert_eq!(press(&mut panel, 0), [OnlineAction::Join(AccountId(2))]);
        // Message asks for the conversation first; the page stays on the list.
        assert_eq!(
            press(&mut panel, 1),
            [OnlineAction::OpenDirect(AccountId(2))]
        );
        assert_eq!(panel.page, Page::Home);
        assert_eq!(
            press(&mut panel, 2),
            [OnlineAction::PartyInvite(vec![AccountId(2)])]
        );
        // View Profile opens the profile screen and leaves the page where it is.
        assert!(press(&mut panel, 3).is_empty());
        assert_eq!(panel.queued, [OnlineAction::OpenProfile(AccountId(2))]);
        // Options, and the row's body, are the friend's page.
        assert!(press(&mut panel, 4).is_empty());
        assert_eq!(panel.page, Page::Friend(AccountId(2)));
        panel.go(Page::Home, &data(&online));
        assert!(press(&mut panel, 9).is_empty());
        assert_eq!(panel.page, Page::Friend(AccountId(2)));
    }

    #[test]
    fn friends_are_grouped_by_what_they_do_and_the_press_finds_the_right_one() {
        let mut online = signed_in();
        let friend = |id: u64, handle: &str, online: bool| Friend {
            account: account(id, handle),
            since: 0,
            presence: Presence {
                online,
                activity: None,
                revision: 0,
            },
        };
        online
            .friends
            .friends
            .extend([friend(3, "Kit", true), friend(4, "Rook", false)]);
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            &ids(&rows)[3..],
            [
                "friendsCaption",
                "friendRow",
                "friendsCaption",
                "friendRow",
                "friendsCaption",
                "friendRow",
            ]
        );
        let titles: Vec<&str> = rows
            .iter()
            .filter(|r| r.id == "friendsCaption")
            .map(|r| r.title.as_str())
            .collect();
        assert_eq!(titles, ["Playing Now", "Online", "Offline"]);
        // The third friend row is Rook, across the captions between them.
        panel.side.nav.row = rows.len() - 1;
        let mut actions = Vec::new();
        panel.handle(
            &Context::default(),
            &Change::Press {
                row: "friendRow",
                index: 99,
            },
            &data(&online),
            &mut actions,
        );
        assert_eq!(panel.page, Page::Friend(AccountId(4)));
    }

    #[test]
    fn the_status_line_names_the_connection_and_the_friends_active() {
        let mut online = signed_in();
        assert_eq!(status_line(&data(&online)), "Online · 1 friend active");
        online.friends.friends[0].presence.online = false;
        assert_eq!(status_line(&data(&online)), "Online · No friends active");
        online.cloud = Some(Cloud::Connecting);
        assert_eq!(status_line(&data(&online)), "Connecting…");
        online.cloud = Some(Cloud::Offline);
        assert_eq!(status_line(&data(&online)), "Offline, trying again");
    }

    #[test]
    fn a_connection_that_is_out_says_so_and_offers_to_try_again() {
        let mut online = signed_in();
        online.cloud = Some(Cloud::Offline);
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(&ids(&rows)[..2], ["offlineNote", "reconnect"]);
        let mut actions = Vec::new();
        panel.run("reconnect", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::Reconnect]);
        online.cloud = Some(Cloud::Connecting);
        assert_eq!(ids(&panel.rows(&data(&online)).2)[0], "connecting");
        online.cloud = Some(Cloud::Online);
        assert_ne!(ids(&panel.rows(&data(&online)).2)[0], "connecting");
    }

    #[test]
    fn every_page_belongs_to_a_section_and_each_section_starts_on_its_own_page() {
        for section in Section::ALL {
            assert_eq!(section.root().section(), Some(section), "{section:?}");
            assert!(section.root().is_root(), "{section:?}");
        }
        assert_eq!(Page::Start.section(), None);
        assert_eq!(Page::Browser.section(), None);
        assert_eq!(Page::Voice.section(), Some(Section::Party));
        assert_eq!(
            Page::Conversation("c".into()).section(),
            Some(Section::Messages)
        );
        assert_eq!(Page::Friend(AccountId(2)).section(), Some(Section::Friends));
        assert!(!Page::Friend(AccountId(2)).is_root());
    }

    #[test]
    fn a_restriction_page_belongs_to_the_page_it_was_opened_from() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Home, &data(&online));
        panel.go(Page::Restriction("muted".into()), &data(&online));
        assert_eq!(panel.section(), Some(Section::Friends));
        panel.go(Page::Account, &data(&online));
        panel.go(Page::Restriction("muted".into()), &data(&online));
        assert_eq!(panel.section(), Some(Section::Settings));
    }

    #[test]
    fn requests_are_answered_in_place_and_more_than_five_lead_to_all_of_them() {
        let mut online = signed_in();
        online.friends.incoming = (3..10)
            .map(|id| Request {
                account: account(id, &format!("Req{id}")),
                since: 0,
            })
            .collect();
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            &ids(&rows)[..7],
            [
                "requestsCaption",
                "requestRow",
                "requestRow",
                "requestRow",
                "requestRow",
                "requestRow",
                "allRequests"
            ]
        );
        // Accept is the first answer: select presses it for the focused request.
        let mut actions = Vec::new();
        panel.side.nav.row = 2;
        panel.handle(
            &Context::default(),
            &Change::Press {
                row: "requestRow",
                index: 0,
            },
            &data(&online),
            &mut actions,
        );
        assert_eq!(actions, [OnlineAction::Accept(AccountId(4))]);
        // Decline is the second.
        let mut actions = Vec::new();
        panel.handle(
            &Context::default(),
            &Change::Press {
                row: "requestRow",
                index: 1,
            },
            &data(&online),
            &mut actions,
        );
        assert_eq!(actions, [OnlineAction::Decline(AccountId(4))]);
        // The row beyond the answers is its body: the request's own page.
        let mut actions = Vec::new();
        panel.handle(
            &Context::default(),
            &Change::Press {
                row: "requestRow",
                index: 2,
            },
            &data(&online),
            &mut actions,
        );
        assert_eq!(
            (actions.len(), panel.page.clone()),
            (0, Page::Request(AccountId(4)))
        );
        // All of them, none capped, each answered the same way.
        panel.go(Page::Requests, &data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Friend Requests");
        assert_eq!(subtitle.as_deref(), Some("7 waiting"));
        assert_eq!(ids(&rows), ["requestRow"; 7]);
        // The last answered request leaves the page for the friends.
        online.friends.incoming.clear();
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Home);
    }

    #[test]
    fn the_home_page_lists_party_invitations_with_the_other_invitations() {
        let mut online = signed_in();
        online
            .social
            .add_party_invitation(online::api::PartyInvitation {
                id: "pi".into(),
                from: account(2, "Lumen"),
                party: "p".into(),
                members: 2,
                expires: 0,
                call: false,
            });
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            &ids(&rows)[..4],
            [
                "requestsCaption",
                "requestRow",
                "invitationsCaption",
                "partyInvitation"
            ]
        );
        let mut actions = Vec::new();
        panel.side.nav.row = 3;
        panel.run("partyInvitation", &data(&online), &mut actions);
        assert_eq!(panel.page, Page::PartyInvitation("pi".into()));
    }

    #[test]
    fn a_friend_who_plays_in_a_joinable_session_can_be_joined() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        let (title, _, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Lumen");
        assert_eq!(
            ids(&rows),
            [
                "join",
                "sendMessage",
                "inviteToParty",
                "call",
                "viewProfile",
                "remove",
                "block",
                "report"
            ]
        );
        assert_eq!(rows[0].detail.as_deref(), Some("1 of 4 players"));
        let mut actions = Vec::new();
        panel.run("join", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::Join(AccountId(2))]);
    }

    #[test]
    fn a_friend_shows_their_trophies_or_that_they_hide_them() {
        let mut online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        assert!(!ids(&panel.rows(&data(&online)).2).contains(&"trophies"));

        let count = |platinum, gold, silver, bronze| Count {
            bronze,
            silver,
            gold,
            platinum,
        };
        online.cards.insert(AccountId(2), Some(count(1, 4, 0, 30)));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            ids(&rows),
            [
                "join",
                "sendMessage",
                "inviteToParty",
                "call",
                "viewProfile",
                "trophies",
                "remove",
                "block",
                "report"
            ]
        );
        assert_eq!(rows[5].control, Control::Info(Some("35".into())));
        assert_eq!(
            rows[5].detail.as_deref(),
            Some("1 Platinum · 4 Gold · 30 Bronze")
        );
        assert!(rows[5].group_start);

        online.cards.insert(AccountId(2), Some(count(0, 0, 0, 0)));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows[5].control, Control::Info(Some("None yet".into())));
        assert_eq!(rows[5].detail, None);

        online.cards.insert(AccountId(2), None);
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows[5].control, Control::Info(Some("Hidden".into())));
    }

    #[test]
    fn the_account_page_picks_who_sees_the_trophies() {
        let mut online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Account, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let choice = rows.iter().position(|r| r.id == "trophyAudience").unwrap();
        assert_eq!(rows[choice + 1].id, "trophyAudienceNote");
        let options = vec!["Friends".to_owned(), "Everyone".into(), "Only You".into()];
        assert_eq!(
            rows[choice].control,
            Control::Choice {
                options: options.clone(),
                selected: 0
            },
            "friends until the cloud says otherwise"
        );
        online.privacy = Some(online::api::Privacy {
            trophies: Audience::Nobody,
            ..online::api::Privacy::default()
        });
        assert_eq!(
            panel.rows(&data(&online)).2[choice].control,
            Control::Choice {
                options,
                selected: 2
            }
        );
        let mut actions = Vec::new();
        let change = Change::Choice {
            row: "trophyAudience",
            index: 1,
        };
        panel.handle(&Context::default(), &change, &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::TrophyAudience(Audience::Everyone)]);
    }

    #[test]
    fn appearing_offline_is_a_switch_on_the_account_page() {
        use crate::screens::rows::Control;
        let mut online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        panel.go(Page::Account, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let row = rows.iter().find(|r| r.id == "appearOffline").unwrap();
        assert_eq!(row.title, "Appear Offline");
        assert_eq!(row.detail.as_deref(), Some("Friends see you as offline"));
        assert_eq!(row.control, Control::Toggle(false));
        // Pressing the switch asks the host.
        let mut actions = Vec::new();
        panel.handle(
            &Context::default(),
            &Change::Toggle {
                row: "appearOffline",
                on: true,
            },
            &data(&online),
            &mut actions,
        );
        assert_eq!(actions, [OnlineAction::AppearOffline(true)]);
        // It shows what the player chose, and the friends page says it under the name.
        online.set_appear_offline(true);
        let (_, _, rows) = panel.rows(&data(&online));
        let row = rows.iter().find(|r| r.id == "appearOffline").unwrap();
        assert_eq!(row.control, Control::Toggle(true));
        // The header of the Friends screen says it under the name.
        assert_eq!(status_line(&data(&online)), "Appearing offline");
    }

    #[test]
    fn the_account_page_turns_relay_only_on() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Account, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let toggle = rows.iter().position(|r| r.id == "relayOnly").unwrap();
        assert_eq!(rows[toggle + 1].id, "relayOnlyNote");
        assert_eq!(rows[toggle].title, "Hide Your IP Address");
        let on = OnlineData {
            relay_only: true,
            ..data(&online)
        };
        assert_eq!(
            panel.rows(&on).2[toggle].control,
            crate::screens::rows::Control::Toggle(true)
        );
    }

    #[test]
    fn the_account_is_managed_on_the_web() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Account, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows[0].id, "manageAccount");
        assert!(
            !ids(&rows).contains(&"delete") && !ids(&rows).contains(&"email"),
            "the e-mail and the deletion live on the web"
        );
        let mut actions = Vec::new();
        panel.run("manageAccount", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::ManageAccount]);
    }

    #[test]
    fn back_leads_up_the_pages_and_closes_at_the_top() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Report(AccountId(2)), &data(&online));
        assert_eq!(
            panel.back_page(&data(&online)),
            Some(Page::Friend(AccountId(2)))
        );
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        assert_eq!(panel.back_page(&data(&online)), Some(Page::Home));
        panel.go(Page::Home, &data(&online));
        assert_eq!(panel.back_page(&data(&online)), None);
    }

    #[test]
    fn an_invitation_page_answers_it() {
        let mut online = signed_in();
        online.invitations.push(Invitation {
            kind: Default::default(),
            id: "i1".into(),
            from: account(2, "Lumen"),
            session: "s1".into(),
            title_id: "TEST00001".into(),
            title: "Test Game".into(),
            version: "01.000".into(),
            expires: 0,
        });
        let mut panel = OnlinePanel::default();
        panel.go(Page::Invitation("i1".into()), &data(&online));
        let (title, subtitle, _) = panel.rows(&data(&online));
        assert_eq!(title, "Lumen invites you");
        assert_eq!(subtitle.as_deref(), Some("Test Game"));
        let mut actions = Vec::new();
        panel.run("joinInvitation", &data(&online), &mut actions);
        assert_eq!(
            actions,
            [OnlineAction::AnswerInvitation {
                id: "i1".into(),
                accept: true
            }]
        );
        // Once it is gone, the page returns to the list.
        online.invitations.clear();
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Home);
    }

    fn feed_item(id: u64, reaction: Option<Reaction>) -> online::api::FeedItem {
        online::api::FeedItem {
            id,
            account: account(2, "Lumen"),
            at: 1_792_670_000,
            event: online::api::FeedEvent::FirstPlayed {
                title_id: "GAME00001".into(),
            },
            reactions: online::api::ReactionCounts::default(),
            mine: reaction,
        }
    }

    #[test]
    fn the_activity_section_asks_for_the_feed_and_back_leaves_the_screen() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.open(&data(&online));
        panel.go(Section::Activity.root(), &data(&online));
        assert_eq!(panel.page, Page::Feed);
        assert_eq!(
            panel.queued[..2],
            [
                OnlineAction::FeedRefresh,
                OnlineAction::Profile(AccountId(1))
            ]
        );
        // The first page of a section: Back leaves.
        assert_eq!(panel.back_page(&data(&online)), None);
        assert!(panel.back_closes(&data(&online)));
    }

    #[test]
    fn a_friends_profile_opens_the_screen_and_the_panel_keeps_the_friend() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        let mut actions = Vec::new();
        panel.run("viewProfile", &data(&online), &mut actions);
        assert_eq!(panel.page, Page::Friend(AccountId(2)));
        assert_eq!(panel.queued, [OnlineAction::OpenProfile(AccountId(2))]);
    }

    #[test]
    fn the_panel_comes_back_on_the_page_it_left() {
        let online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        panel.close();
        panel.dismiss();
        assert!(!panel.is_active());
        panel.resume(&data(&online));
        assert!(panel.is_open());
        assert_eq!(panel.page, Page::Friend(AccountId(2)));
    }

    #[test]
    fn the_feed_page_says_what_it_waits_for() {
        let mut online = signed_in();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Feed, &data(&online));
        // Loading: the state block says so, and there is nothing to press.
        assert!(panel.rows(&data(&online)).2.is_empty());
        assert_eq!(panel.feed_state(&data(&online)).1, F::Loading);
        online.feed.error = Some(online::Failure::new("unavailable", "x"));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(ids(&rows), ["feedRetry"]);
        assert_eq!(panel.feed_state(&data(&online)).1, F::LoadFailedTitle);
        let mut actions = Vec::new();
        panel.run("feedRetry", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::FeedRefresh]);
        online.feed.merge_newest(online::api::Feed::default());
        assert!(
            panel.rows(&data(&online)).2.is_empty(),
            "friends exist, so no Add a Friend"
        );
        assert_eq!(panel.feed_state(&data(&online)).1, F::Empty);
        online.friends.friends.clear();
        assert_eq!(ids(&panel.rows(&data(&online)).2), ["addFriend"]);
        online.cloud = Some(Cloud::Offline);
        online.feed = online::FeedState::default();
        assert!(panel.rows(&data(&online)).2.is_empty());
        assert_eq!(panel.feed_state(&data(&online)).1, F::Offline);
    }

    #[test]
    fn a_reaction_press_sets_it_and_the_own_one_takes_it_back() {
        let mut online = signed_in();
        online.feed.merge_newest(online::api::Feed {
            items: vec![feed_item(9, None), feed_item(8, Some(Reaction::Gg))],
            next: Some(8),
        });
        let mut panel = OnlinePanel::default();
        panel.go(Page::Feed, &data(&online));
        assert!(panel.feed_is_list(&data(&online)));
        assert!(panel.rows(&data(&online)).2.is_empty());
        let mut actions = Vec::new();
        let focus = |item, slot| feed::FeedFocus { item, slot };
        panel.feed_press(focus(0, Some(1)), &data(&online), &mut actions);
        panel.feed_press(focus(1, Some(1)), &data(&online), &mut actions);
        panel.feed_press(focus(1, Some(2)), &data(&online), &mut actions);
        // The row after the cards reads older items.
        panel.feed_press(focus(2, None), &data(&online), &mut actions);
        assert_eq!(
            actions,
            [
                OnlineAction::React(9, Some(Reaction::Gg)),
                OnlineAction::React(8, None),
                OnlineAction::React(8, Some(Reaction::Wow)),
                OnlineAction::FeedOlder,
            ]
        );
        // A card opens its account's profile screen; the panel stays on the feed.
        panel.queued.clear();
        panel.feed_press(focus(0, None), &data(&online), &mut actions);
        assert_eq!(panel.page, Page::Feed);
        assert_eq!(panel.queued, [OnlineAction::OpenProfile(AccountId(2))]);
    }
}
