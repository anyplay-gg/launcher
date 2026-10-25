//! The launcher's side of the AnyPlay network.
//!
//! The net service (`anyplay-net`, from anyplay-gg/network) is a program of its own that holds the
//! account, talks to the cloud and carries game traffic. The launcher starts it, speaks the
//! control protocol with it over its standard streams (`spec/control-protocol.md` there) and shows
//! what it reports. Two parts:
//!
//! - [`NetProcess`] runs the child process: one reader thread, writes that never wait on the
//!   service, and a stop that closes the input so the service can leave its sessions.
//! - [`Online`] is what the launcher knows: sign-in, friends, invitations, the player's session.
//!   It is plain data, updated from the service's output, and keeps track of what each command it
//!   sent was for, so the UI can tell the player how it went.
//!
//! Nothing here goes online by itself: the launcher starts the process only once the player asks
//! to sign in, or when they signed in on this PC before.

pub mod avatar;
mod feed;
mod profile;
mod restrictions;
mod runs;
mod social;
mod voice;

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

pub use anyplay_control::anyplay_api as api;
pub use anyplay_control::{
    self as control, Cloud, Command, DeviceKind, Event, Failure, Link, Output, Pending, Session,
    SignedIn, VoiceMode,
};
use api::{Account, AccountId, Conversation, FriendLists, Invitation, Message, PartyInvitation};
pub use feed::{Ago, FeedState, Rarity, adjusted, party_others, toggled};
pub use profile::{
    BANNER_CONTRAST, BANNER_TEXT_ALPHA, BANNER_TEXT_CONTRAST, ProfileEntry, banner_colour,
    contrast_of_text, contrast_with_white, dominant_colour, hours_minutes, most_played, page_of,
    readable_banner,
};
pub use restrictions::{Blocked, Restrictions, in_force};
pub use runs::Runs;
pub use social::{Fetch, Ingested, KEPT, Listed, Social, others, plain, read_by_other, snippet};
pub use voice::{
    DEFAULT_KEY, DEFAULT_PERCENT, Devices, Hardware, MAX_PERCENT, Mic, PERCENT_STEP, PeerPrefs,
    Problem, Status as VoiceStatus, Voice, VoicePrefs,
};

use crate::events::Notifier;
use crate::trophies::{Earned, Grade};

/// The program name of the net service.
pub const BINARY: &str = if cfg!(windows) {
    "anyplay-net.exe"
} else {
    "anyplay-net"
};

/// The environment variable that names the net service to run instead of the bundled one.
pub const BINARY_VARIABLE: &str = "ANYPLAY_NET";

/// AnyPlay's own cloud, the one players sign in to unless they pick another. A release build
/// names it with `ANYPLAY_DEFAULT_CLOUD`; without it the player has to enter a cloud.
pub const DEFAULT_CLOUD: Option<&str> = option_env!("ANYPLAY_DEFAULT_CLOUD");

/// The cloud the player uses: the one they picked, else AnyPlay's own.
pub fn cloud(picked: Option<&str>) -> Option<&str> {
    picked.or(DEFAULT_CLOUD).filter(|url| !url.is_empty())
}

/// True for an address a cloud can have: https, or http on this PC for a cloud under test.
pub fn is_cloud_address(text: &str) -> bool {
    let text = text.trim();
    let rest = if let Some(rest) = text.strip_prefix("https://") {
        rest
    } else if let Some(rest) = text.strip_prefix("http://") {
        let host = match rest.strip_prefix('[') {
            Some(v6) => v6.split(']').next().unwrap_or_default(),
            None => rest.split(['/', ':']).next().unwrap_or_default(),
        };
        if !matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return false;
        }
        rest
    } else {
        return false;
    };
    !rest.is_empty()
        && text.len() <= 2048
        && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// A line of output, or the end of the process.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Output(Output),
    Ended,
}

/// The net service as a child process.
pub struct NetProcess {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    lines: Receiver<Line>,
}

impl NetProcess {
    /// Where the net service is: `ANYPLAY_NET`, else next to the launcher's own executable.
    pub fn locate() -> Option<PathBuf> {
        if let Some(path) = std::env::var_os(BINARY_VARIABLE).map(PathBuf::from) {
            return path.is_file().then_some(path);
        }
        let exe = std::env::current_exe().ok()?;
        let path = exe.parent()?.join(BINARY);
        path.is_file().then_some(path)
    }

    /// Starts `binary` with its files in `data`, keeping every tunnel on a relay when
    /// `relay_only`; its log goes to the end of `log`. `notify` is called after every line it
    /// writes.
    pub fn spawn(
        binary: &Path,
        data: &Path,
        relay_only: bool,
        log: &Path,
        notify: Notifier,
    ) -> std::io::Result<Self> {
        std::fs::create_dir_all(data)?;
        if let Some(parent) = log.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)?;
        let mut command = Process::new(binary);
        command
            .arg("--data")
            .arg(data)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log);
        if relay_only {
            command.arg("--relay-only");
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no output pipe"))?;
        let stdin = child.stdin.take();
        let (tx, lines) = channel();
        std::thread::Builder::new()
            .name("net-read".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = Vec::new();
                loop {
                    line.clear();
                    let read = (&mut reader)
                        .take(control::MAX_LINE as u64)
                        .read_until(b'\n', &mut line);
                    match read {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let Ok(text) = std::str::from_utf8(&line) else {
                        continue;
                    };
                    if let Some(output) = control::parse_output(text.trim_end()) {
                        if tx.send(Line::Output(output)).is_err() {
                            return;
                        }
                        notify();
                    }
                }
                let _ = tx.send(Line::Ended);
                notify();
            })?;
        Ok(Self {
            child: Some(child),
            stdin,
            lines,
        })
    }

    /// Writes command lines. An error means the service is gone; its end arrives as a line.
    pub fn write(&mut self, lines: &[String]) -> std::io::Result<()> {
        let Some(stdin) = &mut self.stdin else {
            return Err(std::io::ErrorKind::BrokenPipe.into());
        };
        for line in lines {
            stdin.write_all(line.as_bytes())?;
        }
        stdin.flush()
    }

    /// The lines that arrived since the last call.
    pub fn drain(&self) -> Vec<Line> {
        self.lines.try_iter().collect()
    }
}

impl Drop for NetProcess {
    /// Closes the service's input. It leaves its sessions and ends by itself (or, with a game
    /// still connected, once that game ends); a service that does not end in ten seconds while
    /// no game holds it is stopped.
    fn drop(&mut self) {
        drop(self.stdin.take());
        if let Some(mut child) = self.child.take() {
            let _ = std::thread::Builder::new()
                .name("net-stop".into())
                .spawn(move || {
                    for _ in 0..100 {
                        if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    tracing::info!("the net service is still running; a game may hold it");
                });
        }
    }
}

/// Whether the net service runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Not started: the player has not signed in on this PC.
    #[default]
    Off,
    Running,
    /// This installation has no net service.
    Missing,
    /// It could not start or it ended; the reason is for the log.
    Ended(String),
}

/// A join the launcher checks before it starts the game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Join {
    pub session: String,
    pub title_id: String,
    pub title: String,
    pub version: String,
    pub build: String,
    /// The friend whose session it is, for the messages.
    pub friend: String,
}

/// What a command was sent for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Purpose {
    Hello,
    /// Signing in, or creating an account, in the browser.
    BrowserSignIn {
        create: bool,
    },
    CancelSignIn,
    /// A link to the account web.
    AccountPage,
    /// The appeal of the action with this number.
    Appeal(u64),
    SignOut,
    AddFriend(String),
    Accept(AccountId),
    Decline(AccountId),
    RemoveFriend(AccountId),
    Block(AccountId),
    Unblock(AccountId),
    Report(AccountId),
    Playing,
    CheckJoin(Join),
    /// A game start; the number pairs it with the launcher's start.
    GameStart(u64),
    GameEnd,
    Invite,
    AnswerInvitation {
        id: String,
        accept: bool,
    },
    Dialog,
    RelayOnly,
    /// The records of a title; `count` is how many the player has earned in all.
    UploadTrophies {
        title_id: String,
        count: usize,
    },
    Cards,
    Privacy,
    SetPrivacy,
    /// A page of the feed: the newest, or the one that starts before an item.
    Feed {
        before: Option<u64>,
    },
    /// The player's reaction to a feed item, with what the item showed before it.
    React {
        item: u64,
        before: api::Reactions,
    },
    /// The profile page of an account.
    Profile(AccountId),
    /// Minutes per title and day, as sent.
    UploadPlaytime(Vec<api::PlaytimeEntry>),
    /// A run report ("How did it run?").
    RunReport,
    /// What players found for a title; `version` is the game version asked for first.
    RunSummary {
        title_id: String,
        version: Option<String>,
    },
    // Parties and messages
    Party,
    PartyInvite,
    PartyAnswer {
        id: String,
        accept: bool,
    },
    PartyLeave,
    PartyKick(AccountId),
    PartyLeader(AccountId),
    PartyMove,
    Conversations,
    OpenConversation(AccountId),
    /// A fetch of messages; `announce` when a new-message event asked for it, so the player is
    /// told of what arrived.
    Messages {
        conversation: String,
        fetch: Fetch,
        announce: bool,
    },
    SendMessage {
        conversation: String,
        text: String,
    },
    MarkRead {
        conversation: String,
    },
    AcceptRequest(String),
    LeaveConversation(String),
    // Voice
    /// Switching voice on or off, its mode, a member's volume or mute.
    Voice,
    PushToTalk,
    VoiceDevices,
    SetVoiceDevice(DeviceKind),
}

