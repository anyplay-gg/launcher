//! What the launcher knows about the player's party and conversations (network `docs/SOCIAL.md`,
//! S2). Plain data, updated from the net service's events and answers; it sends nothing itself.
//! It says which messages to fetch ([`Fetch`]) and the caller turns that into commands.
//!
//! Messages are numbered per conversation (network ADR 0013): the launcher keeps the highest
//! number it holds and fetches what comes after it. Message text is untrusted data. It is kept as
//! it came and never interpreted here: no links, no markup, nothing that loads.

use std::collections::BTreeMap;

use anyplay_control::Command;

use super::api::{
    Account, AccountId, Conversation, ConversationKind, Conversations, Message, Messages, Party,
    PartyInvitation,
};

/// How many messages of a conversation stay in memory.
pub const KEPT: usize = 200;

/// How many messages a fetch asks for to fill a conversation or to read further back.
pub const PAGE: u32 = 50;

/// How many messages a fetch after a known number asks for: the most the cloud gives.
pub const CATCH_UP: u32 = super::api::MAX_MESSAGES_PAGE;

/// Which messages of a conversation to fetch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fetch {
    /// The newest ones: the first look at a conversation.
    Newest,
    /// The ones after this number.
    After(u64),
    /// The newest ones below this number.
    Before(u64),
}

impl Fetch {
    /// How many messages the fetch asks for.
    pub fn limit(self) -> u32 {
        match self {
            Fetch::After(_) => CATCH_UP,
            Fetch::Newest | Fetch::Before(_) => PAGE,
        }
    }

    /// The command that fetches this from `conversation`.
    pub fn command(self, conversation: &str) -> Command {
        let (after, before) = match self {
            Fetch::Newest => (None, None),
            Fetch::After(n) => (Some(n), None),
            Fetch::Before(n) => (None, Some(n)),
        };
        Command::Messages {
            conversation: conversation.to_owned(),
            after,
            before,
            limit: Some(self.limit()),
        }
    }
}

/// The messages of one conversation as far as the launcher holds them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Thread {
    /// Oldest first, numbers rising; at most [`KEPT`].
    messages: Vec<Message>,
    /// The newest number accounted for: fetched, or known to be hidden by a block.
    highest: u64,
    /// The newest messages were fetched once, so the thread follows the conversation by `After`.
    loaded: bool,
    /// There may be messages older than the first one held.
    older: bool,
}

/// What a page of messages changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ingested {
    /// Messages that were not held before and are newer than what was.
    pub new: Vec<Message>,
    /// A further fetch, when the conversation is still ahead of the thread.
    pub follow: Option<Fetch>,
}

/// What a fresh list of conversations asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listed {
    /// Loaded threads that are behind their conversation.
    pub fetches: Vec<(String, Fetch)>,
    /// Message requests that were not in the list before. Empty for the first list.
    pub new_requests: Vec<Conversation>,
}

/// The party, the conversations and their messages.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Social {
    pub party: Option<Party>,
    pub party_invitations: Vec<PartyInvitation>,
    conversations: Vec<Conversation>,
    requests: Vec<Conversation>,
    threads: BTreeMap<String, Thread>,
    drafts: BTreeMap<String, String>,
    /// The cloud's list was read at least once.
    listed: bool,
}

impl Social {
    // The party

    /// Keeps `party` unless it is an older copy of the party held. Returns whether it changed.
    pub fn apply_party(&mut self, party: Option<Party>) -> bool {
        if let (Some(held), Some(new)) = (&self.party, &party)
            && held.id == new.id
            && new.revision < held.revision
        {
            return false;
        }
        let changed = self.party != party;
        self.party = party;
        changed
    }

    /// A party invitation; a newer one with the same ID replaces the old.
    pub fn add_party_invitation(&mut self, invitation: PartyInvitation) {
        self.party_invitations.retain(|i| i.id != invitation.id);
        self.party_invitations.push(invitation);
    }

    pub fn drop_party_invitation(&mut self, id: &str) {
        self.party_invitations.retain(|i| i.id != id);
    }

