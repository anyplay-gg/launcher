//! The Party page as a room (docs/DESIGN.md 17, "Party room"): the members stand as large avatars
//! in a calm grid, with what each is doing in words (speaking, muted, in a game) and a mark for
//! the leader; under them the cards of what the party can do together now, the player's own voice
//! controls and the actions of the party. Without a party the page is one invitation to start one.
//!
//! The rows of the page (`social_rows`) stay the model of its title and its tests; this module is
//! the picture of them, with a focus of its own like the feed: a grid has two directions, so the
//! rows' one-dimensional focus does not fit. Every stop maps to the action its row ran before.
//!
//! The slot for what a party can do together is [`Room::cards`]: the session to take the party
//! into, the streams. A card of Play Together joins the same list.

use anyps5_core::online::api::AccountId;
use anyps5_core::online::control::ScreenPhase;
use anyps5_core::online::{ScreenCommand, VoiceProvider};
use egui::containers::scroll_area::ScrollBarVisibility;
use egui::{Align, Align2, Layout, Margin, Rect, ScrollArea, Sense, Ui, UiBuilder, pos2, vec2};

use super::local::Local;
use super::social::free_places;
use super::{OnlineAction, OnlineData, OnlinePanel, Page};
use crate::screens::clamped;
use crate::screens::online_call_strings::CallText as CT;
use crate::screens::online_screen_strings::ScreenText as X;
use crate::screens::online_social_strings::SocialText as S;
use crate::screens::online_strings::OnlineText as T;
use crate::screens::online_voice_strings::VoiceText as V;
use crate::screens::room_strings::RoomText as R;
use crate::screens::scroll::Follow;
use crate::screens::together_strings::TogetherText as TT;
use crate::ui::Shell;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::chrome::{RowAvatar, paint_row_avatar};
use crate::ui::components::{self, ConsoleButton, ControlSize, corner, paint_capsule};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{self, Command};
use crate::ui::sound::Cue;
use crate::ui::text;
use crate::ui::theme::{TextStyle, black, color, fade, radius, size, space, ty};

/// Members per row of the grid when the page has the room for it.
const COLUMNS: usize = 5;
/// The least and the most a tile takes of the width.
const TILE_MIN: f32 = 168.0;
const TILE_MAX: f32 = 240.0;
const TILE_HEIGHT: f32 = 214.0;
/// The room above the avatar: the leader chip sits in it.
const AVATAR_TOP: f32 = 36.0;
const TILE_GAP: f32 = size::CONTROL_GAP;
const AVATAR: f32 = 88.0;
/// The fade over the bottom edge of a page that goes on below: its height and its darkest alpha.
const FADE_HEIGHT: f32 = 48.0;
const FADE_ALPHA: u8 = 150;
/// The dark disc and the glyph of a mark (muted, deafened) on an avatar.
const MARK_DISC: f32 = 14.0;
const MARK_GLYPH: f32 = 14.0;
const CARD_HEIGHT: f32 = 76.0;
/// The air between two groups of the page: at least twice the gap inside one.
const GROUP_GAP: f32 = space::XL;

const NAME: TextStyle = ty::ROW_TITLE;
const STATE: TextStyle = ty::DETAIL_LARGE;
const GAME: TextStyle = ty::DETAIL;
const CHIP: TextStyle = ty::MICRO;

/// Something on the page the focus can stand on and the player can press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stop {
    /// The n-th member of the party, the player included.
    Member(usize),
    /// The empty seat: invite a friend.
    Seat,
    /// The button of the n-th card.
    Card(usize),
    Mute,
    Deafen,
    /// Switch the party voice on.
    VoiceOn,
    VoiceSettings,
    OpenDiscord,
    Invite,
    Chat,
    Share,
    Leave,
    /// Start a party (the page without one).
    Start,
    /// The n-th party invitation that waits.
    Invitation(usize),
}

/// Where the focus stands: a row of stops, and the stop in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RoomFocus {
    pub row: usize,
    pub col: usize,
}

/// The stops in the order the focus walks them, row by row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Plan {
    pub rows: Vec<Vec<Stop>>,
    /// The one primary action of the page.
    pub primary: Option<Stop>,
}

impl Plan {
    pub fn stop(&self, focus: RoomFocus) -> Option<Stop> {
        self.rows.get(focus.row)?.get(focus.col).copied()
    }

    pub fn find(&self, stop: Stop) -> Option<RoomFocus> {
        self.rows.iter().enumerate().find_map(|(row, stops)| {
            stops
                .iter()
                .position(|s| *s == stop)
                .map(|col| RoomFocus { row, col })
        })
    }

    /// The focus kept inside the plan, for a party that changed under it.
    pub fn clamp(&self, focus: RoomFocus) -> RoomFocus {
        let row = focus.row.min(self.rows.len().saturating_sub(1));
        let col = focus
            .col
            .min(self.rows.get(row).map_or(0, |r| r.len().saturating_sub(1)));
        RoomFocus { row, col }
    }

    /// Where the focus starts: on the primary action, else on the first stop.
    pub fn start(&self) -> RoomFocus {
        self.primary
            .and_then(|stop| self.find(stop))
            .unwrap_or_default()
    }
}

/// What a command does to the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// The focus moved, or stayed (the flag says which).
    Moved(RoomFocus, bool),
    Press(Stop),
    Back,
}