/// The game's invitation dialog, waiting for the player to pick friends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    pub id: u64,
    pub title: String,
    pub max: u16,
}

/// Something the launcher may want to tell the player about.
#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// A command came back.
    Answered(Purpose, Result<serde_json::Value, Failure>),
    /// Somebody asked to be friends.
    Request(Account),
    /// A friend invited the player.
    Invitation(Invitation),
    /// The running game opened its invitation dialog.
    Dialog(Dialog),
    /// The net service ended.
    Ended,
    /// The browser sign-in finished: the account is signed in.
    SignedIn,
    /// The browser sign-in ran out of time.
    SignInTimedOut,
    /// The leader of a party invited the player.
    PartyInvitation(PartyInvitation),
    /// A new message in a conversation, the newest one the player has not read.
    Message {
        conversation: String,
        message: Message,
    },
    /// A stranger wrote to the player for the first time.
    MessageRequest(Conversation),
    /// The net service could not open the sound devices and switched voice off.
    VoiceFailed(Problem),
    /// A moderator limited the account while the launcher ran.
    Restricted(control::Restriction),
    /// The account was banned and is signed out.
    Banned(control::Restriction),
}

/// What the launcher knows about the player's online state.
#[derive(Debug, Default)]
pub struct Online {
    pub status: Status,
    pub cloud: Option<Cloud>,
    pub account: Option<SignedIn>,
    pub pending: Option<Pending>,
    /// The sign-in page the net service gave, while the browser sign-in waits.
    pub sign_in_url: Option<String>,
    pub friends: FriendLists,
    pub invitations: Vec<Invitation>,
    pub session: Option<Session>,
    pub dialog: Option<Dialog>,
    /// The party, the conversations and their messages.
    pub social: Social,
    /// Party voice: what the player chose and what the service reports.
    pub voice: Voice,
    /// What limits the account: mutes, a new account's limits, a ban.
    pub restrictions: Restrictions,
    /// Friends' trophy counts by grade; `None` when they hide them from the player.
    pub cards: BTreeMap<AccountId, Option<api::Count>>,
    /// The avatar hash of every account the launcher heard of in a card or a profile; `None` when
    /// the account has none or hides it.
    pub avatars: BTreeMap<AccountId, Option<String>>,
    /// What the player and their friends did, once the Feed page asked for it.
    pub feed: FeedState,
    /// The profiles the player looked at, by account.
    pub profiles: BTreeMap<AccountId, ProfileEntry>,
    /// Who sees the player's trophies; `None` until the cloud said.
    pub privacy: Option<api::Privacy>,
    /// What players found per title (run reports).
    pub runs: Runs,
    /// How many trophies of each title the cloud has recorded since this sign-in.
    uploaded: BTreeMap<String, usize>,
    /// The minutes the cloud has recorded per title and day since this sign-in.
    playtime_sent: BTreeMap<(String, String), u32>,
    waiting: BTreeMap<u64, Purpose>,
    next: u64,
    outbox: Vec<String>,
}

impl Online {
    /// Queues `command`; [`Online::take_outbox`] hands it to the process.
    pub fn send(&mut self, purpose: Purpose, command: Command) -> u64 {
        self.next += 1;
        let id = self.next;
        self.outbox.push(control::command_line(id, &command));
        self.waiting.insert(id, purpose);
        id
    }