    /// True when `account` is a member of the party.
    pub fn in_party(&self, account: AccountId) -> bool {
        self.party
            .as_ref()
            .is_some_and(|p| p.members.iter().any(|m| m.account.id == account))
    }

    /// True when the player leads the party.
    pub fn leads(&self, me: AccountId) -> bool {
        self.party.as_ref().is_some_and(|p| p.leader == me)
    }

    // Conversations

    /// Replaces the lists with the cloud's. Threads that are still listed keep their messages;
    /// the rest are forgotten. Read numbers only move forward.
    pub fn set_conversations(&mut self, lists: Conversations) -> Listed {
        let known_requests: Vec<String> = self.requests.iter().map(|c| c.id.clone()).collect();
        let held: BTreeMap<String, u64> = self
            .conversations
            .iter()
            .chain(&self.requests)
            .map(|c| (c.id.clone(), c.read))
            .collect();
        let keep_read = |mut c: Conversation| {
            if let Some(read) = held.get(&c.id) {
                c.read = c.read.max((*read).min(c.last));
            }
            c
        };
        self.conversations = lists.conversations.into_iter().map(keep_read).collect();
        self.requests = lists.requests.into_iter().map(keep_read).collect();
        let first = !self.listed;
        self.listed = true;
        let ids: Vec<&String> = self
            .conversations
            .iter()
            .chain(&self.requests)
            .map(|c| &c.id)
            .collect();
        self.threads.retain(|id, _| ids.contains(&id));
        self.drafts.retain(|id, _| ids.contains(&id));
        let mut listed = Listed::default();
        for conversation in self.conversations.iter().chain(&self.requests) {
            if let Some(thread) = self.threads.get(&conversation.id)
                && thread.loaded
                && conversation.last > thread.highest
            {
                listed
                    .fetches
                    .push((conversation.id.clone(), Fetch::After(thread.highest)));
            }
        }
        if !first {
            listed.new_requests = self
                .requests
                .iter()
                .filter(|c| !known_requests.contains(&c.id))
                .cloned()
                .collect();
        }
        listed
    }

    /// Puts one conversation, as the cloud answered it, in the right list.
    pub fn upsert(&mut self, conversation: Conversation) {
        let read = self.get(&conversation.id).map_or(0, |c| c.read);
        let mut conversation = conversation;
        conversation.read = conversation.read.max(read.min(conversation.last));
        self.conversations.retain(|c| c.id != conversation.id);
        self.requests.retain(|c| c.id != conversation.id);
        if conversation.request {
            self.requests.insert(0, conversation);
        } else {
            self.conversations.insert(0, conversation);
        }
    }

    /// The player is no longer in the conversation.
    pub fn remove(&mut self, id: &str) {
        self.conversations.retain(|c| c.id != id);
        self.requests.retain(|c| c.id != id);
        self.threads.remove(id);
        self.drafts.remove(id);
    }