/// Applies `command` to `focus`. Up and down keep the column as far as the next row reaches.
pub(super) fn step(plan: &Plan, focus: RoomFocus, command: Command) -> Step {
    let focus = plan.clamp(focus);
    let moved = |next: RoomFocus| Step::Moved(next, next != focus);
    let last_row = plan.rows.len().saturating_sub(1);
    match command {
        Command::Up => moved(plan.clamp(RoomFocus {
            row: focus.row.saturating_sub(1),
            col: focus.col,
        })),
        Command::Down => moved(plan.clamp(RoomFocus {
            row: (focus.row + 1).min(last_row),
            col: focus.col,
        })),
        Command::Left => moved(RoomFocus {
            col: focus.col.saturating_sub(1),
            ..focus
        }),
        Command::Right => moved(plan.clamp(RoomFocus {
            col: focus.col + 1,
            ..focus
        })),
        Command::Select => plan
            .stop(focus)
            .map_or(Step::Moved(focus, false), Step::Press),
        Command::Back | Command::Options => Step::Back,
    }
}

/// A mark on an avatar that is not a colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mark {
    /// The member's own microphone is muted.
    Muted,
    /// The member hears nobody.
    Deafened,
    /// The player muted this member.
    MutedByYou,
}

impl Mark {
    fn icon(self) -> Icon {
        match self {
            Mark::Muted => Icon::MICROPHONE_SLASH,
            Mark::Deafened | Mark::MutedByYou => Icon::SPEAKER_SLASH,
        }
    }
}

/// One member as a tile.
#[derive(Clone, Debug)]
pub(super) struct Tile {
    pub account: AccountId,
    pub handle: String,
    pub hash: Option<String>,
    pub you: bool,
    pub leader: bool,
    pub speaking: bool,
    pub mark: Option<Mark>,
    /// Said in words: speaking, muted, not in voice, else online or offline.
    pub state: String,
    /// The game the member is in.
    pub game: Option<String>,
    /// Offline: the avatar is quiet.
    pub away: bool,
    /// What a screen reader reads.
    pub reader: String,
}

/// What pressing a card's button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CardAction {
    /// Start the selected game with a Play Together session.
    PlayTogether,
    /// Take the party into the session.
    Take(String),
    StopSharing,
    Watch(AccountId),
    StopWatching,
    /// Look at the invitation.
    Invitation(String),
}

/// One thing the party can do together now, or a line about it.
#[derive(Clone, Debug)]
pub(super) struct Card {
    pub icon: Icon,
    pub title: String,
    pub detail: String,
    /// The label of the button and what it does; `None` for a line without an action.
    pub action: Option<(String, CardAction)>,
}

/// A button of the page.
#[derive(Clone, Debug)]
pub(super) struct Button {
    pub stop: Stop,
    pub label: String,
    pub icon: Icon,
    pub destructive: bool,
}

/// Everything the page shows, built from the state each frame.
#[derive(Clone, Debug, Default)]
pub(super) struct Room {
    /// There is a party: the grid, the voice and the actions. Else the invitation to start one.
    pub in_party: bool,
    pub tiles: Vec<Tile>,
    /// The leader may invite and a place is free: the empty seat.
    pub seat: bool,
    pub cards: Vec<Card>,
    pub voice: Vec<Button>,
    pub actions: Vec<Button>,
    /// The words of the empty page.
    pub empty_note: String,
    pub primary: Option<Stop>,
}

