//! Messages the way a console shows them (docs/DESIGN.md 17, "Messages"): a list of conversations
//! with the newest message of each, and a conversation as a full-screen thread of bubbles with
//! the composer at the bottom.
//!
//! The list and the thread are drawn here, not as rows: a bubble is not a row, and the composer
//! has to stay where it is while the messages scroll. What the screens say is built by plain
//! functions ([`entries`], [`thread`]) that tests read without a window.

use anyps5_core::online::api::{Account, AccountId, ConversationKind, EDIT_WINDOW_SECS, Message};
use anyps5_core::online::{Blocked, delivered_to_other, others, plain, read_by_other, seen_by};
use egui::containers::scroll_area::ScrollBarVisibility;
use egui::{Align, Align2, Color32, Margin, Rect, ScrollArea, Sense, Ui, pos2, vec2};

use super::feed::paint_avatar;
use super::local::{Avatar, Local};
use super::{OnlineData, OnlinePanel, restriction};
use crate::screens::format;
use crate::screens::online_social_strings::SocialText as S;
use crate::screens::online_strings::OnlineText as T;
use crate::screens::scroll::Follow;
use crate::ui::anim::{AnimExt, Motion};
use crate::ui::components::{self, corner, paint_capsule};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{self, Command};
use crate::ui::text;
use crate::ui::theme::{TextStyle, Weight, color, fade, mix, radius, size, ty, white};

/// The widest the thread is drawn: a line of a message stays readable on a wide screen.
const COLUMN: f32 = 940.0;
/// A bubble takes at most this share of the column.
const BUBBLE_SHARE: f32 = 0.66;
const BUBBLE_PAD_X: f32 = 16.0;
const BUBBLE_PAD_Y: f32 = 10.0;
const BUBBLE_RADIUS: f32 = 20.0;
/// The corner next to the sender's edge flattens inside a run of one sender's messages.
const BUBBLE_JOIN: f32 = 6.0;
const FACE: f32 = 32.0;
const FACE_GAP: f32 = 10.0;
const RUN_GAP: f32 = 14.0;
const LINE_GAP: f32 = 3.0;
/// Messages written within this many seconds of each other by one sender form a run.
const RUN_SECONDS: u64 = 5 * 60;

const BODY: TextStyle = TextStyle::new(17.0, Weight::Regular);
const NAME: TextStyle = TextStyle::new(13.0, Weight::SemiBold);
const META: TextStyle = ty::DETAIL;
const QUOTE: TextStyle = TextStyle::new(14.0, Weight::Regular);
const DAY: TextStyle = TextStyle::new(13.0, Weight::SemiBold);

/// A row of the list of conversations.
const ENTRY_HEIGHT: f32 = 78.0;
const ENTRY_FACE: f32 = 52.0;
const ENTRY_PAD_X: f32 = 16.0;

/// The size of the composer bar and its buttons.
const COMPOSER_HEIGHT: f32 = 56.0;
const COMPOSER_BUTTON: f32 = 44.0;

// ---------------------------------------------------------------------------------------------
// What the list says
// ---------------------------------------------------------------------------------------------

/// A person's picture in the list or the thread.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub account: AccountId,
    pub handle: String,
    pub avatar: Option<Avatar>,
}

/// One conversation in the list.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: String,
    pub kind: ConversationKind,
    /// A message request: the player has not answered it.
    pub request: bool,
    pub title: String,
    /// The newest message in one line: "You: ...", "Juno: ..." in a group, empty when none.
    pub preview: String,
    pub when: String,
    pub unread: u64,
    pub pinned: bool,
    pub muted: bool,
    /// Somebody types: the preview says so.
    pub typing: bool,
    /// The people the picture shows: the other for a direct chat, up to two for a group.
    pub faces: Vec<Face>,
}

fn face(data: &OnlineData<'_>, local: &Local, account: &Account) -> Face {
    let hash = data
        .online
        .avatars
        .get(&account.id)
        .and_then(|h| h.as_deref());
    Face {
        account: account.id,
        handle: account.handle.clone(),
        avatar: local.avatar(hash),
    }
}

fn offset_at(data: &OnlineData<'_>, unix: i64) -> i32 {
    data.utc_offset
        .unwrap_or_else(|| format::local_offset_seconds(unix.saturating_mul(1_000_000)))
}

/// The text of a message in one line, or what stands in its place.
fn line_of(message: &Message, language: Language) -> String {
    if message.deleted {
        S::Removed.text(language).to_owned()
    } else {
        plain(&message.text)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The conversations as the list shows them: requests first, then the pinned ones, then the
/// newest.
pub fn entries(data: &OnlineData<'_>, local: &Local) -> Vec<Entry> {
    let online = data.online;
    let social = &online.social;
    let me = online.account.as_ref().map_or(AccountId(0), |a| a.id);
    let language = data.language;
    let now = data.now;
    let build = |c: &anyps5_core::online::api::Conversation| {
        let others_of = others(c, me);
        let faces: Vec<Face> = match c.kind {
            ConversationKind::Direct => others_of
                .first()
                .map(|a| vec![face(data, local, a)])
                .unwrap_or_default(),
            _ => others_of
                .iter()
                .take(2)
                .map(|a| face(data, local, a))
                .collect(),
        };
        let typers = social.typers(&c.id, u64::try_from(now).unwrap_or(0), me);
        let preview = if !typers.is_empty() {
            S::TypingOne.format(language, &[Arg::Str(&typers[0].handle)])
        } else {
            match social.preview(&c.id) {
                Some(m) => {
                    let text = line_of(m, language);
                    if m.sender.id == me {
                        format!("{} {text}", T::You.text(language).to_owned() + ":")
                    } else if c.kind == ConversationKind::Direct {
                        text
                    } else {
                        format!("{}: {text}", m.sender.handle)
                    }
                }
                None => String::new(),
            }
        };
        let when = social.preview(&c.id).map_or(String::new(), |m| {
            let unix = i64::try_from(m.created).unwrap_or(0);
            format::list_time(unix, now, offset_at(data, unix), language)
        });
        Entry {
            id: c.id.clone(),
            kind: c.kind,
            request: c.request,
            title: super::social::chat_title(c, me, language),
            preview,
            when,
            unread: if c.request { 1 } else { social.unread(&c.id) },
            pinned: c.pinned,
            muted: c.muted,
            typing: !typers.is_empty(),
            faces,
        }
    };
    let mut list: Vec<Entry> = social.requests().iter().map(build).collect();
    list.extend(social.conversations().into_iter().map(build));
    list
}

// ---------------------------------------------------------------------------------------------
// What the thread says
// ---------------------------------------------------------------------------------------------

/// One message as a bubble.
#[derive(Clone, Debug, PartialEq)]
pub struct Bubble {
    /// Its place among the messages shown: what the focus counts.
    pub index: usize,
    pub seq: u64,
    pub mine: bool,
    pub sender: String,
    /// The name above the first bubble of a run, in a crowd.
    pub name: Option<String>,
    /// The picture beside the last bubble of a run, in a crowd.
    pub face: Option<Face>,
    pub text: String,
    pub removed: bool,
    /// Who and what the message answers.
    pub quote: Option<(String, String)>,
    /// Under the last bubble of a run: the time, Edited, Delivered or Seen.
    pub meta: Option<String>,
    pub first: bool,
    pub last: bool,
    /// More than two people write here: others' bubbles carry a name and a picture.
    pub crowd: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    /// "Show Earlier Messages": the first thing the focus reaches going up.
    Earlier,
    Day(String),
    Bubble(Box<Bubble>),
    /// "Lumen is typing…".
    Typing(Face, String),
    /// The thread has no messages, is loading, or could not be loaded.
    State(Icon, String, Option<String>),
}

/// What the thread of one conversation says, top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub items: Vec<Item>,
    /// How many bubbles: the messages the focus can stand on.
    pub messages: usize,
    pub earlier: bool,
}

/// How the composer at the bottom reads.
#[derive(Clone, Debug, PartialEq)]
pub enum Composer {
    /// The field with its buttons; `note` stands above it while the draft answers or edits.
    Write { draft: String, note: Option<Note> },
    /// A restriction pauses writing: this says so, and opens why.
    Paused {
        title: String,
        detail: Option<String>,
    },
    /// The player blocked the other.
    Blocked { who: String },
    /// A message request: the player answers it.
    Request,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    pub text: String,
    pub editing: bool,
}

/// Builds the thread of a conversation: bubbles grouped in runs, a separator at each new day, and
/// what happened to each message.
pub fn thread(data: &OnlineData<'_>, local: &Local, id: &str, shown: usize) -> Option<Thread> {
    let online = data.online;
    let social = &online.social;
    let conversation = social.get(id)?;
    let me = online.account.as_ref().map_or(AccountId(0), |a| a.id);
    let language = data.language;
    let now = data.now;
    let held = social.messages(id);
    let limit = if conversation.request { 5 } else { shown };
    let messages = &held[held.len().saturating_sub(limit)..];
    let direct = conversation.kind == ConversationKind::Direct;
    let read = read_by_other(conversation, me).filter(|_| direct);
    let delivered = delivered_to_other(conversation, me).filter(|_| direct);
    let last_own = messages
        .iter()
        .rev()
        .find(|m| m.sender.id == me)
        .map(|m| m.seq);
    let mut items = Vec::new();
    let earlier = held.len() > messages.len() || social.has_earlier(id);
    if earlier && !conversation.request {
        items.push(Item::Earlier);
    }
    let mut day = None;
    for (index, message) in messages.iter().enumerate() {
        let created = i64::try_from(message.created).unwrap_or(0);
        let offset = offset_at(data, created);
        let this_day = format::day_number(created, offset);
        if day != Some(this_day) {
            day = Some(this_day);
            items.push(Item::Day(format::day_label(created, now, offset, language)));
        }
        let mine = message.sender.id == me;
        let same_run = |other: &Message| {
            other.sender.id == message.sender.id
                && other.deleted == message.deleted
                && format::day_number(i64::try_from(other.created).unwrap_or(0), offset) == this_day
                && other.created.abs_diff(message.created) <= RUN_SECONDS
        };
        let first = index == 0
            || day_changed(&messages[index - 1], message, offset)
            || !same_run(&messages[index - 1]);
        let last = index + 1 == messages.len()
            || day_changed(&messages[index + 1], message, offset)
            || !same_run(&messages[index + 1]);
        let crowd = !direct;
        let name = (crowd && !mine && first).then(|| message.sender.handle.clone());
        let face = (crowd && !mine && last).then(|| {
            face(
                data,
                local,
                &Account {
                    id: message.sender.id,
                    handle: message.sender.handle.clone(),
                },
            )
        });
        let quote = message.reply.as_ref().map(|q| {
            let who = if q.sender.id == me {
                T::You.text(language).to_owned()
            } else {
                q.sender.handle.clone()
            };
            let text = if q.deleted {
                S::Removed.text(language).to_owned()
            } else {
                snippet(&q.text)
            };
            (who, text)
        });
        let meta = last.then(|| {
            let mut parts = vec![format::clock_text(created, offset)];
            if message.edited.is_some() && !message.deleted {
                parts.push(S::Edited.text(language).to_owned());
            }
            if mine && Some(message.seq) == last_own && !message.deleted {
                if read.is_some_and(|r| r >= message.seq) {
                    parts.push(S::Seen.text(language).to_owned());
                } else if delivered.is_some_and(|d| d >= message.seq) {
                    parts.push(S::Delivered.text(language).to_owned());
                } else if !direct {
                    let seen = seen_by(conversation, me, message.seq);
                    if seen > 0 {
                        parts.push(S::SeenBy.format(language, &[Arg::Int(seen as i64)]));
                    }
                }
            }
            parts.join(" · ")
        });
        items.push(Item::Bubble(Box::new(Bubble {
            index,
            seq: message.seq,
            mine,
            sender: message.sender.handle.clone(),
            name,
            face,
            text: if message.deleted {
                S::Removed.text(language).to_owned()
            } else {
                plain(&message.text)
            },
            removed: message.deleted,
            quote,
            meta,
            first,
            last,
            crowd,
        })));
    }
    let loaded = social.is_loaded(id);
    if messages.is_empty() {
        if social.has_failed(id) {
            items.push(Item::State(
                Icon::WARNING,
                S::LoadFailed.text(language).to_owned(),
                None,
            ));
        } else if !loaded && conversation.last > 0 {
            items.push(Item::State(
                Icon::ARROWS_CLOCKWISE,
                S::Loading.text(language).to_owned(),
                None,
            ));
        } else if conversation.request {
            // A request that holds no message yet is not listed; nothing to say.
        } else {
            items.push(Item::State(
                Icon::ENVELOPE_SIMPLE,
                S::NoMessages.text(language).to_owned(),
                None,
            ));
        }
    }
    let typers = social.typers(id, u64::try_from(now).unwrap_or(0), me);
    if let Some(first) = typers.first() {
        let line = super::social::typing_text(&typers, language);
        items.push(Item::Typing(face(data, local, first), line));
    }
    Some(Thread {
        items,
        messages: messages.len(),
        earlier: earlier && !conversation.request,
    })
}

fn day_changed(a: &Message, b: &Message, offset: i32) -> bool {
    let day = |m: &Message| format::day_number(i64::try_from(m.created).unwrap_or(0), offset);
    day(a) != day(b)
}

/// A quoted message in a line of its own, cut.
fn snippet(text: &str) -> String {
    anyps5_core::online::snippet(text, 90)
}

/// The composer under the thread.
pub fn composer(data: &OnlineData<'_>, id: &str) -> Option<Composer> {
    let online = data.online;
    let social = &online.social;
    let conversation = social.get(id)?;
    let language = data.language;
    let me = online.account.as_ref().map_or(AccountId(0), |a| a.id);
    if conversation.request {
        return Some(Composer::Request);
    }
    let blocked = (conversation.kind == ConversationKind::Direct)
        .then(|| others(conversation, me).first().copied())
        .flatten()
        .filter(|other| online.friends.blocked.iter().any(|b| b.id == other.id));
    if let Some(other) = blocked {
        return Some(Composer::Blocked {
            who: other.handle.clone(),
        });
    }
    let what = if online.is_stranger(conversation) {
        Blocked::Stranger
    } else {
        Blocked::Message
    };
    let now = u64::try_from(data.now).unwrap_or(0);
    if let Some(paused) = online.restrictions.blocking(what, now) {
        let row = restriction::paused_row("pausedSend", what, paused, data);
        return Some(Composer::Paused {
            title: row.title,
            detail: row.detail,
        });
    }
    let note = if social.editing(id).is_some() {
        Some(Note {
            text: S::EditingMessage.text(language).to_owned(),
            editing: true,
        })
    } else {
        social.replying_to(id).map(|m| {
            let who = if m.sender.id == me {
                T::You.text(language).to_owned()
            } else {
                m.sender.handle.clone()
            };
            Note {
                text: S::ReplyingTo.format(
                    language,
                    &[Arg::Str(&who), Arg::Str(&snippet_of(m, language))],
                ),
                editing: false,
            }
        })
    };
    Some(Composer::Write {
        draft: social.draft(id).to_owned(),
        note,
    })
}

fn snippet_of(message: &Message, language: Language) -> String {
    if message.deleted {
        S::Removed.text(language).to_owned()
    } else {
        anyps5_core::online::snippet(&message.text, 40)
    }
}

/// Can the player still edit the message?
pub fn editable(message: &Message, me: AccountId, now: i64) -> bool {
    message.sender.id == me
        && !message.deleted
        && u64::try_from(now).is_ok_and(|now| now <= message.created + EDIT_WINDOW_SECS)
}

// ---------------------------------------------------------------------------------------------
// Where the focus stands
// ---------------------------------------------------------------------------------------------

/// The buttons of the composer, left to right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Field,
    Send,
}

