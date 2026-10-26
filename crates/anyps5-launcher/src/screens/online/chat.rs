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
use crate::ui::components::{self, ConsoleButton, ControlSize, corner, paint_capsule};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;
use crate::ui::input::{self, Command};
use crate::ui::text;
use crate::ui::theme::{TextStyle, color, fade, radius, size, space, ty};

/// The widest the thread is drawn: a line of a message stays readable on a wide screen. The
/// header, the bubbles and the composer stand on the two edges of this column.
const COLUMN: f32 = 940.0;
/// A bubble takes at most this share of the column.
const BUBBLE_SHARE: f32 = 0.66;
const BUBBLE_PAD_X: f32 = space::L;
const BUBBLE_PAD_Y: f32 = space::S;
const BUBBLE_MIN: f32 = 56.0;
const BUBBLE_RADIUS: f32 = radius::CARD;
/// The corner on the sender's side is the small one: it joins the bubbles of a group and ends it.
const BUBBLE_JOIN: f32 = radius::KEYCAP;
const FACE: f32 = 32.0;
const FACE_GAP: f32 = space::S;
/// Between two groups, and between two messages of one group.
const RUN_GAP: f32 = space::BUBBLE_BETWEEN;
const LINE_GAP: f32 = space::BUBBLE_IN_GROUP;
/// Messages written within this many seconds of each other by one sender form a group.
const RUN_SECONDS: u64 = 5 * 60;
/// Leading of the words of a message.
const BODY_LEADING: f32 = 1.45;
/// The name over a group, the time under it.
const NAME_HEIGHT: f32 = 20.0;
const META_HEIGHT: f32 = 20.0;
/// What a quote in a bubble keeps: its bar and the room between the bar and its words.
const QUOTE_BAR: f32 = 2.0;
const QUOTE_INSET: f32 = space::M;
/// The three dots of somebody typing: their size and the row they stand in.
const DOT: f32 = 3.0;
/// Items fade out over this length before they pass under the header (see `top_fade`).
const FADE_START: f32 = 56.0;
const FADE_LENGTH: f32 = 36.0;
/// The count badge: its height and its two widths.
const BADGE_ONE: f32 = 22.0;
const BADGE_MANY: f32 = 30.0;
const TYPING_HEIGHT: f32 = 24.0;

const BODY: TextStyle = ty::BODY;
const NAME: TextStyle = ty::BADGE;
const META: TextStyle = ty::DETAIL;
const QUOTE_NAME: TextStyle = ty::BADGE;
const QUOTE: TextStyle = ty::NOTE;
const DAY: TextStyle = ty::BADGE;

/// A row of the list of conversations: no box at rest, the avatar on the edge the title starts on.
const ENTRY_HEIGHT: f32 = 72.0;
const ENTRY_FACE: f32 = 48.0;
/// The size of the marks (pinned, muted) and of the count.
const MARK: f32 = 14.0;
const BADGE_HEIGHT: f32 = 22.0;