    pub fn get(&self, id: &str) -> Option<&Conversation> {
        self.conversations
            .iter()
            .chain(&self.requests)
            .find(|c| c.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut Conversation> {
        self.conversations
            .iter_mut()
            .chain(self.requests.iter_mut())
            .find(|c| c.id == id)
    }

    /// The conversations apart from message requests: those with unread messages first, then the
    /// newest.
    pub fn conversations(&self) -> Vec<&Conversation> {
        let mut list: Vec<&Conversation> = self.conversations.iter().collect();
        list.sort_by_key(|c| (self.unread_of(c) == 0, std::cmp::Reverse(c.updated)));
        list
    }

    /// The message requests, newest first.
    pub fn requests(&self) -> &[Conversation] {
        &self.requests
    }

    /// The direct conversation with `account`.
    pub fn direct_with(&self, account: AccountId) -> Option<&Conversation> {
        self.conversations.iter().find(|c| {
            c.kind == ConversationKind::Direct && c.members.iter().any(|m| m.id == account)
        })
    }

    /// The party's chat, once the conversation list has it.
    pub fn party_chat(&self) -> Option<&Conversation> {
        let id = &self.party.as_ref()?.conversation;
        self.get(id)
    }

    // Unread

    fn unread_of(&self, c: &Conversation) -> u64 {
        c.last.saturating_sub(c.read)
    }

    /// How many messages of the conversation the player has not read: an upper bound, since a
    /// block can hide some of the numbers.
    pub fn unread(&self, id: &str) -> u64 {
        self.get(id).map_or(0, |c| self.unread_of(c))
    }

    /// Unread messages in all conversations, those of message requests included: one number for
    /// the profile button, the Messages row and the Messages page.
    pub fn unread_total(&self) -> u64 {
        self.conversations
            .iter()
            .chain(&self.requests)
            .map(|c| self.unread_of(c))
            .sum()
    }

    /// The player read the conversation up to `seq`. Returns true when that moved the mark.
    pub fn mark_read(&mut self, id: &str, seq: u64) -> bool {
        match self.get_mut(id) {
            Some(c) if seq > c.read && c.read < c.last => {
                c.read = seq.min(c.last).max(c.read);
                true
            }
            _ => false,
        }
    }

    /// The number to mark as read when the player looks at the conversation: the newest one held.
    pub fn read_target(&self, id: &str) -> Option<u64> {
        let conversation = self.get(id)?;
        let thread = self.threads.get(id)?;
        (thread.loaded && thread.highest > conversation.read).then_some(thread.highest)
    }

    /// Somebody read up to `seq`. The player's own mark moves the unread count; another member's
    /// mark is kept for "Seen".
    pub fn apply_read(&mut self, id: &str, account: AccountId, seq: u64, me: AccountId) {
        if account == me {
            self.mark_read(id, seq);
            return;
        }
        if let Some(c) = self.get_mut(id) {
            match c.marks.iter_mut().find(|m| m.account == account) {
                Some(mark) => mark.seq = mark.seq.max(seq),
                None => c.marks.push(super::api::ReadMark { account, seq }),
            }
        }
    }

    // Messages

    /// A message with this number exists. Returns false for a conversation the list lacks, which
    /// the caller answers by reading the list again.
    pub fn note_last(&mut self, id: &str, seq: u64) -> bool {
        match self.get_mut(id) {
            Some(c) => {
                c.last = c.last.max(seq);
                true
            }
            None => false,
        }
    }

    /// The messages held, oldest first.
    pub fn messages(&self, id: &str) -> &[Message] {
        self.threads.get(id).map_or(&[], |t| &t.messages)
    }

    /// The thread was fetched at least once.
    pub fn is_loaded(&self, id: &str) -> bool {
        self.threads.get(id).is_some_and(|t| t.loaded)
    }

    /// There may be messages older than the first one held, and room to hold them.
    pub fn has_earlier(&self, id: &str) -> bool {
        self.threads
            .get(id)
            .is_some_and(|t| t.loaded && t.older && t.messages.len() < KEPT)
    }

    /// What to fetch to bring the thread up to date: the newest messages for a thread never
    /// fetched, else what came after the newest number held. `None` when it is current.
    pub fn plan(&self, id: &str) -> Option<Fetch> {
        let conversation = self.get(id)?;
        match self.threads.get(id) {
            Some(t) if t.loaded => {
                (conversation.last > t.highest).then_some(Fetch::After(t.highest))
            }
            _ => (conversation.last > 0).then_some(Fetch::Newest),
        }
    }

    /// What to fetch to read further back, if there is anything.
    pub fn plan_earlier(&self, id: &str) -> Option<Fetch> {
        let thread = self.threads.get(id)?;
        let first = thread.messages.first()?.seq;
        self.has_earlier(id).then_some(Fetch::Before(first))
    }

    /// Takes a page of messages that answers `fetch`.
    pub fn ingest(&mut self, id: &str, fetch: Fetch, page: Messages) -> Ingested {
        let limit = fetch.limit() as usize;
        let full = page.messages.len() >= limit;
        let page_last = page.last;
        if let Some(c) = self.get_mut(id) {
            c.last = c.last.max(page_last);
        }
        let last = self.get(id).map_or(page_last, |c| c.last.max(page_last));
        let thread = self.threads.entry(id.to_owned()).or_default();
        let mut incoming = page.messages;
        incoming.sort_by_key(|m| m.seq);
        incoming.dedup_by_key(|m| m.seq);
        let mut new = Vec::new();
        match fetch {
            Fetch::Newest => {
                let held: Vec<u64> = thread.messages.iter().map(|m| m.seq).collect();
                new = incoming
                    .iter()
                    .filter(|m| !held.contains(&m.seq))
                    .cloned()
                    .collect();
                if incoming.len() > KEPT {
                    incoming.drain(..incoming.len() - KEPT);
                }
                thread.messages = incoming;
                thread.loaded = true;
                thread.older = full;
                thread.highest = page_last.max(thread.messages.last().map_or(0, |m| m.seq));
            }
            Fetch::After(_) => {
                let top = thread.messages.last().map_or(thread.highest, |m| m.seq);
                for message in incoming {
                    if message.seq > top {
                        new.push(message.clone());
                        thread.messages.push(message);
                    }
                }
                if thread.messages.len() > KEPT {
                    thread.messages.drain(..thread.messages.len() - KEPT);
                    thread.older = true;
                }
                let reached = thread.messages.last().map_or(0, |m| m.seq);
                thread.highest = if full {
                    thread.highest.max(reached)
                } else {
                    thread.highest.max(reached).max(page_last)
                };
            }
            Fetch::Before(_) => {
                let first = thread.messages.first().map_or(u64::MAX, |m| m.seq);
                let mut older: Vec<Message> =
                    incoming.into_iter().filter(|m| m.seq < first).collect();
                let room = KEPT.saturating_sub(thread.messages.len());
                let cut = older.len() > room;
                if cut {
                    older.drain(..older.len() - room);
                }
                older.append(&mut thread.messages);
                thread.messages = older;
                thread.older = full || cut;
            }
        }
        let highest = thread.highest;
        let follow = (thread.loaded && last > highest).then_some(Fetch::After(highest));
        Ingested { new, follow }
    }

    /// The player's own message, as the cloud answered it. Held when it is the next number;
    /// otherwise the answer is a fetch.
    pub fn ingest_sent(&mut self, message: Message) -> Ingested {
        let id = message.conversation.clone();
        if let Some(c) = self.get_mut(&id) {
            c.last = c.last.max(message.seq);
        }
        let last = self.get(&id).map_or(message.seq, |c| c.last);
        let Some(thread) = self.threads.get_mut(&id).filter(|t| t.loaded) else {
            return Ingested::default();
        };
        if message.seq == thread.highest + 1 {
            thread.highest = message.seq;
            thread.messages.push(message);
            if thread.messages.len() > KEPT {
                thread.messages.drain(..thread.messages.len() - KEPT);
                thread.older = true;
            }
        }
        Ingested {
            new: Vec::new(),
            follow: (last > thread.highest).then_some(Fetch::After(thread.highest)),
        }
    }

    // Drafts

    /// What the player typed and has not sent.
    pub fn draft(&self, id: &str) -> &str {
        self.drafts.get(id).map_or("", String::as_str)
    }

    pub fn set_draft(&mut self, id: &str, text: String) {
        if text.is_empty() {
            self.drafts.remove(id);
        } else {
            self.drafts.insert(id.to_owned(), text);
        }
    }

    /// Forgets the draft once the message it was sent as arrived, unless the player went on
    /// typing.
    pub fn sent(&mut self, id: &str, text: &str) {
        if self.draft(id) == text {
            self.drafts.remove(id);
        }
    }
}

/// Message text as it is shown. It stays text: control characters other than line breaks go (a tab
/// becomes a space), and so do the characters that reorder what is around them (bidirectional
/// overrides and isolates), which could make a sender's words read as somebody else's.
pub fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !(matches!(
                *c,
                '\u{061C}'
                    | '\u{2028}'
                    | '\u{2029}'
                    | '\u{200E}'
                    | '\u{200F}'
                    | '\u{202A}'..='\u{202E}'
                    | '\u{2066}'..='\u{2069}'
            ) || c.is_control() && *c != '\n' && *c != '\t')
        })
        .map(|c| if c == '\t' { ' ' } else { c })
        .collect()
}