    pub fn take_outbox(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outbox)
    }

    /// True while a command for which `test` holds waits for its answer.
    pub fn is_waiting(&self, test: impl Fn(&Purpose) -> bool) -> bool {
        self.waiting.values().any(test)
    }

    /// The player is signed in and the cloud answers.
    pub fn is_online(&self) -> bool {
        self.account.is_some() && self.cloud == Some(Cloud::Online)
    }

    pub fn is_signed_in(&self) -> bool {
        self.account.is_some()
    }

    /// How many friends are online now; none while signed out.
    pub fn friends_online(&self) -> usize {
        if !self.is_signed_in() {
            return 0;
        }
        self.friends
            .friends
            .iter()
            .filter(|f| f.presence.online)
            .count()
    }

    /// Forgets the process and its state, keeping nothing of the account.
    pub fn stopped(&mut self, status: Status) -> Vec<Notice> {
        let notices = self
            .waiting
            .values()
            .map(|purpose| {
                Notice::Answered(
                    purpose.clone(),
                    Err(Failure::new(Failure::OFFLINE, "The net service ended.")),
                )
            })
            .collect();
        let mut voice = std::mem::take(&mut self.voice);
        voice.reset();
        *self = Online {
            status,
            next: self.next,
            voice,
            ..Online::default()
        };
        notices
    }

    /// Applies one line of the service's output.
    pub fn apply(&mut self, line: Line) -> Vec<Notice> {
        match line {
            Line::Ended => {
                let mut notices = self.stopped(Status::Ended("the net service ended".into()));
                notices.push(Notice::Ended);
                notices
            }
            Line::Output(Output::Answer(answer)) => {
                let Some(purpose) = self.waiting.remove(&answer.id) else {
                    return Vec::new();
                };
                let result = answer.into_result::<serde_json::Value>();
                let mut notices = Vec::new();
                match &result {
                    Ok(value) => self.answered(&purpose, value, &mut notices),
                    Err(failure) => self.refused(&purpose, failure),
                }
                notices.push(Notice::Answered(purpose, result));
                notices
            }
            Line::Output(Output::Event(event)) => self.event(event),
        }
    }

    /// Keeps what a refusal tells about the account: the restriction that stopped the command.
    fn refused(&mut self, purpose: &Purpose, failure: &Failure) {
        match purpose {
            Purpose::Feed { .. } => self.feed.error = Some(failure.clone()),
            Purpose::React { item, before } => self.feed.set_reactions(*item, *before),
            Purpose::Profile(account) => {
                // A page that is shown stays; only a profile without one is a failure.
                let entry = self.profiles.entry(*account).or_default();
                entry.failed = entry.page.is_none();
            }
            _ => {}
        }
        let Some(restriction) = &failure.restriction else {
            return;
        };
        let blocked = self.blocked_by(purpose);
        self.restrictions.learn(blocked, restriction);
    }

    /// The action a command was, as far as a restriction can stop it.
    pub fn blocked_by(&self, purpose: &Purpose) -> Option<Blocked> {
        match purpose {
            Purpose::AddFriend(_) => Some(Blocked::FriendRequest),
            Purpose::OpenConversation(account) => Some(if self.friend(*account).is_some() {
                Blocked::Message
            } else {
                Blocked::Stranger
            }),
            Purpose::SendMessage { conversation, .. } => Some(
                if self
                    .social
                    .get(conversation)
                    .is_some_and(|c| self.is_stranger(c))
                {
                    Blocked::Stranger
                } else {
                    Blocked::Message
                },
            ),
            _ => None,
        }
    }

    /// True for a direct conversation with someone who is not a friend: writing in it is what a
    /// new account may not do yet.
    pub fn is_stranger(&self, conversation: &Conversation) -> bool {
        let Some(me) = self.account.as_ref().map(|a| a.id) else {
            return false;
        };
        conversation.kind == api::ConversationKind::Direct
            && others(conversation, me)
                .iter()
                .any(|other| self.friend(other.id).is_none())
    }

    /// Keeps what a successful answer tells about the account.
    fn answered(
        &mut self,
        purpose: &Purpose,
        value: &serde_json::Value,
        notices: &mut Vec<Notice>,
    ) {
        match purpose {
            Purpose::BrowserSignIn { .. } => {
                if let Ok(opened) = serde_json::from_value::<control::Opened>(value.clone()) {
                    self.sign_in_url = Some(opened.url);
                }
            }
            Purpose::UploadTrophies { title_id, count } => {
                let known = self.uploaded.entry(title_id.clone()).or_default();
                *known = (*known).max(*count);
            }
            Purpose::Cards => {
                if let Ok(cards) = serde_json::from_value::<api::Cards>(value.clone()) {
                    for card in cards.cards {
                        self.avatars.insert(card.account.id, card.avatar);
                        self.cards.insert(card.account.id, card.trophies);
                    }
                }
            }
            Purpose::RunSummary { title_id, version } => {
                if let Ok(summary) = serde_json::from_value::<api::RunSummary>(value.clone()) {
                    self.runs.keep(title_id, version.as_deref(), summary);
                }
            }
            Purpose::Feed { before } => {
                if let Ok(page) = serde_json::from_value::<api::Feed>(value.clone()) {
                    match before {
                        Some(before) => self.feed.merge_older(*before, page),
                        None => self.feed.merge_newest(page),
                    }
                }
            }
            Purpose::React { item, .. } => {
                if let Ok(now) = serde_json::from_value::<api::Reactions>(value.clone()) {
                    self.feed.set_reactions(*item, now);
                }
            }
            Purpose::Profile(account) => {
                if let Ok(page) = serde_json::from_value::<api::ProfilePage>(value.clone()) {
                    self.avatars.insert(*account, page.profile.avatar.clone());
                    self.profiles.insert(
                        *account,
                        ProfileEntry {
                            page: Some(page),
                            failed: false,
                        },
                    );
                }
            }
            Purpose::Privacy | Purpose::SetPrivacy => {
                if let Ok(privacy) = serde_json::from_value(value.clone()) {
                    self.privacy = Some(privacy);
                }
            }
            Purpose::UploadPlaytime(entries) => {
                for entry in entries {
                    let known = self
                        .playtime_sent
                        .entry((entry.title_id.clone(), entry.day.clone()))
                        .or_default();
                    *known = (*known).max(entry.minutes);
                }
            }
            Purpose::Hello => self.voice_hello(),
            Purpose::VoiceDevices => {
                if let Ok(devices) = serde_json::from_value::<control::VoiceDevices>(value.clone())
                {
                    self.voice.devices = Some(devices.into());
                }
            }
            Purpose::Party => {
                if let Ok(state) = serde_json::from_value::<control::PartyState>(value.clone()) {
                    self.party_changed(state.party);
                }
            }
            Purpose::PartyAnswer { id, accept } => {
                self.social.drop_party_invitation(id);
                // A refusal answers an empty object: it says nothing about the player's party.
                if *accept
                    && let Ok(state) = serde_json::from_value::<control::PartyState>(value.clone())
                {
                    self.party_changed(state.party);
                }
            }
            Purpose::Conversations => {
                if let Ok(lists) = serde_json::from_value::<api::Conversations>(value.clone()) {
                    self.listed(lists, notices);
                }
            }
            Purpose::OpenConversation(_) | Purpose::AcceptRequest(_) => {
                if let Ok(conversation) = serde_json::from_value::<Conversation>(value.clone()) {
                    self.social.upsert(conversation);
                }
            }
            Purpose::LeaveConversation(id) => self.social.remove(id),
            Purpose::Messages {
                conversation,
                fetch,
                announce,
            } => {
                if let Ok(page) = serde_json::from_value::<api::Messages>(value.clone()) {
                    let got = self.social.ingest(conversation, *fetch, page);
                    if let Some(next) = got.follow {
                        self.fetch(conversation, next, *announce);
                    }
                    if *announce {
                        self.announce(conversation, &got.new, notices);
                    }
                }
            }
            Purpose::SendMessage { conversation, text } => {
                if let Ok(message) = serde_json::from_value::<Message>(value.clone()) {
                    self.social.sent(conversation, text);
                    let seq = message.seq;
                    let got = self.social.ingest_sent(message);
                    // Writing reads up to the message written.
                    self.social.mark_read(conversation, seq);
                    if let Some(next) = got.follow {
                        self.fetch(conversation, next, false);
                    }
                }
            }
            _ => {}
        }
    }

    /// Keeps a party the service reported or answered, unless it is an older copy.
    fn party_changed(&mut self, party: Option<api::Party>) {
        let chat = party.as_ref().map(|p| p.conversation.clone());
        if self.social.apply_party(party)
            && let Some(chat) = chat
            && self.social.get(&chat).is_none()
        {
            self.refresh_conversations();
        }
    }

    /// Takes a fresh list of conversations and fetches what loaded threads missed.
    fn listed(&mut self, lists: api::Conversations, notices: &mut Vec<Notice>) {
        let listed = self.social.set_conversations(lists);
        for (id, fetch) in listed.fetches {
            self.fetch(&id, fetch, false);
        }
        notices.extend(listed.new_requests.into_iter().map(Notice::MessageRequest));
    }

    /// Asks for the list of conversations, unless that question waits already.
    pub fn refresh_conversations(&mut self) {
        if !self.is_waiting(|p| *p == Purpose::Conversations) {
            self.send(Purpose::Conversations, Command::Conversations);
        }
    }

    /// Fetches messages of a conversation. A fetch of the same kind that waits already is not
    /// repeated; its answer asks for more when the conversation is still ahead.
    pub fn fetch(&mut self, conversation: &str, fetch: Fetch, announce: bool) {
        let same = |f: &Fetch| std::mem::discriminant(f) == std::mem::discriminant(&fetch);
        let mut waiting = false;
        for purpose in self.waiting.values_mut() {
            if let Purpose::Messages {
                conversation: c,
                fetch: f,
                announce: a,
            } = purpose
                && c == conversation
                && same(f)
            {
                *a |= announce;
                waiting = true;
            }
        }
        if !waiting {
            self.send(
                Purpose::Messages {
                    conversation: conversation.to_owned(),
                    fetch,
                    announce,
                },
                fetch.command(conversation),
            );
        }
    }

    /// The newest message the player has not read and did not write, as a notice.
    fn announce(&self, conversation: &str, new: &[Message], notices: &mut Vec<Notice>) {
        let me = self.account.as_ref().map(|a| a.id);
        let read = self.social.get(conversation).map_or(0, |c| c.read);
        if let Some(message) = new
            .iter()
            .rev()
            .find(|m| Some(m.sender.id) != me && m.seq > read)
        {
            notices.push(Notice::Message {
                conversation: conversation.to_owned(),
                message: message.clone(),
            });
        }
    }

    /// Starts reading a conversation: fetches what the thread lacks.
    pub fn open_thread(&mut self, conversation: &str) {
        if let Some(fetch) = self.social.plan(conversation) {
            self.fetch(conversation, fetch, false);
        }
    }

    /// Reads further back in a conversation.
    pub fn load_earlier(&mut self, conversation: &str) {
        if let Some(fetch) = self.social.plan_earlier(conversation) {
            self.fetch(conversation, fetch, false);
        }
    }

    /// The player looks at the conversation: its newest message held counts as read, here at once
    /// and in the cloud.
    pub fn mark_read(&mut self, conversation: &str) {
        if let Some(seq) = self.social.read_target(conversation)
            && self.social.mark_read(conversation, seq)
        {
            self.send(
                Purpose::MarkRead {
                    conversation: conversation.to_owned(),
                },
                Command::MarkRead {
                    conversation: conversation.to_owned(),
                    seq,
                },
            );
        }
    }

    /// Sends the draft of a conversation. False when there is nothing to send, the text is not
    /// one the cloud takes, or the draft is on its way already.
    pub fn send_draft(&mut self, conversation: &str) -> bool {
        let text = self.social.draft(conversation).to_owned();
        let sending = |p: &Purpose| matches!(p, Purpose::SendMessage { conversation: c, .. } if c == conversation);
        if !api::is_message_text(&text) || self.is_waiting(sending) {
            return false;
        }
        self.send(
            Purpose::SendMessage {
                conversation: conversation.to_owned(),
                text: text.clone(),
            },
            Command::SendMessage {
                conversation: conversation.to_owned(),
                text,
            },
        );
        true
    }

    /// Opens the direct conversation with a friend, unless there is one.
    pub fn open_direct(&mut self, account: AccountId) {
        let opening = |p: &Purpose| matches!(p, Purpose::OpenConversation(a) if *a == account);
        if self.social.direct_with(account).is_none() && !self.is_waiting(opening) {
            self.send(
                Purpose::OpenConversation(account),
                Command::OpenConversation {
                    accounts: vec![account],
                    name: None,
                },
            );
        }
    }

    /// Sends the trophies the player earned in a title, unless the cloud has them all already or
    /// an upload of the title still waits. The cloud keeps the first time of each trophy, so a
    /// repeat is harmless; this only saves the request.
    pub fn upload_trophies(&mut self, title_id: &str, earned: &[Earned]) {
        let count = earned.len();
        let sending =
            |p: &Purpose| matches!(p, Purpose::UploadTrophies { title_id: t, .. } if t == title_id);
        if !self.is_online()
            || count == 0
            || !api::is_title_id(title_id)
            || self
                .uploaded
                .get(title_id)
                .is_some_and(|&known| known >= count)
            || self.is_waiting(sending)
        {
            return;
        }
        let trophies: Vec<api::Trophy> = earned
            .iter()
            .filter_map(|e| {
                Some(api::Trophy {
                    number: u16::try_from(e.number)
                        .ok()
                        .filter(|n| *n < api::MAX_TROPHY_NUMBER)?,
                    grade: match e.grade {
                        Grade::Platinum => api::Grade::Platinum,
                        Grade::Gold => api::Grade::Gold,
                        Grade::Silver => api::Grade::Silver,
                        Grade::Bronze => api::Grade::Bronze,
                    },
                    earned: u64::try_from(e.unlocked.div_euclid(1_000_000)).unwrap_or(0),
                })
            })
            .collect();
        let chunks = trophies.chunks(api::MAX_TROPHY_UPLOAD).count();
        for (index, chunk) in trophies.chunks(api::MAX_TROPHY_UPLOAD).enumerate() {
            // Only the last part's answer says that the cloud has them all.
            let count = if index + 1 == chunks { count } else { 0 };
            self.send(
                Purpose::UploadTrophies {
                    title_id: title_id.to_owned(),
                    count,
                },
                Command::UploadTrophies {
                    title_id: title_id.to_owned(),
                    trophies: chunk.to_vec(),
                },
            );
        }
    }

    /// Sends the days whose minutes grew since the cloud last recorded them. The cloud keeps the
    /// highest count of a day, so a repeat is harmless; this only saves the request. While an
    /// upload waits, the next call tries again.
    pub fn upload_playtime(&mut self, entries: Vec<api::PlaytimeEntry>) {
        if !self.is_online() || self.is_waiting(|p| matches!(p, Purpose::UploadPlaytime(_))) {
            return;
        }
        let fresh: Vec<api::PlaytimeEntry> = entries
            .into_iter()
            .filter(|e| e.minutes > 0 && api::is_title_id(&e.title_id))
            .filter(|e| {
                self.playtime_sent
                    .get(&(e.title_id.clone(), e.day.clone()))
                    .is_none_or(|&known| known < e.minutes)
            })
            .collect();
        for chunk in fresh.chunks(api::MAX_PLAYTIME_UPLOAD) {
            self.send(
                Purpose::UploadPlaytime(chunk.to_vec()),
                Command::UploadPlaytime {
                    entries: chunk.to_vec(),
                },
            );
        }
    }

    /// Asks for the gamer cards of the friends: all of them, or only those without one yet.
    pub fn request_cards(&mut self, all: bool) {
        if !self.is_online() || self.is_waiting(|p| *p == Purpose::Cards) {
            return;
        }
        let accounts: Vec<AccountId> = self
            .friends
            .friends
            .iter()
            .map(|f| f.account.id)
            .filter(|id| all || !self.cards.contains_key(id))
            .collect();
        for chunk in accounts.chunks(api::MAX_CARDS) {
            self.send(
                Purpose::Cards,
                Command::Cards {
                    accounts: chunk.to_vec(),
                },
            );
        }
    }

    /// Asks for the newest page of the feed, unless that question waits already.
    pub fn request_feed(&mut self) {
        if !self.is_signed_in() || self.is_waiting(|p| *p == Purpose::Feed { before: None }) {
            return;
        }
        self.send(
            Purpose::Feed { before: None },
            Command::Feed {
                before: None,
                limit: None,
            },
        );
    }

    /// Asks for the page older than the items held, when there is one and none is on its way.
    pub fn request_older_feed(&mut self) {
        let Some(before) = self.feed.next else {
            return;
        };
        if !self.is_signed_in() || self.is_waiting(|p| matches!(p, Purpose::Feed { .. })) {
            return;
        }
        self.send(
            Purpose::Feed {
                before: Some(before),
            },
            Command::Feed {
                before: Some(before),
                limit: None,
            },
        );
    }

    /// True while a page of the feed is on its way.
    pub fn feed_loading(&self) -> bool {
        self.is_waiting(|p| matches!(p, Purpose::Feed { .. }))
    }

    /// Sets the player's reaction to a feed item and shows it at once; the cloud's answer, or its
    /// refusal, settles it. A press on an item whose last reaction is still on its way is ignored.
    pub fn react(&mut self, item: u64, reaction: Option<api::Reaction>) {
        if !self.is_signed_in()
            || self.is_waiting(|p| matches!(p, Purpose::React { item: i, .. } if *i == item))
        {
            return;
        }
        let Some(before) = self.feed.react(item, reaction) else {
            return;
        };
        self.send(
            Purpose::React { item, before },
            Command::React { item, reaction },
        );
    }

    /// Asks for the profile page of an account, unless that question waits already.
    pub fn request_profile(&mut self, account: AccountId) {
        if !self.is_signed_in() || self.is_waiting(|p| *p == Purpose::Profile(account)) {
            return;
        }
        self.send(Purpose::Profile(account), Command::Profile { account });
    }

    /// Changes who sees the player's trophies. The cloud replaces the whole setting, so the other
    /// parts go back as the cloud last said; before it said anything, the launcher asks again
    /// instead of guessing them. The choice shows at once and comes back with the answer.
    pub fn set_trophy_audience(&mut self, trophies: api::Audience) {
        if !self.is_signed_in() {
            return;
        }
        let Some(mut privacy) = self.privacy else {
            self.send(Purpose::Privacy, Command::Privacy);
            return;
        };
        privacy.trophies = trophies;
        self.privacy = Some(privacy);
        self.send(Purpose::SetPrivacy, Command::SetPrivacy { privacy });
    }

    fn event(&mut self, event: Event) -> Vec<Notice> {
        let mut notices = Vec::new();
        match event {
            Event::State {
                cloud,
                account,
                pending,
            } => {
                let was_online = self.is_online();
                let was_waiting = self.pending == Some(Pending::Browser);
                self.cloud = Some(cloud);
                self.account = account;
                self.pending = pending;
                if self.pending != Some(Pending::Browser) {
                    self.sign_in_url = None;
                }
                if self.account.is_none() {
                    self.restrictions.signed_out();
                    self.friends = FriendLists::default();
                    self.invitations.clear();
                    self.social = Social::default();
                    self.voice.reset();
                    self.session = None;
                    self.cards.clear();
                    self.avatars.clear();
                    self.feed = FeedState::default();
                    self.profiles.clear();
                    self.privacy = None;
                    self.uploaded.clear();
                    self.playtime_sent.clear();
                }
                if self.account.is_some() {
                    self.restrictions.signed_in();
                }
                if was_waiting && self.account.is_some() {
                    notices.push(Notice::SignedIn);
                }
                if !was_online && self.is_online() {
                    self.send(Purpose::Privacy, Command::Privacy);
                    self.request_cards(true);
                    // The page the player left open catches up with what happened offline.
                    if self.feed.loaded {
                        self.request_feed();
                    }
                }
            }
            Event::Friends(lists) => {
                for request in &lists.incoming {
                    let known = self
                        .friends
                        .incoming
                        .iter()
                        .any(|r| r.account.id == request.account.id);
                    if !known && self.account.is_some() {
                        notices.push(Notice::Request(request.account.clone()));
                    }
                }
                self.friends = lists;
                self.request_cards(false);
            }
            Event::Invitation { invitation } => {
                self.invitations.retain(|i| i.id != invitation.id);
                self.invitations.push(invitation.clone());
                notices.push(Notice::Invitation(invitation));
            }
            Event::InvitationGone { id } => self.invitations.retain(|i| i.id != id),
            Event::Session { session } => self.session = session,
            Event::InviteDialog { id, title, max } => {
                let dialog = Dialog { id, title, max };
                self.dialog = Some(dialog.clone());
                notices.push(Notice::Dialog(dialog));
            }
            Event::Notice { code, message } => {
                tracing::info!("net service: {code}: {message}");
                if code == "sign_in_timed_out" {
                    notices.push(Notice::SignInTimedOut);
                } else if code == "voice_device" {
                    notices.push(self.voice_failed(&message));
                }
            }
            Event::Restrictions { restrictions } => {
                for restriction in self.restrictions.list(restrictions) {
                    notices.push(Notice::Restricted(restriction));
                }
            }
            Event::Banned { restriction } => {
                self.restrictions.ban(restriction.clone());
                notices.push(Notice::Banned(restriction));
            }
            Event::Party { party } => self.party_changed(party),
            Event::PartyInvitation { invitation } => {
                self.social.add_party_invitation(invitation.clone());
                notices.push(Notice::PartyInvitation(invitation));
            }
            Event::PartyInvitationGone { id } => self.social.drop_party_invitation(&id),
            Event::Conversations(lists) => self.listed(lists, &mut notices),
            Event::Message { conversation, seq } => {
                if self.social.note_last(&conversation, seq) {
                    if let Some(fetch) = self.social.plan(&conversation) {
                        self.fetch(&conversation, fetch, true);
                    }
                } else {
                    self.refresh_conversations();
                }
            }
            // The request is in the list the cloud gives; reading it announces what is new.
            Event::MessageRequest { .. } | Event::Conversation { .. } => {
                self.refresh_conversations();
            }
            Event::Read {
                conversation,
                account,
                seq,
            } => {
                if let Some(me) = self.account.as_ref().map(|a| a.id) {
                    self.social.apply_read(&conversation, account, seq, me);
                }
            }
            Event::ConversationGone { id } => self.social.remove(&id),
            // `enabled` and `mode` are not taken over: an event sent before the service heard a
            // choice would undo it. A service that gave up says so in a notice.
            Event::Voice {
                state, me, peers, ..
            } => self.voice_event(state, me, peers),
            // News, not data: the page that was looked at reads the feed again.
            Event::Feed => {
                if self.feed.loaded {
                    self.request_feed();
                }
            }
            // Share screen has no launcher page yet.
            Event::Screen { .. } => {}
            Event::Unknown => {}
        }
        notices
    }

    /// Friends sorted for the list: those whose game can be joined, then those who play, then the
    /// others online, then the rest; by name within each.
    pub fn sorted_friends(&self) -> Vec<&api::Friend> {
        let mut friends: Vec<&api::Friend> = self.friends.friends.iter().collect();
        friends.sort_by_key(|f| {
            let joinable = f
                .presence
                .activity
                .as_ref()
                .and_then(|a| a.session.as_ref())
                .is_some_and(|s| s.joinable);
            let rank = match (&f.presence.activity, f.presence.online) {
                (Some(_), _) if joinable => 0,
                (Some(_), _) => 1,
                (None, true) => 2,
                (None, false) => 3,
            };
            (rank, f.account.handle.to_lowercase())
        });
        friends
    }

    /// The friend with this account, if they are one.
    pub fn friend(&self, account: AccountId) -> Option<&api::Friend> {
        self.friends
            .friends
            .iter()
            .find(|f| f.account.id == account)
    }

    /// The account behind an incoming request, an outgoing one or a block.
    pub fn known(&self, account: AccountId) -> Option<&Account> {
        self.friends
            .friends
            .iter()
            .map(|f| &f.account)
            .chain(self.friends.incoming.iter().map(|r| &r.account))
            .chain(self.friends.outgoing.iter().map(|r| &r.account))
            .chain(self.friends.blocked.iter())
            .find(|a| a.id == account)
    }
}