impl Room {
    pub fn new(data: &OnlineData<'_>) -> Room {
        let online = data.online;
        let language = data.language;
        let me = online.account.as_ref().map_or(AccountId(0), |a| a.id);
        let Some(party) = online.social.party.as_ref() else {
            return Room::empty(data);
        };
        let voice = &online.voice;
        let voice_on = voice.enabled();
        let leads = party.leader == me;
        let alone = party.members.len() <= 1;
        let tiles = party
            .members
            .iter()
            .map(|member| {
                let id = member.account.id;
                let you = id == me;
                let friend = online.friend(id);
                let speaking = voice_on
                    && if you {
                        voice.me.speaking
                    } else {
                        voice.is_speaking(id)
                    };
                let muted_by_you = voice_on && !you && voice.prefs().peer(id).1;
                let (deafened, muted) = (
                    you && voice_on && voice.deafened,
                    you && voice_on && voice.muted,
                );
                let mark = if deafened {
                    Some(Mark::Deafened)
                } else if muted {
                    Some(Mark::Muted)
                } else if muted_by_you {
                    Some(Mark::MutedByYou)
                } else {
                    None
                };
                let state = if deafened {
                    CT::YouDeafened.text(language).to_owned()
                } else if muted {
                    CT::YouMuted.text(language).to_owned()
                } else if muted_by_you {
                    R::MutedByYou.text(language).to_owned()
                } else if speaking {
                    V::Speaking.text(language).to_owned()
                } else if voice_on && !you && !member.voice {
                    V::NotInVoice.text(language).to_owned()
                } else if you {
                    T::You.text(language).to_owned()
                } else if friend.is_some_and(|f| f.presence.online) {
                    T::FriendOnline.text(language).to_owned()
                } else {
                    T::FriendOffline.text(language).to_owned()
                };
                let game = if you {
                    online.session.as_ref().map(|s| s.title.clone())
                } else {
                    friend
                        .and_then(|f| f.presence.activity.as_ref())
                        .map(|a| a.title.clone())
                };
                let game_line = game
                    .as_deref()
                    .map(|title| T::Playing.format(language, &[Arg::Str(title)]));
                let leader = id == party.leader;
                let mut reader = vec![member.account.handle.clone()];
                if leader {
                    reader.push(S::Leader.text(language).to_owned());
                }
                reader.push(state.clone());
                reader.extend(game_line.clone());
                Tile {
                    account: id,
                    handle: member.account.handle.clone(),
                    hash: online.avatars.get(&id).and_then(|h| h.clone()),
                    you,
                    leader,
                    speaking,
                    mark,
                    state,
                    game: game_line,
                    away: !you && !friend.is_some_and(|f| f.presence.online),
                    reader: reader.join(", "),
                }
            })
            .collect();
        let seat = leads && free_places(online) > 0;
        let invite = seat.then(|| Button {
            stop: Stop::Invite,
            label: S::InviteFriends.text(language).to_owned(),
            icon: Icon::USER_PLUS,
            destructive: false,
        });
        let cards = cards(data, leads && !alone);

        // The player's voice.
        let mut voice_buttons = Vec::new();
        let settings = Button {
            stop: Stop::VoiceSettings,
            label: V::VoiceSettings.text(language).to_owned(),
            icon: Icon::GEAR_SIX,
            destructive: false,
        };
        if voice.provider() == VoiceProvider::Discord {
            if party.discord.is_some() {
                voice_buttons.push(Button {
                    stop: Stop::OpenDiscord,
                    label: CT::OpenInDiscord.text(language).to_owned(),
                    icon: Icon::DISCORD_LOGO,
                    destructive: false,
                });
            }
        } else if voice_on {
            voice_buttons.push(Button {
                stop: Stop::Mute,
                label: if voice.muted { CT::Unmute } else { CT::Mute }
                    .text(language)
                    .to_owned(),
                icon: if voice.muted {
                    Icon::MICROPHONE_SLASH
                } else {
                    Icon::MICROPHONE
                },
                destructive: false,
            });
            voice_buttons.push(Button {
                stop: Stop::Deafen,
                label: if voice.deafened {
                    CT::Undeafen
                } else {
                    CT::Deafen
                }
                .text(language)
                .to_owned(),
                icon: if voice.deafened {
                    Icon::SPEAKER_SLASH
                } else {
                    Icon::HEADPHONES
                },
                destructive: false,
            });
        } else {
            voice_buttons.push(Button {
                stop: Stop::VoiceOn,
                label: R::VoiceOn.text(language).to_owned(),
                icon: Icon::MICROPHONE,
                destructive: false,
            });
        }
        voice_buttons.push(settings);

        // The party's actions.
        let mut actions = Vec::new();
        actions.extend(invite);
        actions.push(Button {
            stop: Stop::Chat,
            label: S::PartyChat.text(language).to_owned(),
            icon: Icon::ENVELOPE_SIMPLE,
            destructive: false,
        });
        if online.screen.can_start() && online.screen.hosting().is_none() {
            actions.push(Button {
                stop: Stop::Share,
                label: X::ShareScreen.text(language).to_owned(),
                icon: Icon::APP_WINDOW,
                destructive: false,
            });
        }
        actions.push(Button {
            stop: Stop::Leave,
            label: S::LeaveParty.text(language).to_owned(),
            icon: Icon::SIGN_OUT,
            destructive: true,
        });

        // One primary action: a party of one starts by inviting; any other has it on the card
        // that takes it into the game.
        let primary = if alone && leads {
            Some(if actions.iter().any(|b| b.stop == Stop::Invite) {
                Stop::Invite
            } else {
                Stop::Seat
            })
        } else {
            cards
                .iter()
                .position(|c| {
                    matches!(
                        c.action,
                        Some((_, CardAction::Take(_) | CardAction::PlayTogether))
                    )
                })
                .map(Stop::Card)
        };
        Room {
            in_party: true,
            tiles,
            seat,
            cards,
            voice: voice_buttons,
            actions,
            empty_note: String::new(),
            primary,
        }
    }

    /// The page without a party.
    fn empty(data: &OnlineData<'_>) -> Room {
        let language = data.language;
        let online = data.online;
        let cards = online
            .social
            .party_invitations
            .iter()
            .map(|invitation| Card {
                icon: Icon::USERS,
                title: T::InvitedBy.format(language, &[Arg::Str(&invitation.from.handle)]),
                detail: S::PartyInvitationDetail
                    .format(language, &[Arg::Int(i64::from(invitation.members))]),
                action: Some((
                    R::ViewInvitation.text(language).to_owned(),
                    CardAction::Invitation(invitation.id.clone()),
                )),
            })
            .collect();
        let mut voice = Vec::new();
        if online.is_online() {
            voice.push(Button {
                stop: Stop::VoiceSettings,
                label: V::VoiceSettings.text(language).to_owned(),
                icon: Icon::GEAR_SIX,
                destructive: false,
            });
        }
        Room {
            in_party: false,
            cards,
            voice,
            empty_note: crate::screens::friends_strings::FriendsText::NotInPartyNote
                .text(language)
                .to_owned(),
            primary: Some(Stop::Start),
            ..Room::default()
        }
    }