/// The start of a message in one line, at most `max` characters, for a toast.
pub fn snippet(text: &str, max: usize) -> String {
    let line: String = plain(text).split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        return line;
    }
    let mut cut: String = line.chars().take(max.saturating_sub(1)).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// The other accounts of a conversation: everyone but `me`.
pub fn others(conversation: &Conversation, me: AccountId) -> Vec<&Account> {
    conversation.members.iter().filter(|m| m.id != me).collect()
}

/// When the other member of a direct conversation read up to: their mark, if they have one.
pub fn read_by_other(conversation: &Conversation, me: AccountId) -> Option<u64> {
    conversation
        .marks
        .iter()
        .find(|m| m.account != me)
        .map(|m| m.seq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::api::{PartyMember, ReadMark};

    fn account(id: u64, handle: &str) -> Account {
        Account {
            id: AccountId(id),
            handle: handle.into(),
        }
    }

    fn conversation(id: &str, last: u64, read: u64) -> Conversation {
        Conversation {
            id: id.into(),
            kind: ConversationKind::Direct,
            name: None,
            members: vec![account(1, "Mira"), account(2, "Lumen")],
            last,
            read,
            marks: Vec::new(),
            updated: last,
            request: false,
            party: None,
        }
    }

    fn message(id: &str, seq: u64, sender: u64) -> Message {
        Message {
            conversation: id.into(),
            seq,
            sender: account(sender, "x"),
            text: format!("m{seq}"),
            created: 1_000 + seq,
        }
    }

    fn page(id: &str, from: u64, to: u64, last: u64) -> Messages {
        Messages {
            messages: (from..=to).map(|n| message(id, n, 2)).collect(),
            last,
        }
    }

    fn lists(conversations: Vec<Conversation>, requests: Vec<Conversation>) -> Conversations {
        Conversations {
            conversations,
            requests,
        }
    }

    fn party(revision: u64, leader: u64) -> Party {
        Party {
            id: "p1".into(),
            leader: AccountId(leader),
            members: vec![PartyMember {
                account: account(1, "Mira"),
                joined: 0,
                endpoint: String::new(),
                relay: None,
                voice: false,
            }],
            max_members: 8,
            conversation: "pc".into(),
            created: 0,
            revision,
            discord: None,
        }
    }

    #[test]
    fn an_older_copy_of_the_party_is_dropped() {
        let mut social = Social::default();
        assert!(social.apply_party(Some(party(3, 1))));
        assert!(!social.apply_party(Some(party(2, 2))), "older");
        assert_eq!(social.party.as_ref().unwrap().leader, AccountId(1));
        assert!(social.apply_party(Some(party(4, 2))));
        assert!(social.leads(AccountId(2)));
        assert!(social.apply_party(None), "the party ended");
        assert!(
            social.apply_party(Some(party(1, 1))),
            "a new party starts again"
        );
        assert!(
            !social.apply_party(Some(party(1, 1))),
            "a repeat changes nothing"
        );
    }

    #[test]
    fn party_invitations_are_kept_by_id() {
        let mut social = Social::default();
        let invitation = |id: &str, members| PartyInvitation {
            id: id.into(),
            from: account(2, "Lumen"),
            party: "p1".into(),
            members,
            expires: 0,
            call: false,
        };
        social.add_party_invitation(invitation("a", 2));
        social.add_party_invitation(invitation("a", 3));
        social.add_party_invitation(invitation("b", 1));
        assert_eq!(social.party_invitations.len(), 2);
        assert_eq!(
            social.party_invitations[0].members, 3,
            "the newer one stays"
        );
        social.drop_party_invitation("a");
        assert_eq!(social.party_invitations.len(), 1);
    }

    #[test]
    fn unread_is_last_minus_read_and_unread_conversations_come_first() {
        let mut social = Social::default();
        let mut old = conversation("old", 5, 5);
        old.updated = 900;
        let mut fresh = conversation("fresh", 9, 9);
        fresh.updated = 800;
        let mut unread = conversation("unread", 7, 4);
        unread.updated = 100;
        social.set_conversations(lists(
            vec![old, fresh, unread],
            vec![conversation("r", 1, 0)],
        ));
        let order: Vec<&str> = social
            .conversations()
            .iter()
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(order, ["unread", "old", "fresh"]);
        assert_eq!(social.unread("unread"), 3);
        assert_eq!(
            social.unread_total(),
            3 + 1,
            "the messages of a request count too"
        );
        assert!(social.mark_read("unread", 7));
        assert!(!social.mark_read("unread", 7), "nothing moves twice");
        assert_eq!(social.unread("unread"), 0);
        assert!(!social.mark_read("unread", 3), "marks only go forward");
    }

    #[test]
    fn a_new_message_raises_the_unread_count_at_once() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("c", 4, 4)], vec![]));
        assert!(social.note_last("c", 5));
        assert_eq!(social.unread("c"), 1);
        assert!(!social.note_last("nope", 1), "the list must be read again");
    }

    #[test]
    fn the_first_look_fetches_the_newest_and_then_follows_by_number() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("c", 120, 100)], vec![]));
        assert_eq!(social.plan("c"), Some(Fetch::Newest));
        let got = social.ingest("c", Fetch::Newest, page("c", 71, 120, 120));
        assert_eq!(got.new.len(), 50);
        assert_eq!(got.follow, None);
        assert!(social.is_loaded("c"));
        assert!(
            social.has_earlier("c"),
            "a full page means there may be more"
        );
        assert_eq!(social.plan("c"), None, "current");
        social.note_last("c", 121);
        assert_eq!(social.plan("c"), Some(Fetch::After(120)));
        let got = social.ingest("c", Fetch::After(120), page("c", 121, 121, 121));
        assert_eq!(got.new.iter().map(|m| m.seq).collect::<Vec<_>>(), [121]);
        assert_eq!(social.messages("c").last().unwrap().seq, 121);
        assert_eq!(social.plan("c"), None);
    }

    #[test]
    fn a_short_first_page_has_nothing_before_it() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("c", 3, 0)], vec![]));
        social.ingest("c", Fetch::Newest, page("c", 1, 3, 3));
        assert!(!social.has_earlier("c"));
        assert_eq!(social.plan_earlier("c"), None);
    }

    #[test]
    fn a_repeated_page_adds_nothing() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("c", 3, 0)], vec![]));
        social.ingest("c", Fetch::Newest, page("c", 1, 3, 3));
        let again = social.ingest("c", Fetch::After(1), page("c", 1, 3, 3));
        assert!(again.new.is_empty());
        assert_eq!(social.messages("c").len(), 3);
    }

    #[test]
    fn numbers_a_block_hid_do_not_loop() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("c", 3, 0)], vec![]));
        social.ingest("c", Fetch::Newest, page("c", 1, 3, 3));
        // Numbers 4 and 5 were written by somebody the player blocked: the fetch is empty.
        social.note_last("c", 5);
        let got = social.ingest(
            "c",
            Fetch::After(3),
            Messages {
                messages: Vec::new(),
                last: 5,
            },
        );
        assert_eq!(got.follow, None);
        assert_eq!(social.plan("c"), None, "5 is accounted for");
    }

    #[test]
    fn a_long_catch_up_goes_on_until_the_last_number() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("c", 3, 3)], vec![]));
        social.ingest("c", Fetch::Newest, page("c", 1, 3, 3));
        social.note_last("c", 400);
        let got = social.ingest("c", Fetch::After(3), page("c", 4, 103, 400));
        assert_eq!(
            got.follow,
            Some(Fetch::After(103)),
            "a full page: ask again"
        );
        let got = social.ingest("c", Fetch::After(103), page("c", 104, 203, 400));
        assert_eq!(got.follow, Some(Fetch::After(203)));
        let got = social.ingest("c", Fetch::After(203), page("c", 204, 303, 400));
        assert_eq!(got.follow, Some(Fetch::After(303)));
        let got = social.ingest("c", Fetch::After(303), page("c", 304, 400, 400));
        assert_eq!(got.follow, None);
        let held = social.messages("c");
        assert_eq!(held.len(), KEPT, "only the last 200 stay");
        assert_eq!(held.first().unwrap().seq, 201);
        assert_eq!(held.last().unwrap().seq, 400);
        assert!(
            !social.has_earlier("c"),
            "full: no room to read further back"
        );
    }

    #[test]
    fn reading_further_back_fills_up_to_the_limit() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("c", 300, 300)], vec![]));
        social.ingest("c", Fetch::Newest, page("c", 251, 300, 300));
        assert_eq!(social.plan_earlier("c"), Some(Fetch::Before(251)));
        social.ingest("c", Fetch::Before(251), page("c", 201, 250, 300));
        assert_eq!(social.messages("c").first().unwrap().seq, 201);
        social.ingest("c", Fetch::Before(201), page("c", 151, 200, 300));
        social.ingest("c", Fetch::Before(151), page("c", 101, 150, 300));
        social.ingest("c", Fetch::Before(101), page("c", 51, 100, 300));
        assert_eq!(social.messages("c").len(), KEPT);
        assert_eq!(social.plan_earlier("c"), None, "the memory is full");
        assert_eq!(
            social.messages("c").last().unwrap().seq,
            300,
            "the newest stay"
        );
    }

    #[test]
    fn a_reconnect_fetches_only_what_loaded_threads_missed() {
        let mut social = Social::default();
        let first = social.set_conversations(lists(
            vec![conversation("a", 3, 3), conversation("b", 5, 5)],
            vec![conversation("r", 1, 0)],
        ));
        assert!(first.fetches.is_empty() && first.new_requests.is_empty());
        social.ingest("a", Fetch::Newest, page("a", 1, 3, 3));
        // The connection broke; a gained a message, b did too but was never opened, r is not new,
        // and a stranger wrote for the first time in q.
        let listed = social.set_conversations(lists(
            vec![conversation("a", 4, 3), conversation("b", 6, 5)],
            vec![conversation("r", 1, 0), conversation("q", 1, 0)],
        ));
        assert_eq!(listed.fetches, [("a".to_owned(), Fetch::After(3))]);
        assert_eq!(
            listed
                .new_requests
                .iter()
                .map(|c| c.id.as_str())
                .collect::<Vec<_>>(),
            ["q"]
        );
        assert_eq!(social.unread("b"), 1, "unread needs no fetch");
    }

    #[test]
    fn a_conversation_that_left_the_list_is_forgotten() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("a", 3, 3)], vec![]));
        social.ingest("a", Fetch::Newest, page("a", 1, 3, 3));
        social.set_draft("a", "hello".into());
        social.set_conversations(lists(vec![], vec![]));
        assert!(social.messages("a").is_empty());
        assert_eq!(social.draft("a"), "");
        assert!(social.get("a").is_none());
    }

    #[test]
    fn the_players_own_read_mark_never_goes_back_when_the_list_is_older() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("a", 9, 5)], vec![]));
        social.mark_read("a", 9);
        social.set_conversations(lists(vec![conversation("a", 9, 5)], vec![]));
        assert_eq!(social.unread("a"), 0);
    }

    #[test]
    fn looking_at_a_conversation_marks_the_newest_held_number() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("a", 4, 1)], vec![]));
        assert_eq!(social.read_target("a"), None, "nothing is held yet");
        social.ingest("a", Fetch::Newest, page("a", 1, 4, 4));
        assert_eq!(social.read_target("a"), Some(4));
        social.mark_read("a", 4);
        assert_eq!(social.read_target("a"), None);
    }

    #[test]
    fn read_marks_of_others_are_kept_and_the_own_one_moves_the_count() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("a", 6, 2)], vec![]));
        social.apply_read("a", AccountId(2), 4, AccountId(1));
        social.apply_read("a", AccountId(2), 3, AccountId(1));
        assert_eq!(
            social.get("a").unwrap().marks,
            [ReadMark {
                account: AccountId(2),
                seq: 4
            }]
        );
        assert_eq!(
            read_by_other(social.get("a").unwrap(), AccountId(1)),
            Some(4)
        );
        assert_eq!(social.unread("a"), 4);
        social.apply_read("a", AccountId(1), 6, AccountId(1));
        assert_eq!(social.unread("a"), 0);
    }

    #[test]
    fn the_own_message_is_held_when_it_is_the_next_number() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("a", 2, 2)], vec![]));
        // Not fetched yet: the answer changes nothing, opening the conversation will fetch.
        assert_eq!(social.ingest_sent(message("a", 3, 1)), Ingested::default());
        social.ingest("a", Fetch::Newest, page("a", 1, 3, 3));
        let got = social.ingest_sent(message("a", 4, 1));
        assert_eq!(got.follow, None);
        assert_eq!(social.messages("a").last().unwrap().seq, 4);
        // Somebody wrote 5 before our 6 arrived: ours is not held, a fetch brings both.
        social.note_last("a", 5);
        let got = social.ingest_sent(message("a", 6, 1));
        assert_eq!(got.follow, Some(Fetch::After(4)));
        assert_eq!(social.messages("a").last().unwrap().seq, 4);
        let got = social.ingest("a", Fetch::After(4), page("a", 5, 6, 6));
        assert_eq!(got.new.len(), 2);
    }

    #[test]
    fn a_draft_survives_until_its_message_arrived() {
        let mut social = Social::default();
        social.set_conversations(lists(vec![conversation("a", 0, 0)], vec![]));
        social.set_draft("a", "hello".into());
        assert_eq!(social.draft("a"), "hello");
        social.set_draft("a", "hello there".into());
        social.sent("a", "hello");
        assert_eq!(
            social.draft("a"),
            "hello there",
            "the player went on typing"
        );
        social.sent("a", "hello there");
        assert_eq!(social.draft("a"), "");
    }

    #[test]
    fn upsert_sorts_requests_and_conversations_apart() {
        let mut social = Social::default();
        let mut request = conversation("r", 1, 0);
        request.request = true;
        social.upsert(request.clone());
        assert_eq!(social.requests().len(), 1);
        request.request = false;
        social.upsert(request);
        assert!(social.requests().is_empty(), "accepting moves it");
        assert_eq!(social.conversations().len(), 1);
        assert!(social.direct_with(AccountId(2)).is_some());
        assert!(social.direct_with(AccountId(9)).is_none());
        social.remove("r");
        assert!(social.get("r").is_none());
    }

    #[test]
    fn the_fetch_commands_carry_the_numbers() {
        let line = |f: Fetch| anyplay_control::command_line(1, &f.command("c1"));
        assert_eq!(
            line(Fetch::After(7)),
            "{\"id\":1,\"do\":\"messages\",\"conversation\":\"c1\",\"after\":7,\"before\":null,\"limit\":100}\n"
        );
        assert!(line(Fetch::Before(7)).contains("\"before\":7"));
        assert!(line(Fetch::Newest).contains("\"after\":null"));
    }

    #[test]
    fn shown_text_is_plain() {
        assert_eq!(plain("hi\u{7} there\tyou\nok"), "hi there you\nok");
        // A right-to-left override and an isolate go; an emoji joiner stays.
        assert_eq!(plain("a\u{202E}b\u{2066}c\u{2028}d"), "abcd");
        assert_eq!(
            plain("\u{1F468}\u{200D}\u{1F469}"),
            "\u{1F468}\u{200D}\u{1F469}"
        );
        assert_eq!(
            plain("<b>bold</b> [x](http://a.example)"),
            "<b>bold</b> [x](http://a.example)"
        );
    }

    #[test]
    fn a_toast_shows_one_short_line() {
        assert_eq!(
            snippet(
                "ready?

let's go",
                40
            ),
            "ready? let's go"
        );
        let long = "word ".repeat(30);
        let cut = snippet(&long, 20);
        assert!(cut.chars().count() <= 20 && cut.ends_with('…'), "{cut}");
        assert_eq!(snippet("", 10), "");
    }

    #[test]
    fn others_leave_the_player_out() {
        let c = conversation("a", 0, 0);
        let names: Vec<&str> = others(&c, AccountId(1))
            .iter()
            .map(|a| a.handle.as_str())
            .collect();
        assert_eq!(names, ["Lumen"]);
    }
}