/// The size of the composer bar (a Medium control) and its buttons; the note over it (a reply, an
/// edit).
const COMPOSER_HEIGHT: f32 = 44.0;
const COMPOSER_BUTTON: f32 = 44.0;
const SEND_BUTTON: f32 = ControlSize::Small.height();
const NOTE_BAR: f32 = 36.0;
/// The room of the two answers (Accept, Decline) or of Block in the request bar.
const ANSWER_SLOT: f32 = 260.0;
/// The header of the thread.
const HEAD_HEIGHT: f32 = 72.0;

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
    /// The conversation holds no message at all (not: its newest one is not fetched yet).
    pub empty: bool,
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
            empty: c.last == 0,
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
    /// The name above the first bubble of a group, in a crowd.
    pub name: Option<String>,
    /// The picture beside the first bubble of a group, in a crowd.
    pub face: Option<Face>,
    pub text: String,
    pub removed: bool,
    /// Who and what the message answers.
    pub quote: Option<(String, String)>,
    /// Under the last bubble of a group: the time, Edited, Delivered or Seen.
    pub meta: Option<String>,
    /// What a screen reader says of the bubble: who, the words, the time and what became of it.
    pub reader: String,
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
    Typing(Option<Face>, String),
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
        // One sender's messages within five minutes form a group, an answer among them: its quote
        // is inside its own bubble, so the bubbles can stand close.
        let same_group = |earlier: &Message, later: &Message| {
            earlier.sender.id == later.sender.id
                && earlier.deleted == later.deleted
                && !day_changed(earlier, later, offset)
                && later.created.abs_diff(earlier.created) <= RUN_SECONDS
        };
        let first = index == 0 || !same_group(&messages[index - 1], message);
        let last = index + 1 == messages.len() || !same_group(message, &messages[index + 1]);
        let crowd = !direct;
        let name = (crowd && !mine && first).then(|| message.sender.handle.clone());
        let face = (crowd && !mine && first).then(|| {
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
        // The time and what became of the message, said once under a group and for every
        // message to the screen reader.
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
        let meta = (last && !message.deleted).then(|| parts.join(" · "));
        let text = if message.deleted {
            S::Removed.text(language).to_owned()
        } else {
            plain(&message.text)
        };
        let sender_name = if mine {
            T::You.text(language).to_owned()
        } else {
            message.sender.handle.clone()
        };
        let answers = quote.as_ref().map_or(String::new(), |(who, line)| {
            format!(
                "{}, ",
                S::InReplyTo.format(language, &[Arg::Str(who), Arg::Str(line)])
            )
        });
        let reader = format!("{sender_name}, {answers}{text}, {}", parts.join(", "));
        items.push(Item::Bubble(Box::new(Bubble {
            index,
            seq: message.seq,
            mine,
            sender: message.sender.handle.clone(),
            name,
            face,
            text,
            removed: message.deleted,
            quote,
            meta,
            reader,
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
        let crowd = conversation.kind != ConversationKind::Direct;
        items.push(Item::Typing(crowd.then(|| face(data, local, first)), line));
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
    /// The button in the header: the conversation's options.
    Header,
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
        F::Header => F::Header,
        F::Note if !shape.note => shape.entry(),
        F::Composer(_) if shape.kind != Kind::Write => shape.entry(),
        F::Answer(_) if shape.kind != Kind::Request => shape.entry(),
        F::Bar if shape.kind != Kind::Bar => shape.entry(),
        other => other,
    };
    match command {
        Command::Up => match focus {
            F::Header => Step::Moved(focus, false),
            F::Earlier => Step::Moved(F::Header, true),
            F::Message(0) => Step::Moved(if shape.earlier { F::Earlier } else { F::Header }, true),
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
            F::Header => {
                let to = shape.top().unwrap_or_else(|| shape.bottom());
                Step::Moved(to, true)
            }
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
            F::Message(i) => moved(F::Message(i.saturating_sub(5))),
            F::Composer(Slot::Send) => moved(F::Composer(Slot::Field)),
            F::Answer(Answer::Block) => moved(F::Answer(Answer::Decline)),
            F::Answer(Answer::Decline) => moved(F::Answer(Answer::Accept)),
            _ => Step::Moved(focus, false),
        },
        Command::Right => match focus {
            F::Message(i) if i + 1 < shape.messages => {
                moved(F::Message((i + 5).min(shape.messages - 1)))
            }
            F::Message(_) => moved(shape.bottom()),
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
    /// Puts the focus on a message of the thread of the conversation (previews).
    pub fn set_chat_focus(&mut self, conversation: &str, index: usize) {
        self.chat.opened = Some(conversation.to_owned());
        self.chat.focus = ThreadFocus::Message(index);
    }

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
            super::Page::Confirm(_, from) => matches!(
                **from,
                super::Page::Conversation(_)
                    | super::Page::MessageRequest(_)
                    | super::Page::Message(..)
                    | super::Page::ChatOptions(_)
            ),
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
            ThreadFocus::Header => self.go(Page::ChatOptions(id.to_owned()), data),
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
            ThreadFocus::Composer(Slot::Send) => {
                // Send with nothing written does nothing.
                if matches!(composer, Composer::Write { draft, .. } if anyps5_core::online::api::is_message_text(draft))
                {
                    actions.push(A::SendMessage(id.to_owned()));
                }
            }
            ThreadFocus::Answer(Answer::Accept) => actions.push(A::AcceptRequest(id.to_owned())),
            ThreadFocus::Answer(Answer::Decline) => {
                actions.push(A::LeaveConversation(id.to_owned()));
                self.go(Page::Messages, data);
            }
            ThreadFocus::Answer(Answer::Block) => {
                self.ask(super::Ask::BlockRequest(id.to_owned()), data);
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
            painter.circle_filled(rect.center(), rect.width() * 0.5, color::surface_hover());
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                Icon::USERS.glyph(),
                Icon::USERS.font(rect.width() * 0.46, false),
                color::text_detail(),
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

/// Items fade out over the 36 dp before they pass under the header, so a bubble the scroll cuts
/// does not leave a stray line behind.
fn top_fade(ui: &Ui, top: f32) -> f32 {
    ((ui.cursor().min.y - top + FADE_START) / FADE_LENGTH).clamp(0.0, 1.0)
}

/// The section label of the list ("Message Requests", "Conversations"): the caption of every
/// other list of the area, on the edge the avatars start on.
fn caption(ui: &mut Ui, text: &str) {
    let painter = ui.painter().clone();
    let galley = text::layout(
        &painter,
        ty::SECTION_CAPTION,
        text,
        Color32::PLACEHOLDER,
        f32::INFINITY,
    );
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), galley.size().y + space::L),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Heading, true, text));
    painter.galley(
        pos2(
            rect.min.x + size::ROW_PADDING_X,
            rect.max.y - galley.size().y,
        ),
        galley,
        color::text_caption(),
    );
}

/// What a screen reader says of a row: everything the eye reads in it.
fn entry_name(entry: &Entry, language: Language) -> String {
    let mut parts = vec![entry.title.clone()];
    if entry.request {
        parts.push(S::RequestTag.text(language).to_owned());
    }
    if entry.pinned {
        parts.push(S::Pinned.text(language).to_owned());
    }
    if entry.muted {
        parts.push(S::Muted.text(language).to_owned());
    }
    let preview = entry_preview(entry, language);
    if !preview.is_empty() {
        parts.push(preview);
    }
    if !entry.when.is_empty() {
        parts.push(entry.when.clone());
    }
    if entry.unread > 0 && !entry.muted && !entry.request {
        parts.push(S::NewCount.format(language, &[Arg::Int(entry.unread as i64)]));
    }
    parts.join(", ")
}

/// The line under a conversation's name: its newest message, else what is to say instead.
fn entry_preview(entry: &Entry, language: Language) -> String {
    if !entry.preview.is_empty() {
        entry.preview.clone()
    } else if entry.request {
        S::WantsToMessage.text(language).to_owned()
    } else if entry.empty {
        S::NoPreview.text(language).to_owned()
    } else {
        // The newest message is on its way: the line stays empty rather than say something wrong.
        String::new()
    }
}

/// One conversation: the picture, the name with its tag, the newest message in one line, the time
/// and the marks at the trailing end of the first line and the count at the end of the second. No
/// box at rest; the focus and the pointer lift the row.
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
        egui::WidgetInfo::labeled(egui::Role::Button, true, entry_name(entry, language))
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
        Color32::TRANSPARENT,
        hover_t,
        t,
        0.0,
        size::FOCUS_RING,
    );
    let face = Rect::from_min_size(
        pos2(
            rect.min.x + size::ROW_PADDING_X,
            rect.center().y - ENTRY_FACE * 0.5,
        ),
        vec2(ENTRY_FACE, ENTRY_FACE),
    );
    paint_faces(&painter, face, &entry.faces, entry.kind);
    let text_x = face.max.x + space::L;
    let right = rect.max.x - size::ROW_PADDING_X;
    let unread = entry.unread > 0 && !entry.muted;

    // The first line: the time at the end, the marks before it, the name in the room left.
    let title_style = if unread {
        ty::INFO_VALUE
    } else {
        ty::ROW_TITLE
    };
    let first_h = title_style.line_height();
    let preview_style = ty::SENTENCE;
    let second_h = preview_style.line_height();
    let top = rect.center().y - (first_h + space::XS + second_h) * 0.5;
    let first_y = top + first_h * 0.5;
    let mut x = right;
    if !entry.when.is_empty() {
        let when = text::layout(
            &painter,
            META,
            &entry.when,
            Color32::PLACEHOLDER,
            f32::INFINITY,
        );
        x -= when.size().x;
        let ink = if unread {
            color::text_label()
        } else {
            color::text_note()
        };
        painter.galley(pos2(x, first_y - when.size().y * 0.5), when, ink);
        x -= space::S;
    }
    for (on, icon) in [
        (entry.muted, Icon::BELL_SLASH),
        (entry.pinned, Icon::PUSH_PIN),
    ] {
        if on {
            x -= MARK;
            painter.text(
                pos2(x + MARK * 0.5, first_y),
                Align2::CENTER_CENTER,
                icon.glyph(),
                icon.font(MARK, false),
                color::text_note(),
            );
            x -= space::XS;
        }
    }
    let tag = entry.request.then(|| {
        text::layout(
            &painter,
            ty::MICRO,
            S::RequestTag.text(language),
            Color32::PLACEHOLDER,
            f32::INFINITY,
        )
    });
    // The Request tag keeps its room: a long name is cut, never the tag.
    let tag_room = tag.as_ref().map_or(0.0, |t| t.size().x + 3.0 * space::S);
    let title_room = (x - text_x - space::S - tag_room).max(40.0);
    let title = one_line(&painter, title_style, &entry.title, title_room);
    let mut cut = title.elided;
    let title_w = title.size().x;
    painter.galley(
        pos2(text_x, first_y - title.size().y * 0.5),
        title,
        color::text(),
    );
    if let Some(tag) = tag {
        let chip = Rect::from_center_size(
            pos2(
                text_x + title_w + space::S + (tag.size().x + 2.0 * space::S) * 0.5,
                first_y,
            ),
            vec2(tag.size().x + 2.0 * space::S, tag.size().y + space::XS),
        );
        paint_capsule(&painter, chip, color::surface_hover());
        painter.galley(
            pos2(
                chip.center().x - tag.size().x * 0.5,
                chip.center().y - tag.size().y * 0.5,
            ),
            tag,
            color::text_detail(),
        );
    }

    // The second line: the newest message, and the count at its end.
    let second_y = top + first_h + space::XS + second_h * 0.5;
    let badge_w = if unread && !entry.request {
        if entry.unread > 9 {
            BADGE_MANY
        } else {
            BADGE_ONE
        }
    } else {
        0.0
    };
    let preview_room = (right - text_x - badge_w - space::S).max(40.0);
    let ink = if entry.typing {
        color::text()
    } else if unread {
        color::text_label()
    } else if entry.empty && !entry.request {
        color::text_note()
    } else {
        color::text_detail()
    };
    let said = entry_preview(entry, language);
    let preview = one_line(&painter, preview_style, &said, preview_room);
    cut |= preview.elided;
    painter.galley(
        pos2(text_x, second_y - preview.size().y * 0.5),
        preview,
        ink,
    );
    if badge_w > 0.0 {
        let count = if entry.unread > 99 {
            "99+".to_owned()
        } else {
            entry.unread.to_string()
        };
        let badge = Rect::from_center_size(
            pos2(right - badge_w * 0.5, second_y),
            vec2(badge_w, BADGE_HEIGHT),
        );
        paint_capsule(&painter, badge, color::primary());
        text::paint_tabular_aligned(
            &painter,
            badge,
            Align::Center,
            ty::BADGE,
            &count,
            color::on_primary(),
        );
    }
    // A name or a line cut to the row's width is read in full while the pointer or the focus is
    // on it.
    if cut {
        crate::ui::reveal::full_text(
            &ctx,
            response.id,
            rect,
            &format!("{}\n{said}", entry.title),
            hovered || (focused && input::focus_visible(&ctx)),
            true,
        );
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

/// Text wrapped at `width` with room between the lines; a word longer than the line breaks.
fn wrapped(
    painter: &egui::Painter,
    style: TextStyle,
    leading: f32,
    text: &str,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job =
        egui::text::LayoutJob::simple(text.to_owned(), style.font(), Color32::PLACEHOLDER, width);
    job.wrap.break_anywhere = true;
    job.sections[0].format.line_height = Some(style.size * leading);
    painter.layout_job(job)
}

// ---------------------------------------------------------------------------------------------
// Drawing: the thread
// ---------------------------------------------------------------------------------------------

/// The thread: a header, the bubbles that scroll, and the composer that does not. The header, the
/// bubbles and the composer stand on the same two edges.
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

    // The header: the picture, the name and the line under it, and the options at the end.
    let head = Rect::from_min_size(column.min, vec2(width, HEAD_HEIGHT));
    let face_box = Rect::from_min_size(
        pos2(head.min.x, head.center().y - ENTRY_FACE * 0.5),
        vec2(ENTRY_FACE, ENTRY_FACE),
    );
    paint_faces(&painter, face_box, &header.faces, header.kind);
    let name_x = face_box.max.x + space::L;
    let room = width - ENTRY_FACE - space::L - COMPOSER_BUTTON - space::L;
    let name = one_line(&painter, ty::ITEM_TITLE, &header.title, room);
    let name_h = name.size().y;
    let status = header
        .status
        .as_ref()
        .map(|status| one_line(&painter, ty::NOTE, status, room));
    let status_h = status.as_ref().map_or(0.0, |s| s.size().y + space::XS);
    let name_y = head.center().y - (name_h + status_h) * 0.5;
    painter.galley(pos2(name_x, name_y), name, color::text());
    if let Some(status) = status {
        painter.galley(
            pos2(name_x, name_y + name_h + space::XS),
            status,
            color::text_detail(),
        );
    }
    let more = Rect::from_center_size(
        pos2(head.max.x - COMPOSER_BUTTON * 0.5, head.center().y),
        vec2(COMPOSER_BUTTON, COMPOSER_BUTTON),
    );
    icon_button(
        ui,
        more,
        Icon::DOTS_THREE,
        S::ChatOptions.text(language),
        focus == Some(ThreadFocus::Header),
        true,
        false,
        ThreadFocus::Header,
        &mut events,
    );

    // The composer sits at the bottom; the thread takes the room between.
    let note_h = match composer {
        Composer::Write { note: Some(_), .. } => NOTE_BAR + space::S,
        Composer::Request => 44.0,
        _ => 0.0,
    };
    let bar_h = COMPOSER_HEIGHT + note_h + space::L;
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
        ui.add_space(space::L);
        for item in &thread.items {
            match item {
                Item::Earlier => {
                    let focused = focus == Some(ThreadFocus::Earlier);
                    let rect = earlier_button(ui, focused, language, &mut events);
                    geometry.push((rect.min.y - origin, rect.height()));
                }
                Item::Day(label) => {
                    let amount = top_fade(ui, body.min.y);
                    ui.scope(|ui| {
                        ui.multiply_opacity(amount);
                        day_separator(ui, label);
                    });
                }
                Item::Bubble(bubble) => {
                    let focused = focus == Some(ThreadFocus::Message(bubble.index));
                    let amount = top_fade(ui, body.min.y);
                    let rect = ui
                        .scope(|ui| {
                            ui.multiply_opacity(amount);
                            bubble_view(ui, bubble, inner_width, focused, &mut events)
                        })
                        .inner;
                    geometry.push((rect.min.y - origin, rect.height()));
                }
                Item::Typing(who, line) => typing_view(ui, who.as_ref(), line),
                Item::State(icon, title, note) => state_view(ui, *icon, title, note.as_deref()),
            }
        }
        ui.add_space(space::S);
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
    composer_view(ui, bar, composer, focus, type_now, language, &mut events);
    events
}

/// "Show Earlier Messages": a small button in the middle of the column.
fn earlier_button(ui: &mut Ui, focused: bool, language: Language, events: &mut Vec<Mouse>) -> Rect {
    let ctx = ui.ctx().clone();
    let label = S::ShowEarlier.text(language);
    let (row, _) = ui.allocate_exact_size(
        vec2(
            ui.available_width(),
            ControlSize::Small.height() + 2.0 * space::M,
        ),
        Sense::hover(),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(row)
            .layout(egui::Layout::top_down(Align::Center)),
    );
    child.add_space(space::M);
    let response = ConsoleButton::new(label)
        .size(ControlSize::Small)
        .focused(focused)
        .show(&mut child);
    crate::ui::a11y::follow(&ctx, response.id, focused);
    if response.clicked() {
        events.push(Mouse::Press(ThreadFocus::Earlier));
    }
    row
}

/// The label over the messages of a day, in the middle: quiet words, no box.
fn day_separator(ui: &mut Ui, label: &str) {
    let painter = ui.painter().clone();
    let galley = text::layout(&painter, DAY, label, Color32::PLACEHOLDER, f32::INFINITY);
    let (rect, response) = ui.allocate_exact_size(
        vec2(
            ui.available_width(),
            galley.size().y + space::BUBBLE_DAY + space::M,
        ),
        Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Heading, true, label));
    painter.galley(
        pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.max.y - space::M - galley.size().y,
        ),
        galley,
        color::text_note(),
    );
}

/// Somebody types: three quiet dots in the column the bubbles stand in. The words are for the
/// screen reader and the list.
fn typing_view(ui: &mut Ui, who: Option<&Face>, line: &str) {
    let painter = ui.painter().clone();
    let width = ui.available_width();
    let (row, response) =
        ui.allocate_exact_size(vec2(width, TYPING_HEIGHT + RUN_GAP), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Status, true, line));
    let mut left = row.min.x;
    if let Some(who) = who {
        let face = Rect::from_min_size(pos2(left, row.max.y - FACE), vec2(FACE, FACE));
        paint_avatar(&painter, face, who.avatar, who.account, &who.handle);
        left = face.max.x + FACE_GAP;
    }
    // On the avatar's centre line when there is one, from the edge the words of a bubble start on.
    let y = if who.is_some() {
        row.max.y - FACE * 0.5
    } else {
        row.max.y - TYPING_HEIGHT * 0.5
    };
    for (n, alpha) in [1.0, 0.7, 0.4].into_iter().enumerate() {
        painter.circle_filled(
            pos2(left + BUBBLE_PAD_X + n as f32 * space::M, y),
            DOT,
            fade(color::text_note(), alpha),
        );
    }
}