    /// The stops in focus order for a grid of `columns`.
    pub fn plan(&self, columns: usize) -> Plan {
        let columns = columns.max(1);
        let mut rows: Vec<Vec<Stop>> = Vec::new();
        if self.in_party {
            let mut seats: Vec<Stop> = (0..self.tiles.len()).map(Stop::Member).collect();
            if self.seat {
                seats.push(Stop::Seat);
            }
            rows.extend(seats.chunks(columns).map(<[Stop]>::to_vec));
        } else {
            rows.push(vec![Stop::Start]);
        }
        for (n, card) in self.cards.iter().enumerate() {
            if card.action.is_some() {
                rows.push(vec![Stop::Card(n)]);
            }
        }
        if !self.voice.is_empty() {
            rows.push(self.voice.iter().map(|b| b.stop).collect());
        }
        if !self.actions.is_empty() {
            rows.push(self.actions.iter().map(|b| b.stop).collect());
        }
        Plan {
            rows,
            primary: self.primary,
        }
    }
}

/// What the party can do together now, one card each.
fn cards(data: &OnlineData<'_>, may_take: bool) -> Vec<Card> {
    let online = data.online;
    let language = data.language;
    let screen = &online.screen;
    let mut cards = Vec::new();
    let me = online.account.as_ref().map_or(AccountId(0), |a| a.id);
    // The card of Play Together: the selected game, started with a session the party can join.
    if let Some(title) = data.play_together {
        cards.push(Card {
            icon: Icon::GAME_CONTROLLER,
            title: TT::PlayTogether.text(language).to_owned(),
            detail: TT::PlayTogetherDetail.format(language, &[Arg::Str(title)]),
            action: Some((
                TT::PlayTogether.text(language).to_owned(),
                CardAction::PlayTogether,
            )),
        });
    }
    if may_take
        && let Some(session) = online.session.as_ref().filter(|s| {
            s.visibility != anyps5_core::online::api::Visibility::Closed
                && s.members.iter().any(|m| m.account.id == me)
        })
    {
        cards.push(Card {
            icon: Icon::GAME_CONTROLLER,
            title: session.title.clone(),
            detail: T::JoinDetail.format(
                language,
                &[
                    Arg::Int(session.members.len() as i64),
                    Arg::Int(i64::from(session.max_members)),
                ],
            ),
            action: Some((
                S::TakeParty.text(language).to_owned(),
                CardAction::Take(session.id.clone()),
            )),
        });
    }
    if let Some(hosting) = screen.hosting() {
        let phase = match hosting.phase {
            ScreenPhase::Connecting => X::HostStarting,
            ScreenPhase::Live => X::HostLive,
            ScreenPhase::Stalled => X::HostStalled,
        };
        cards.push(Card {
            icon: Icon::APP_WINDOW,
            title: super::screen::shared_with(screen.watchers(), language),
            detail: format!("{} · {}", hosting.title, phase.text(language)),
            action: Some((
                X::StopSharing.text(language).to_owned(),
                CardAction::StopSharing,
            )),
        });
    }
    for offer in screen.offers() {
        let name = super::screen::name_of(online, offer.account, language);
        let action = screen.can_watch().then(|| {
            (
                X::Watch.format(language, &[Arg::Str(&name)]),
                CardAction::Watch(offer.account),
            )
        });
        cards.push(Card {
            icon: Icon::APP_WINDOW,
            title: name,
            detail: X::SharingTitle.format(language, &[Arg::Str(&offer.title)]),
            action,
        });
    }
    if let Some(watching) = screen.watching() {
        let name = super::screen::name_of(online, watching.account, language);
        let phase = match watching.phase {
            ScreenPhase::Connecting => X::PhaseConnecting,
            ScreenPhase::Live => X::PhaseLive,
            ScreenPhase::Stalled => X::PhaseStalled,
        };
        cards.push(Card {
            icon: Icon::APP_WINDOW,
            title: X::Watching.format(language, &[Arg::Str(&name)]),
            detail: format!(
                "{} · {} · {}",
                watching.title,
                phase.text(language),
                super::screen::path_text(watching.link, watching.rtt_ms, language)
            ),
            action: Some((
                X::StopWatching.text(language).to_owned(),
                CardAction::StopWatching,
            )),
        });
    }
    cards
}

// ---------------------------------------------------------------------------------------------
// The panel
// ---------------------------------------------------------------------------------------------

impl OnlinePanel {
    /// The Party page is the room.
    pub(super) fn room_is_page(&self) -> bool {
        self.page == Page::Party
    }

    fn room_columns(&self) -> usize {
        if self.room_columns == 0 {
            COLUMNS
        } else {
            self.room_columns
        }
    }

    /// The room and its plan, with the focus kept inside it.
    fn room_now(&mut self, data: &OnlineData<'_>) -> (Room, Plan) {
        let room = Room::new(data);
        let plan = room.plan(self.room_columns());
        self.room_focus = plan.clamp(self.room_focus);
        (room, plan)
    }

    /// Puts the focus where the page starts: on the member the player came back from, else on the
    /// primary action.
    pub(super) fn room_start(&mut self, data: &OnlineData<'_>, from: Option<AccountId>) {
        let room = Room::new(data);
        let plan = room.plan(self.room_columns());
        self.room_focus = from
            .and_then(|id| room.tiles.iter().position(|t| t.account == id))
            .and_then(|n| plan.find(Stop::Member(n)))
            .unwrap_or_else(|| plan.start());
    }

    /// Whether Left on the focus would leave the page for the rail.
    pub(super) fn room_left_leaves(&self) -> bool {
        self.room_focus.col == 0
    }