/// What of a request's answers has the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Accept,
    Decline,
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadFocus {
    /// "Show Earlier Messages".
    Earlier,
    /// A message, by its place among those shown.
    Message(usize),
    /// The note above the field: cancel the answer or the edit.
    Note,
    Composer(Slot),
    /// The answers of a message request.
    Answer(Answer),
    /// The paused or blocked bar.
    Bar,
}

impl Default for ThreadFocus {
    fn default() -> Self {
        ThreadFocus::Composer(Slot::Field)
    }
}

/// What the focus is made of.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub messages: usize,
    pub earlier: bool,
    pub note: bool,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Write,
    Bar,
    Request,
}

impl Shape {
    /// Where the focus goes down to from the last message.
    fn bottom(self) -> ThreadFocus {
        if self.note && self.kind == Kind::Write {
            ThreadFocus::Note
        } else {
            self.entry()
        }
    }

    fn entry(self) -> ThreadFocus {
        match self.kind {
            Kind::Write => ThreadFocus::Composer(Slot::Field),
            Kind::Bar => ThreadFocus::Bar,
            Kind::Request => ThreadFocus::Answer(Answer::Accept),
        }
    }

    fn top(self) -> Option<ThreadFocus> {
        match (self.earlier, self.messages) {
            (true, _) => Some(ThreadFocus::Earlier),
            (false, 0) => None,
            (false, _) => Some(ThreadFocus::Message(0)),
        }
    }

    fn last(self) -> ThreadFocus {
        if self.messages > 0 {
            ThreadFocus::Message(self.messages - 1)
        } else if self.earlier {
            ThreadFocus::Earlier
        } else {
            self.entry()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// The focus moved, or tried to.
    Moved(ThreadFocus, bool),
    Press(ThreadFocus),
    Back,
    /// The conversation's options.
    Options,
}

/// Applies a command to the focus of a thread.
pub fn step(focus: ThreadFocus, command: Command, shape: Shape) -> Step {
    use ThreadFocus as F;
    let moved = |to: F| Step::Moved(to, to != focus);
    // A focus that no longer exists (the messages shrank) lands on the nearest one.
    let focus = match focus {
        F::Message(_) if shape.messages == 0 => shape.last(),
        F::Message(i) => F::Message(i.min(shape.messages - 1)),
        F::Earlier if !shape.earlier => shape.top().unwrap_or_else(|| shape.entry()),
        F::Note if !shape.note => shape.entry(),
        F::Composer(_) if shape.kind != Kind::Write => shape.entry(),
        F::Answer(_) if shape.kind != Kind::Request => shape.entry(),
        F::Bar if shape.kind != Kind::Bar => shape.entry(),
        other => other,
    };
    match command {
        Command::Up => match focus {
            F::Earlier => Step::Moved(focus, false),
            F::Message(0) => Step::Moved(
                if shape.earlier { F::Earlier } else { focus },
                shape.earlier,
            ),
            F::Message(i) => Step::Moved(F::Message(i - 1), true),
            F::Note => Step::Moved(shape.last(), true),
            F::Composer(_) | F::Answer(_) | F::Bar => {
                if shape.note && shape.kind == Kind::Write {
                    Step::Moved(F::Note, true)
                } else {
                    let to = shape.last();
                    Step::Moved(to, to != focus)
                }
            }
        },
        Command::Down => match focus {
            F::Earlier => {
                let to = if shape.messages > 0 {
                    F::Message(0)
                } else {
                    shape.bottom()
                };
                Step::Moved(to, true)
            }
            F::Message(i) if i + 1 < shape.messages => Step::Moved(F::Message(i + 1), true),
            F::Message(_) => Step::Moved(shape.bottom(), true),
            F::Note => Step::Moved(shape.entry(), true),
            F::Composer(_) | F::Answer(_) | F::Bar => Step::Moved(focus, false),
        },
        Command::Left => match focus {
            F::Composer(Slot::Send) => moved(F::Composer(Slot::Field)),
            F::Answer(Answer::Block) => moved(F::Answer(Answer::Decline)),
            F::Answer(Answer::Decline) => moved(F::Answer(Answer::Accept)),
            _ => Step::Moved(focus, false),
        },
        Command::Right => match focus {
            F::Composer(Slot::Field) => moved(F::Composer(Slot::Send)),
            F::Answer(Answer::Accept) => moved(F::Answer(Answer::Decline)),
            F::Answer(Answer::Decline) => moved(F::Answer(Answer::Block)),
            _ => Step::Moved(focus, false),
        },
        Command::Select => Step::Press(focus),
        Command::Back => Step::Back,
        Command::Options => Step::Options,
    }
}

/// The list's focus: the entry by its conversation, so a new message that moves it up the list
/// does not move the focus off it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListFocus {
    pub id: Option<String>,
}

impl ListFocus {
    /// The index of the focused entry; the first when the conversation is gone.
    pub fn index(&self, entries: &[Entry]) -> usize {
        self.id
            .as_deref()
            .and_then(|id| entries.iter().position(|e| e.id == id))
            .unwrap_or(0)
    }