fn state_view(ui: &mut Ui, icon: Icon, title: &str, note: Option<&str>) {
    let painter = ui.painter().clone();
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 160.0), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, title));
    painter.text(
        pos2(rect.center().x, rect.center().y - space::XL),
        Align2::CENTER_CENTER,
        icon.glyph(),
        icon.font(36.0, false),
        color::text_note(),
    );
    text::paint(
        &painter,
        pos2(rect.center().x, rect.center().y + space::M),
        Align2::CENTER_CENTER,
        ty::LABEL,
        title,
        color::text_detail(),
    );
    if let Some(note) = note {
        text::paint(
            &painter,
            pos2(rect.center().x, rect.center().y + space::XL + space::S),
            Align2::CENTER_CENTER,
            ty::DETAIL,
            note,
            color::text_note(),
        );
    }
}

/// A message that was taken back: a small line of words in the middle, not a bubble. It stays a
/// stop of the focus, since it opens the message like any other.
fn removed_view(
    ui: &mut Ui,
    b: &Bubble,
    row_width: f32,
    focused: bool,
    events: &mut Vec<Mouse>,
) -> Rect {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let gap = space::L;
    let galley = text::layout(&painter, ty::NOTE, &b.text, Color32::PLACEHOLDER, row_width);
    let height = (galley.size().y + space::S).max(ControlSize::Small.height());
    let (row, _) = ui.allocate_exact_size(vec2(row_width, gap + height), Sense::hover());
    let pill = Rect::from_center_size(
        pos2(row.center().x, row.max.y - height * 0.5),
        vec2(galley.size().x + 2.0 * space::M, height),
    );
    let response = ui.interact(
        pill,
        ui.make_persistent_id(("bubble", b.seq)),
        Sense::click(),
    );
    crate::ui::a11y::follow(&ctx, response.id, focused);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, b.reader.as_str()));
    if response.clicked() {
        events.push(Mouse::Press(ThreadFocus::Message(b.index)));
    }
    let hovered = input::hovered(&ctx, &response);
    if hovered {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if ui.is_rect_visible(row) {
        let t = ctx.focus_t((response.id, "bubble"), focused);
        if hovered {
            paint_capsule(&painter, pill, color::surface_row());
        }
        components::paint_focus_ring(&painter, pill, pill.height() * 0.5, t);
        painter.galley(
            pos2(
                pill.center().x - galley.size().x * 0.5,
                pill.center().y - galley.size().y * 0.5,
            ),
            galley,
            color::text_note(),
        );
    }
    row
}