    /// A command on the room. `true` leaves the page (Back).
    pub(super) fn room_command(
        &mut self,
        shell: &mut Shell,
        command: Command,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) -> bool {
        let (room, plan) = self.room_now(data);
        match step(&plan, self.room_focus, command) {
            Step::Moved(focus, moved) => {
                if moved {
                    shell.play(Cue::Move);
                }
                self.room_focus = focus;
                false
            }
            Step::Press(stop) => {
                shell.play(Cue::Select);
                self.room_press(&room, stop, data, actions);
                false
            }
            Step::Back => true,
        }
    }

    /// What pressing a stop does: what its row did before the page was a room.
    fn room_press(
        &mut self,
        room: &Room,
        stop: Stop,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) {
        let online = data.online;
        let voice = &online.voice;
        match stop {
            Stop::Member(n) => {
                if let Some(tile) = room.tiles.get(n) {
                    if tile.you {
                        self.open_profile(tile.account);
                    } else {
                        self.go(Page::PartyMember(tile.account), data);
                    }
                }
            }
            Stop::Seat | Stop::Invite => self.run("inviteFriends", data, actions),
            Stop::Start => self.run("startParty", data, actions),
            Stop::Card(n) => {
                let Some((_, action)) = room.cards.get(n).and_then(|c| c.action.clone()) else {
                    return;
                };
                match action {
                    CardAction::PlayTogether => actions.push(OnlineAction::PlayTogether),
                    CardAction::Take(session) => actions.push(OnlineAction::PartyMove(session)),
                    CardAction::StopSharing => {
                        actions.push(OnlineAction::Screen(ScreenCommand::Stop))
                    }
                    CardAction::Watch(account) => {
                        actions.push(OnlineAction::Screen(ScreenCommand::Watch(account)));
                    }
                    CardAction::StopWatching => {
                        actions.push(OnlineAction::Screen(ScreenCommand::Leave));
                    }
                    CardAction::Invitation(id) => self.go(Page::PartyInvitation(id), data),
                }
            }
            Stop::Invitation(n) => {
                if let Some(invitation) = online.social.party_invitations.get(n) {
                    self.go(Page::PartyInvitation(invitation.id.clone()), data);
                }
            }
            Stop::Mute => actions.push(OnlineAction::MicMute(!voice.muted)),
            Stop::Deafen => actions.push(OnlineAction::Deafen(!voice.deafened)),
            Stop::VoiceOn => actions.push(OnlineAction::Voice(true)),
            Stop::VoiceSettings => self.run("voiceSettings", data, actions),
            Stop::OpenDiscord => self.run("openDiscord", data, actions),
            Stop::Chat => self.run("partyChat", data, actions),
            Stop::Share => actions.push(OnlineAction::Screen(ScreenCommand::Start)),
            Stop::Leave => self.run("leaveParty", data, actions),
        }
    }