    pub fn set(&mut self, entries: &[Entry], index: usize) {
        self.id = entries.get(index).map(|e| e.id.clone());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListStep {
    Moved(usize, bool),
    Press(usize),
    Back,
}

/// A command on the list of conversations. Left is the rail's, so it is not taken here.
pub fn list_step(index: usize, command: Command, count: usize) -> ListStep {
    let last = count.saturating_sub(1);
    let index = index.min(last);
    match command {
        Command::Up => ListStep::Moved(index.saturating_sub(1), index > 0),
        Command::Down => ListStep::Moved((index + 1).min(last), index < last),
        Command::Select => ListStep::Press(index),
        Command::Back | Command::Options => ListStep::Back,
        Command::Left | Command::Right => ListStep::Moved(index, false),
    }
}

// ---------------------------------------------------------------------------------------------
// The panel's part
// ---------------------------------------------------------------------------------------------

/// What the mouse did.
#[derive(Clone, Debug, PartialEq)]
pub enum Mouse {
    Entry(usize),
    Focus(ThreadFocus),
    Press(ThreadFocus),
    Typed(String),
    Submit,
}

/// What the panel keeps of the chat screens between frames.
#[derive(Clone, Debug, Default)]
pub struct ChatState {
    pub list: ListFocus,
    pub focus: ThreadFocus,
    /// The conversation the thread was drawn for last: a new one starts at the bottom.
    pub opened: Option<String>,
    /// The text field takes the keys on the next frame.
    pub type_now: bool,
    /// How many bubbles the thread has shown, to scroll down when one arrives.
    pub seen: usize,
}

impl OnlinePanel {
    /// The list of conversations shows: the page is Messages and there is a conversation.
    pub(super) fn chat_is_list(&self, data: &OnlineData<'_>) -> bool {
        self.page == super::Page::Messages && !self.chat_entries(data).is_empty()
    }

    /// The thread of a conversation or request shows full screen.
    pub fn chat_is_thread(&self, data: &OnlineData<'_>) -> bool {
        match &self.page {
            super::Page::Conversation(id) | super::Page::MessageRequest(id) => {
                data.online.social.get(id).is_some()
            }
            _ => false,
        }
    }

    /// Pages of the chat that stand without the rail: the thread and the pages it opens.
    pub fn chat_screen(&self, data: &OnlineData<'_>) -> bool {
        match &self.page {
            super::Page::Conversation(_) | super::Page::MessageRequest(_) => {
                self.chat_is_thread(data)
            }
            super::Page::Message(..)
            | super::Page::QuickReplies(_)
            | super::Page::Keyboard(_)
            | super::Page::ChatOptions(_) => true,
            _ => false,
        }
    }

    pub(super) fn chat_entries(&self, data: &OnlineData<'_>) -> Vec<Entry> {
        entries(data, &self.local)
    }

    fn chat_shape(&self, data: &OnlineData<'_>, id: &str) -> Option<(Thread, Composer, Shape)> {
        let thread = thread(data, &self.local, id, self.shown.max(super::social::SHOWN))?;
        let composer = composer(data, id)?;
        let shape = Shape {
            messages: thread.messages,
            earlier: thread.earlier,
            note: matches!(&composer, Composer::Write { note: Some(_), .. }),
            kind: match composer {
                Composer::Write { .. } => Kind::Write,
                Composer::Request => Kind::Request,
                _ => Kind::Bar,
            },
        };
        Some((thread, composer, shape))
    }

    /// A command on the list. `true` leaves the screen.
    pub(super) fn chat_list_command(
        &mut self,
        shell: &mut crate::ui::Shell,
        command: Command,
        data: &OnlineData<'_>,
        actions: &mut Vec<super::OnlineAction>,
    ) -> bool {
        let entries = self.chat_entries(data);
        let index = self.chat.list.index(&entries);
        match list_step(index, command, entries.len()) {
            ListStep::Moved(to, moved) => {
                if moved {
                    shell.play(crate::ui::sound::Cue::Move);
                }
                self.chat.list.set(&entries, to);
                false
            }
            ListStep::Press(at) => {
                shell.play(crate::ui::sound::Cue::Select);
                self.chat_open(&entries, at, data);
                let _ = actions;
                false
            }
            ListStep::Back => true,
        }
    }

    fn chat_open(&mut self, entries: &[Entry], at: usize, data: &OnlineData<'_>) {
        let Some(entry) = entries.get(at) else { return };
        self.chat.list.set(entries, at);
        let page = if entry.request {
            super::Page::MessageRequest(entry.id.clone())
        } else {
            super::Page::Conversation(entry.id.clone())
        };
        self.go(page, data);
    }

    /// A command on the thread. `true` leaves the thread.
    pub(super) fn chat_thread_command(
        &mut self,
        shell: &mut crate::ui::Shell,
        command: Command,
        data: &OnlineData<'_>,
        actions: &mut Vec<super::OnlineAction>,
    ) -> bool {
        let id = match &self.page {
            super::Page::Conversation(id) | super::Page::MessageRequest(id) => id.clone(),
            _ => return false,
        };
        let Some((_, composer, shape)) = self.chat_shape(data, &id) else {
            return false;
        };
        match step(self.chat.focus, command, shape) {
            Step::Moved(to, moved) => {
                if moved {
                    shell.play(crate::ui::sound::Cue::Move);
                }
                self.chat.focus = to;
                self.chat.type_now = false;
                false
            }
            Step::Press(focus) => {
                shell.play(crate::ui::sound::Cue::Select);
                self.chat.focus = focus;
                self.chat_press(focus, &id, &composer, shell.input.pad_in_use, data, actions);
                false
            }
            Step::Options => {
                shell.play(crate::ui::sound::Cue::Select);
                self.go(super::Page::ChatOptions(id), data);
                false
            }
            Step::Back => true,
        }
    }

    /// What pressing the focused thing does.
    pub(super) fn chat_press(
        &mut self,
        focus: ThreadFocus,
        id: &str,
        composer: &Composer,
        pad: bool,
        data: &OnlineData<'_>,
        actions: &mut Vec<super::OnlineAction>,
    ) {
        use super::{OnlineAction as A, Page};
        match focus {
            ThreadFocus::Earlier => {
                self.shown += super::social::SHOWN;
                if data.online.social.messages(id).len() < self.shown {
                    actions.push(A::LoadEarlier(id.to_owned()));
                }
            }
            ThreadFocus::Message(index) => {
                let seq = self.chat_message_seq(data, id, index);
                if let Some(seq) = seq {
                    self.go(Page::Message(id.to_owned(), seq), data);
                }
            }
            ThreadFocus::Note => actions.push(A::CancelCompose(id.to_owned())),
            ThreadFocus::Composer(Slot::Field) => {
                // A pad has no keys to type with: the keyboard on the screen opens. A keyboard
                // types into the field.
                if pad {
                    self.go(Page::Keyboard(id.to_owned()), data);
                } else {
                    self.chat.type_now = true;
                }
            }
            ThreadFocus::Composer(Slot::Send) => actions.push(A::SendMessage(id.to_owned())),
            ThreadFocus::Answer(Answer::Accept) => actions.push(A::AcceptRequest(id.to_owned())),
            ThreadFocus::Answer(Answer::Decline) => {
                actions.push(A::LeaveConversation(id.to_owned()));
                self.go(Page::Messages, data);
            }
            ThreadFocus::Answer(Answer::Block) => {
                let me = super::OnlinePanel::me(data);
                if let Some(c) = data.online.social.get(id) {
                    for other in others(c, me) {
                        actions.push(A::Block(other.id));
                    }
                }
                actions.push(A::LeaveConversation(id.to_owned()));
                self.go(Page::Messages, data);
            }
            ThreadFocus::Bar => match composer {
                Composer::Paused { .. } => {
                    self.run("pausedSend", data, actions);
                }
                Composer::Blocked { .. } => {
                    let me = super::OnlinePanel::me(data);
                    if let Some(other) = data
                        .online
                        .social
                        .get(id)
                        .and_then(|c| others(c, me).first().map(|a| a.id))
                    {
                        actions.push(A::Unblock(other));
                    }
                }
                _ => {}
            },
        }
    }

    fn chat_message_seq(&self, data: &OnlineData<'_>, id: &str, index: usize) -> Option<u64> {
        let held = data.online.social.messages(id);
        let limit = if matches!(self.page, super::Page::MessageRequest(_)) {
            5
        } else {
            self.shown.max(super::social::SHOWN)
        };
        let shown = &held[held.len().saturating_sub(limit)..];
        shown.get(index).map(|m| m.seq)
    }

    /// Draws the list of conversations into `ui`.
    pub(super) fn chat_list_draw(
        &mut self,
        ui: &mut Ui,
        data: &OnlineData<'_>,
        focused: bool,
        actions: &mut Vec<super::OnlineAction>,
    ) {
        let entries = self.chat_entries(data);
        let index = self.chat.list.index(&entries);
        // Previews of the first conversations are asked for once; the rest as the focus walks.
        let ids: Vec<String> = entries
            .iter()
            .skip(index.saturating_sub(4))
            .take(14)
            .map(|e| e.id.clone())
            .collect();
        actions.push(super::OnlineAction::NeedPreviews(ids));
        let events = list(ui, &entries, focused.then_some(index), data.language);
        for event in events {
            if let Mouse::Entry(at) = event {
                self.chat_open(&entries, at, data);
            }
        }
    }

    /// Draws the thread and its composer into `ui`.
    pub(super) fn chat_thread_draw(
        &mut self,
        ui: &mut Ui,
        data: &OnlineData<'_>,
        focused: bool,
        actions: &mut Vec<super::OnlineAction>,
    ) {
        let id = match &self.page {
            super::Page::Conversation(id) | super::Page::MessageRequest(id) => id.clone(),
            _ => return,
        };
        let Some((thread, composer, shape)) = self.chat_shape(data, &id) else {
            return;
        };
        let Some(conversation) = data.online.social.get(&id) else {
            return;
        };
        if self.chat.opened.as_deref() != Some(id.as_str()) {
            self.chat.opened = Some(id.clone());
            self.chat.focus = shape.entry();
            self.chat.seen = 0;
        }
        let me = Self::me(data);
        let header = Header {
            kind: conversation.kind,
            title: super::social::chat_title(conversation, me, data.language),
            status: self.conversation_status(data, conversation),
            faces: match conversation.kind {
                ConversationKind::Direct => others(conversation, me)
                    .first()
                    .map(|a| vec![face(data, &self.local, a)])
                    .unwrap_or_default(),
                _ => others(conversation, me)
                    .iter()
                    .take(2)
                    .map(|a| face(data, &self.local, a))
                    .collect(),
            },
        };
        let focus = focused.then_some(self.chat.focus);
        let grew = thread.messages != self.chat.seen;
        self.chat.seen = thread.messages;
        let events = thread_view(
            ui,
            &header,
            &thread,
            &composer,
            focus,
            self.chat.type_now,
            grew,
            data.language,
        );
        self.chat.type_now = false;
        let pad = false;
        for event in events {
            match event {
                Mouse::Focus(to) => self.chat.focus = to,
                Mouse::Press(to) => {
                    self.chat.focus = to;
                    self.chat_press(to, &id, &composer, pad, data, actions);
                }
                Mouse::Typed(text) => actions.push(super::OnlineAction::Draft {
                    conversation: id.clone(),
                    text,
                }),
                Mouse::Submit => actions.push(super::OnlineAction::SendMessage(id.clone())),
                Mouse::Entry(_) => {}
            }
        }
    }

    /// The line under a conversation's name: a friend's presence, the people of a group.
    fn conversation_status(
        &self,
        data: &OnlineData<'_>,
        conversation: &anyps5_core::online::api::Conversation,
    ) -> Option<String> {
        self.conversation_detail(data, conversation)
    }
}

struct Header {
    kind: ConversationKind,
    title: String,
    status: Option<String>,
    faces: Vec<Face>,
}

// ---------------------------------------------------------------------------------------------
// Drawing: the list
// ---------------------------------------------------------------------------------------------

fn paint_faces(painter: &egui::Painter, rect: Rect, faces: &[Face], kind: ConversationKind) {
    match faces {
        [] => {
            painter.circle_filled(rect.center(), rect.width() * 0.5, white(0.14));
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                Icon::USERS.glyph(),
                Icon::USERS.font(rect.width() * 0.46, false),
                white(0.8),
            );
        }
        [one] if kind == ConversationKind::Direct => {
            paint_avatar(painter, rect, one.avatar, one.account, &one.handle);
        }
        many => {
            // Two faces overlap: the second sits low and to the right.
            let small = rect.width() * 0.66;
            let a = Rect::from_min_size(rect.min, vec2(small, small));
            let b = Rect::from_min_size(rect.max - vec2(small, small), vec2(small, small));
            if let Some(second) = many.get(1) {
                paint_avatar(painter, b, second.avatar, second.account, &second.handle);
                painter.circle_stroke(
                    b.center(),
                    small * 0.5 + 1.5,
                    egui::Stroke::new(2.0, color::surface_row()),
                );
            }
            if let Some(first) = many.first() {
                paint_avatar(painter, a, first.avatar, first.account, &first.handle);
            }
        }
    }
}

/// The scrolling list of conversations.
fn list(ui: &mut Ui, entries: &[Entry], focus: Option<usize>, language: Language) -> Vec<Mouse> {
    let ctx = ui.ctx().clone();
    let id = ui.make_persistent_id("chat-list");
    let (follow, scroll_to) = Follow::begin(&ctx, id, focus);
    let mut events = Vec::new();
    let mut geometry: Vec<(f32, f32)> = Vec::with_capacity(entries.len());
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
                ui.spacing_mut().item_spacing.y = size::ROW_GAP;
                let origin = ui.cursor().min.y;
                let mut previous_request = None;
                for (index, entry) in entries.iter().enumerate() {
                    if previous_request != Some(entry.request) {
                        previous_request = Some(entry.request);
                        if entry.request {
                            caption(ui, S::MessageRequests.text(language));
                        } else if index > 0 {
                            caption(ui, S::Conversations.text(language));
                        }
                    }
                    let shown = entry_row(
                        ui,
                        entry,
                        index,
                        focus == Some(index),
                        language,
                        &mut events,
                    );
                    geometry.push((shown.min.y - origin, shown.height()));
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
    events
}

fn caption(ui: &mut Ui, text: &str) {
    let painter = ui.painter().clone();
    let galley = text::layout(
        &painter,
        ty::SECTION_CAPTION,
        text,
        Color32::PLACEHOLDER,
        f32::INFINITY,
    );
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), galley.size().y + 14.0),
        Sense::hover(),
    );
    painter.galley(
        pos2(rect.min.x + ENTRY_PAD_X, rect.max.y - galley.size().y - 2.0),
        galley,
        color::text_row_detail(),
    );
}