/// The fill of a bubble: the others' resting surface, the player's one step lighter.
fn bubble_fill(mine: bool) -> Color32 {
    if mine {
        color::bubble_own()
    } else {
        color::bubble_other()
    }
}

/// One bubble with what stands around it: the name above the first of a group, the picture beside
/// it, the quote in it, and the time under the last.
fn bubble_view(
    ui: &mut Ui,
    b: &Bubble,
    row_width: f32,
    focused: bool,
    events: &mut Vec<Mouse>,
) -> Rect {
    if b.removed {
        return removed_view(ui, b, row_width, focused, events);
    }
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let gutter = if b.crowd && !b.mine {
        FACE + FACE_GAP
    } else {
        0.0
    };
    let max_bubble = (row_width * BUBBLE_SHARE).max(160.0);
    let text_width = max_bubble - 2.0 * BUBBLE_PAD_X;

    // Measure: the quote, the text.
    let quote = b.quote.as_ref().map(|(who, text)| {
        let room = text_width - QUOTE_INSET;
        let who = one_line(&painter, QUOTE_NAME, who, room);
        let text = crate::screens::clamped(&painter, QUOTE, text, Color32::PLACEHOLDER, room, 2);
        (who, text)
    });
    let body = wrapped(&painter, BODY, BODY_LEADING, &b.text, text_width);
    let quote_h = quote
        .as_ref()
        .map_or(0.0, |(w, t)| w.size().y + t.size().y + space::S);
    let mut inner_w = body.size().x;
    if let Some((w, t)) = &quote {
        inner_w = inner_w.max(w.size().x.max(t.size().x) + QUOTE_INSET);
    }
    let bubble_w = (inner_w + 2.0 * BUBBLE_PAD_X).max(BUBBLE_MIN);
    let bubble_h = body.size().y + 2.0 * BUBBLE_PAD_Y + quote_h;
    let name_h = if b.name.is_some() { NAME_HEIGHT } else { 0.0 };
    let meta_h = if b.meta.is_some() { META_HEIGHT } else { 0.0 };
    let gap = if b.first { RUN_GAP } else { LINE_GAP };
    let total_h = gap + name_h + bubble_h + meta_h;

    let (row, _) = ui.allocate_exact_size(vec2(row_width, total_h), Sense::hover());
    let bubble_x = if b.mine {
        row.max.x - bubble_w
    } else {
        row.min.x + gutter
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
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, b.reader.as_str()));
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

    // The name above the first bubble of another's group.
    if let Some(name) = &b.name {
        let g = one_line(&painter, NAME, name, bubble_w.max(120.0));
        painter.galley(
            pos2(
                bubble.min.x + BUBBLE_PAD_X,
                bubble.min.y - g.size().y - space::XS,
            ),
            g,
            color::text_detail(),
        );
    }
    // The picture beside the first bubble of another's group.
    if let Some(face) = &b.face {
        let rect = Rect::from_min_size(pos2(row.min.x, bubble.min.y), vec2(FACE, FACE));
        paint_avatar(&painter, rect, face.avatar, face.account, &face.handle);
    }
    // The player's own words are one step lighter than the others', never white.
    let fill = bubble_fill(b.mine);
    let fill = if hovered {
        components::lighten(fill, components::LIFT_HOVER)
    } else {
        fill
    };
    let r = BUBBLE_RADIUS;
    let tight = BUBBLE_JOIN;
    // The corners on the sender's side are small inside a group; the first bubble keeps its top
    // one round, the last ends with the small one at the bottom like the tail of a line.
    let (tl, tr, bl, br) = if b.mine {
        (r, if b.first { r } else { tight }, r, tight)
    } else {
        (if b.first { r } else { tight }, r, tight, r)
    };
    let corners = egui::CornerRadius {
        nw: tl as u8,
        ne: tr as u8,
        sw: bl as u8,
        se: br as u8,
    };
    painter.rect_filled(bubble, corners, fill);
    // The small corners make the ring flatter there too.
    let ring = (r + tight) * 0.5;
    components::paint_focus_ring(&painter, bubble, ring, t);
    let mut y = bubble.min.y + BUBBLE_PAD_Y;
    if let Some((who, text)) = quote {
        let bar = Rect::from_min_size(
            pos2(bubble.min.x + BUBBLE_PAD_X, y),
            vec2(QUOTE_BAR, quote_h - space::S),
        );
        painter.rect_filled(bar, corner(QUOTE_BAR * 0.5), color::text_note());
        let x = bubble.min.x + BUBBLE_PAD_X + QUOTE_INSET;
        let who_h = who.size().y;
        painter.galley(pos2(x, y), who, color::text_label());
        painter.galley(pos2(x, y + who_h), text, color::text_detail());
        y += quote_h;
    }
    painter.galley(pos2(bubble.min.x + BUBBLE_PAD_X, y), body, color::text());

    // The time (and what became of the message) under the last bubble of a group.
    if let Some(meta) = &b.meta {
        let g = text::layout(&painter, META, meta, Color32::PLACEHOLDER, f32::INFINITY);
        let x = if b.mine {
            bubble.max.x - g.size().x - BUBBLE_PAD_X
        } else {
            bubble.min.x + BUBBLE_PAD_X
        };
        painter.galley(pos2(x, bubble.max.y + space::XS), g, color::text_note());
    }
    row
}