    /// Draws the room into `ui` and applies what the mouse did. `focused` is false while the
    /// focus stands on the rail.
    pub(super) fn room_draw(
        &mut self,
        shell: &mut Shell,
        ui: &mut Ui,
        data: &OnlineData<'_>,
        focused: bool,
        actions: &mut Vec<OnlineAction>,
    ) {
        let (room, plan) = self.room_now(data);
        let focus = focused.then_some(self.room_focus);
        let view = draw(ui, &room, &plan, focus, &self.local, data);
        if view.columns != 0 && view.columns != self.room_columns {
            self.room_columns = view.columns;
        }
        if let Some(pressed) = view.pressed {
            self.room_focus = pressed;
            if let Some(stop) = plan.stop(pressed) {
                shell.play(Cue::Select);
                self.room_press(&room, stop, data, actions);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------------------------

/// What the frame found out.
struct View {
    /// The stop the pointer pressed.
    pressed: Option<RoomFocus>,
    /// How many tiles stand in a row at this width.
    columns: usize,
}

/// How many tiles of at least [`TILE_MIN`] stand in `width`, and how wide each is.
fn grid(width: f32) -> (usize, f32) {
    let fit = (((width + TILE_GAP) / (TILE_MIN + TILE_GAP)).floor() as usize).clamp(1, COLUMNS);
    // One width for every tile of the page, so a party of two stands on the grid of a party of eight.
    let tile = ((width - TILE_GAP * (fit as f32 - 1.0)) / fit as f32).min(TILE_MAX);
    (fit, tile)
}

fn draw(
    ui: &mut Ui,
    room: &Room,
    plan: &Plan,
    focus: Option<RoomFocus>,
    local: &Local,
    data: &OnlineData<'_>,
) -> View {
    let ctx = ui.ctx().clone();
    let language = data.language;
    let id = ui.make_persistent_id("party-room");
    let (follow, scroll_to) = Follow::begin(&ctx, id, focus.map(|f| f.row));
    let mut view = View {
        pressed: None,
        columns: 0,
    };
    let mut geometry: Vec<(f32, f32)> = Vec::with_capacity(plan.rows.len());
    let mut area = ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden);
    if let Some(offset) = scroll_to {
        area = area.vertical_scroll_offset(offset);
    }
    let output = area.show(ui, |ui| {
        egui::Frame::NONE
            .inner_margin(Margin::same(size::LIST_INSET as i8))
            .show(ui, |ui| {
                let origin = ui.cursor().min.y;
                let width = ui.available_width();
                let mut row = 0;
                let note = |ui: &mut Ui, top: f32, geometry: &mut Vec<(f32, f32)>| {
                    geometry.push((top - origin, ui.cursor().min.y - top));
                };
                if room.in_party {
                    let (fit, tile_width) = grid(width);
                    view.columns = fit;
                    let mut seats: Vec<Option<usize>> = (0..room.tiles.len()).map(Some).collect();
                    if room.seat {
                        seats.push(None);
                    }
                    for chunk in seats.chunks(fit) {
                        let top = ui.cursor().min.y;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = TILE_GAP;
                            for (col, seat) in chunk.iter().enumerate() {
                                let here = focus.is_some_and(|f| f.row == row && f.col == col);
                                let pressed = match seat {
                                    Some(n) => {
                                        tile(ui, &room.tiles[*n], tile_width, here, local, data)
                                    }
                                    None => seat_tile(ui, tile_width, here, language),
                                };
                                if pressed {
                                    view.pressed = Some(RoomFocus { row, col });
                                }
                            }
                        });
                        note(ui, top, &mut geometry);
                        ui.add_space(TILE_GAP);
                        row += 1;
                    }
                    ui.add_space(GROUP_GAP - TILE_GAP);
                } else {
                    let top = ui.cursor().min.y;
                    let here = focus.is_some_and(|f| f.row == row);
                    if empty_state(ui, room, here, language) {
                        view.pressed = Some(RoomFocus { row, col: 0 });
                    }
                    note(ui, top, &mut geometry);
                    ui.add_space(GROUP_GAP);
                    row += 1;
                }

                for (n, card) in room.cards.iter().enumerate() {
                    let top = ui.cursor().min.y;
                    let here = card.action.is_some() && focus.is_some_and(|f| f.row == row);
                    let primary = room.primary == Some(Stop::Card(n));
                    if show_card(ui, card, here, primary, width) {
                        view.pressed = Some(RoomFocus { row, col: 0 });
                    }
                    if card.action.is_some() {
                        note(ui, top, &mut geometry);
                        row += 1;
                    }
                    ui.add_space(TILE_GAP);
                }
                if !room.cards.is_empty() {
                    ui.add_space(GROUP_GAP - TILE_GAP);
                }

                if !room.voice.is_empty() {
                    let top = ui.cursor().min.y;
                    let pressed = button_row(
                        ui,
                        &room.voice,
                        room.primary,
                        focus.filter(|f| f.row == row),
                    );
                    if let Some(col) = pressed {
                        view.pressed = Some(RoomFocus { row, col });
                    }
                    note(ui, top, &mut geometry);
                    ui.add_space(GROUP_GAP);
                    row += 1;
                }
                if !room.actions.is_empty() {
                    let top = ui.cursor().min.y;
                    let pressed = button_row(
                        ui,
                        &room.actions,
                        room.primary,
                        focus.filter(|f| f.row == row),
                    );
                    if let Some(col) = pressed {
                        view.pressed = Some(RoomFocus { row, col });
                    }
                    note(ui, top, &mut geometry);
                    ui.add_space(GROUP_GAP);
                }
            });
    });
    follow.end(
        &ctx,
        geometry,
        output.content_size.y,
        output.inner_rect.height(),
        output.state.clamped_offset().y,
    );
    // More below: the bottom edge fades, so the page does not end in a cut button.
    let hidden =
        output.content_size.y - output.inner_rect.height() - output.state.clamped_offset().y;
    if hidden > 2.0 {
        paint_more_below(ui.painter(), output.inner_rect);
    }
    view
}

/// A soft dark band along the bottom edge of the scrolling page: there is more under it.
fn paint_more_below(painter: &egui::Painter, area: Rect) {
    let band = Rect::from_min_max(pos2(area.min.x, area.max.y - FADE_HEIGHT), area.max);
    let clear = egui::Color32::from_black_alpha(0);
    let dark = egui::Color32::from_black_alpha(FADE_ALPHA);
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(band.left_top(), clear);
    mesh.colored_vertex(band.right_top(), clear);
    mesh.colored_vertex(band.right_bottom(), dark);
    mesh.colored_vertex(band.left_bottom(), dark);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// A row of buttons, 12 dp apart, wrapping at the width of the page. The ones that end something
/// (Leave Party) stand alone at the far end, apart from the everyday ones. Returns the one pressed.
fn button_row(
    ui: &mut Ui,
    buttons: &[Button],
    primary: Option<Stop>,
    focus: Option<RoomFocus>,
) -> Option<usize> {
    let mut pressed = None;
    let mut one = |ui: &mut Ui, col: usize, button: &Button| {
        let response = ConsoleButton::new(&button.label)
            .icon(button.icon)
            .size(ControlSize::Medium)
            .prominent(primary == Some(button.stop))
            .destructive(button.destructive)
            .focused(focus.is_some_and(|f| f.col == col))
            .show(ui);
        if response.clicked() {
            pressed = Some(col);
        }
    };
    if buttons.iter().any(|b| b.destructive) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(size::CONTROL_GAP, size::CONTROL_GAP);
            for (col, button) in buttons.iter().enumerate().filter(|(_, b)| !b.destructive) {
                one(ui, col, button);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                for (col, button) in buttons
                    .iter()
                    .enumerate()
                    .rev()
                    .filter(|(_, b)| b.destructive)
                {
                    one(ui, col, button);
                }
            });
        });
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(size::CONTROL_GAP, size::CONTROL_GAP);
            for (col, button) in buttons.iter().enumerate() {
                one(ui, col, button);
            }
        });
    }
    pressed
}