fn entry_row(
    ui: &mut Ui,
    entry: &Entry,
    index: usize,
    focused: bool,
    language: Language,
    events: &mut Vec<Mouse>,
) -> Rect {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, ENTRY_HEIGHT), Sense::click());
    crate::ui::a11y::follow(&ctx, response.id, focused);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!(
                "{}, {}{}",
                entry.title,
                entry.preview,
                if entry.unread > 0 {
                    format!(
                        ", {}",
                        S::NewCount.format(language, &[Arg::Int(entry.unread as i64)])
                    )
                } else {
                    String::new()
                }
            ),
        )
    });
    if response.clicked() {
        events.push(Mouse::Entry(index));
    }
    let hovered = input::hovered(&ctx, &response);
    if hovered {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if !ui.is_rect_visible(rect) {
        return rect;
    }
    let t = ctx.focus_t((response.id, "entry"), focused);
    let hover_t = ctx.animate_to((response.id, "hover"), f32::from(hovered), Motion::Hover);
    components::paint_surface(
        &painter,
        rect,
        radius::ROW,
        color::surface_row(),
        hover_t,
        t,
        0.0,
        size::FOCUS_RING,
    );
    let face = Rect::from_min_size(
        pos2(rect.min.x + ENTRY_PAD_X, rect.center().y - ENTRY_FACE * 0.5),
        vec2(ENTRY_FACE, ENTRY_FACE),
    );
    if entry.request {
        painter.circle_filled(face.center(), ENTRY_FACE * 0.5, white(0.14));
        painter.text(
            face.center(),
            Align2::CENTER_CENTER,
            Icon::ENVELOPE_SIMPLE.glyph(),
            Icon::ENVELOPE_SIMPLE.font(ENTRY_FACE * 0.46, false),
            white(0.85),
        );
    } else {
        paint_faces(&painter, face, &entry.faces, entry.kind);
    }
    let text_x = face.max.x + 16.0;
    let right = rect.max.x - ENTRY_PAD_X;
    let unread = entry.unread > 0 && !entry.muted;
    // The time and the marks on the right of the first line.
    let when = text::layout(
        &painter,
        META,
        &entry.when,
        Color32::PLACEHOLDER,
        f32::INFINITY,
    );
    let mut x = right;
    let top = rect.center().y - 22.0;
    if !entry.when.is_empty() {
        x -= when.size().x;
        painter.galley(
            pos2(x, top + 3.0),
            when,
            fade(Color32::WHITE, if unread { 0.9 } else { 0.55 }),
        );
        x -= 10.0;
    }
    for (on, icon) in [
        (entry.muted, Icon::BELL_SLASH),
        (entry.pinned, Icon::PUSH_PIN),
    ] {
        if on {
            x -= 14.0;
            painter.text(
                pos2(x + 7.0, top + 11.0),
                Align2::CENTER_CENTER,
                icon.glyph(),
                icon.font(14.0, false),
                fade(Color32::WHITE, 0.55),
            );
            x -= 6.0;
        }
    }
    // The title, in the room left.
    let title_style = if unread {
        TextStyle::new(ty::ROW_TITLE.size, Weight::SemiBold)
    } else {
        ty::ROW_TITLE
    };
    let title = one_line(
        &painter,
        title_style,
        &entry.title,
        (x - text_x - 8.0).max(40.0),
    );
    painter.galley(pos2(text_x, top), title, Color32::WHITE);
    // The preview and the count.
    let badge_width = if unread { 30.0 } else { 0.0 };
    let preview_width = (right - text_x - badge_width - 8.0).max(40.0);
    let ink = if entry.typing {
        fade(Color32::WHITE, 0.9)
    } else if unread {
        fade(Color32::WHITE, 0.82)
    } else {
        fade(Color32::WHITE, 0.58)
    };
    let preview = one_line(
        &painter,
        TextStyle::new(ty::ROW_INFO.size, Weight::Regular),
        &if entry.request && entry.preview.is_empty() {
            S::WantsToMessage.text(language).to_owned()
        } else {
            entry.preview.clone()
        },
        preview_width,
    );
    painter.galley(pos2(text_x, top + 27.0), preview, ink);
    if unread {
        let count = if entry.request {
            None
        } else {
            Some(if entry.unread > 99 {
                "99+".to_owned()
            } else {
                entry.unread.to_string()
            })
        };
        let size = vec2(
            if count.as_ref().is_some_and(|c| c.len() > 1) {
                30.0
            } else {
                22.0
            },
            22.0,
        );
        let badge = Rect::from_min_size(pos2(right - size.x, top + 26.0), size);
        match count {
            Some(count) => {
                paint_capsule(&painter, badge, color::primary());
                painter.text(
                    badge.center(),
                    Align2::CENTER_CENTER,
                    count,
                    TextStyle::new(13.0, Weight::SemiBold).font(),
                    color::on_primary(),
                );
            }
            None => {
                painter.circle_filled(badge.center(), 6.0, color::primary());
            }
        }
    }
    rect
}

/// One line of text cut with an ellipsis at `width`.
fn one_line(
    painter: &egui::Painter,
    style: TextStyle,
    text: &str,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job =
        egui::text::LayoutJob::simple(text.to_owned(), style.font(), Color32::PLACEHOLDER, width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.sections[0].format.line_height = Some(style.line_height());
    painter.layout_job(job)
}

// ---------------------------------------------------------------------------------------------
// Drawing: the thread
// ---------------------------------------------------------------------------------------------

/// The thread: a header, the bubbles that scroll, and the composer that does not.
#[allow(clippy::too_many_arguments)]
fn thread_view(
    ui: &mut Ui,
    header: &Header,
    thread: &Thread,
    composer: &Composer,
    focus: Option<ThreadFocus>,
    type_now: bool,
    grew: bool,
    language: Language,
) -> Vec<Mouse> {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect();
    let width = full.width().min(COLUMN);
    let column = Rect::from_min_size(
        pos2(full.center().x - width * 0.5, full.min.y),
        vec2(width, full.height()),
    );
    let mut events = Vec::new();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(column));
    let ui = &mut child;
    let painter = ui.painter().clone();

    // The header: the picture, the name and the line under it.
    let head = Rect::from_min_size(column.min, vec2(width, 64.0));
    let face_box = Rect::from_min_size(
        pos2(head.min.x + 4.0, head.center().y - 22.0),
        vec2(44.0, 44.0),
    );
    paint_faces(&painter, face_box, &header.faces, header.kind);
    let name_x = face_box.max.x + 14.0;
    let name = one_line(&painter, ty::TOP_BAR_TITLE, &header.title, width - 80.0);
    let name_h = name.size().y;
    let status_h = if header.status.is_some() { 20.0 } else { 0.0 };
    let name_y = head.center().y - (name_h + status_h) * 0.5;
    painter.galley(pos2(name_x, name_y), name, Color32::WHITE);
    if let Some(status) = &header.status {
        let line = one_line(&painter, ty::DETAIL_LARGE, status, width - 80.0);
        painter.galley(
            pos2(name_x, name_y + name_h + 1.0),
            line,
            fade(Color32::WHITE, 0.6),
        );
    }

    // The composer sits at the bottom; the thread takes the room between.
    let note_h = match composer {
        Composer::Write { note: Some(_), .. } => 40.0,
        Composer::Request => 30.0,
        _ => 0.0,
    };
    let bar_h = COMPOSER_HEIGHT + note_h + 16.0;
    let body = Rect::from_min_max(
        pos2(column.min.x, head.max.y),
        pos2(column.max.x, column.max.y - bar_h),
    );
    let bar = Rect::from_min_max(pos2(column.min.x, column.max.y - bar_h), column.max);

    let focus_item = focus.and_then(|f| match f {
        ThreadFocus::Earlier => Some(0),
        ThreadFocus::Message(i) => Some(i + usize::from(thread.earlier)),
        _ => None,
    });
    let id = ui.make_persistent_id("chat-thread");
    let (follow, scroll_to) = Follow::begin(&ctx, id, focus_item);
    let bottom_id = id.with("at-bottom");
    let at_bottom: bool = ctx.data(|d| d.get_temp(bottom_id)).unwrap_or(true);
    let focus_here = matches!(
        focus,
        Some(
            ThreadFocus::Composer(_)
                | ThreadFocus::Note
                | ThreadFocus::Answer(_)
                | ThreadFocus::Bar
        )
    );
    let mut area = ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
        .max_height(body.height());
    // A thread rests at its end: it stays there while messages come in, unless the player walked
    // up. Walking back down to the composer takes it to the end again.
    let height_id = id.with("height");
    let last_height: f32 = ctx.data(|d| d.get_temp(height_id)).unwrap_or(0.0);
    let end_id = id.with("end");
    let end: f32 = ctx.data(|d| d.get_temp(end_id)).unwrap_or(0.0);
    area = area.stick_to_bottom(true);
    if let Some(offset) = scroll_to {
        area = area.vertical_scroll_offset(offset);
    } else if focus_here && !at_bottom && end > 0.0 {
        area = area.vertical_scroll_offset(end);
    }
    let _ = grew;
    let mut geometry: Vec<(f32, f32)> = Vec::new();
    let mut body_ui = ui.new_child(egui::UiBuilder::new().max_rect(body));
    let output = area.show(&mut body_ui, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let inner_width = ui.available_width();
        // The messages sit at the bottom of a short thread: the room it leaves goes above them.
        let spare = (body.height() - last_height).max(0.0);
        ui.add_space(spare);
        let origin = ui.cursor().min.y;
        ui.add_space(8.0);
        for item in &thread.items {
            match item {
                Item::Earlier => {
                    let focused = focus == Some(ThreadFocus::Earlier);
                    let rect = earlier_button(ui, focused, language, &mut events);
                    geometry.push((rect.min.y - origin, rect.height()));
                }
                Item::Day(label) => day_separator(ui, label),
                Item::Bubble(bubble) => {
                    let focused = focus == Some(ThreadFocus::Message(bubble.index));
                    let rect = bubble_view(ui, bubble, inner_width, focused, &mut events);
                    geometry.push((rect.min.y - origin, rect.height()));
                }
                Item::Typing(who, line) => typing_view(ui, who, line),
                Item::State(icon, title, note) => state_view(ui, *icon, title, note.as_deref()),
            }
        }
        ui.add_space(8.0);
    });
    let max_offset = (output.content_size.y - output.inner_rect.height()).max(0.0);
    ctx.data_mut(|d| d.insert_temp(end_id, max_offset));
    let spare_now = (body.height() - last_height).max(0.0);
    ctx.data_mut(|d| d.insert_temp(height_id, output.content_size.y - spare_now));
    ctx.data_mut(|d| {
        d.insert_temp(
            bottom_id,
            output.state.clamped_offset().y >= max_offset - 4.0,
        )
    });
    follow.end(
        &ctx,
        geometry,
        output.content_size.y,
        output.inner_rect.height(),
        output.state.clamped_offset().y,
    );

    // The composer.
    composer_view(
        ui,
        bar,
        note_h,
        composer,
        focus,
        type_now,
        language,
        &mut events,
    );
    events
}