/// The variables a game gets beyond its own settings: the bridge's from the net service, and
/// the observation log when the player records network calls.
/// The playtime the network records: whole minutes per title and UTC day, the days of every copy
/// of a title added up, at most a day's minutes. `title_of` names the title of a game ID.
pub fn playtime_entries(
    days: &BTreeMap<String, BTreeMap<String, u64>>,
    title_of: impl Fn(&str) -> Option<String>,
) -> Vec<api::PlaytimeEntry> {
    let mut seconds: BTreeMap<(String, String), u64> = BTreeMap::new();
    for (game, per_day) in days {
        let Some(title) = title_of(game) else {
            continue;
        };
        for (day, played) in per_day {
            let sum = seconds.entry((title.clone(), day.clone())).or_default();
            *sum = sum.saturating_add(*played);
        }
    }
    seconds
        .into_iter()
        .map(|((title_id, day), played)| api::PlaytimeEntry {
            title_id,
            day,
            minutes: u32::try_from(played / 60)
                .unwrap_or(u32::MAX)
                .min(api::MAX_DAY_MINUTES),
        })
        .filter(|e| e.minutes > 0)
        .collect()
}

pub fn game_environment(
    bridge: Option<&BTreeMap<String, String>>,
    record: Option<&Path>,
) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    if let Some(bridge) = bridge {
        for (name, value) in bridge {
            // Only the bridge's own variables, whatever else an answer might carry.
            if name.starts_with("ANYPS5_NET_") {
                env.insert(name.clone(), value.clone());
            }
        }
    }
    if let Some(record) = record {
        env.insert(
            "ANYPS5_NET_OBSERVE".to_owned(),
            record.to_string_lossy().into_owned(),
        );
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use api::{Activity, Friend, Presence, Request};

    fn account(id: u64, handle: &str) -> Account {
        Account {
            id: AccountId(id),
            handle: handle.into(),
        }
    }

    fn event(event: Event) -> Line {
        Line::Output(Output::Event(event))
    }

    fn signed_in(online: &mut Online) {
        online.apply(event(Event::State {
            cloud: Cloud::Online,
            account: Some(SignedIn {
                id: AccountId(1),
                handle: "Alec".into(),
                email: "a@example.org".into(),
            }),
            pending: None,
        }));
    }

    fn refusal(online: &mut Online, id: u64, failure: Failure) -> Vec<Notice> {
        online.apply(Line::Output(Output::Answer(control::Answer::error(
            id, failure,
        ))))
    }

    fn refuse_last(online: &mut Online, failure: Failure) -> Vec<Notice> {
        let id = online.next;
        refusal(online, id, failure)
    }

    fn restricted(kind: &str, until: Option<u64>) -> Failure {
        Failure {
            restriction: Some(Box::new(control::Restriction::new(kind, until))),
            ..Failure::new("restricted", "x")
        }
    }

    #[test]
    fn friends_online_counts_those_online_and_none_while_signed_out() {
        let friend = |id, online| Friend {
            account: account(id, "x"),
            since: 0,
            presence: Presence {
                online,
                activity: None,
                revision: 0,
            },
        };
        let mut online = Online::default();
        online.friends.friends = vec![friend(2, true), friend(3, false), friend(4, true)];
        assert_eq!(online.friends_online(), 0, "signed out");
        signed_in(&mut online);
        assert_eq!(online.friends_online(), 2);
        online.friends.friends.clear();
        assert_eq!(online.friends_online(), 0);
    }

    #[test]
    fn a_refused_friend_request_teaches_what_stops_the_next() {
        let mut online = Online::default();
        signed_in(&mut online);
        let id = online.send(
            Purpose::AddFriend("Lumen".into()),
            Command::AddFriend {
                handle: "Lumen".into(),
            },
        );
        refusal(&mut online, id, restricted("new_account", Some(2000)));
        let blocked = |what| online.restrictions.blocking(what, 1000).map(|r| r.until);
        assert_eq!(blocked(Blocked::FriendRequest), Some(Some(2000)));
        assert_eq!(blocked(Blocked::Stranger), None);
        assert_eq!(blocked(Blocked::Message), None);
        assert_eq!(
            online.restrictions.blocking(Blocked::FriendRequest, 2000),
            None
        );
    }

    #[test]
    fn a_refused_message_to_a_stranger_blocks_strangers_only() {
        let mut online = Online::default();
        signed_in(&mut online);
        // Nobody is a friend, so opening the conversation is writing to a stranger.
        let id = online.send(
            Purpose::OpenConversation(AccountId(7)),
            Command::OpenConversation {
                accounts: vec![AccountId(7)],
                name: None,
            },
        );
        refusal(&mut online, id, restricted("new_account", Some(2000)));
        assert!(online.restrictions.blocking(Blocked::Stranger, 0).is_some());
        assert!(online.restrictions.blocking(Blocked::Message, 0).is_none());
    }

    #[test]
    fn the_mutes_the_service_lists_stop_every_action_and_a_new_one_is_announced() {
        let mut online = Online::default();
        signed_in(&mut online);
        let mute = |action, until| control::Restriction {
            action: Some(action),
            ..control::Restriction::new(control::Restriction::MUTED, Some(until))
        };
        let first = online.apply(event(Event::Restrictions {
            restrictions: vec![mute(1, 5000)],
        }));
        assert!(first.is_empty(), "known on going online: no announcement");
        assert!(
            online
                .restrictions
                .blocking(Blocked::FriendRequest, 10)
                .is_some()
        );
        let later = online.apply(event(Event::Restrictions {
            restrictions: vec![mute(1, 5000), mute(2, 9000)],
        }));
        assert_eq!(
            later,
            [Notice::Restricted(mute(2, 9000))],
            "a mute that began while the launcher ran"
        );
        online.apply(event(Event::Restrictions {
            restrictions: Vec::new(),
        }));
        assert!(online.restrictions.active(10).is_empty());
    }

    #[test]
    fn a_ban_is_kept_while_signed_out_and_ends_with_the_next_sign_in() {
        let mut online = Online::default();
        signed_in(&mut online);
        let ban = control::Restriction {
            reason: Some("Cheating in a game.".into()),
            ..control::Restriction::new(control::Restriction::BANNED, Some(9000))
        };
        let notices = online.apply(event(Event::Banned {
            restriction: ban.clone(),
        }));
        assert_eq!(notices, [Notice::Banned(ban.clone())]);
        // The net service then signs the account out.
        online.apply(event(Event::State {
            cloud: Cloud::Online,
            account: None,
            pending: None,
        }));
        assert!(!online.is_signed_in());
        assert_eq!(online.restrictions.banned(), Some(&ban));
        signed_in(&mut online);
        assert_eq!(online.restrictions.banned(), None);
    }

    #[test]
    fn a_ban_answered_to_a_command_is_kept_too() {
        let mut online = Online::default();
        signed_in(&mut online);
        let id = online.send(Purpose::Privacy, Command::Privacy);
        refusal(
            &mut online,
            id,
            Failure {
                restriction: Some(Box::new(control::Restriction::new(
                    control::Restriction::BANNED,
                    None,
                ))),
                ..Failure::new("banned", "x")
            },
        );
        assert!(online.restrictions.banned().is_some());
    }

    #[test]
    fn a_stranger_is_a_direct_conversation_with_someone_who_is_no_friend() {
        let mut online = Online::default();
        signed_in(&mut online);
        let direct = chat("a", 0, 0, false);
        assert!(online.is_stranger(&direct));
        online.friends.friends.push(api::Friend {
            account: account(2, "Lumen"),
            since: 0,
            presence: Default::default(),
        });
        assert!(!online.is_stranger(&direct));
    }

    #[test]
    fn answers_come_back_with_what_they_were_for() {
        let mut online = Online::default();
        let id = online.send(
            Purpose::BrowserSignIn { create: true },
            Command::BrowserSignIn { create: true },
        );
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"browser_sign_in\""));
        assert!(online.is_waiting(|p| matches!(p, Purpose::BrowserSignIn { .. })));
        let url = "https://cloud.example.org/web/authorize?x=1";
        let notices = online.apply(Line::Output(Output::Answer(control::Answer::ok(
            id,
            control::Opened { url: url.into() },
        ))));
        assert!(matches!(
            &notices[..],
            [Notice::Answered(
                Purpose::BrowserSignIn { create: true },
                Ok(_)
            )]
        ));
        assert_eq!(online.sign_in_url.as_deref(), Some(url));
        assert!(!online.is_waiting(|_| true));
        // A stray answer is ignored.
        assert!(
            online
                .apply(Line::Output(Output::Answer(control::Answer::ok(99, ()))))
                .is_empty()
        );
    }

    #[test]
    fn a_new_request_is_announced_once() {
        let mut online = Online::default();
        signed_in(&mut online);
        let lists = FriendLists {
            incoming: vec![Request {
                account: account(2, "Lumen"),
                since: 1,
            }],
            ..FriendLists::default()
        };
        let first = online.apply(event(Event::Friends(lists.clone())));
        assert_eq!(first, vec![Notice::Request(account(2, "Lumen"))]);
        assert!(online.apply(event(Event::Friends(lists))).is_empty());
    }

    #[test]
    fn signing_out_forgets_the_lists() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.apply(event(Event::Friends(FriendLists {
            friends: vec![Friend {
                account: account(2, "Lumen"),
                since: 1,
                presence: Presence::default(),
            }],
            ..FriendLists::default()
        })));
        assert!(online.is_online());
        online.apply(event(Event::State {
            cloud: Cloud::Online,
            account: None,
            pending: None,
        }));
        assert!(online.friends.friends.is_empty());
        assert!(!online.is_signed_in());
    }

    #[test]
    fn the_end_of_the_service_fails_what_waits() {
        let mut online = Online {
            status: Status::Running,
            ..Online::default()
        };
        online.send(Purpose::SignOut, Command::SignOut);
        let notices = online.apply(Line::Ended);
        assert!(matches!(
            &notices[0],
            Notice::Answered(Purpose::SignOut, Err(f)) if f.code == Failure::OFFLINE
        ));
        assert_eq!(notices.last(), Some(&Notice::Ended));
        assert!(matches!(online.status, Status::Ended(_)));
        // Numbers keep counting, so a late answer of the old process never matches a new command.
        assert_eq!(online.send(Purpose::Hello, Command::SignOut), 2);
    }

    fn answer(online: &mut Online, id: u64, value: impl serde::Serialize) {
        online.apply(Line::Output(Output::Answer(control::Answer::ok(id, value))));
    }

    fn earned(number: u32, grade: Grade, unlocked: i64) -> Earned {
        Earned {
            number,
            grade,
            unlocked,
        }
    }

    #[test]
    fn signing_in_asks_for_the_privacy_choice() {
        let mut online = Online::default();
        signed_in(&mut online);
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"privacy\""));
        answer(
            &mut online,
            1,
            api::Privacy {
                trophies: api::Audience::Everyone,
                ..api::Privacy::default()
            },
        );
        assert_eq!(
            online.privacy.map(|p| p.trophies),
            Some(api::Audience::Everyone)
        );
    }

    #[test]
    fn the_trophy_choice_keeps_the_other_parts_of_the_privacy() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        online.set_trophy_audience(api::Audience::Nobody);
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("\"do\":\"privacy\""),
            "it asks before it knows the rest"
        );
        let known = api::Privacy {
            about: api::Audience::Everyone,
            ..api::Privacy::default()
        };
        answer(&mut online, 1, known);
        online.set_trophy_audience(api::Audience::Nobody);
        let (_, command) = control::parse_command(&online.take_outbox()[0]).unwrap();
        assert_eq!(
            command,
            Command::SetPrivacy {
                privacy: api::Privacy {
                    trophies: api::Audience::Nobody,
                    ..known
                },
            }
        );
    }

    #[test]
    fn playtime_goes_up_per_title_and_day_once_it_grew() {
        let days = BTreeMap::from([
            (
                "d:/a".to_owned(),
                BTreeMap::from([
                    ("2026-10-24".to_owned(), 600),
                    ("2026-10-25".to_owned(), 59),
                ]),
            ),
            (
                "e:/a-copy".to_owned(),
                BTreeMap::from([("2026-10-24".to_owned(), 130)]),
            ),
            (
                "d:/b".to_owned(),
                BTreeMap::from([("2026-10-24".to_owned(), 200_000)]),
            ),
            (
                "d:/gone".to_owned(),
                BTreeMap::from([("2026-10-24".to_owned(), 600)]),
            ),
        ]);
        let title = |game: &str| match game {
            "d:/a" | "e:/a-copy" => Some("GAME00001".to_owned()),
            "d:/b" => Some("GAME00002".to_owned()),
            _ => None,
        };
        let entries = playtime_entries(&days, title);
        let entry = |title: &str, day: &str, minutes| api::PlaytimeEntry {
            title_id: title.into(),
            day: day.into(),
            minutes,
        };
        assert_eq!(
            entries,
            [
                entry("GAME00001", "2026-10-24", 12),
                entry("GAME00002", "2026-10-24", api::MAX_DAY_MINUTES),
            ],
            "copies add up, minutes are whole and capped, a removed game is left out"
        );

        let mut online = Online::default();
        online.upload_playtime(entries.clone());
        assert!(
            online.take_outbox().is_empty(),
            "nothing goes up signed out"
        );
        signed_in(&mut online);
        online.take_outbox();
        online.upload_playtime(entries.clone());
        online.upload_playtime(entries.clone());
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1, "a second call waits for the first");
        answer(&mut online, 2, api::PlaytimeAdded { changed: 2 });
        online.upload_playtime(entries.clone());
        assert!(online.take_outbox().is_empty(), "the cloud has these");
        online.upload_playtime(vec![entry("GAME00001", "2026-10-24", 13)]);
        assert_eq!(online.take_outbox().len(), 1, "a minute more goes up");
    }

    #[test]
    fn trophies_go_up_once_for_each_new_count() {
        let mut online = Online::default();
        let first = [
            earned(0, Grade::Platinum, 3_500_000),
            earned(7, Grade::Silver, 1),
        ];
        online.upload_trophies("GAME01234", &first);
        assert!(
            online.take_outbox().is_empty(),
            "nothing goes up signed out"
        );
        signed_in(&mut online);
        online.take_outbox();

        online.upload_trophies("GAME01234", &first);
        online.upload_trophies("GAME01234", &first);
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1, "a second call waits for the first");
        let (_, command) = control::parse_command(&lines[0]).unwrap();
        assert_eq!(
            command,
            Command::UploadTrophies {
                title_id: "GAME01234".into(),
                trophies: vec![
                    api::Trophy {
                        number: 0,
                        grade: api::Grade::Platinum,
                        earned: 3,
                    },
                    api::Trophy {
                        number: 7,
                        grade: api::Grade::Silver,
                        earned: 0,
                    },
                ],
            }
        );
        answer(&mut online, 2, api::TrophiesAdded { added: 2 });
        online.upload_trophies("GAME01234", &first);
        assert!(online.take_outbox().is_empty(), "the cloud has these");
        online.upload_trophies(
            "GAME01234",
            &[first[0], first[1], earned(9, Grade::Bronze, 1)],
        );
        assert_eq!(online.take_outbox().len(), 1);
        online.upload_trophies("../admin", &first);
        assert!(
            online.take_outbox().is_empty(),
            "a title ID goes into a path"
        );
    }

    #[test]
    fn friends_get_their_cards_once_and_hidden_ones_stay_none() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        let friend = |id, handle: &str| Friend {
            account: account(id, handle),
            since: 0,
            presence: Presence::default(),
        };
        let lists = FriendLists {
            friends: vec![friend(2, "Lumen"), friend(3, "Bea")],
            ..FriendLists::default()
        };
        online.apply(event(Event::Friends(lists.clone())));
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"cards\""));
        let count = api::Count {
            bronze: 3,
            silver: 2,
            gold: 1,
            platinum: 0,
        };
        answer(
            &mut online,
            2,
            api::Cards {
                cards: vec![
                    api::Card {
                        account: account(2, "Lumen"),
                        trophies: Some(count),
                        avatar: None,
                    },
                    api::Card {
                        account: account(3, "Bea"),
                        trophies: None,
                        avatar: None,
                    },
                ],
            },
        );
        assert_eq!(online.cards.get(&AccountId(2)), Some(&Some(count)));
        assert_eq!(online.cards.get(&AccountId(3)), Some(&None));
        online.apply(event(Event::Friends(lists)));
        assert!(online.take_outbox().is_empty(), "every friend has a card");
        online.request_cards(true);
        assert_eq!(online.take_outbox().len(), 1, "a refresh asks for all");
    }

    #[test]
    fn friends_who_play_come_first() {
        use api::SessionRef;
        let mut online = Online::default();
        let friend = |id, handle: &str, online: bool, playing: bool, joinable: bool| Friend {
            account: account(id, handle),
            since: 0,
            presence: Presence {
                online,
                activity: playing.then(|| Activity {
                    title_id: "TEST00001".into(),
                    title: "Test".into(),
                    version: "01.000".into(),
                    session: joinable.then(|| SessionRef {
                        id: "s".into(),
                        joinable: true,
                        members: 1,
                        max_members: 4,
                    }),
                }),
                revision: 0,
            },
        };
        online.friends.friends = vec![
            friend(2, "zed", false, false, false),
            friend(3, "Bea", true, false, false),
            friend(4, "amy", true, true, false),
            friend(5, "Abe", false, false, false),
            friend(6, "Zoe", true, true, true),
        ];
        let order: Vec<&str> = online
            .sorted_friends()
            .iter()
            .map(|f| f.account.handle.as_str())
            .collect();
        assert_eq!(order, ["Zoe", "amy", "Bea", "Abe", "zed"]);
    }

    fn chat(id: &str, last: u64, read: u64, request: bool) -> api::Conversation {
        api::Conversation {
            id: id.into(),
            kind: api::ConversationKind::Direct,
            name: None,
            members: vec![account(1, "Alec"), account(2, "Lumen")],
            last,
            read,
            marks: Vec::new(),
            updated: last,
            request,
            party: None,
        }
    }

    fn texts(id: &str, from: u64, to: u64, sender: u64) -> api::Messages {
        api::Messages {
            messages: (from..=to)
                .map(|seq| api::Message {
                    conversation: id.into(),
                    seq,
                    sender: account(sender, "x"),
                    text: format!("m{seq}"),
                    created: seq,
                })
                .collect(),
            last: to,
        }
    }

    fn list(
        online: &mut Online,
        conversations: Vec<api::Conversation>,
        requests: Vec<api::Conversation>,
    ) -> Vec<Notice> {
        online.apply(event(Event::Conversations(api::Conversations {
            conversations,
            requests,
        })))
    }

    /// Answers the newest command in the outbox and returns the notices.
    fn answer_last(online: &mut Online, value: impl serde::Serialize) -> Vec<Notice> {
        let id = online.next;
        online.apply(Line::Output(Output::Answer(control::Answer::ok(id, value))))
    }

    #[test]
    fn a_new_message_is_fetched_after_the_number_held_and_announced_once() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        list(&mut online, vec![chat("c", 4, 4, false)], vec![]);
        online.open_thread("c");
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"messages\"") && lines[0].contains("\"after\":null"));
        let notices = answer_last(&mut online, texts("c", 1, 4, 2));
        assert!(
            !notices.iter().any(|n| matches!(n, Notice::Message { .. })),
            "opening a conversation is not news"
        );

        // A message arrives while the panel shows something else.
        online.apply(event(Event::Message {
            conversation: "c".into(),
            seq: 5,
        }));
        assert_eq!(
            online.social.unread("c"),
            1,
            "the mark shows before the text is here"
        );
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"after\":4") && lines[0].contains("\"limit\":100"));
        // A second event for the same number does not send a second fetch.
        online.apply(event(Event::Message {
            conversation: "c".into(),
            seq: 5,
        }));
        assert!(online.take_outbox().is_empty());
        let notices = answer_last(&mut online, texts("c", 5, 5, 2));
        let announced: Vec<_> = notices
            .iter()
            .filter_map(|n| match n {
                Notice::Message { message, .. } => Some(message.seq),
                _ => None,
            })
            .collect();
        assert_eq!(announced, [5]);
        assert_eq!(online.social.messages("c").len(), 5);

        // The player's own messages, from another device, are no news.
        online.apply(event(Event::Message {
            conversation: "c".into(),
            seq: 6,
        }));
        online.take_outbox();
        let notices = answer_last(&mut online, texts("c", 6, 6, 1));
        assert!(!notices.iter().any(|n| matches!(n, Notice::Message { .. })));
    }

    #[test]
    fn a_message_for_an_unknown_conversation_reads_the_list() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        online.apply(event(Event::Message {
            conversation: "new".into(),
            seq: 1,
        }));
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"conversations\""));
        online.apply(event(Event::Conversation { id: "new".into() }));
        assert!(
            online.take_outbox().is_empty(),
            "the question waits already"
        );
    }

    #[test]
    fn after_a_reconnect_loaded_conversations_catch_up_silently() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        list(
            &mut online,
            vec![chat("a", 3, 3, false), chat("b", 2, 2, false)],
            vec![],
        );
        online.open_thread("a");
        online.take_outbox();
        answer_last(&mut online, texts("a", 1, 3, 2));
        // The connection came back with news in both conversations.
        let notices = list(
            &mut online,
            vec![chat("a", 5, 3, false), chat("b", 4, 2, false)],
            vec![],
        );
        assert!(notices.is_empty());
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1, "b was never opened: its count is enough");
        assert!(lines[0].contains("\"after\":3"));
        let notices = answer_last(&mut online, texts("a", 4, 5, 2));
        assert!(
            !notices.iter().any(|n| matches!(n, Notice::Message { .. })),
            "a recovery does not toast every missed message"
        );
        assert_eq!(online.social.unread("a"), 2);
        assert_eq!(online.social.unread("b"), 2);
        assert_eq!(online.social.unread_total(), 4);
    }

    #[test]
    fn a_stranger_is_announced_after_the_first_list_only() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        let first = list(&mut online, vec![], vec![chat("r1", 1, 0, true)]);
        assert!(first.is_empty());
        online.apply(event(Event::MessageRequest {
            conversation: "r2".into(),
        }));
        assert!(online.take_outbox()[0].contains("\"do\":\"conversations\""));
        let notices = answer_last(
            &mut online,
            api::Conversations {
                conversations: vec![],
                requests: vec![chat("r2", 1, 0, true), chat("r1", 1, 0, true)],
            },
        );
        let announced: Vec<_> = notices
            .iter()
            .filter_map(|n| match n {
                Notice::MessageRequest(c) => Some(c.id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(announced, ["r2"]);
    }

    #[test]
    fn the_party_follows_its_revision_and_a_declined_invitation_keeps_it() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        let party = |revision, leader| api::Party {
            id: "p".into(),
            leader: AccountId(leader),
            members: vec![api::PartyMember {
                account: account(1, "Alec"),
                joined: 0,
                endpoint: String::new(),
                relay: None,
                voice: false,
            }],
            max_members: 8,
            conversation: "pc".into(),
            created: 0,
            revision,
        };
        online.apply(event(Event::Party {
            party: Some(party(2, 1)),
        }));
        assert!(
            online.take_outbox()[0].contains("\"do\":\"conversations\""),
            "the party's chat is not in the list yet"
        );
        online.apply(event(Event::Party {
            party: Some(party(1, 9)),
        }));
        assert_eq!(online.social.party.as_ref().unwrap().leader, AccountId(1));

        let id = online.send(
            Purpose::PartyAnswer {
                id: "i".into(),
                accept: false,
            },
            Command::PartyAnswer {
                id: "i".into(),
                accept: false,
            },
        );
        online.apply(Line::Output(Output::Answer(control::Answer::ok(id, ()))));
        assert!(
            online.social.party.is_some(),
            "declining answers an empty object and changes nothing"
        );

        let invitation = api::PartyInvitation {
            id: "i2".into(),
            from: account(2, "Lumen"),
            party: "p9".into(),
            members: 2,
            expires: 0,
        };
        let notices = online.apply(event(Event::PartyInvitation {
            invitation: invitation.clone(),
        }));
        assert_eq!(notices, vec![Notice::PartyInvitation(invitation)]);
        online.apply(event(Event::PartyInvitationGone { id: "i2".into() }));
        assert!(online.social.party_invitations.is_empty());

        online.apply(event(Event::Party { party: None }));
        assert!(online.social.party.is_none());
    }

    #[test]
    fn signing_out_forgets_the_party_and_the_messages() {
        let mut online = Online::default();
        signed_in(&mut online);
        list(&mut online, vec![chat("c", 1, 0, false)], vec![]);
        online.social.set_draft("c", "hi".into());
        online.apply(event(Event::State {
            cloud: Cloud::Online,
            account: None,
            pending: None,
        }));
        assert_eq!(online.social, Social::default());
    }

    #[test]
    fn a_draft_is_sent_once_and_leaves_when_the_cloud_took_it() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        list(&mut online, vec![chat("c", 2, 2, false)], vec![]);
        online.open_thread("c");
        online.take_outbox();
        answer_last(&mut online, texts("c", 1, 2, 2));
        assert!(!online.send_draft("c"), "nothing to send");
        online.social.set_draft("c", "  \n ".into());
        assert!(!online.send_draft("c"), "only white space");
        online.social.set_draft("c", "On my way".into());
        assert!(online.send_draft("c"));
        assert!(!online.send_draft("c"), "it is on its way");
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"send_message\"") && lines[0].contains("On my way"));
        // A refusal keeps the text for another try.
        let id = online.next;
        online.apply(Line::Output(Output::Answer(control::Answer::error(
            id,
            Failure::new("restricted", "New account."),
        ))));
        assert_eq!(online.social.draft("c"), "On my way");
        assert!(online.send_draft("c"));
        let notices = answer_last(
            &mut online,
            api::Message {
                conversation: "c".into(),
                seq: 3,
                sender: account(1, "Alec"),
                text: "On my way".into(),
                created: 3,
            },
        );
        assert!(!notices.iter().any(|n| matches!(n, Notice::Message { .. })));
        assert_eq!(online.social.draft("c"), "");
        assert_eq!(online.social.messages("c").last().unwrap().seq, 3);
        assert_eq!(
            online.social.unread("c"),
            0,
            "writing reads up to the message"
        );
    }

    #[test]
    fn looking_at_a_conversation_marks_it_read_once() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        list(&mut online, vec![chat("c", 3, 0, false)], vec![]);
        online.mark_read("c");
        assert!(online.take_outbox().is_empty(), "nothing is held to mark");
        online.open_thread("c");
        online.take_outbox();
        answer_last(&mut online, texts("c", 1, 3, 2));
        online.mark_read("c");
        online.mark_read("c");
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"mark_read\"") && lines[0].contains("\"seq\":3"));
        assert_eq!(online.social.unread("c"), 0);
    }

    #[test]
    fn a_conversation_with_a_friend_is_opened_once() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        online.open_direct(AccountId(2));
        online.open_direct(AccountId(2));
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"do\":\"open_conversation\""));
        answer_last(&mut online, chat("c", 0, 0, false));
        assert_eq!(online.social.direct_with(AccountId(2)).unwrap().id, "c");
        online.open_direct(AccountId(2));
        assert!(online.take_outbox().is_empty());
    }

    #[test]
    fn a_cloud_is_reached_over_https_or_on_this_pc() {
        assert!(is_cloud_address("https://cloud.example.org"));
        assert!(is_cloud_address(" https://cloud.example.org:8443/ "));
        assert!(is_cloud_address("http://127.0.0.1:8080"));
        assert!(is_cloud_address("http://localhost:8080"));
        assert!(is_cloud_address("http://[::1]:8080"));
        assert!(!is_cloud_address("http://cloud.example.org"));
        assert!(!is_cloud_address("http://192.168.1.4:8080"));
        assert!(!is_cloud_address("https://"));
        assert!(!is_cloud_address("cloud.example.org"));
        assert!(!is_cloud_address("https://a b.org"));
        assert_eq!(
            cloud(Some("https://mine.example.org")),
            Some("https://mine.example.org")
        );
        assert_eq!(cloud(None), DEFAULT_CLOUD.filter(|u| !u.is_empty()));
    }

    #[test]
    fn a_game_gets_only_the_bridge_variables() {
        let mut bridge = BTreeMap::new();
        bridge.insert(
            "ANYPS5_NET_ENDPOINT".to_owned(),
            "127.0.0.1:5000".to_owned(),
        );
        bridge.insert("ANYPS5_NET_TOKEN".to_owned(), "ab".to_owned());
        bridge.insert("PATH".to_owned(), "nope".to_owned());
        let env = game_environment(Some(&bridge), Some(Path::new("C:/Logs/X-network.jsonl")));
        assert_eq!(env.len(), 3);
        assert!(!env.contains_key("PATH"));
        assert_eq!(env["ANYPS5_NET_OBSERVE"], "C:/Logs/X-network.jsonl");
        assert!(game_environment(None, None).is_empty());
    }

    fn feed_item(id: u64) -> api::FeedItem {
        api::FeedItem {
            id,
            account: account(2, "Lumen"),
            at: 1_000 + id,
            event: api::FeedEvent::FirstPlayed {
                title_id: "GAME00001".into(),
            },
            reactions: api::ReactionCounts::default(),
            mine: None,
        }
    }

    fn feed_page(ids: &[u64], next: Option<u64>) -> api::Feed {
        api::Feed {
            items: ids.iter().map(|id| feed_item(*id)).collect(),
            next,
        }
    }

    /// A signed-in player whose feed holds items 3, 2 and 1, with nothing waiting.
    fn with_feed() -> Online {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        online.request_feed();
        answer_last(&mut online, feed_page(&[3, 2, 1], Some(1)));
        online.take_outbox();
        online
    }

    #[test]
    fn the_feed_is_asked_for_once_and_kept() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        online.request_feed();
        online.request_feed();
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1, "the question waits already");
        assert!(lines[0].contains("\"do\":\"feed\""));
        assert!(online.feed_loading());
        answer_last(&mut online, feed_page(&[3, 2, 1], Some(1)));
        assert!(!online.feed_loading() && online.feed.loaded);
        assert_eq!(online.feed.items.len(), 3);
        // The next page starts before the oldest item held.
        online.request_older_feed();
        let lines = online.take_outbox();
        assert!(lines[0].contains("\"before\":1"), "{}", lines[0]);
        answer_last(&mut online, feed_page(&[0], None));
        assert_eq!(online.feed.next, None);
        online.request_older_feed();
        assert!(online.take_outbox().is_empty(), "nothing older");
    }

    #[test]
    fn a_failed_feed_says_so_until_it_loads() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        online.request_feed();
        refuse_last(&mut online, Failure::new("unavailable", "x"));
        assert!(online.feed.error.is_some() && !online.feed.loaded);
        online.request_feed();
        answer_last(&mut online, feed_page(&[], None));
        assert!(online.feed.error.is_none() && online.feed.loaded);
    }

    #[test]
    fn a_reaction_shows_at_once_and_goes_back_when_refused() {
        let mut online = with_feed();
        online.react(2, Some(api::Reaction::Gg));
        assert_eq!(online.feed.item(2).unwrap().reactions.gg, 1);
        assert_eq!(online.feed.item(2).unwrap().mine, Some(api::Reaction::Gg));
        let lines = online.take_outbox();
        assert!(lines[0].contains("\"do\":\"react\"") && lines[0].contains("\"item\":2"));
        assert!(lines[0].contains("\"reaction\":\"gg\""));
        refuse_last(&mut online, Failure::new("rate_limited", "x"));
        assert_eq!(online.feed.item(2).unwrap().reactions.gg, 0);
        assert_eq!(online.feed.item(2).unwrap().mine, None);
    }

    #[test]
    fn the_answer_to_a_reaction_is_what_the_item_shows() {
        let mut online = with_feed();
        online.react(2, Some(api::Reaction::Wow));
        online.take_outbox();
        // Somebody else reacted in the meantime: the cloud's counts win.
        answer_last(
            &mut online,
            api::Reactions {
                reactions: api::ReactionCounts {
                    wow: 4,
                    ..Default::default()
                },
                mine: Some(api::Reaction::Wow),
            },
        );
        assert_eq!(online.feed.item(2).unwrap().reactions.wow, 4);
        // Taking it back sends null.
        online.react(2, None);
        let lines = online.take_outbox();
        assert!(lines[0].contains("\"reaction\":null"), "{}", lines[0]);
        assert_eq!(online.feed.item(2).unwrap().reactions.wow, 3);
    }

    #[test]
    fn a_second_press_waits_for_the_first_on_the_same_item() {
        let mut online = with_feed();
        online.react(2, Some(api::Reaction::Gg));
        online.react(2, Some(api::Reaction::Wow));
        online.react(3, Some(api::Reaction::Nice));
        assert_eq!(online.take_outbox().len(), 2, "one per item on its way");
        assert_eq!(online.feed.item(2).unwrap().mine, Some(api::Reaction::Gg));
        // An item the player does not hold is not sent at all.
        online.react(99, Some(api::Reaction::Nice));
        assert!(online.take_outbox().is_empty());
    }

    #[test]
    fn news_reads_the_feed_again_only_once_it_was_looked_at() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        online.apply(event(Event::Feed));
        assert!(online.take_outbox().is_empty(), "nobody looked");
        let mut online = with_feed();
        online.apply(event(Event::Feed));
        online.apply(event(Event::Feed));
        let lines = online.take_outbox();
        assert_eq!(lines.len(), 1, "one question at a time");
        assert!(lines[0].contains("\"do\":\"feed\""));
    }

    #[test]
    fn a_profile_page_teaches_the_avatar_and_a_failure_is_remembered() {
        let mut online = Online::default();
        signed_in(&mut online);
        online.take_outbox();
        online.request_profile(AccountId(2));
        online.request_profile(AccountId(2));
        assert_eq!(online.take_outbox().len(), 1);
        let hash = "ab".repeat(32);
        answer_last(
            &mut online,
            api::ProfilePage {
                profile: api::Profile {
                    account: account(2, "Lumen"),
                    about: Some("Hi".into()),
                    favourites: None,
                    avatar: Some(hash.clone()),
                    playtime: None,
                },
                trophies: None,
                links: Vec::new(),
            },
        );
        let page = page_of(&online.profiles, AccountId(2)).unwrap();
        assert_eq!(page.profile.about.as_deref(), Some("Hi"));
        assert_eq!(online.avatars[&AccountId(2)], Some(hash));
        // A later failure keeps the page that is shown and does not mark it failed.
        online.request_profile(AccountId(2));
        refuse_last(&mut online, Failure::new("unavailable", "x"));
        assert!(online.profiles[&AccountId(2)].page.is_some());
        online.request_profile(AccountId(7));
        refuse_last(&mut online, Failure::new("not_found", "x"));
        assert!(online.profiles[&AccountId(7)].failed);
    }

    #[test]
    fn signing_out_forgets_the_feed_and_the_profiles() {
        let mut online = with_feed();
        online.request_profile(AccountId(2));
        online.apply(event(Event::State {
            cloud: Cloud::Online,
            account: None,
            pending: None,
        }));
        assert!(!online.feed.loaded && online.feed.items.is_empty());
        assert!(online.profiles.is_empty() && online.avatars.is_empty());
    }
}