// ---------------------------------------------------------------------------------------------
// Drawing: the composer
// ---------------------------------------------------------------------------------------------

/// The bar under the thread. Its parts stand on the thread's two edges.
#[allow(clippy::too_many_arguments)]
fn composer_view(
    ui: &mut Ui,
    bar: Rect,
    composer: &Composer,
    focus: Option<ThreadFocus>,
    type_now: bool,
    language: Language,
    events: &mut Vec<Mouse>,
) {
    let ctx = ui.ctx().clone();
    let painter = ui.painter().clone();
    let inner = Rect::from_min_max(
        pos2(bar.min.x, bar.min.y + space::S),
        pos2(bar.max.x, bar.max.y - space::S),
    );
    match composer {
        Composer::Write { draft, note } => {
            if let Some(note) = note {
                let rect = Rect::from_min_size(inner.min, vec2(inner.width(), NOTE_BAR));
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
                paint_capsule(&painter, rect, color::surface());
                components::paint_focus_ring(&painter, rect, rect.height() * 0.5, t);
                let line = one_line(
                    &painter,
                    ty::LABEL,
                    &note.text,
                    rect.width() - 2.0 * space::XXL,
                );
                painter.galley(
                    pos2(
                        rect.min.x + space::XL,
                        rect.center().y - line.size().y * 0.5,
                    ),
                    line,
                    color::text_detail(),
                );
                painter.text(
                    pos2(rect.max.x - space::XL, rect.center().y),
                    Align2::CENTER_CENTER,
                    Icon::X.glyph(),
                    Icon::X.font(14.0, false),
                    color::text_detail(),
                );
            }
            let pill =
                Rect::from_min_max(pos2(inner.min.x, inner.max.y - COMPOSER_HEIGHT), inner.max);
            // A filled field with a hairline: the ring is only for the focus.
            paint_capsule(&painter, pill, color::composer());
            components::paint_hairline(
                &painter,
                pill,
                pill.height() * 0.5,
                color::hairline_control(),
            );
            // Send sits in the field's trailing end: quiet until there is something to send.
            let ready = anyps5_core::online::api::is_message_text(draft);
            let send = Rect::from_center_size(
                pos2(pill.max.x - space::XS - SEND_BUTTON * 0.5, pill.center().y),
                vec2(SEND_BUTTON, SEND_BUTTON),
            );
            let field = Rect::from_min_max(
                pos2(pill.min.x + BUBBLE_PAD_X, pill.min.y),
                pos2(send.min.x - space::S, pill.max.y),
            );
            icon_button(
                ui,
                send,
                Icon::PAPER_PLANE_RIGHT,
                S::Send.text(language),
                focus == Some(ThreadFocus::Composer(Slot::Send)),
                ready,
                ready,
                ThreadFocus::Composer(Slot::Send),
                events,
            );
            if draft.chars().count() > anyps5_core::online::api::MAX_MESSAGE_CHARS {
                text::paint(
                    &painter,
                    pos2(pill.max.x - 2.0 * COMPOSER_BUTTON, pill.min.y - space::M),
                    Align2::RIGHT_CENTER,
                    ty::DETAIL,
                    S::TooLong.text(language),
                    color::text_detail(),
                );
            }
            // The field, as large as the words it holds.
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
            field_ui.visuals_mut().weak_text_color = Some(color::text_note());
            let hint = if note.as_ref().is_some_and(|n| n.editing) {
                S::EditingMessage.text(language)
            } else {
                S::MessageField.text(language)
            };
            let edit = egui::TextEdit::singleline(&mut text)
                .id(field_id)
                .hint_text(egui::RichText::new(hint).font(BODY.font()))
                .desired_width(field.width())
                .frame(egui::Frame::NONE)
                .font(BODY.font())
                .text_color(color::text())
                .margin(Margin::ZERO);
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
            paint_capsule(&painter, rect, color::surface());
            components::paint_focus_ring(&painter, rect, rect.height() * 0.5, t);
            painter.text(
                pos2(rect.min.x + space::XXL, rect.center().y),
                Align2::CENTER_CENTER,
                Icon::LOCK.glyph(),
                Icon::LOCK.font(20.0, false),
                color::text_detail(),
            );
            let room = rect.width() - 3.0 * space::XXL;
            let line = one_line(&painter, ty::ROW_TITLE, title, room);
            let sub = detail
                .as_ref()
                .map(|d| one_line(&painter, ty::DETAIL, d, room));
            let h = line.size().y + sub.as_ref().map_or(0.0, |s| s.size().y + 2.0);
            let y = rect.center().y - h * 0.5;
            let lh = line.size().y;
            let x = rect.min.x + space::XXL + space::XL;
            painter.galley(pos2(x, y), line, color::text());
            if let Some(sub) = sub {
                painter.galley(pos2(x, y + lh + 2.0), sub, color::text_detail());
            }
        }
        Composer::Blocked { who } => {
            let rect = inner;
            let focused = focus == Some(ThreadFocus::Bar);
            let label = T::Unblock.text(language);
            paint_capsule(&painter, rect, color::surface());
            let button_h = ControlSize::Small.height();
            let slot = Rect::from_min_size(
                pos2(
                    rect.max.x - space::S - 160.0,
                    rect.center().y - button_h * 0.5,
                ),
                vec2(160.0, button_h),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(slot)
                    .layout(egui::Layout::right_to_left(Align::Center)),
            );
            let response = ConsoleButton::new(label)
                .size(ControlSize::Small)
                .focused(focused)
                .show(&mut child);
            crate::ui::a11y::follow(&ctx, response.id, focused);
            if response.clicked() {
                events.push(Mouse::Press(ThreadFocus::Bar));
            }
            let note = S::BlockedNote.format(language, &[Arg::Str(who)]);
            let line = one_line(
                &painter,
                ty::LABEL,
                &note,
                rect.width() - response.rect.width() - 3.0 * space::XL,
            );
            painter.galley(
                pos2(
                    rect.min.x + space::XL,
                    rect.center().y - line.size().y * 0.5,
                ),
                line,
                color::text_detail(),
            );
        }
        Composer::Request => {
            let rect =
                Rect::from_min_max(pos2(inner.min.x, inner.max.y - COMPOSER_HEIGHT), inner.max);
            let note = S::RequestNote.text(language);
            text::paint(
                &painter,
                pos2(inner.center().x, rect.min.y - space::XL * 0.75),
                Align2::CENTER_CENTER,
                ty::NOTE,
                note,
                color::text_note(),
            );
            // Accept and Decline lead, Block stands alone at the other end; the buttons stand on
            // the page, a box behind them would only hide Decline.
            let answers = [
                (Answer::Accept, T::Accept.text(language), true, false),
                (Answer::Decline, T::Decline.text(language), false, false),
                (Answer::Block, T::Block.text(language), false, true),
            ];
            let button_h = ControlSize::Medium.height();
            let slot = |from_start: bool| {
                let w = ANSWER_SLOT;
                // On the column's two edges, like the header and the bubbles.
                let x = if from_start {
                    rect.min.x
                } else {
                    rect.max.x - w
                };
                Rect::from_min_size(pos2(x, rect.center().y - button_h * 0.5), vec2(w, button_h))
            };
            let mut lead = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(slot(true))
                    .layout(egui::Layout::left_to_right(Align::Center)),
            );
            lead.spacing_mut().item_spacing.x = size::CONTROL_GAP;
            let mut tail = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(slot(false))
                    .layout(egui::Layout::right_to_left(Align::Center)),
            );
            for (answer, label, primary, danger) in answers {
                let focused = focus == Some(ThreadFocus::Answer(answer));
                let host = if danger { &mut tail } else { &mut lead };
                let response = ConsoleButton::new(label)
                    .size(ControlSize::Medium)
                    .prominent(primary)
                    .destructive(danger)
                    .focused(focused)
                    .show(host);
                crate::ui::a11y::follow(&ctx, response.id, focused);
                if response.clicked() {
                    events.push(Mouse::Press(ThreadFocus::Answer(answer)));
                }
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
    enabled: bool,
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
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));
    if response.clicked() && enabled {
        events.push(Mouse::Press(target));
    }
    let hovered = enabled && input::hovered(&ctx, &response);
    if hovered {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let t = ctx.focus_t((response.id, "icon"), focused);
    // A control at rest has a surface, in the bar and in the header alike.
    let in_field = matches!(target, ThreadFocus::Composer(_));
    let fill = if primary {
        color::primary()
    } else if hovered || focused {
        color::surface_hover()
    } else if in_field {
        // Inside the field there is a surface already.
        Color32::TRANSPARENT
    } else {
        color::surface()
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
        } else if enabled {
            color::text()
        } else {
            color::text_inactive()
        },
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
            play_together: None,
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
        // Lumen's two lines after the day change form a group: the name once above it, the
        // picture beside its first bubble, the time under its last.
        assert!(b[1].first && !b[1].last && b[1].name.as_deref() == Some("Lumen"));
        assert!(b[1].face.is_some());
        assert!(!b[2].first && b[2].last && b[2].name.is_none() && b[2].face.is_none());
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
    fn an_answer_stays_in_its_group_and_every_bubble_reads_with_its_time() {
        let mut online = online();
        put(
            &mut online,
            vec![chat("d", ConversationKind::Direct, &[(2, "Lumen")], 3)],
            vec![],
        );
        let n = NOW as u64;
        let mut answer = said("d", 3, 2, "Lumen", "an answer", n - 540);
        answer.reply = Some(anyps5_core::online::api::Quote {
            seq: 1,
            sender: Account {
                id: AccountId(1),
                handle: "Mira".into(),
            },
            text: "first".into(),
            deleted: false,
        });
        load(
            &mut online,
            "d",
            vec![
                said("d", 1, 2, "Lumen", "one", n - 600),
                said("d", 2, 2, "Lumen", "two", n - 590),
                answer,
            ],
        );
        let t = thread(&data(&online), &Local::default(), "d", 40).unwrap();
        let b = bubbles(&t);
        // The answer joins the plain lines before it: one group, the time once at its end.
        assert!(b[0].first && !b[0].last && !b[1].first && !b[1].last);
        assert!(!b[2].first && b[2].last && b[2].quote.is_some());
        // The screen reader gets who, the words and the time for each, not only the last.
        assert!(b[0].reader.starts_with("Lumen, one, "));
        assert!(b[0].reader.contains(':'), "{}", b[0].reader);
        assert!(b[0].meta.is_none() && b[1].meta.is_none() && b[2].meta.is_some());
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
            matches!(t.items.last(), Some(Item::Typing(None, line)) if line == "Lumen is typing…")
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
        assert_eq!(go(F::Earlier, Command::Up), F::Header);
        assert_eq!(go(F::Header, Command::Up), F::Header);
        assert_eq!(go(F::Header, Command::Down), F::Earlier);
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
    fn left_and_right_jump_five_messages_along_a_long_thread() {
        use ThreadFocus as F;
        let s = shape(12, false, false, Kind::Write);
        let go = |focus, command| match step(focus, command, s) {
            Step::Moved(to, _) => to,
            other => panic!("{other:?}"),
        };
        assert_eq!(go(F::Message(11), Command::Left), F::Message(6));
        assert_eq!(go(F::Message(2), Command::Left), F::Message(0));
        assert_eq!(go(F::Message(3), Command::Right), F::Message(8));
        assert_eq!(go(F::Message(9), Command::Right), F::Message(11));
        assert_eq!(go(F::Message(11), Command::Right), F::Composer(Slot::Field));
    }

    #[test]
    fn a_stranger_row_without_a_preview_still_reads_in_full() {
        let entry = Entry {
            id: "c".into(),
            kind: ConversationKind::Direct,
            request: false,
            title: "Mira".into(),
            preview: String::new(),
            when: "09:41".into(),
            unread: 3,
            pinned: true,
            muted: true,
            typing: false,
            empty: false,
            faces: Vec::new(),
        };
        let name = entry_name(&entry, Language::English);
        assert_eq!(
            name, "Mira, Pinned, Muted, 09:41",
            "a muted row speaks no count"
        );
    }

    #[test]
    fn a_line_says_there_is_no_message_only_when_there_is_none() {
        let mut entry = Entry {
            id: "c".into(),
            kind: ConversationKind::Direct,
            request: false,
            title: "Mira".into(),
            preview: String::new(),
            when: String::new(),
            unread: 2,
            pinned: false,
            muted: false,
            typing: false,
            empty: false,
            faces: Vec::new(),
        };
        // The newest message has not been fetched yet: the line stays empty, it does not lie.
        assert_eq!(entry_preview(&entry, Language::English), "");
        entry.empty = true;
        assert_eq!(entry_preview(&entry, Language::English), "No messages yet");
        entry.request = true;
        assert_eq!(
            entry_preview(&entry, Language::English),
            "Wants to message you"
        );
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