fn earlier_button(ui: &mut Ui, focused: bool, language: Language, events: &mut Vec<Mouse>) -> Rect {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let label = S::ShowEarlier.text(language);
    let w = text::width(&painter, ty::BUTTON_MEDIUM, label) + 40.0;
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 56.0), Sense::hover());
    let rect = Rect::from_center_size(row.center(), vec2(w, 36.0));
    let response = ui.interact(rect, ui.make_persistent_id("earlier"), Sense::click());
    crate::ui::a11y::follow(&ctx, response.id, focused);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if response.clicked() {
        events.push(Mouse::Press(ThreadFocus::Earlier));
    }
    let t = ctx.focus_t((response.id, "earlier"), focused);
    let hovered = input::hovered(&ctx, &response);
    paint_capsule(&painter, rect, white(if hovered { 0.16 } else { 0.10 }));
    components::paint_focus_ring(&painter, rect, 18.0, t);
    text::paint(
        &painter,
        rect.center(),
        Align2::CENTER_CENTER,
        ty::BUTTON_MEDIUM,
        label,
        fade(Color32::WHITE, 0.9),
    );
    row
}

fn day_separator(ui: &mut Ui, label: &str) {
    let painter = ui.painter().clone();
    let galley = text::layout(&painter, DAY, label, Color32::PLACEHOLDER, f32::INFINITY);
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), galley.size().y + 26.0),
        Sense::hover(),
    );
    let w = galley.size().x + 28.0;
    let pill = Rect::from_center_size(rect.center(), vec2(w, galley.size().y + 8.0));
    paint_capsule(&painter, pill, white(0.08));
    painter.galley(
        pos2(
            pill.center().x - galley.size().x * 0.5,
            pill.center().y - galley.size().y * 0.5,
        ),
        galley,
        fade(Color32::WHITE, 0.7),
    );
}

fn typing_view(ui: &mut Ui, who: &Face, line: &str) {
    let painter = ui.painter().clone();
    let width = ui.available_width();
    let galley = text::layout(&painter, QUOTE, line, Color32::PLACEHOLDER, width * 0.6);
    let h = galley.size().y + 2.0 * BUBBLE_PAD_Y;
    let (row, _) = ui.allocate_exact_size(vec2(width, h + 12.0), Sense::hover());
    let face = Rect::from_min_size(
        pos2(row.min.x + 4.0, row.max.y - FACE - 6.0),
        vec2(FACE, FACE),
    );
    paint_avatar(&painter, face, who.avatar, who.account, &who.handle);
    let bubble = Rect::from_min_size(
        pos2(face.max.x + FACE_GAP, row.min.y + 6.0),
        vec2(galley.size().x + 2.0 * BUBBLE_PAD_X, h),
    );
    paint_capsule(&painter, bubble, white(0.08));
    painter.galley(
        pos2(bubble.min.x + BUBBLE_PAD_X, bubble.min.y + BUBBLE_PAD_Y),
        galley,
        fade(Color32::WHITE, 0.72),
    );
}

fn state_view(ui: &mut Ui, icon: Icon, title: &str, note: Option<&str>) {
    let painter = ui.painter().clone();
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 160.0), Sense::hover());
    painter.text(
        pos2(rect.center().x, rect.center().y - 24.0),
        Align2::CENTER_CENTER,
        icon.glyph(),
        icon.font(34.0, false),
        fade(Color32::WHITE, 0.5),
    );
    text::paint(
        &painter,
        pos2(rect.center().x, rect.center().y + 14.0),
        Align2::CENTER_CENTER,
        ty::ROW_INFO,
        title,
        fade(Color32::WHITE, 0.7),
    );
    if let Some(note) = note {
        text::paint(
            &painter,
            pos2(rect.center().x, rect.center().y + 38.0),
            Align2::CENTER_CENTER,
            ty::DETAIL,
            note,
            fade(Color32::WHITE, 0.5),
        );
    }
}

/// One bubble with what stands around it: the name above, the quote in it, the
/// time under it, the picture beside it.
fn bubble_view(
    ui: &mut Ui,
    b: &Bubble,
    row_width: f32,
    focused: bool,
    events: &mut Vec<Mouse>,
) -> Rect {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let crowd_gutter = FACE + FACE_GAP;
    let max_bubble = (row_width * BUBBLE_SHARE).max(160.0);
    let text_width = max_bubble - 2.0 * BUBBLE_PAD_X;

    // Measure: the quote, the text.
    let quote = b.quote.as_ref().map(|(who, text)| {
        let who_g = one_line(&painter, NAME, who, text_width - 14.0);
        let text_g = one_line(&painter, QUOTE, text, text_width - 14.0);
        (who_g, text_g)
    });
    let body = text::layout_font(
        &painter,
        BODY.font(),
        1.4 * BODY.size,
        0.0,
        &b.text,
        Color32::PLACEHOLDER,
        text_width,
    );
    let quote_h = quote
        .as_ref()
        .map_or(0.0, |(w, t)| w.size().y + t.size().y + 8.0 + 8.0);
    let mut inner_w = body.size().x;
    if let Some((w, t)) = &quote {
        inner_w = inner_w.max(w.size().x.max(t.size().x) + 14.0);
    }
    let bubble_w = (inner_w + 2.0 * BUBBLE_PAD_X).max(54.0);
    let bubble_h = body.size().y + 2.0 * BUBBLE_PAD_Y + quote_h;
    let name_h = if b.name.is_some() { 18.0 } else { 0.0 };
    let meta_h = if b.meta.is_some() { 20.0 } else { 0.0 };
    let gap = if b.first { RUN_GAP } else { LINE_GAP };
    let total_h = gap + name_h + bubble_h + meta_h;

    let (row, _) = ui.allocate_exact_size(vec2(row_width, total_h), Sense::hover());
    let left_gutter = if b.crowd && !b.mine {
        crowd_gutter
    } else {
        0.0
    };
    let bubble_x = if b.mine {
        row.max.x - 4.0 - bubble_w
    } else {
        row.min.x + 4.0 + left_gutter
    };
    let bubble = Rect::from_min_size(
        pos2(bubble_x, row.min.y + gap + name_h),
        vec2(bubble_w, bubble_h),
    );
    let response = ui.interact(
        bubble,
        ui.make_persistent_id(("bubble", b.seq)),
        Sense::click(),
    );
    crate::ui::a11y::follow(&ctx, response.id, focused);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("{}, {}", b.sender, b.text),
        )
    });
    if response.clicked() {
        events.push(Mouse::Press(ThreadFocus::Message(b.index)));
    }
    let hovered = input::hovered(&ctx, &response);
    if hovered {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if !ui.is_rect_visible(row) {
        return row;
    }
    let t = ctx.focus_t((response.id, "bubble"), focused);

    // The name above the first bubble of another's run.
    if let Some(name) = &b.name {
        let g = one_line(&painter, NAME, name, bubble_w.max(120.0));
        painter.galley(
            pos2(bubble.min.x + 6.0, row.min.y + gap),
            g,
            fade(Color32::WHITE, 0.62),
        );
    }
    // The picture beside the last bubble of another's run.
    if let Some(face) = &b.face {
        let rect =
            Rect::from_min_size(pos2(row.min.x + 4.0, bubble.max.y - FACE), vec2(FACE, FACE));
        paint_avatar(&painter, rect, face.avatar, face.account, &face.handle);
    }
    // The bubble: the player's own is the light one, others' are dark glass.
    let (fill, ink) = if b.removed {
        (white(0.0), fade(Color32::WHITE, 0.55))
    } else if b.mine {
        (color::primary(), color::on_primary())
    } else {
        (white(0.11), Color32::WHITE)
    };
    let fill = if hovered && !b.removed {
        mix(fill, white(0.9), 0.06)
    } else {
        fill
    };
    let r = BUBBLE_RADIUS;
    let tight = BUBBLE_JOIN;
    // A run keeps its inner corners small on the sender's side.
    let (tl, tr, bl, br) = if b.mine {
        (
            r,
            if b.first { r } else { tight },
            r,
            if b.last { r } else { tight },
        )
    } else {
        (
            if b.first { r } else { tight },
            r,
            if b.last { r } else { tight },
            r,
        )
    };
    let corners = egui::CornerRadius {
        nw: tl as u8,
        ne: tr as u8,
        sw: bl as u8,
        se: br as u8,
    };
    painter.rect_filled(bubble, corners, fill);
    if b.removed {
        painter.rect_stroke(
            bubble,
            corners,
            egui::Stroke::new(1.0, white(0.22)),
            egui::StrokeKind::Inside,
        );
    }
    components::paint_focus_ring(&painter, bubble, r, t);
    let mut y = bubble.min.y + BUBBLE_PAD_Y;
    if let Some((who, text)) = quote {
        let quote_box = Rect::from_min_size(
            pos2(bubble.min.x + BUBBLE_PAD_X, y),
            vec2(bubble_w - 2.0 * BUBBLE_PAD_X, quote_h - 8.0),
        );
        painter.rect_filled(
            Rect::from_min_size(quote_box.min, vec2(3.0, quote_box.height())),
            corner(2.0),
            fade(ink, 0.5),
        );
        let (wh, th) = (who.size().y, text.size().y);
        painter.galley(
            pos2(quote_box.min.x + 12.0, quote_box.min.y),
            who,
            fade(ink, 0.75),
        );
        painter.galley(
            pos2(quote_box.min.x + 12.0, quote_box.min.y + wh + 2.0),
            text,
            fade(ink, 0.65),
        );
        let _ = th;
        y += quote_h;
    }
    painter.galley(pos2(bubble.min.x + BUBBLE_PAD_X, y), body, ink);

    let below = bubble.max.y;
    if let Some(meta) = &b.meta {
        let g = text::layout(&painter, META, meta, Color32::PLACEHOLDER, f32::INFINITY);
        let x = if b.mine {
            bubble.max.x - g.size().x - 4.0
        } else {
            bubble.min.x + 6.0
        };
        painter.galley(pos2(x, below + 4.0), g, fade(Color32::WHITE, 0.5));
    }
    row
}