/// The surface of a tile or a card with its focus, hover and press.
fn surface(ui: &mut Ui, rect: Rect, response: &egui::Response, focused: bool, rest: egui::Color32) {
    let ctx = ui.ctx().clone();
    let hovered = input::hovered(&ctx, response);
    if hovered {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let t = ctx.focus_t((response.id, "focus"), focused);
    let hover = ctx.animate_to((response.id, "hover"), f32::from(hovered), Motion::Hover);
    let press = ctx.animate_to(
        (response.id, "press"),
        f32::from(
            response.is_pointer_button_down_on() || (focused && crate::ui::press::active(&ctx)),
        ),
        Motion::Press,
    );
    components::paint_surface(
        ui.painter(),
        rect,
        radius::CARD,
        rest,
        hover,
        t,
        press,
        size::FOCUS_RING_LARGE,
    );
}

/// One member. Returns whether the pointer pressed it.
fn tile(
    ui: &mut Ui,
    tile: &Tile,
    width: f32,
    focused: bool,
    local: &Local,
    data: &OnlineData<'_>,
) -> bool {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let (rect, response) = ui.allocate_exact_size(vec2(width, TILE_HEIGHT), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tile.reader.as_str()));
    crate::ui::a11y::follow(&ctx, response.id, focused);
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    surface(ui, rect, &response, focused, color::surface_row());

    let centre = pos2(rect.center().x, rect.min.y + AVATAR_TOP + AVATAR * 0.5);
    let avatar = Rect::from_center_size(centre, vec2(AVATAR, AVATAR));
    // The voice activity: a ring that fades in and out, with the word under the name.
    let speaking = ctx.animate_to(
        (response.id, "speaking"),
        f32::from(tile.speaking),
        Motion::Hover,
    );
    if speaking > 0.003 {
        painter.circle_stroke(
            centre,
            AVATAR * 0.5 + 5.0,
            egui::Stroke::new(3.0, fade(data.accent, speaking)),
        );
    }
    let picture = local.avatar(tile.hash.as_deref());
    paint_row_avatar(
        &painter,
        avatar,
        RowAvatar {
            texture: picture.map(|a| a.texture),
            colour: super::monogram_colour(tile.account),
            initial: tile.handle.chars().next(),
            dot: None,
            away: tile.away,
        },
    );
    if let Some(mark) = tile.mark {
        let at = centre + vec2(AVATAR * 0.35, AVATAR * 0.35);
        painter.circle_filled(at, MARK_DISC, black(0.72));
        painter.circle_stroke(
            at,
            MARK_DISC,
            egui::Stroke::new(1.0, color::hairline_control()),
        );
        painter.text(
            at,
            Align2::CENTER_CENTER,
            mark.icon().glyph(),
            mark.icon().font(MARK_GLYPH, false),
            color::text(),
        );
    }
    if tile.leader {
        let label = S::Leader.text(data.language);
        let galley = text::layout(&painter, CHIP, label, color::text(), f32::INFINITY);
        let chip = Rect::from_min_size(
            rect.min + vec2(space::M, space::M),
            vec2(galley.size().x + 16.0, 20.0),
        );
        paint_capsule(&painter, chip, color::surface_hover());
        painter.galley(
            pos2(chip.min.x + 8.0, chip.center().y - galley.size().y * 0.5),
            galley,
            color::text(),
        );
    }

    let inner = width - 24.0;
    let mut y = avatar.max.y + 16.0;
    let name = clamped(
        &painter,
        NAME,
        &tile.handle,
        egui::Color32::PLACEHOLDER,
        inner,
        1,
    );
    let name_height = name.size().y;
    let mut cut = name.elided;
    painter.galley(
        pos2(rect.center().x - name.size().x * 0.5, y),
        name,
        color::text(),
    );
    y += name_height + 2.0;
    let state_ink = if tile.speaking {
        color::text()
    } else {
        color::text_row_detail()
    };
    let state = clamped(
        &painter,
        STATE,
        &tile.state,
        egui::Color32::PLACEHOLDER,
        inner,
        1,
    );
    let state_height = state.size().y;
    cut |= state.elided;
    painter.galley(
        pos2(rect.center().x - state.size().x * 0.5, y),
        state,
        state_ink,
    );
    y += state_height + 2.0;
    if let Some(game) = &tile.game {
        let line = clamped(&painter, GAME, game, egui::Color32::PLACEHOLDER, inner, 1);
        cut |= line.elided;
        painter.galley(
            pos2(rect.center().x - line.size().x * 0.5, y),
            line,
            color::text_row_detail(),
        );
    }
    // A name, a state or a game cut to the tile's width is read in full while the pointer or the
    // focus is on it.
    if cut {
        let mut said = format!("{}\n{}", tile.handle, tile.state);
        if let Some(game) = &tile.game {
            said.push('\n');
            said.push_str(game);
        }
        crate::ui::reveal::full_text(
            &ctx,
            response.id,
            rect,
            &said,
            crate::ui::input::hovered(&ctx, &response)
                || (focused && crate::ui::input::focus_visible(&ctx)),
            true,
        );
    }
    response.clicked()
}