// ---------------------------------------------------------------------------------------------
// Drawing: the composer
// ---------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn composer_view(
    ui: &mut Ui,
    bar: Rect,
    note_h: f32,
    composer: &Composer,
    focus: Option<ThreadFocus>,
    type_now: bool,
    language: Language,
    events: &mut Vec<Mouse>,
) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let inner = Rect::from_min_max(
        pos2(bar.min.x, bar.min.y + 8.0),
        pos2(bar.max.x, bar.max.y - 8.0),
    );
    match composer {
        Composer::Write { draft, note } => {
            if let Some(note) = note {
                let rect = Rect::from_min_size(inner.min, vec2(inner.width(), note_h - 4.0));
                let response = ui.interact(rect, ui.make_persistent_id("note"), Sense::click());
                let focused = focus == Some(ThreadFocus::Note);
                crate::ui::a11y::follow(&ctx, response.id, focused);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, note.text.as_str())
                });
                if response.clicked() {
                    events.push(Mouse::Press(ThreadFocus::Note));
                }
                let t = ctx.focus_t((response.id, "note"), focused);
                paint_capsule(&painter, rect, white(0.10));
                components::paint_focus_ring(&painter, rect, rect.height() * 0.5, t);
                let line = one_line(&painter, ty::ROW_INFO, &note.text, rect.width() - 64.0);
                painter.galley(
                    pos2(rect.min.x + 18.0, rect.center().y - line.size().y * 0.5),
                    line,
                    fade(Color32::WHITE, 0.85),
                );
                painter.text(
                    pos2(rect.max.x - 24.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    Icon::X.glyph(),
                    Icon::X.font(16.0, false),
                    fade(Color32::WHITE, 0.8),
                );
            }
            let pill =
                Rect::from_min_max(pos2(inner.min.x, inner.max.y - COMPOSER_HEIGHT), inner.max);
            paint_capsule(&painter, pill, white(0.07));
            // Send sits in the field's trailing end: quiet until there is something to send.
            let ready = anyps5_core::online::api::is_message_text(draft);
            let send = Rect::from_center_size(
                pos2(pill.max.x - 8.0 - COMPOSER_BUTTON * 0.5, pill.center().y),
                vec2(COMPOSER_BUTTON, COMPOSER_BUTTON),
            );
            let field = Rect::from_min_max(
                pos2(pill.min.x + 14.0, pill.min.y + 6.0),
                pos2(send.min.x - 8.0, pill.max.y - 6.0),
            );
            icon_button(
                ui,
                send,
                Icon::PAPER_PLANE_RIGHT,
                S::Send.text(language),
                focus == Some(ThreadFocus::Composer(Slot::Send)),
                ready,
                ThreadFocus::Composer(Slot::Send),
                events,
            );
            if draft.chars().count() > anyps5_core::online::api::MAX_MESSAGE_CHARS {
                text::paint(
                    &painter,
                    pos2(pill.max.x - 70.0, pill.min.y - 12.0),
                    Align2::RIGHT_CENTER,
                    ty::DETAIL,
                    S::TooLong.text(language),
                    fade(Color32::WHITE, 0.7),
                );
            }
            // The field.
            let focused = focus == Some(ThreadFocus::Composer(Slot::Field));
            let field_id = ui.make_persistent_id("compose-field");
            let ring = ctx.focus_t((field_id, "ring"), focused);
            components::paint_focus_ring(&painter, pill, pill.height() * 0.5, ring);
            let mut text = draft.clone();
            let mut field_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(field)
                    .layout(egui::Layout::left_to_right(Align::Center)),
            );
            let hint = if note.as_ref().is_some_and(|n| n.editing) {
                S::EditingMessage.text(language)
            } else {
                S::MessageField.text(language)
            };
            let edit = egui::TextEdit::singleline(&mut text)
                .id(field_id)
                .hint_text(hint)
                .desired_width(field.width())
                .frame(egui::Frame::NONE)
                .font(BODY.font())
                .margin(Margin::symmetric(8, 0));
            let response = field_ui.add(edit);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::TextInput, true, hint));
            if type_now {
                response.request_focus();
            }
            if response.changed() {
                events.push(Mouse::Typed(text.clone()));
            }
            if response.has_focus() && !focused {
                events.push(Mouse::Focus(ThreadFocus::Composer(Slot::Field)));
            }
            if response.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
                events.push(Mouse::Submit);
                response.request_focus();
            }
        }
        Composer::Paused { title, detail } => {
            let rect = inner;
            let response = ui.interact(rect, ui.make_persistent_id("bar"), Sense::click());
            let focused = focus == Some(ThreadFocus::Bar);
            crate::ui::a11y::follow(&ctx, response.id, focused);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::Role::Button, true, title.as_str())
            });
            if response.clicked() {
                events.push(Mouse::Press(ThreadFocus::Bar));
            }
            let t = ctx.focus_t((response.id, "bar"), focused);
            paint_capsule(&painter, rect, white(0.10));
            components::paint_focus_ring(&painter, rect, rect.height() * 0.5, t);
            painter.text(
                pos2(rect.min.x + 30.0, rect.center().y),
                Align2::CENTER_CENTER,
                Icon::LOCK.glyph(),
                Icon::LOCK.font(20.0, false),
                fade(Color32::WHITE, 0.8),
            );
            let line = one_line(&painter, ty::ROW_TITLE, title, rect.width() - 100.0);
            let sub = detail
                .as_ref()
                .map(|d| one_line(&painter, ty::DETAIL, d, rect.width() - 100.0));
            let h = line.size().y + sub.as_ref().map_or(0.0, |s| s.size().y + 2.0);
            let y = rect.center().y - h * 0.5;
            let lh = line.size().y;
            painter.galley(pos2(rect.min.x + 56.0, y), line, Color32::WHITE);
            if let Some(sub) = sub {
                painter.galley(
                    pos2(rect.min.x + 56.0, y + lh + 2.0),
                    sub,
                    fade(Color32::WHITE, 0.6),
                );
            }
        }
        Composer::Blocked { who } => {
            let rect = inner;
            let focused = focus == Some(ThreadFocus::Bar);
            let label = T::Unblock.text(language);
            let button_w = text::width(&painter, ty::BUTTON_MEDIUM, label) + 40.0;
            let button = Rect::from_center_size(
                pos2(rect.max.x - 12.0 - button_w * 0.5, rect.center().y),
                vec2(button_w, 38.0),
            );
            paint_capsule(&painter, rect, white(0.10));
            let note = S::BlockedNote.format(language, &[Arg::Str(who)]);
            let line = one_line(
                &painter,
                ty::ROW_INFO,
                &note,
                rect.width() - button_w - 60.0,
            );
            painter.galley(
                pos2(rect.min.x + 22.0, rect.center().y - line.size().y * 0.5),
                line,
                fade(Color32::WHITE, 0.8),
            );
            icon_text_button(ui, button, label, focused, ThreadFocus::Bar, events);
        }
        Composer::Request => {
            let rect =
                Rect::from_min_max(pos2(inner.min.x, inner.max.y - COMPOSER_HEIGHT), inner.max);
            let note = S::RequestNote.text(language);
            text::paint(
                &painter,
                pos2(inner.center().x, rect.min.y - 14.0),
                Align2::CENTER_CENTER,
                ty::DETAIL_LARGE,
                note,
                fade(Color32::WHITE, 0.55),
            );
            paint_capsule(&painter, rect, white(0.06));
            let labels = [
                (Answer::Accept, T::Accept.text(language), false),
                (Answer::Decline, T::Decline.text(language), false),
                (Answer::Block, T::Block.text(language), true),
            ];
            let widths: Vec<f32> = labels
                .iter()
                .map(|(_, l, _)| text::width(&painter, ty::BUTTON_MEDIUM, l) + 44.0)
                .collect();
            let total = widths.iter().sum::<f32>() + 12.0 * 2.0;
            let mut x = rect.center().x - total * 0.5;
            for ((answer, label, danger), w) in labels.iter().zip(&widths) {
                let button = Rect::from_min_size(pos2(x, rect.center().y - 19.0), vec2(*w, 38.0));
                let focused = focus == Some(ThreadFocus::Answer(*answer));
                let _ = danger;
                icon_text_button(
                    ui,
                    button,
                    label,
                    focused,
                    ThreadFocus::Answer(*answer),
                    events,
                );
                x += w + 12.0;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn icon_button(
    ui: &mut Ui,
    rect: Rect,
    icon: Icon,
    name: &str,
    focused: bool,
    primary: bool,
    target: ThreadFocus,
    events: &mut Vec<Mouse>,
) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let response = ui.interact(
        rect,
        ui.make_persistent_id(("button", name)),
        Sense::click(),
    );
    crate::ui::a11y::follow(&ctx, response.id, focused);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    if response.clicked() {
        events.push(Mouse::Press(target));
    }
    let hovered = input::hovered(&ctx, &response);
    if hovered {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let t = ctx.focus_t((response.id, "icon"), focused);
    let fill = if primary {
        color::primary()
    } else {
        white(if hovered || focused { 0.2 } else { 0.0 })
    };
    paint_capsule(&painter, rect, fill);
    components::paint_focus_ring(&painter, rect, rect.height() * 0.5, t);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        icon.glyph(),
        icon.font(20.0, false),
        if primary {
            color::on_primary()
        } else {
            fade(Color32::WHITE, 0.9)
        },
    );
}

fn icon_text_button(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    focused: bool,
    target: ThreadFocus,
    events: &mut Vec<Mouse>,
) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let response = ui.interact(
        rect,
        ui.make_persistent_id(("text-button", label)),
        Sense::click(),
    );
    crate::ui::a11y::follow(&ctx, response.id, focused);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if response.clicked() {
        events.push(Mouse::Press(target));
    }
    let hovered = input::hovered(&ctx, &response);
    if hovered {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let t = ctx.focus_t((response.id, "text-button"), focused);
    let fill = mix(
        white(if hovered { 0.22 } else { 0.14 }),
        color::primary(),
        t,
    );
    paint_capsule(&painter, rect, fill);
    components::paint_focus_ring(&painter, rect, rect.height() * 0.5, t);
    text::paint(
        &painter,
        rect.center(),
        Align2::CENTER_CENTER,
        ty::BUTTON_MEDIUM,
        label,
        mix(Color32::WHITE, color::on_primary(), t),
    );
}

#[cfg(test)]
mod tests {
    use anyps5_core::online::api::{
        Conversation, Conversations, Friend, Messages, Presence, Quote, ReadMark,
    };
    use anyps5_core::online::{Cloud, Fetch, Online, SignedIn, Status};
    use egui::Color32;

    use super::*;
    use crate::screens::online::Page;
    use crate::ui::i18n::Language;

    fn account(id: u64, handle: &str) -> Account {
        Account {
            id: AccountId(id),
            handle: handle.into(),
        }
    }

    fn online() -> Online {
        let mut online = Online::default();
        online.status = Status::Running;
        online.cloud = Some(Cloud::Online);
        online.account = Some(SignedIn {
            id: AccountId(1),
            handle: "Mira".into(),
            email: "mira@example.org".into(),
            appear_offline: false,
        });
        online.friends.friends = ["Lumen", "Juno", "Kit"]
            .iter()
            .enumerate()
            .map(|(i, handle)| Friend {
                account: account(i as u64 + 2, handle),
                since: 0,
                presence: Presence {
                    online: true,
                    activity: None,
                    revision: 0,
                },
            })
            .collect();
        online
    }

    fn data(online: &Online) -> OnlineData<'_> {
        OnlineData {
            online,
            cloud: Some("https://cloud.example.org"),
            custom_cloud: false,
            official_cloud: true,
            relay_only: false,
            language: Language::English,
            accent: Color32::WHITE,
            now: NOW,
            utc_offset: Some(0),
            game_running: false,
        }
    }

    /// 12:00 on the 22nd.
    const NOW: i64 = 1_792_670_400;
    const DAY: u64 = 86_400;

    fn chat(id: &str, kind: ConversationKind, with: &[(u64, &str)], last: u64) -> Conversation {
        let mut members = vec![account(1, "Mira")];
        members.extend(with.iter().map(|(i, h)| account(*i, h)));
        Conversation {
            id: id.into(),
            kind,
            name: None,
            members,
            last,
            read: last,
            marks: Vec::new(),
            updated: last,
            request: false,
            party: None,
            unread: 0,
            muted: false,
            pinned: false,
            rev: 0,
        }
    }

    fn said(id: &str, seq: u64, who: u64, handle: &str, text: &str, created: u64) -> Message {
        Message {
            conversation: id.into(),
            seq,
            sender: account(who, handle),
            text: text.into(),
            created,
            reply: None,
            edited: None,
            deleted: false,
            reactions: Vec::new(),
            rev: 0,
        }
    }

    fn put(online: &mut Online, conversations: Vec<Conversation>, requests: Vec<Conversation>) {
        online.social.set_conversations(Conversations {
            conversations,
            requests,
        });
    }

    fn load(online: &mut Online, id: &str, messages: Vec<Message>) {
        let last = messages.last().map_or(0, |m| m.seq);
        online.social.ingest(
            id,
            Fetch::Newest,
            Messages {
                messages,
                last,
                rev: 0,
            },
        );
    }

    fn bubbles(thread: &Thread) -> Vec<&Bubble> {
        thread
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Bubble(b) => Some(b.as_ref()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_list_has_one_entry_per_conversation_with_the_newest_message_in_one_line() {
        let mut online = online();
        let mut direct = chat("d", ConversationKind::Direct, &[(2, "Lumen")], 3);
        direct.updated = 300;
        direct.unread = 2;
        direct.read = 1;
        let mut group = chat(
            "g",
            ConversationKind::Group,
            &[(2, "Lumen"), (3, "Juno")],
            2,
        );
        group.name = Some("Raid night".into());
        group.pinned = true;
        group.updated = 100;
        let mut quiet = chat("q", ConversationKind::Direct, &[(4, "Kit")], 1);
        quiet.muted = true;
        quiet.updated = 200;
        put(&mut online, vec![direct, group, quiet], vec![]);
        load(
            &mut online,
            "d",
            vec![said(
                "d",
                3,
                2,
                "Lumen",
                "See you\nat  eight",
                NOW as u64 - 60,
            )],
        );
        load(
            &mut online,
            "g",
            vec![said("g", 2, 3, "Juno", "Bring snacks", NOW as u64 - DAY)],
        );
        load(
            &mut online,
            "q",
            vec![said("q", 1, 1, "Mira", "ok", NOW as u64 - 3 * DAY)],
        );
        let list = entries(&data(&online), &Local::default());
        assert_eq!(
            list.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["g", "d", "q"],
            "pinned first, then the newest"
        );
        let by = |id: &str| list.iter().find(|e| e.id == id).unwrap();
        assert_eq!(
            by("d").preview,
            "See you at eight",
            "a direct chat shows the text"
        );
        assert_eq!(by("d").when, "11:59");
        assert_eq!(by("d").unread, 2);
        assert_eq!(
            by("g").preview,
            "Juno: Bring snacks",
            "a group names the sender"
        );
        assert_eq!(by("g").when, "Yesterday");
        assert_eq!(by("g").title, "Raid night");
        assert_eq!(by("g").faces.len(), 2);
        assert!(by("g").pinned);
        assert_eq!(by("q").preview, "You: ok", "your own words say so");
        assert_eq!(by("q").when, "Oct 19");
        assert!(by("q").muted);
    }

    #[test]
    fn requests_come_first_and_say_who_wants_to_write() {
        let mut online = online();
        let mut request = chat("r", ConversationKind::Direct, &[(9, "Nyx")], 2);
        request.request = true;
        put(
            &mut online,
            vec![chat("d", ConversationKind::Direct, &[(2, "Lumen")], 1)],
            vec![request],
        );
        let list = entries(&data(&online), &Local::default());
        assert_eq!(list[0].id, "r");
        assert!(list[0].request);
        assert!(!list[1].request);
    }

    #[test]
    fn a_conversation_without_a_preview_waits_for_one_and_typing_replaces_it() {
        let mut online = online();
        put(
            &mut online,
            vec![chat("d", ConversationKind::Direct, &[(2, "Lumen")], 5)],
            vec![],
        );
        assert_eq!(entries(&data(&online), &Local::default())[0].preview, "");
        assert_eq!(online.social.wants_previews(&["d".into()]), ["d"]);
        assert!(
            online.social.wants_previews(&["d".into()]).is_empty(),
            "asked once"
        );
        online
            .social
            .set_preview(said("d", 5, 2, "Lumen", "Hello there", NOW as u64 - 10));
        assert_eq!(
            entries(&data(&online), &Local::default())[0].preview,
            "Hello there"
        );
        online
            .social
            .apply_typing("d", AccountId(2), true, NOW as u64);
        let entry = &entries(&data(&online), &Local::default())[0];
        assert_eq!(entry.preview, "Lumen is typing…");
        assert!(entry.typing);
    }

    #[test]
    fn the_thread_groups_runs_and_separates_days() {
        let mut online = online();
        put(
            &mut online,
            vec![chat(
                "g",
                ConversationKind::Group,
                &[(2, "Lumen"), (3, "Juno")],
                6,
            )],
            vec![],
        );
        let n = NOW as u64;
        load(
            &mut online,
            "g",
            vec![
                said("g", 1, 2, "Lumen", "yesterday", n - DAY),
                said("g", 2, 2, "Lumen", "one", n - 600),
                said("g", 3, 2, "Lumen", "two", n - 590),
                said("g", 4, 3, "Juno", "three", n - 580),
                said("g", 5, 1, "Mira", "mine", n - 570),
                said("g", 6, 1, "Mira", "also mine", n - 560),
            ],
        );
        let t = thread(&data(&online), &Local::default(), "g", 40).unwrap();
        let days: Vec<&str> = t
            .items
            .iter()
            .filter_map(|i| match i {
                Item::Day(d) => Some(d.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(days, ["Yesterday", "Today"]);
        let b = bubbles(&t);
        assert_eq!(b.len(), 6);
        assert_eq!(t.messages, 6);
        // Lumen's two lines after the day change form a run: the name once above it, the picture
        // and the time under its last bubble.
        assert!(b[1].first && !b[1].last && b[1].name.as_deref() == Some("Lumen"));
        assert!(!b[2].first && b[2].last && b[2].name.is_none() && b[2].face.is_some());
        assert!(b[2].meta.as_deref().is_some_and(|m| m.starts_with("11:50")));
        assert!(b[1].meta.is_none());
        // Juno's single message is a run of one; the player's own carry no name or picture.
        assert!(b[3].first && b[3].last && b[3].name.as_deref() == Some("Juno"));
        assert!(b[4].mine && b[4].name.is_none() && b[4].face.is_none());
        assert!(!b[4].last && b[5].last);
        // A run is broken by a pause of more than five minutes.
        assert!(b[0].first && b[0].last, "the day before stands alone");
    }

    #[test]
    fn a_direct_thread_has_no_names_and_no_pictures() {
        let mut online = online();
        put(
            &mut online,
            vec![chat("d", ConversationKind::Direct, &[(2, "Lumen")], 2)],
            vec![],
        );
        let n = NOW as u64;
        load(
            &mut online,
            "d",
            vec![
                said("d", 1, 2, "Lumen", "hi", n - 60),
                said("d", 2, 1, "Mira", "hey", n - 30),
            ],
        );
        let t = thread(&data(&online), &Local::default(), "d", 40).unwrap();
        for b in bubbles(&t) {
            assert!(b.name.is_none() && b.face.is_none() && !b.crowd);
        }
    }

    #[test]
    fn a_bubble_says_what_became_of_its_message() {
        let mut online = online();
        let mut c = chat("d", ConversationKind::Direct, &[(2, "Lumen")], 4);
        c.marks = vec![ReadMark {
            account: AccountId(2),
            seq: 0,
            delivered: 4,
        }];
        put(&mut online, vec![c], vec![]);
        let n = NOW as u64;
        let mut m = vec![
            said("d", 1, 2, "Lumen", "first", n - 4000),
            said("d", 2, 1, "Mira", "taken back", n - 3000),
            said("d", 3, 2, "Lumen", "answer", n - 2000),
            said("d", 4, 1, "Mira", "last one", n - 1000),
        ];
        m[0].edited = Some(n - 3900);
        m[1].deleted = true;
        m[1].text.clear();
        m[2].reply = Some(Quote {
            seq: 2,
            sender: account(1, "Mira"),
            text: String::new(),
            deleted: true,
        });
        load(&mut online, "d", m);
        let t = thread(&data(&online), &Local::default(), "d", 40).unwrap();
        let b = bubbles(&t);
        assert_eq!(b[0].meta.as_deref(), Some("10:53 · Edited"));
        assert!(b[1].removed);
        assert_eq!(b[1].text, "Message removed");
        assert_eq!(
            b[2].quote,
            Some(("You".to_owned(), "Message removed".to_owned()))
        );
        assert_eq!(b[3].meta.as_deref(), Some("11:43 · Delivered"));
        // Read says Seen instead.
        online.social.apply_read("d", AccountId(2), 4, AccountId(1));
        let t = thread(&data(&online), &Local::default(), "d", 40).unwrap();
        assert_eq!(bubbles(&t)[3].meta.as_deref(), Some("11:43 · Seen"));
    }

    #[test]
    fn the_thread_names_who_types_and_what_it_waits_for() {
        let mut online = online();
        put(
            &mut online,
            vec![chat("d", ConversationKind::Direct, &[(2, "Lumen")], 3)],
            vec![],
        );
        let t = thread(&data(&online), &Local::default(), "d", 40).unwrap();
        assert!(
            matches!(&t.items[..], [Item::State(Icon::ARROWS_CLOCKWISE, text, None)] if text == "Loading…"),
            "{:?}",
            t.items
        );
        online.social.fetch_failed(Some("d"));
        let t = thread(&data(&online), &Local::default(), "d", 40).unwrap();
        assert!(matches!(&t.items[..], [Item::State(Icon::WARNING, ..)]));
        load(
            &mut online,
            "d",
            vec![said("d", 3, 2, "Lumen", "hi", NOW as u64 - 5)],
        );
        online
            .social
            .apply_typing("d", AccountId(2), true, NOW as u64);
        let t = thread(&data(&online), &Local::default(), "d", 40).unwrap();
        assert!(
            matches!(t.items.last(), Some(Item::Typing(f, line)) if f.handle == "Lumen" && line == "Lumen is typing…")
        );
    }

    #[test]
    fn earlier_messages_are_offered_while_more_are_held() {
        let mut online = online();
        put(
            &mut online,
            vec![chat("d", ConversationKind::Direct, &[(2, "Lumen")], 60)],
            vec![],
        );
        let n = NOW as u64;
        load(
            &mut online,
            "d",
            (1..=60)
                .map(|k| said("d", k, 2, "Lumen", "x", n - 5000 + k * 10))
                .collect(),
        );
        let t = thread(&data(&online), &Local::default(), "d", 40).unwrap();
        assert_eq!(t.messages, 40);
        assert!(t.earlier);
        assert_eq!(t.items.first(), Some(&Item::Earlier));
        let all = thread(&data(&online), &Local::default(), "d", 80).unwrap();
        assert_eq!(all.messages, 60);
    }

    #[test]
    fn the_composer_follows_what_the_conversation_allows() {
        let mut online = online();
        let mut request = chat("r", ConversationKind::Direct, &[(9, "Nyx")], 1);
        request.request = true;
        put(
            &mut online,
            vec![chat("d", ConversationKind::Direct, &[(2, "Lumen")], 1)],
            vec![request],
        );
        assert_eq!(composer(&data(&online), "r"), Some(Composer::Request));
        assert_eq!(
            composer(&data(&online), "d"),
            Some(Composer::Write {
                draft: String::new(),
                note: None
            })
        );
        load(
            &mut online,
            "d",
            vec![said("d", 1, 2, "Lumen", "Message one", NOW as u64 - 5)],
        );
        online.social.start_reply("d", 1);
        match composer(&data(&online), "d") {
            Some(Composer::Write { note: Some(n), .. }) => {
                assert_eq!(n.text, "Replying to Lumen: Message one");
                assert!(!n.editing);
            }
            other => panic!("{other:?}"),
        }
        online.friends.blocked = vec![account(2, "Lumen")];
        assert_eq!(
            composer(&data(&online), "d"),
            Some(Composer::Blocked {
                who: "Lumen".into()
            })
        );
        assert_eq!(composer(&data(&online), "nope"), None);
    }

    fn shape(messages: usize, earlier: bool, note: bool, kind: Kind) -> Shape {
        Shape {
            messages,
            earlier,
            note,
            kind,
        }
    }

    #[test]
    fn up_and_down_walk_the_messages_between_the_composer_and_the_start() {
        use ThreadFocus as F;
        let s = shape(3, true, false, Kind::Write);
        let go = |focus, command| match step(focus, command, s) {
            Step::Moved(to, _) => to,
            other => panic!("{other:?}"),
        };
        // From the field up: the newest message, then older ones, then Show Earlier.
        assert_eq!(go(F::Composer(Slot::Field), Command::Up), F::Message(2));
        assert_eq!(go(F::Message(2), Command::Up), F::Message(1));
        assert_eq!(go(F::Message(0), Command::Up), F::Earlier);
        assert_eq!(go(F::Earlier, Command::Up), F::Earlier);
        assert_eq!(go(F::Earlier, Command::Down), F::Message(0));
        assert_eq!(go(F::Message(2), Command::Down), F::Composer(Slot::Field));
        assert_eq!(
            go(F::Composer(Slot::Field), Command::Down),
            F::Composer(Slot::Field),
            "nothing below the field"
        );
        // The buttons of the composer are walked sideways.
        assert_eq!(
            go(F::Composer(Slot::Field), Command::Right),
            F::Composer(Slot::Send)
        );
        assert_eq!(
            go(F::Composer(Slot::Send), Command::Left),
            F::Composer(Slot::Field)
        );
        assert_eq!(
            go(F::Composer(Slot::Field), Command::Left),
            F::Composer(Slot::Field)
        );
        assert_eq!(
            step(F::Message(1), Command::Select, s),
            Step::Press(F::Message(1))
        );
        assert_eq!(step(F::Message(1), Command::Back, s), Step::Back);
        assert_eq!(step(F::Message(1), Command::Options, s), Step::Options);
    }

    #[test]
    fn a_note_above_the_field_sits_between_the_messages_and_it() {
        use ThreadFocus as F;
        let s = shape(2, false, true, Kind::Write);
        let go = |focus, command| match step(focus, command, s) {
            Step::Moved(to, _) => to,
            other => panic!("{other:?}"),
        };
        assert_eq!(go(F::Composer(Slot::Field), Command::Up), F::Note);
        assert_eq!(go(F::Note, Command::Up), F::Message(1));
        assert_eq!(go(F::Message(1), Command::Down), F::Note);
        assert_eq!(go(F::Note, Command::Down), F::Composer(Slot::Field));
    }

    #[test]
    fn a_request_is_answered_with_three_buttons_and_an_empty_thread_has_only_the_field() {
        use ThreadFocus as F;
        let s = shape(2, false, false, Kind::Request);
        let go = |focus, command| match step(focus, command, s) {
            Step::Moved(to, _) => to,
            other => panic!("{other:?}"),
        };
        assert_eq!(go(F::Message(1), Command::Down), F::Answer(Answer::Accept));
        assert_eq!(
            go(F::Answer(Answer::Accept), Command::Right),
            F::Answer(Answer::Decline)
        );
        assert_eq!(
            go(F::Answer(Answer::Decline), Command::Right),
            F::Answer(Answer::Block)
        );
        assert_eq!(go(F::Answer(Answer::Block), Command::Up), F::Message(1));
        // A focus that is gone lands on what is there.
        let empty = shape(0, false, false, Kind::Write);
        assert_eq!(
            step(F::Message(4), Command::Select, empty),
            Step::Press(F::Composer(Slot::Field))
        );
        assert_eq!(
            step(F::Composer(Slot::Field), Command::Up, empty),
            Step::Moved(F::Composer(Slot::Field), false)
        );
    }

    #[test]
    fn the_list_is_walked_by_up_and_down_and_left_is_the_rails() {
        assert_eq!(list_step(0, Command::Down, 3), ListStep::Moved(1, true));
        assert_eq!(list_step(2, Command::Down, 3), ListStep::Moved(2, false));
        assert_eq!(list_step(0, Command::Up, 3), ListStep::Moved(0, false));
        assert_eq!(list_step(1, Command::Select, 3), ListStep::Press(1));
        assert_eq!(list_step(1, Command::Back, 3), ListStep::Back);
        assert_eq!(
            list_step(9, Command::Select, 3),
            ListStep::Press(2),
            "clamped"
        );
    }

    #[test]
    fn the_list_focus_follows_its_conversation_when_the_order_changes() {
        let mut online = online();
        put(
            &mut online,
            vec![
                chat("a", ConversationKind::Direct, &[(2, "Lumen")], 1),
                chat("b", ConversationKind::Direct, &[(3, "Juno")], 1),
            ],
            vec![],
        );
        let mut focus = ListFocus::default();
        let list = entries(&data(&online), &Local::default());
        focus.set(&list, 1);
        let id = list[1].id.clone();
        // A message in the other one moves it to the top: the focus stays with its own.
        online.social.note_last(&list[0].id, 5);
        let list = entries(&data(&online), &Local::default());
        assert_eq!(list[focus.index(&list)].id, id);
        // Gone: the first.
        online.social.remove(&id);
        let list = entries(&data(&online), &Local::default());
        assert_eq!(focus.index(&list), 0);
    }

    #[test]
    fn only_the_players_own_messages_can_be_edited_and_only_for_fifteen_minutes() {
        let me = AccountId(1);
        let n = NOW as u64;
        let own = said("c", 1, 1, "Mira", "x", n - 100);
        assert!(editable(&own, me, NOW));
        assert!(!editable(&own, me, NOW + 16 * 60));
        assert!(!editable(&said("c", 2, 2, "Lumen", "x", n - 100), me, NOW));
        let mut gone = own.clone();
        gone.deleted = true;
        assert!(!editable(&gone, me, NOW));
    }

    #[test]
    fn a_request_shows_its_first_messages_read_only() {
        let mut online = online();
        let mut request = chat("r", ConversationKind::Direct, &[(9, "Nyx")], 2);
        request.request = true;
        put(&mut online, vec![], vec![request]);
        let n = NOW as u64;
        load(
            &mut online,
            "r",
            vec![
                said("r", 1, 9, "Nyx", "Hi!", n - 100),
                said("r", 2, 9, "Nyx", "Want to team up?", n - 90),
            ],
        );
        let t = thread(&data(&online), &Local::default(), "r", 40).unwrap();
        assert_eq!(bubbles(&t).len(), 2);
        assert!(!t.earlier);
    }

    #[test]
    fn message_text_is_plain_and_pressing_earlier_asks_for_more() {
        let mut online = online();
        put(
            &mut online,
            vec![chat("d", ConversationKind::Direct, &[(2, "Lumen")], 60)],
            vec![],
        );
        let n = NOW as u64;
        let mut m: Vec<Message> = (1..=60)
            .map(|k| said("d", k, 2, "Lumen", "x", n - 5000 + k * 10))
            .collect();
        m[59].text = "click https://evil.example/\u{202E}gpj.exe <b>now</b>".into();
        load(&mut online, "d", m);
        let t = thread(&data(&online), &Local::default(), "d", 40).unwrap();
        assert_eq!(
            bubbles(&t).last().unwrap().text,
            "click https://evil.example/gpj.exe <b>now</b>",
            "no direction override, no markup interpreted"
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("d".into()), &data(&online));
        let mut actions = Vec::new();
        let bar = composer(&data(&online), "d").unwrap();
        panel.chat_press(
            ThreadFocus::Earlier,
            "d",
            &bar,
            false,
            &data(&online),
            &mut actions,
        );
        assert_eq!(panel.shown, super::super::social::SHOWN * 2);
        // 80 are asked for and 60 are held: the rest is fetched.
        assert_eq!(
            actions,
            [crate::screens::online::OnlineAction::LoadEarlier(
                "d".into()
            )]
        );
    }
}