/// The empty seat: invite a friend.
fn seat_tile(ui: &mut Ui, width: f32, focused: bool, language: Language) -> bool {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let (rect, response) = ui.allocate_exact_size(vec2(width, TILE_HEIGHT), Sense::click());
    let label = R::InviteAFriend.text(language);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    crate::ui::a11y::follow(&ctx, response.id, focused);
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    surface(ui, rect, &response, focused, egui::Color32::TRANSPARENT);
    components::paint_hairline(&painter, rect, radius::CARD, color::hairline_control());
    let centre = pos2(rect.center().x, rect.min.y + AVATAR_TOP + AVATAR * 0.5);
    painter.circle_stroke(
        centre,
        AVATAR * 0.5,
        egui::Stroke::new(1.5, color::hairline_control()),
    );
    painter.text(
        centre,
        Align2::CENTER_CENTER,
        Icon::PLUS.glyph(),
        Icon::PLUS.font(24.0, false),
        color::text_detail(),
    );
    let name = clamped(
        &painter,
        NAME,
        label,
        egui::Color32::PLACEHOLDER,
        width - 24.0,
        1,
    );
    painter.galley(
        pos2(
            rect.center().x - name.size().x * 0.5,
            centre.y + AVATAR * 0.5 + 16.0,
        ),
        name,
        color::text_row_detail(),
    );
    response.clicked()
}

/// The page without a party: a quiet picture, one sentence and the one action.
fn empty_state(ui: &mut Ui, room: &Room, focused: bool, language: Language) -> bool {
    let painter = ui.painter().clone();
    let width = ui.available_width();
    ui.add_space(space::XL);
    let disc = 96.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, disc), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Label, true, S::StartPartyDetail.text(language))
    });
    let centre = rect.center();
    painter.circle_filled(centre, disc * 0.5, color::surface_row());
    painter.circle_stroke(
        centre,
        disc * 0.5,
        egui::Stroke::new(1.0, color::hairline_control()),
    );
    painter.text(
        centre,
        Align2::CENTER_CENTER,
        Icon::USERS.glyph(),
        Icon::USERS.font(36.0, false),
        color::text_detail(),
    );
    ui.add_space(space::L);
    let wrap = (width - 48.0).min(360.0);
    let mut job = egui::text::LayoutJob::simple(
        room.empty_note.clone(),
        ty::NOTE.font(),
        egui::Color32::PLACEHOLDER,
        wrap,
    );
    job.halign = Align::Center;
    job.sections[0].format.line_height = Some(1.4 * ty::NOTE.size);
    let galley = painter.layout_job(job);
    let (note, _) = ui.allocate_exact_size(vec2(width, galley.size().y), Sense::hover());
    painter.galley(
        pos2(note.center().x, note.min.y),
        galley,
        color::text_detail(),
    );
    ui.add_space(space::XL);
    ui.allocate_ui_with_layout(
        vec2(width, size::CONTROL_L),
        Layout::top_down(Align::Center),
        |ui| {
            ConsoleButton::new(S::StartParty.text(language))
                .icon(Icon::USER_PLUS)
                .size(ControlSize::Large)
                .prominent(true)
                .focused(focused)
                .show(ui)
                .clicked()
        },
    )
    .inner
}

/// A card: what the party can do together, with its one button on the right. Returns whether the
/// button was pressed.
fn show_card(ui: &mut Ui, card: &Card, focused: bool, primary: bool, width: f32) -> bool {
    let painter = ui.painter().clone();
    let (rect, response) = ui.allocate_exact_size(vec2(width, CARD_HEIGHT), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Label,
            true,
            format!("{}, {}", card.title, card.detail),
        )
    });
    if !ui.is_rect_visible(rect) {
        return false;
    }
    painter.rect_filled(rect, corner(radius::CARD), color::surface_row());
    let disc = Rect::from_center_size(
        pos2(rect.min.x + 20.0 + 20.0, rect.center().y),
        vec2(40.0, 40.0),
    );
    painter.circle_filled(disc.center(), 20.0, color::surface());
    painter.text(
        disc.center(),
        Align2::CENTER_CENTER,
        card.icon.glyph(),
        card.icon.font(20.0, false),
        color::text(),
    );
    let mut pressed = false;
    let mut button_left = rect.max.x - 16.0;
    if let Some((label, _)) = &card.action {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(rect.min.x + 120.0, rect.min.y),
                    pos2(rect.max.x - 16.0, rect.max.y),
                ))
                .layout(Layout::right_to_left(Align::Center)),
        );
        let response = ConsoleButton::new(label)
            .size(ControlSize::Medium)
            .prominent(primary)
            .focused(focused)
            .show(&mut child);
        pressed = response.clicked();
        button_left = response.rect.min.x - 16.0;
    }
    let x = disc.max.x + 16.0;
    let room = (button_left - x).max(80.0);
    let title = clamped(
        &painter,
        NAME,
        &card.title,
        egui::Color32::PLACEHOLDER,
        room,
        1,
    );
    let detail = clamped(
        &painter,
        STATE,
        &card.detail,
        egui::Color32::PLACEHOLDER,
        room,
        1,
    );
    let block = title.size().y + 2.0 + detail.size().y;
    let top = rect.center().y - block * 0.5;
    let title_height = title.size().y;
    painter.galley(pos2(x, top), title, color::text());
    painter.galley(
        pos2(x, top + title_height + 2.0),
        detail,
        color::text_row_detail(),
    );
    pressed
}

#[cfg(test)]
mod tests;
