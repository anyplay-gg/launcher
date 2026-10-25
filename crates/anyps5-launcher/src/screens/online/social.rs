//! Parties and messages in the online panel: the pages, what they follow and what their rows do.
//! A page is rows like any other (docs/DESIGN.md 17). Message text is untrusted: it is shown as
//! plain text in rows and nowhere interpreted, so no link in it ever opens.

use anyps5_core::online::api::{
    Account, AccountId, Conversation, ConversationKind, Friend, Message, Visibility,
};
use anyps5_core::online::{Blocked, Online, Purpose, Social, others, plain, read_by_other};

use super::{OnlineAction, OnlineData, OnlinePanel, Page, presence_text, restriction};
use crate::screens::format;
use crate::screens::friends_strings::FriendsText as FX;
use crate::screens::online_social_strings::SocialText as S;
use crate::screens::online_strings::OnlineText as T;
use crate::screens::online_voice_strings::VoiceText as V;
use crate::screens::rows::{self, Row};
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;

/// How many messages a conversation page shows to begin with, and how many more each press of
/// "Show Earlier Messages" adds. The rest stay in memory (`anyps5_core::online::KEPT`).
pub(super) const SHOWN: usize = 40;

/// How many messages of a request the page shows: a stranger may send five before an answer.
const REQUEST_SHOWN: usize = 5;

/// Where the focus stands on a page whose rows change under it. New messages come in above the
/// field, so a row is found again by what it is, not by its place in the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Anchor {
    /// A message row, by its number.
    Message(u64),
    /// Any other row, by its distance from the end of the list: the field, Send, the answers.
    FromEnd(usize),
}

/// A message as one line for its row: the line breaks and runs of white space become one space.
/// The page of the message has the text as written.
fn one_line(text: &str) -> String {
    plain(text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A conversation's name for the player: the other account, the group's name or its members, or
/// the party's chat.
pub fn chat_title(conversation: &Conversation, me: AccountId, language: Language) -> String {
    match conversation.kind {
        ConversationKind::Party => S::PartyChat.text(language).to_owned(),
        ConversationKind::Direct => others(conversation, me)
            .first()
            .map(|a| a.handle.clone())
            .unwrap_or_default(),
        ConversationKind::Group => conversation.name.clone().unwrap_or_else(|| {
            others(conversation, me)
                .iter()
                .map(|a| a.handle.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }),
    }
}

/// The friends a party can invite: those who are online and not in it, sorted like the list.
pub(super) fn party_candidates(online: &Online) -> Vec<&Friend> {
    online
        .sorted_friends()
        .into_iter()
        .filter(|f| f.presence.online && !online.social.in_party(f.account.id))
        .collect()
}

/// How many more members the party takes; seven for a party that does not exist yet.
pub(super) fn free_places(online: &Online) -> usize {
    let (max, members) = online
        .social
        .party
        .as_ref()
        .map_or((anyps5_core::online::api::MAX_PARTY_MEMBERS, 1), |p| {
            (p.max_members, p.members.len())
        });
    usize::from(max).saturating_sub(members)
}

impl OnlinePanel {
    fn me(data: &OnlineData<'_>) -> AccountId {
        data.online.account.as_ref().map_or(AccountId(0), |a| a.id)
    }

    /// Party invitations, listed with the other invitations on the Home page.
    pub(super) fn party_invitation_rows(&self, data: &OnlineData<'_>, rows: &mut Vec<Row>) {
        let language = data.language;
        for invitation in &data.online.social.party_invitations {
            rows.push(
                Row::action(
                    "partyInvitation",
                    T::InvitedBy.format(language, &[Arg::Str(&invitation.from.handle)]),
                )
                .icon(Icon::USERS)
                .detail(
                    S::PartyInvitationDetail
                        .format(language, &[Arg::Int(i64::from(invitation.members))]),
                ),
            );
        }
    }

    /// The rows the friend's page adds: a message, and an invitation to the party.
    pub(super) fn friend_social_rows(
        &self,
        data: &OnlineData<'_>,
        friend: AccountId,
        rows: &mut Vec<Row>,
    ) {
        let language = data.language;
        let online = data.online;
        let social = &online.social;
        let opening = self.opening == Some(friend)
            && online.is_waiting(|p| matches!(p, Purpose::OpenConversation(_)));
        let mut message =
            Row::action("sendMessage", S::SendMessage.text(language)).icon(Icon::ENVELOPE_SIMPLE);
        if opening {
            message = message.detail(S::Opening.text(language));
        } else if let Some(direct) = social.direct_with(friend)
            && social.unread(&direct.id) > 0
        {
            message = message.detail(
                S::NewCount.format(language, &[Arg::Int(social.unread(&direct.id) as i64)]),
            );
        }
        rows.push(message);
        let me = Self::me(data);
        let may_invite = social.party.is_none() || social.leads(me);
        if may_invite && !social.in_party(friend) && free_places(online) > 0 {
            rows.push(
                Row::command("inviteToParty", S::InviteToParty.text(language)).icon(Icon::USERS),
            );
        }
    }

    /// The title, subtitle and rows of a party or message page; `None` for any other page.
    pub(super) fn social_rows(
        &self,
        data: &OnlineData<'_>,
        rows: &mut Vec<Row>,
    ) -> Option<(String, Option<String>)> {
        let language = data.language;
        let online = data.online;
        let social = &online.social;
        let me = Self::me(data);
        let t = |text: S| text.text(language);
        // The party's chat can be asked for before the list of conversations has it.
        if let Page::Conversation(id) = &self.page
            && social.get(id).is_none()
        {
            rows.push(Row::info("loading", t(S::Loading), None));
            let title = social
                .party
                .as_ref()
                .filter(|p| &p.conversation == id)
                .map_or(String::new(), |_| t(S::PartyChat).to_owned());
            return Some((title, None));
        }
        Some(match &self.page {
            Page::Party => {
                let Some(party) = social.party.as_ref() else {
                    // The first page of the section stands without a party: how to start one,
                    // the invitations that wait, and the voice settings.
                    rows.push(Row::info(
                        "notInParty",
                        FX::NotInPartyNote.text(language),
                        None,
                    ));
                    self.party_invitation_rows(data, rows);
                    let mut start = Row::action("startParty", t(S::StartParty))
                        .icon(Icon::USERS)
                        .detail(t(S::StartPartyDetail));
                    start.group_start = rows.len() > 1;
                    rows.push(start);
                    if online.is_online() {
                        rows.push(
                            Row::action("voice", V::Voice.text(language))
                                .icon(Icon::MICROPHONE)
                                .detail(super::voice::home_detail(data)),
                        );
                    }
                    return Some((
                        t(S::Party).to_owned(),
                        Some(FX::NotInParty.text(language).to_owned()),
                    ));
                };
                let leads = party.leader == me;
                let alone = party.members.len() <= 1;
                let chat = social.party_chat();
                let mut chat_row =
                    Row::action("partyChat", t(S::PartyChat)).icon(Icon::ENVELOPE_SIMPLE);
                if let Some(chat) = chat
                    && social.unread(&chat.id) > 0
                {
                    chat_row = chat_row.detail(
                        S::NewCount.format(language, &[Arg::Int(social.unread(&chat.id) as i64)]),
                    );
                }
                let invite = (leads && free_places(online) > 0).then(|| {
                    Row::action("inviteFriends", t(S::InviteFriends)).icon(Icon::USER_PLUS)
                });
                // A party of one starts by inviting; any other party by talking.
                if alone {
                    rows.extend(invite);
                    rows.push(chat_row);
                } else {
                    rows.push(chat_row);
                    rows.extend(invite);
                }
                if leads
                    && !alone
                    && let Some(session) = online.session.as_ref().filter(|s| {
                        s.visibility != Visibility::Closed
                            && s.members.iter().any(|m| m.account.id == me)
                    })
                {
                    rows.push(
                        Row::command("takeParty", t(S::TakeParty))
                            .icon(Icon::GAME_CONTROLLER)
                            .detail(session.title.clone()),
                    );
                }
                rows.push(Row::caption("voiceCaption", V::Voice.text(language)).group_start());
                self.voice_rows(data, rows, false);
                rows.push(
                    Row::action("voiceSettings", V::VoiceSettings.text(language))
                        .icon(Icon::GEAR_SIX),
                );
                super::screen::rows(data, rows);
                rows.push(Row::caption("membersCaption", t(S::PartyMembers)).group_start());
                let voice = &online.voice;
                let voice_on = voice.enabled();
                for member in &party.members {
                    let id = member.account.id;
                    let friend = online.friend(id);
                    let mut detail = Vec::new();
                    // Who is heard, in words; the icon says it again.
                    let speaking = voice_on
                        && if id == me {
                            voice.me.speaking
                        } else {
                            voice.is_speaking(id)
                        };
                    let muted = voice_on && id != me && voice.prefs().peer(id).1;
                    if muted {
                        detail.push(V::MutedByYou.text(language).to_owned());
                    } else if speaking {
                        detail.push(V::Speaking.text(language).to_owned());
                    } else if voice_on && id != me && !member.voice {
                        detail.push(V::NotInVoice.text(language).to_owned());
                    }
                    if id == party.leader {
                        detail.push(t(S::Leader).to_owned());
                    }
                    if let Some(friend) = friend {
                        detail.push(presence_text(friend, language));
                    }
                    let icon = if muted {
                        Icon::SPEAKER_SLASH
                    } else if speaking {
                        Icon::SPEAKER_HIGH
                    } else {
                        match friend.map(|f| (&f.presence.activity, f.presence.online)) {
                            Some((Some(_), _)) => Icon::GAME_CONTROLLER,
                            Some((None, false)) => Icon::CIRCLE_DASHED,
                            _ => Icon::USER_CIRCLE,
                        }
                    };
                    let handle = member.account.handle.clone();
                    let mut row = if id == me {
                        Row::info("member", handle, Some(T::You.text(language).to_owned()))
                    } else {
                        Row::action("partyMember", handle)
                    }
                    .icon(icon);
                    if !detail.is_empty() {
                        row = row.detail(detail.join(" · "));
                    }
                    rows.push(row);
                }
                rows.push(
                    Row::command("leaveParty", t(S::LeaveParty))
                        .icon(Icon::SIGN_OUT)
                        .destructive()
                        .group_start(),
                );
                (
                    t(S::Party).to_owned(),
                    Some(S::PartySubtitle.format(
                        language,
                        &[
                            Arg::Int(party.members.len() as i64),
                            Arg::Int(i64::from(party.max_members)),
                        ],
                    )),
                )
            }
            Page::PartyMember(id) => {
                let handle = online
                    .social
                    .party
                    .as_ref()
                    .and_then(|p| p.members.iter().find(|m| m.account.id == *id))
                    .map(|m| m.account.handle.clone())
                    .unwrap_or_default();
                self.member_voice_rows(*id, data, rows);
                // Only the leader manages the party; everyone sets how they hear a member.
                if social.leads(me) {
                    rows.push(
                        Row::command("makeLeader", t(S::MakeLeader))
                            .icon(Icon::USER_CIRCLE)
                            .detail(S::MakeLeaderDetail.format(language, &[Arg::Str(&handle)]))
                            .group_start(),
                    );
                    rows.push(
                        Row::command("removeMember", t(S::RemoveFromParty))
                            .icon(Icon::USER_MINUS)
                            .detail(S::RemoveDetail.format(language, &[Arg::Str(&handle)]))
                            .destructive(),
                    );
                }
                let presence = online.friend(*id).map(|f| presence_text(f, language));
                (handle, presence)
            }
            Page::PartyPicker => {
                let free = free_places(online);
                let candidates = party_candidates(online);
                for friend in &candidates {
                    rows.push(
                        Row::toggle(
                            "pickParty",
                            friend.account.handle.clone(),
                            self.picked.contains(&friend.account.id),
                        )
                        .detail(presence_text(friend, language)),
                    );
                }
                if candidates.is_empty() {
                    rows.push(Row::info("none", t(S::NobodyToInvite), None));
                }
                let mut send = Row::command("sendPartyInvites", T::SendInvitations.text(language))
                    .icon(Icon::PAPER_PLANE_RIGHT)
                    .group_start();
                if free == 0 {
                    send = send.detail(t(S::PartyFull));
                } else if self.picked.is_empty() {
                    send = send.detail(t(S::PickFirst));
                }
                rows.push(send);
                rows.push(Row::command("cancel", language.text(Key::Cancel)).icon(Icon::X));
                (
                    t(S::InviteFriends).to_owned(),
                    Some(T::PickerSubtitle.format(language, &[Arg::Int(free as i64)])),
                )
            }
            Page::PartyInvitation(id) => {
                let invitation = social.party_invitations.iter().find(|i| &i.id == id);
                // A player in a party cannot join another: the page says so instead of offering it.
                if social.party.is_some() {
                    rows.push(Row::info("leaveFirst", t(S::LeaveFirst), None));
                } else {
                    rows.push(Row::command("joinParty", t(S::JoinParty)).icon(Icon::CHECK));
                }
                rows.push(Row::command("declineParty", T::Decline.text(language)).icon(Icon::X));
                match invitation {
                    Some(i) => (
                        T::InvitedBy.format(language, &[Arg::Str(&i.from.handle)]),
                        Some(
                            S::PartyInvitationDetail
                                .format(language, &[Arg::Int(i64::from(i.members))]),
                        ),
                    ),
                    None => (t(S::Party).to_owned(), None),
                }
            }
            Page::Messages => {
                let requests = social.requests();
                if !requests.is_empty() {
                    rows.push(Row::caption("requestsCaption", t(S::MessageRequests)));
                    for request in requests {
                        rows.push(
                            Row::action("messageRequest", chat_title(request, me, language))
                                .icon(Icon::ENVELOPE_SIMPLE)
                                .detail(t(S::WantsToMessage)),
                        );
                    }
                }
                let conversations = social.conversations();
                if !conversations.is_empty() {
                    let mut caption = Row::caption("conversationsCaption", t(S::Conversations));
                    if !requests.is_empty() {
                        caption = caption.group_start();
                    }
                    rows.push(caption);
                    for conversation in conversations {
                        let unread = social.unread(&conversation.id);
                        let detail = if unread > 0 {
                            Some(S::NewCount.format(language, &[Arg::Int(unread as i64)]))
                        } else {
                            self.conversation_detail(data, conversation)
                        };
                        let icon = match conversation.kind {
                            ConversationKind::Direct => Icon::USER_CIRCLE,
                            ConversationKind::Group | ConversationKind::Party => Icon::USERS,
                        };
                        let mut row =
                            Row::action("conversation", chat_title(conversation, me, language))
                                .icon(icon);
                        if let Some(detail) = detail {
                            row = row.detail(detail);
                        }
                        rows.push(row);
                    }
                }
                if rows.is_empty() {
                    rows.push(Row::info("none", t(S::NoConversations), None));
                }
                let unread = social.unread_total();
                let subtitle = if unread > 0 {
                    S::MessagesUnread.format(language, &[Arg::Int(unread as i64)])
                } else {
                    t(S::MessagesQuiet).to_owned()
                };
                (t(S::Messages).to_owned(), Some(subtitle))
            }
            Page::Conversation(id) => {
                let conversation = social.get(id)?;
                let held = social.messages(id);
                let shown = self.shown_messages(social, id);
                if held.len() > shown.len() || social.has_earlier(id) {
                    rows.push(Row::command("earlier", t(S::ShowEarlier)).icon(Icon::CLOCK));
                }
                let seen = read_by_other(conversation, me)
                    .filter(|_| conversation.kind == ConversationKind::Direct);
                let last_own = shown
                    .iter()
                    .rev()
                    .find(|m| m.sender.id == me)
                    .map(|m| m.seq);
                // Where several people write, the one who spoke comes first in the line.
                let crowd = conversation.kind != ConversationKind::Direct;
                for message in shown {
                    let mine = message.sender.id == me;
                    let text = one_line(&message.text);
                    let (title, mut meta) = if crowd && !mine {
                        (
                            format!("{}: {text}", message.sender.handle),
                            self.time_text(data, message.created),
                        )
                    } else {
                        (text, self.message_meta(data, message))
                    };
                    if Some(message.seq) == last_own && seen.is_some_and(|s| s >= message.seq) {
                        meta = meta + " · " + t(S::Seen);
                    }
                    rows.push(Row::action("message", title).detail(meta));
                }
                if social.is_loaded(id) && held.is_empty() {
                    rows.push(Row::info("quiet", t(S::NoMessages), None));
                } else if !social.is_loaded(id) && conversation.last > 0 {
                    rows.push(Row::info("loading", t(S::Loading), None));
                }
                let draft = social.draft(id);
                let what = if online.is_stranger(conversation) {
                    Blocked::Stranger
                } else {
                    Blocked::Message
                };
                let now = u64::try_from(data.now).unwrap_or(0);
                if let Some(paused) = online.restrictions.blocking(what, now) {
                    // A restriction stops writing: the row that says so stands where the field
                    // would be, and the draft waits.
                    let mut paused = restriction::paused_row("pausedSend", what, paused, data);
                    if !shown.is_empty() || held.is_empty() {
                        paused = paused.group_start();
                    }
                    rows.push(paused);
                } else {
                    let mut compose = Row::text("compose", t(S::MessageField), draft).wide();
                    if !shown.is_empty() || held.is_empty() {
                        compose = compose.group_start();
                    }
                    rows.push(compose);
                    let mut send = Row::command("send", t(S::Send)).icon(Icon::PAPER_PLANE_RIGHT);
                    if online.is_waiting(
                        |p| matches!(p, Purpose::SendMessage { conversation: c, .. } if c == id),
                    ) {
                        send = send.detail(T::Sending.text(language));
                    } else if draft.chars().count() > anyps5_core::online::api::MAX_MESSAGE_CHARS {
                        send = send.detail(t(S::TooLong));
                    }
                    rows.push(send);
                }
                if conversation.kind == ConversationKind::Group {
                    rows.push(
                        Row::command("leaveConversation", t(S::LeaveConversation))
                            .icon(Icon::SIGN_OUT)
                            .destructive()
                            .group_start(),
                    );
                }
                let subtitle = match conversation.kind {
                    ConversationKind::Direct => others(conversation, me)
                        .first()
                        .and_then(|a| online.friend(a.id))
                        .map(|f| presence_text(f, language)),
                    ConversationKind::Group => Some(
                        S::GroupPeople
                            .format(language, &[Arg::Int(conversation.members.len() as i64)]),
                    ),
                    ConversationKind::Party => social.party.as_ref().map(|p| {
                        S::PartySubtitle.format(
                            language,
                            &[
                                Arg::Int(p.members.len() as i64),
                                Arg::Int(i64::from(p.max_members)),
                            ],
                        )
                    }),
                };
                (chat_title(conversation, me, language), subtitle)
            }
            Page::MessageRequest(id) => {
                let request = social.get(id)?;
                for message in self.shown_messages(social, id) {
                    rows.push(
                        Row::action("message", one_line(&message.text))
                            .detail(self.message_meta(data, message)),
                    );
                }
                rows.push(
                    Row::command("acceptRequest", T::Accept.text(language))
                        .icon(Icon::CHECK)
                        .group_start(),
                );
                rows.push(Row::command("declineRequest", T::Decline.text(language)).icon(Icon::X));
                rows.push(
                    Row::command("blockSender", T::Block.text(language))
                        .icon(Icon::PROHIBIT)
                        .destructive(),
                );
                rows.push(Row::info("requestNote", t(S::RequestNote), None).group_start());
                (
                    chat_title(request, me, language),
                    Some(t(S::WantsToMessage).to_owned()),
                )
            }
            Page::Message(id, seq) => {
                let message = social.messages(id).iter().find(|m| m.seq == *seq)?;
                rows.push(Row::info("body", plain(&message.text), None));
                rows.push(
                    Row::command("copyText", t(S::CopyText))
                        .icon(Icon::FILE_TEXT)
                        .group_start(),
                );
                if message.sender.id != me {
                    rows.push(
                        Row::action("reportMessage", T::Report.text(language)).icon(Icon::FLAG),
                    );
                }
                (
                    self.sender_name(data, &message.sender),
                    Some(self.time_text(data, message.created)),
                )
            }
            _ => return None,
        })
    }

    /// The messages a conversation or request page shows: the newest ones.
    fn shown_messages<'a>(&self, social: &'a Social, id: &str) -> &'a [Message] {
        let held = social.messages(id);
        let limit = if matches!(self.page, Page::MessageRequest(_)) {
            REQUEST_SHOWN
        } else {
            self.shown
        };
        &held[held.len().saturating_sub(limit)..]
    }

    /// Where the focus stands now, if the page is one that changes under it.
    pub(super) fn anchor_now(&self, data: &OnlineData<'_>, rows: &[Row]) -> Option<Anchor> {
        let (Page::Conversation(id) | Page::MessageRequest(id)) = &self.page else {
            return None;
        };
        let row = self.side.nav.row;
        if rows.get(row)?.id == "message" {
            let first = rows::position(rows, "message")?;
            let message = self
                .shown_messages(&data.online.social, id)
                .get(row.checked_sub(first)?)?;
            Some(Anchor::Message(message.seq))
        } else {
            Some(Anchor::FromEnd(rows.len() - row))
        }
    }

    /// Puts the focus back on the row it stood on before the rows changed.
    pub(super) fn restore_anchor(&mut self, data: &OnlineData<'_>, rows: &[Row]) {
        let (Some(anchor), Page::Conversation(id) | Page::MessageRequest(id)) =
            (self.anchor, &self.page)
        else {
            return;
        };
        let row = match anchor {
            Anchor::FromEnd(distance) => rows.len().checked_sub(distance),
            Anchor::Message(seq) => {
                let first = rows::position(rows, "message");
                let at = self
                    .shown_messages(&data.online.social, id)
                    .iter()
                    .position(|m| m.seq == seq);
                first.zip(at).map(|(first, at)| first + at)
            }
        };
        if let Some(row) = row.filter(|row| *row < rows.len()) {
            self.side.nav.row = row;
        }
    }

    /// "You" for the player, else the handle.
    fn sender_name(&self, data: &OnlineData<'_>, sender: &Account) -> String {
        if sender.id == Self::me(data) {
            T::You.text(data.language).to_owned()
        } else {
            sender.handle.clone()
        }
    }

    fn time_text(&self, data: &OnlineData<'_>, created: u64) -> String {
        let unix = i64::try_from(created).unwrap_or(0);
        let offset = data
            .utc_offset
            .unwrap_or_else(|| format::local_offset_seconds(unix.saturating_mul(1_000_000)));
        format::message_time(unix, data.now, offset, data.language)
    }

    /// "Lumen · 14:32": who wrote it and when.
    fn message_meta(&self, data: &OnlineData<'_>, message: &Message) -> String {
        format!(
            "{} · {}",
            self.sender_name(data, &message.sender),
            self.time_text(data, message.created)
        )
    }

    /// What a read conversation says under its name: a friend's presence.
    fn conversation_detail(
        &self,
        data: &OnlineData<'_>,
        conversation: &Conversation,
    ) -> Option<String> {
        let language = data.language;
        match conversation.kind {
            ConversationKind::Direct => others(conversation, Self::me(data))
                .first()
                .and_then(|a| data.online.friend(a.id))
                .map(|f| presence_text(f, language)),
            ConversationKind::Group => Some(
                S::GroupPeople.format(language, &[Arg::Int(conversation.members.len() as i64)]),
            ),
            ConversationKind::Party => None,
        }
    }

    /// Where the player is sent when the page they are on no longer fits what the service says.
    pub(super) fn social_follow(&mut self, data: &OnlineData<'_>) -> Option<Page> {
        let online = data.online;
        let social = &online.social;
        let me = Self::me(data);
        // A conversation the player asked to open appears in the list.
        // The player may have left the friend's page since: then the chat stays closed.
        if let Some(account) = self.opening {
            if self.page != Page::Friend(account) && self.page != Page::Home {
                self.opening = None;
            } else if let Some(direct) = social.direct_with(account) {
                self.opening = None;
                return Some(Page::Conversation(direct.id.clone()));
            } else if !online.is_waiting(|p| matches!(p, Purpose::OpenConversation(_))) {
                self.opening = None;
            }
        }
        match &self.page {
            Page::PartyMember(_) if social.party.is_none() => Some(Page::Party),
            Page::PartyMember(id) if !social.in_party(*id) => Some(Page::Party),
            Page::PartyPicker if social.party.is_some() && !social.leads(me) => Some(Page::Party),
            Page::PartyInvitation(id) if !social.party_invitations.iter().any(|i| &i.id == id) => {
                // Accepting it made the party.
                Some(if social.party.is_some() {
                    Page::Party
                } else {
                    Page::Home
                })
            }
            Page::Conversation(id) => match social.get(id) {
                None if social.party.as_ref().is_some_and(|p| &p.conversation == id) => None,
                None => Some(Page::Messages),
                Some(c) if c.request => Some(Page::MessageRequest(id.clone())),
                Some(_) => None,
            },
            Page::MessageRequest(id) => match social.get(id) {
                Some(c) if c.request => None,
                Some(_) => Some(Page::Conversation(id.clone())),
                None => Some(Page::Messages),
            },
            Page::Message(id, seq) if !social.messages(id).iter().any(|m| m.seq == *seq) => {
                Some(Page::Conversation(id.clone()))
            }
            _ => None,
        }
    }

    /// The page Back leads to from a party or message page, if the page is one of them.
    pub(super) fn social_back(&self, data: &OnlineData<'_>) -> Option<Option<Page>> {
        Some(Some(match &self.page {
            // The first pages of their sections: Back leaves the screen.
            Page::Party | Page::Messages => return Some(None),
            Page::PartyInvitation(_) => Page::Home,
            Page::PartyMember(_) | Page::PartyPicker => Page::Party,
            Page::Conversation(_) | Page::MessageRequest(_) => match &self.previous {
                Some(page @ (Page::Friend(_) | Page::Party | Page::Home)) => page.clone(),
                _ => Page::Messages,
            },
            Page::Message(id, _) => {
                if data.online.social.get(id).is_some_and(|c| c.request) {
                    Page::MessageRequest(id.clone())
                } else {
                    Page::Conversation(id.clone())
                }
            }
            _ => return None,
        }))
    }

    /// A toggle on the party picker. Returns true when the row was one of ours.
    pub(super) fn social_toggle(&mut self, row: &str, on: bool, data: &OnlineData<'_>) -> bool {
        if row != "pickParty" {
            return false;
        }
        let index = self.nth(data, "pickParty");
        if let Some(friend) = party_candidates(data.online).get(index) {
            let id = friend.account.id;
            if !on {
                self.picked.remove(&id);
            } else if self.picked.len() < free_places(data.online) {
                self.picked.insert(id);
            }
        }
        true
    }

    /// Opens the conversation with a friend, asking for it first when there is none yet.
    pub(super) fn message_friend(
        &mut self,
        id: AccountId,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) {
        match data.online.social.direct_with(id) {
            Some(direct) => self.go(Page::Conversation(direct.id.clone()), data),
            None => {
                self.opening = Some(id);
                actions.push(OnlineAction::OpenDirect(id));
            }
        }
    }

    /// What the rows of the party and message pages do. Returns true when `row` was one of them.
    pub(super) fn social_run(
        &mut self,
        row: &str,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) -> bool {
        let online = data.online;
        let social = &online.social;
        let me = Self::me(data);
        match (row, self.page.clone()) {
            ("startParty", _) => self.go(Page::PartyPicker, data),
            ("inviteFriends", _) => self.go(Page::PartyPicker, data),
            ("partyChat", _) => {
                if let Some(chat) = social.party_chat() {
                    self.go(Page::Conversation(chat.id.clone()), data);
                }
            }
            ("partyInvitation", _) => {
                let index = self.nth(data, "partyInvitation");
                if let Some(invitation) = social.party_invitations.get(index) {
                    self.go(Page::PartyInvitation(invitation.id.clone()), data);
                }
            }
            ("joinParty", Page::PartyInvitation(id)) if social.party.is_none() => {
                actions.push(OnlineAction::PartyAnswer { id, accept: true });
            }
            ("declineParty", Page::PartyInvitation(id)) => {
                actions.push(OnlineAction::PartyAnswer { id, accept: false });
            }
            ("partyMember", _) => {
                let index = self.nth(data, "partyMember");
                let others: Vec<AccountId> = social
                    .party
                    .iter()
                    .flat_map(|p| p.members.iter())
                    .map(|m| m.account.id)
                    .filter(|id| *id != me)
                    .collect();
                if let Some(id) = others.get(index) {
                    self.go(Page::PartyMember(*id), data);
                }
            }
            ("makeLeader", Page::PartyMember(id)) => {
                actions.push(OnlineAction::PartyLeader(id));
                self.go(Page::Party, data);
            }
            ("removeMember", Page::PartyMember(id)) => {
                actions.push(OnlineAction::PartyKick(id));
                self.go(Page::Party, data);
            }
            ("takeParty", _) => {
                if let Some(session) = &online.session {
                    actions.push(OnlineAction::PartyMove(session.id.clone()));
                }
            }
            ("leaveParty", _) => actions.push(OnlineAction::PartyLeave),
            ("sendPartyInvites", Page::PartyPicker) if !self.picked.is_empty() => {
                actions.push(OnlineAction::PartyInvite(
                    self.picked.iter().copied().collect(),
                ));
                self.go(Page::Party, data);
            }
            ("cancel", Page::PartyPicker) => self.go(Page::Party, data),
            ("sendMessage", Page::Friend(id)) => self.message_friend(id, data, actions),
            ("inviteToParty", Page::Friend(id)) => {
                actions.push(OnlineAction::PartyInvite(vec![id]));
            }
            ("conversation", _) => {
                let index = self.nth(data, "conversation");
                if let Some(c) = social.conversations().get(index) {
                    self.go(Page::Conversation(c.id.clone()), data);
                }
            }
            ("messageRequest", _) => {
                let index = self.nth(data, "messageRequest");
                if let Some(c) = social.requests().get(index) {
                    self.go(Page::MessageRequest(c.id.clone()), data);
                }
            }
            ("message", Page::Conversation(id) | Page::MessageRequest(id)) => {
                let index = self.nth(data, "message");
                if let Some(message) = self.shown_messages(social, &id).get(index) {
                    self.go(Page::Message(id, message.seq), data);
                }
            }
            ("earlier", Page::Conversation(id)) => {
                self.shown += SHOWN;
                if social.messages(&id).len() < self.shown {
                    actions.push(OnlineAction::LoadEarlier(id));
                }
            }
            ("send", Page::Conversation(id)) => actions.push(OnlineAction::SendMessage(id)),
            ("leaveConversation", Page::Conversation(id)) => {
                actions.push(OnlineAction::LeaveConversation(id));
            }
            ("acceptRequest", Page::MessageRequest(id)) => {
                actions.push(OnlineAction::AcceptRequest(id));
            }
            ("declineRequest", Page::MessageRequest(id)) => {
                actions.push(OnlineAction::LeaveConversation(id));
                self.go(Page::Messages, data);
            }
            ("blockSender", Page::MessageRequest(id)) => {
                let strangers = social
                    .get(&id)
                    .map(|c| others(c, me).iter().map(|a| a.id).collect::<Vec<_>>())
                    .unwrap_or_default();
                for account in strangers {
                    actions.push(OnlineAction::Block(account));
                }
                actions.push(OnlineAction::LeaveConversation(id));
                self.go(Page::Messages, data);
            }
            ("copyText", Page::Message(id, seq)) => {
                if let Some(message) = social.messages(&id).iter().find(|m| m.seq == seq) {
                    actions.push(OnlineAction::CopyText(plain(&message.text)));
                }
            }
            ("reportMessage", Page::Message(id, seq)) => {
                if let Some(message) = social.messages(&id).iter().find(|m| m.seq == seq) {
                    let page = self.page.clone();
                    self.report_from = Some(page);
                    self.go(Page::Report(message.sender.id), data);
                }
            }
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use anyps5_core::online::api::{
        ConversationKind, Conversations, Friend, Message, Party, PartyInvitation, PartyMember,
        Presence, ReadMark,
    };
    use anyps5_core::online::{Cloud, Fetch, SignedIn, Status};
    use egui::Color32;

    use super::*;
    use crate::screens::rows::Control;
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
            now: 1_792_670_400,
            utc_offset: Some(0),
            game_running: false,
        }
    }

    fn party(leader: u64, members: &[(u64, &str)]) -> Party {
        Party {
            id: "p".into(),
            leader: AccountId(leader),
            members: members
                .iter()
                .map(|(id, handle)| PartyMember {
                    account: account(*id, handle),
                    joined: 0,
                    endpoint: String::new(),
                    relay: None,
                    voice: false,
                })
                .collect(),
            max_members: 8,
            conversation: "pc".into(),
            created: 0,
            revision: 1,
            discord: None,
        }
    }

    fn conversation(
        id: &str,
        kind: ConversationKind,
        with: &[(u64, &str)],
        last: u64,
        read: u64,
    ) -> Conversation {
        let mut members = vec![account(1, "Mira")];
        members.extend(with.iter().map(|(i, h)| account(*i, h)));
        Conversation {
            id: id.into(),
            kind,
            name: None,
            members,
            last,
            read,
            marks: Vec::new(),
            updated: last,
            request: false,
            party: None,
        }
    }

    fn texts(id: &str, from: u64, to: u64) -> anyps5_core::online::api::Messages {
        anyps5_core::online::api::Messages {
            messages: (from..=to)
                .map(|seq| Message {
                    conversation: id.into(),
                    seq,
                    sender: if seq % 2 == 0 {
                        account(1, "Mira")
                    } else {
                        account(2, "Lumen")
                    },
                    text: format!("Message {seq}"),
                    created: 1_792_670_400 - 600 + seq * 60,
                })
                .collect(),
            last: to,
        }
    }

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.id).collect()
    }

    fn list(online: &mut Online, conversations: Vec<Conversation>, requests: Vec<Conversation>) {
        online.social.set_conversations(Conversations {
            conversations,
            requests,
        });
    }

    #[test]
    fn the_party_section_offers_a_party_to_start_or_shows_the_one_in_progress() {
        let mut online = online();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Party");
        assert_eq!(subtitle.as_deref(), Some("You are not in a party"));
        assert_eq!(ids(&rows), ["notInParty", "startParty", "voice"]);
        assert_eq!(
            panel.page,
            Page::Party,
            "the first page of the section stays"
        );
        assert_eq!(panel.back_page(&data(&online)), None);

        online
            .social
            .apply_party(Some(party(1, &[(1, "Mira"), (2, "Lumen")])));
        let (_, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(subtitle.as_deref(), Some("2 of 8 members"));
        assert!(!ids(&rows).contains(&"notInParty"));
    }

    #[test]
    fn the_messages_page_counts_what_waits() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                5,
                2,
            )],
            vec![conversation(
                "r",
                ConversationKind::Direct,
                &[(9, "Vale")],
                1,
                0,
            )],
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Messages, &data(&online));
        assert_eq!(
            panel.rows(&data(&online)).1.as_deref(),
            Some("4 unread"),
            "the same number as the button and the rail: every message not read"
        );
    }

    #[test]
    fn the_party_page_puts_the_chat_first_and_the_leader_manages() {
        let mut online = online();
        online
            .social
            .apply_party(Some(party(1, &[(1, "Mira"), (2, "Lumen"), (3, "Juno")])));
        list(
            &mut online,
            vec![conversation(
                "pc",
                ConversationKind::Party,
                &[(2, "Lumen"), (3, "Juno")],
                4,
                1,
            )],
            vec![],
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Party");
        assert_eq!(subtitle.as_deref(), Some("3 of 8 members"));
        assert_eq!(
            ids(&rows),
            [
                "partyChat",
                "inviteFriends",
                "voiceCaption",
                "voiceProvider",
                "voiceOn",
                "voiceSettings",
                "membersCaption",
                "member",
                "partyMember",
                "partyMember",
                "leaveParty"
            ]
        );
        assert_eq!(rows[0].detail.as_deref(), Some("3 new"));
        assert_eq!(
            rows[7].detail.as_deref(),
            Some("Leader"),
            "the leader's mark is a word"
        );
        assert_eq!(rows[7].control, Control::Info(Some("You".into())));
        assert_eq!(rows[8].detail.as_deref(), Some("Online"));
        assert!(rows[10].destructive && rows[10].group_start);
        assert_eq!(panel.side.nav.row, 0, "opens on the chat");
    }

    #[test]
    fn a_party_of_one_starts_by_inviting() {
        let mut online = online();
        online.social.apply_party(Some(party(1, &[(1, "Mira")])));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(ids(&rows)[..2], ["inviteFriends", "partyChat"]);
    }

    #[test]
    fn a_member_who_is_no_leader_cannot_manage_or_invite() {
        let mut online = online();
        online
            .social
            .apply_party(Some(party(2, &[(2, "Lumen"), (1, "Mira")])));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            ids(&rows),
            [
                "partyChat",
                "voiceCaption",
                "voiceProvider",
                "voiceOn",
                "voiceSettings",
                "membersCaption",
                "partyMember",
                "member",
                "leaveParty"
            ]
        );
        assert_eq!(rows[6].detail.as_deref(), Some("Leader · Online"));
        // Any member opens the member page for volume and mute; it has no leader rows.
        panel.go(Page::PartyMember(AccountId(2)), &data(&online));
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::PartyMember(AccountId(2)));
        assert!(!ids(&panel.rows(&data(&online)).2).contains(&"makeLeader"));
        // The leader's pages (the picker) lead back.
        panel.go(Page::PartyPicker, &data(&online));
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Party);
    }

    #[test]
    fn the_leader_takes_the_party_into_a_session_that_is_open() {
        use anyps5_core::online::Session;
        use anyps5_core::online::api::Visibility;
        use anyps5_core::online::control::Member;
        let mut online = online();
        online
            .social
            .apply_party(Some(party(1, &[(1, "Mira"), (2, "Lumen")])));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        assert!(!ids(&panel.rows(&data(&online)).2).contains(&"takeParty"));
        let mut session = Session {
            id: "s1".into(),
            title_id: "TEST00001".into(),
            title: "Test Game".into(),
            version: "01.000".into(),
            leader: AccountId(1),
            max_members: 4,
            visibility: Visibility::Friends,
            members: vec![Member {
                account: account(1, "Mira"),
                address: "198.18.0.1".into(),
                joined: 0,
                link: None,
            }],
            created: 0,
        };
        online.session = Some(session.clone());
        let rows = panel.rows(&data(&online)).2;
        let take = rows.iter().position(|r| r.id == "takeParty").unwrap();
        assert_eq!(rows[take].detail.as_deref(), Some("Test Game"));
        let mut actions = Vec::new();
        panel.run("takeParty", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::PartyMove("s1".into())]);
        session.visibility = Visibility::Closed;
        online.session = Some(session);
        assert!(
            !ids(&panel.rows(&data(&online)).2).contains(&"takeParty"),
            "a closed game takes nobody"
        );
    }

    #[test]
    fn inviting_friends_picks_online_friends_up_to_the_free_places() {
        let mut online = online();
        online.social.apply_party(Some(Party {
            max_members: 3,
            ..party(1, &[(1, "Mira")])
        }));
        let mut panel = OnlinePanel::default();
        panel.go(Page::PartyPicker, &data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Invite Friends");
        assert_eq!(subtitle.as_deref(), Some("Pick up to 2 friends"));
        assert_eq!(
            ids(&rows),
            [
                "pickParty",
                "pickParty",
                "pickParty",
                "sendPartyInvites",
                "cancel"
            ]
        );
        // Three places for two: the third pick is refused.
        for index in 0..3 {
            panel.side.nav.row = index;
            panel.social_toggle("pickParty", true, &data(&online));
        }
        assert_eq!(panel.picked.len(), 2);
        let mut actions = Vec::new();
        panel.run("sendPartyInvites", &data(&online), &mut actions);
        assert!(matches!(&actions[..], [OnlineAction::PartyInvite(who)] if who.len() == 2));
        assert_eq!(panel.page, Page::Party);
    }

    #[test]
    fn nobody_is_invited_without_a_pick() {
        let online = online();
        let mut panel = OnlinePanel::default();
        panel.go(Page::PartyPicker, &data(&online));
        let mut actions = Vec::new();
        panel.run("sendPartyInvites", &data(&online), &mut actions);
        assert!(actions.is_empty());
        assert_eq!(panel.page, Page::PartyPicker);
        panel.run("cancel", &data(&online), &mut actions);
        assert_eq!(panel.page, Page::Party);
    }

    #[test]
    fn the_leader_makes_somebody_leader_or_removes_them() {
        let mut online = online();
        online
            .social
            .apply_party(Some(party(1, &[(1, "Mira"), (2, "Lumen")])));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        panel.side.nav.row =
            crate::screens::rows::position(&panel.rows(&data(&online)).2, "partyMember").unwrap();
        let mut actions = Vec::new();
        panel.run("partyMember", &data(&online), &mut actions);
        assert_eq!(panel.page, Page::PartyMember(AccountId(2)));
        let (title, _, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Lumen");
        assert_eq!(
            ids(&rows),
            ["peerVolume", "peerMute", "makeLeader", "removeMember"]
        );
        panel.run("makeLeader", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::PartyLeader(AccountId(2))]);
        panel.go(Page::PartyMember(AccountId(2)), &data(&online));
        panel.run("removeMember", &data(&online), &mut actions);
        assert_eq!(actions[1], OnlineAction::PartyKick(AccountId(2)));
    }

    #[test]
    fn a_party_invitation_is_answered_unless_the_player_has_a_party() {
        let mut online = online();
        online.social.add_party_invitation(PartyInvitation {
            id: "i".into(),
            from: account(2, "Lumen"),
            party: "p9".into(),
            members: 3,
            expires: 0,
            call: false,
        });
        let mut panel = OnlinePanel::default();
        let mut rows = Vec::new();
        panel.party_invitation_rows(&data(&online), &mut rows);
        assert_eq!(rows[0].title, "Lumen invites you");
        assert_eq!(rows[0].detail.as_deref(), Some("Party of 3"));
        panel.go(Page::PartyInvitation("i".into()), &data(&online));
        let (title, _, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Lumen invites you");
        assert_eq!(ids(&rows), ["joinParty", "declineParty"]);
        let mut actions = Vec::new();
        panel.run("joinParty", &data(&online), &mut actions);
        assert_eq!(
            actions,
            [OnlineAction::PartyAnswer {
                id: "i".into(),
                accept: true
            }]
        );
        // With a party of their own they leave it first: the page offers no join that would do
        // nothing.
        online.social.apply_party(Some(party(1, &[(1, "Mira")])));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(ids(&rows), ["leaveFirst", "declineParty"]);
        assert_eq!(rows[0].title, "Leave your party first to join this one.");
        panel.go(Page::PartyInvitation("i".into()), &data(&online));
        assert_eq!(panel.side.nav.row, 1, "it opens on Decline");
        let mut actions = Vec::new();
        panel.run("joinParty", &data(&online), &mut actions);
        assert!(actions.is_empty());
        // Once it is gone the page leaves for the party, or for Home.
        online.social.drop_party_invitation("i");
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Party);
    }

    #[test]
    fn the_message_list_puts_requests_apart_and_unread_first() {
        let mut online = online();
        let mut quiet = conversation("quiet", ConversationKind::Direct, &[(2, "Lumen")], 9, 9);
        quiet.updated = 900;
        let mut news = conversation("news", ConversationKind::Direct, &[(3, "Juno")], 4, 1);
        news.updated = 100;
        let mut group = conversation(
            "group",
            ConversationKind::Group,
            &[(2, "Lumen"), (3, "Juno")],
            2,
            2,
        );
        group.name = Some("Raid night".into());
        list(
            &mut online,
            vec![quiet, news, group],
            vec![conversation(
                "r",
                ConversationKind::Direct,
                &[(9, "Vale")],
                2,
                0,
            )],
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Messages, &data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Messages");
        assert_eq!(
            subtitle.as_deref(),
            Some("5 unread"),
            "three messages and the two of a request"
        );
        assert_eq!(
            ids(&rows),
            [
                "requestsCaption",
                "messageRequest",
                "conversationsCaption",
                "conversation",
                "conversation",
                "conversation"
            ]
        );
        assert_eq!(rows[1].title, "Vale");
        assert_eq!(rows[1].detail.as_deref(), Some("Wants to message you"));
        assert!(rows[2].group_start);
        assert_eq!(rows[3].title, "Juno");
        assert_eq!(rows[3].detail.as_deref(), Some("3 new"));
        assert_eq!(rows[4].title, "Lumen");
        assert_eq!(rows[4].detail.as_deref(), Some("Online"));
        assert_eq!(rows[5].title, "Raid night");
        assert_eq!(rows[5].detail.as_deref(), Some("3 people"));
        // The first row to take focus is the request.
        assert_eq!(panel.side.nav.row, 1);
    }

    #[test]
    fn an_empty_inbox_says_how_to_start() {
        let online = online();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Messages, &data(&online));
        let (_, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(subtitle.as_deref(), Some("Talk to your friends"));
        assert_eq!(ids(&rows), ["none"]);
    }

    #[test]
    fn a_conversation_shows_the_newest_at_the_bottom_above_the_field() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                4,
                4,
            )],
            vec![],
        );
        online.social.ingest("a", Fetch::Newest, texts("a", 1, 4));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Lumen");
        assert_eq!(subtitle.as_deref(), Some("Online"));
        assert_eq!(
            ids(&rows),
            [
                "message", "message", "message", "message", "compose", "send"
            ]
        );
        assert_eq!(rows[0].title, "Message 1");
        assert_eq!(rows[0].detail.as_deref(), Some("Lumen · 11:51"));
        assert_eq!(rows[3].detail.as_deref(), Some("You · 11:54"));
        assert_eq!(rows[3].title, "Message 4");
        assert!(rows[4].wide && rows[4].group_start);
        // It opens on the field, where the player writes.
        assert_eq!(panel.side.nav.row, 4);
        assert_eq!(
            panel.queued,
            [OnlineAction::OpenThread("a".into())],
            "the host fetches what the thread lacks"
        );
    }

    #[test]
    fn the_last_own_message_shows_when_the_other_read_it() {
        let mut online = online();
        let mut c = conversation("a", ConversationKind::Direct, &[(2, "Lumen")], 4, 4);
        c.marks = vec![ReadMark {
            account: AccountId(2),
            seq: 4,
        }];
        list(&mut online, vec![c], vec![]);
        online.social.ingest("a", Fetch::Newest, texts("a", 1, 4));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows[3].detail.as_deref(), Some("You · 11:54 · Seen"));
        assert_eq!(
            rows[1].detail.as_deref(),
            Some("You · 11:52"),
            "only the last one"
        );
    }

    #[test]
    fn message_text_is_shown_as_plain_text() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                1,
                1,
            )],
            vec![],
        );
        let mut page = texts("a", 1, 1);
        page.messages[0].text = "click https://evil.example/\u{202E}gpj.exe <b>now</b>".into();
        online.social.ingest("a", Fetch::Newest, page);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            rows[0].title,
            "click https://evil.example/gpj.exe <b>now</b>"
        );
        assert!(
            matches!(rows[0].control, Control::Action),
            "the row opens the message page and nothing else"
        );
    }

    #[test]
    fn writing_edits_the_draft_and_sends_it() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                0,
                0,
            )],
            vec![],
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            ids(&rows),
            ["compose", "send"],
            "an empty conversation has no group gap above the field"
        );
        let mut actions = Vec::new();
        panel.typed("compose", "On my way".into(), &mut actions);
        assert_eq!(
            actions,
            [OnlineAction::Draft {
                conversation: "a".into(),
                text: "On my way".into()
            }]
        );
        online.social.set_draft("a", "On my way".into());
        assert_eq!(
            panel.rows(&data(&online)).2[0].control,
            Control::Text("On my way".into())
        );
        let mut actions = Vec::new();
        panel.run("send", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::SendMessage("a".into())]);
        online.send_draft("a");
        assert_eq!(
            panel.rows(&data(&online)).2[1].detail.as_deref(),
            Some("Sending…")
        );
        // A text the cloud would refuse says so, and nothing goes out.
        online.social.upsert(conversation(
            "b",
            ConversationKind::Direct,
            &[(3, "Juno")],
            0,
            0,
        ));
        online.social.set_draft("b", "x".repeat(2001));
        panel.go(Page::Conversation("b".into()), &data(&online));
        assert_eq!(
            panel.rows(&data(&online)).2[1].detail.as_deref(),
            Some("Shorten it to 2,000 characters"),
        );
        assert!(!online.send_draft("b"));
    }

    #[test]
    fn enter_in_the_field_sends() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                0,
                0,
            )],
            vec![],
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        let mut actions = Vec::new();
        let ctx = egui::Context::default();
        panel.handle(
            &ctx,
            &crate::screens::rows::Change::Submit { row: "compose" },
            &data(&online),
            &mut actions,
        );
        assert_eq!(actions, [OnlineAction::SendMessage("a".into())]);
    }

    #[test]
    fn reading_further_back_fetches_when_the_memory_is_short() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                120,
                120,
            )],
            vec![],
        );
        online
            .social
            .ingest("a", Fetch::Newest, texts("a", 71, 120));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            rows.iter().filter(|r| r.id == "message").count(),
            40,
            "the page shows the last 40"
        );
        assert_eq!(rows[0].id, "earlier");
        // Ten more are in memory and show at once; the rest is fetched.
        let mut actions = Vec::new();
        panel.run("earlier", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::LoadEarlier("a".into())]);
        assert_eq!(
            panel
                .rows(&data(&online))
                .2
                .iter()
                .filter(|r| r.id == "message")
                .count(),
            50
        );
    }

    #[test]
    fn a_request_is_read_before_it_is_answered() {
        let mut online = online();
        let mut request = conversation("r", ConversationKind::Direct, &[(9, "Vale")], 2, 0);
        request.request = true;
        list(&mut online, vec![], vec![request]);
        let mut page = texts("r", 1, 2);
        for m in &mut page.messages {
            m.sender = account(9, "Vale");
        }
        online.social.ingest("r", Fetch::Newest, page);
        let mut panel = OnlinePanel::default();
        panel.go(Page::MessageRequest("r".into()), &data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Vale");
        assert_eq!(subtitle.as_deref(), Some("Wants to message you"));
        assert_eq!(
            ids(&rows),
            [
                "message",
                "message",
                "acceptRequest",
                "declineRequest",
                "blockSender",
                "requestNote"
            ]
        );
        assert_eq!(
            panel.side.nav.row, 0,
            "it opens on the first message: read before answering"
        );
        let mut actions = Vec::new();
        panel.run("acceptRequest", &data(&online), &mut actions);
        panel.run("blockSender", &data(&online), &mut actions);
        assert_eq!(
            actions,
            [
                OnlineAction::AcceptRequest("r".into()),
                OnlineAction::Block(AccountId(9)),
                OnlineAction::LeaveConversation("r".into())
            ]
        );
        assert_eq!(panel.page, Page::Messages);

        // Accepted: the page becomes the conversation.
        panel.go(Page::MessageRequest("r".into()), &data(&online));
        let mut accepted = online.social.get("r").unwrap().clone();
        accepted.request = false;
        online.social.upsert(accepted);
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Conversation("r".into()));
        // Declined or left: back to the list.
        online.social.remove("r");
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Messages);
    }

    #[test]
    fn a_message_page_copies_and_reports() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                2,
                2,
            )],
            vec![],
        );
        online.social.ingest("a", Fetch::Newest, texts("a", 1, 2));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Message("a".into(), 1), &data(&online));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(title, "Lumen");
        assert_eq!(subtitle.as_deref(), Some("11:51"));
        assert_eq!(ids(&rows), ["body", "copyText", "reportMessage"]);
        let mut actions = Vec::new();
        panel.run("copyText", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::CopyText("Message 1".into())]);
        panel.run("reportMessage", &data(&online), &mut actions);
        assert_eq!(panel.page, Page::Report(AccountId(2)));
        assert_eq!(
            panel.back_page(&data(&online)),
            Some(Page::Message("a".into(), 1)),
            "Back from the report returns to the message"
        );
        // The player's own messages cannot be reported.
        panel.go(Page::Message("a".into(), 2), &data(&online));
        assert_eq!(ids(&panel.rows(&data(&online)).2), ["body", "copyText"]);
        assert_eq!(panel.rows(&data(&online)).0, "You");
    }

    #[test]
    fn a_friend_can_be_written_to_and_invited_to_the_party() {
        let mut online = online();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            ids(&rows),
            [
                "sendMessage",
                "inviteToParty",
                "call",
                "viewProfile",
                "remove",
                "block",
                "report"
            ]
        );
        let mut actions = Vec::new();
        panel.run("sendMessage", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::OpenDirect(AccountId(2))]);
        // The row says it is opening while the question waits.
        online.open_direct(AccountId(2));
        assert_eq!(
            panel.rows(&data(&online)).2[0].detail.as_deref(),
            Some("Opening…")
        );
        // The answer lists the conversation, and the page follows into it.
        online.social.upsert(conversation(
            "a",
            ConversationKind::Direct,
            &[(2, "Lumen")],
            0,
            0,
        ));
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Conversation("a".into()));
        assert_eq!(
            panel.back_page(&data(&online)),
            Some(Page::Friend(AccountId(2)))
        );

        panel.go(Page::Friend(AccountId(2)), &data(&online));
        let mut actions = Vec::new();
        panel.run("sendMessage", &data(&online), &mut actions);
        assert!(actions.is_empty(), "there is a conversation already");
        assert_eq!(panel.page, Page::Conversation("a".into()));

        panel.go(Page::Friend(AccountId(3)), &data(&online));
        let mut actions = Vec::new();
        panel.run("inviteToParty", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::PartyInvite(vec![AccountId(3)])]);

        // A member who leads no party invites nobody.
        online
            .social
            .apply_party(Some(party(2, &[(2, "Lumen"), (1, "Mira")])));
        assert!(!ids(&panel.rows(&data(&online)).2).contains(&"inviteToParty"));
    }

    #[test]
    fn a_group_can_be_left_but_a_friend_and_the_party_chat_cannot() {
        let mut online = online();
        let mut group = conversation(
            "g",
            ConversationKind::Group,
            &[(2, "Lumen"), (3, "Juno")],
            0,
            0,
        );
        group.name = Some("Raid night".into());
        list(
            &mut online,
            vec![
                group,
                conversation("a", ConversationKind::Direct, &[(2, "Lumen")], 0, 0),
                conversation("pc", ConversationKind::Party, &[(2, "Lumen")], 0, 0),
            ],
            vec![],
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("g".into()), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows.last().unwrap().id, "leaveConversation");
        assert!(rows.last().unwrap().destructive);
        for other in ["a", "pc"] {
            panel.go(Page::Conversation(other.into()), &data(&online));
            assert!(!ids(&panel.rows(&data(&online)).2).contains(&"leaveConversation"));
        }
    }

    #[test]
    fn pages_follow_the_service_when_what_they_show_is_gone() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                1,
                1,
            )],
            vec![],
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        online.social.remove("a");
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Messages);
        panel.go(Page::PartyMember(AccountId(2)), &data(&online));
        panel.follow(&data(&online));
        assert_eq!(
            panel.page,
            Page::Party,
            "no party: the first page of the section"
        );
        // Signed out, everything returns to the start.
        panel.go(Page::Messages, &data(&online));
        online.account = None;
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Start);
    }

    #[test]
    fn back_leads_up_through_the_new_pages() {
        let mut online = online();
        online
            .social
            .apply_party(Some(party(1, &[(1, "Mira"), (2, "Lumen")])));
        list(
            &mut online,
            vec![conversation(
                "pc",
                ConversationKind::Party,
                &[(2, "Lumen")],
                0,
                0,
            )],
            vec![],
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        assert_eq!(
            panel.back_page(&data(&online)),
            None,
            "the first page of its section: Back leaves the screen"
        );
        panel.go(Page::Conversation("pc".into()), &data(&online));
        assert_eq!(
            panel.back_page(&data(&online)),
            Some(Page::Party),
            "the party's chat opened from the party"
        );
        panel.go(Page::Messages, &data(&online));
        panel.go(Page::Conversation("pc".into()), &data(&online));
        assert_eq!(panel.back_page(&data(&online)), Some(Page::Messages));
        panel.go(Page::PartyPicker, &data(&online));
        assert_eq!(panel.back_page(&data(&online)), Some(Page::Party));
    }

    #[test]
    fn the_field_keeps_the_focus_when_messages_arrive_above_it() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                0,
                0,
            )],
            vec![],
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        assert_eq!(ids(&panel.rows(&data(&online)).2), ["compose", "send"]);
        panel.anchor = {
            let rows = panel.rows(&data(&online)).2;
            panel.anchor_now(&data(&online), &rows)
        };
        assert_eq!(panel.anchor, Some(Anchor::FromEnd(2)));
        // The first fifty messages come in while the panel shows the field.
        online.social.note_last("a", 50);
        online.social.ingest("a", Fetch::Newest, texts("a", 1, 50));
        let rows = panel.rows(&data(&online)).2;
        panel.restore_anchor(&data(&online), &rows);
        assert_eq!(rows[panel.side.nav.row].id, "compose");
        // One more arrives: still the field.
        online.social.note_last("a", 51);
        online
            .social
            .ingest("a", Fetch::After(50), texts("a", 51, 51));
        let rows = panel.rows(&data(&online)).2;
        panel.restore_anchor(&data(&online), &rows);
        assert_eq!(rows[panel.side.nav.row].id, "compose");
    }

    #[test]
    fn a_message_keeps_the_focus_by_its_number_when_older_ones_load_above_it() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                120,
                120,
            )],
            vec![],
        );
        online
            .social
            .ingest("a", Fetch::Newest, texts("a", 71, 120));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        // Focus the 31st of the forty shown, which is message 111.
        let rows = panel.rows(&data(&online)).2;
        let first = rows::position(&rows, "message").unwrap();
        panel.side.nav.row = first + 30;
        assert_eq!(rows[panel.side.nav.row].title, "Message 111");
        panel.anchor = panel.anchor_now(&data(&online), &rows);
        assert_eq!(panel.anchor, Some(Anchor::Message(111)));
        // Forty more messages come in above it: the focus follows the message.
        online
            .social
            .ingest("a", Fetch::Before(71), texts("a", 21, 70));
        panel.shown += 40;
        let rows = panel.rows(&data(&online)).2;
        panel.restore_anchor(&data(&online), &rows);
        assert_eq!(rows[panel.side.nav.row].title, "Message 111");
    }

    #[test]
    fn the_leader_sees_what_a_member_page_does_before_pressing() {
        let mut online = online();
        online
            .social
            .apply_party(Some(party(1, &[(1, "Mira"), (2, "Lumen")])));
        let mut panel = OnlinePanel::default();
        panel.go(Page::PartyMember(AccountId(2)), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows[2].detail.as_deref(), Some("Lumen will lead the party"));
        assert_eq!(
            rows[3].detail.as_deref(),
            Some("Lumen is taken out of the party")
        );
    }

    #[test]
    fn sending_invitations_says_why_it_cannot_yet() {
        let mut online = online();
        let mut panel = OnlinePanel::default();
        panel.go(Page::PartyPicker, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let send = rows.iter().find(|r| r.id == "sendPartyInvites").unwrap();
        assert_eq!(send.detail.as_deref(), Some("Pick a friend first"));
        panel.picked.insert(AccountId(2));
        let rows = panel.rows(&data(&online)).2;
        let send = rows.iter().find(|r| r.id == "sendPartyInvites").unwrap();
        assert_eq!(send.detail, None);
        // A full party has no place to give.
        let members: Vec<(u64, String)> = (1..=8).map(|i| (i, format!("m{i}"))).collect();
        let members: Vec<(u64, &str)> = members.iter().map(|(i, h)| (*i, h.as_str())).collect();
        online.social.apply_party(Some(party(1, &members)));
        let rows = panel.rows(&data(&online)).2;
        let send = rows.iter().find(|r| r.id == "sendPartyInvites").unwrap();
        assert_eq!(send.detail.as_deref(), Some("The party is full"));
    }

    #[test]
    fn in_a_group_the_one_who_spoke_comes_first() {
        let mut online = online();
        let mut group = conversation(
            "g",
            ConversationKind::Group,
            &[(2, "Lumen"), (3, "Juno")],
            2,
            2,
        );
        group.name = Some("Raid night".into());
        list(&mut online, vec![group], vec![]);
        online.social.ingest("g", Fetch::Newest, texts("g", 1, 2));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("g".into()), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows[0].title, "Lumen: Message 1");
        assert_eq!(rows[0].detail.as_deref(), Some("11:51"));
        assert_eq!(
            rows[1].title, "Message 2",
            "the player's own words need no name"
        );
        assert_eq!(rows[1].detail.as_deref(), Some("You · 11:52"));
    }

    #[test]
    fn a_row_shows_a_message_on_one_line_and_the_page_keeps_its_breaks() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                1,
                1,
            )],
            vec![],
        );
        let mut page = texts("a", 1, 1);
        page.messages[0].text = "first\n\n\n   second".into();
        online.social.ingest("a", Fetch::Newest, page);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        assert_eq!(panel.rows(&data(&online)).2[0].title, "first second");
        panel.go(Page::Message("a".into(), 1), &data(&online));
        assert_eq!(
            panel.rows(&data(&online)).2[0].title,
            "first\n\n\n   second"
        );
    }

    #[test]
    fn a_chat_that_was_not_fetched_yet_says_it_loads() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                9,
                9,
            )],
            vec![],
        );
        let mut panel = OnlinePanel::default();
        panel.go(Page::Conversation("a".into()), &data(&online));
        assert_eq!(
            ids(&panel.rows(&data(&online)).2),
            ["loading", "compose", "send"]
        );
        online.social.ingest("a", Fetch::Newest, texts("a", 1, 9));
        assert!(!ids(&panel.rows(&data(&online)).2).contains(&"loading"));
    }

    #[test]
    fn a_chat_opened_from_a_friend_is_not_forced_on_a_player_who_left() {
        let mut online = online();
        let mut panel = OnlinePanel::default();
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        let mut actions = Vec::new();
        panel.run("sendMessage", &data(&online), &mut actions);
        assert_eq!(panel.opening, Some(AccountId(2)));
        panel.go(Page::Account, &data(&online));
        online.social.upsert(conversation(
            "a",
            ConversationKind::Direct,
            &[(2, "Lumen")],
            0,
            0,
        ));
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Account);
        assert_eq!(panel.opening, None);
    }

    #[test]
    fn back_from_a_message_does_not_forget_where_the_chat_was_opened() {
        let mut online = online();
        list(
            &mut online,
            vec![conversation(
                "a",
                ConversationKind::Direct,
                &[(2, "Lumen")],
                2,
                2,
            )],
            vec![],
        );
        online.social.ingest("a", Fetch::Newest, texts("a", 1, 2));
        let mut panel = OnlinePanel::default();
        panel.go(Page::Friend(AccountId(2)), &data(&online));
        panel.go(Page::Conversation("a".into()), &data(&online));
        panel.go(Page::Message("a".into(), 1), &data(&online));
        panel.go(Page::Conversation("a".into()), &data(&online));
        assert_eq!(
            panel.back_page(&data(&online)),
            Some(Page::Friend(AccountId(2)))
        );
    }

    #[test]
    fn the_title_of_a_chat_follows_its_kind() {
        let me = AccountId(1);
        let en = Language::English;
        let direct = conversation("a", ConversationKind::Direct, &[(2, "Lumen")], 0, 0);
        assert_eq!(chat_title(&direct, me, en), "Lumen");
        let mut group = conversation(
            "g",
            ConversationKind::Group,
            &[(2, "Lumen"), (3, "Juno")],
            0,
            0,
        );
        assert_eq!(chat_title(&group, me, en), "Lumen, Juno");
        group.name = Some("Raid night".into());
        assert_eq!(chat_title(&group, me, en), "Raid night");
        let party = conversation("p", ConversationKind::Party, &[(2, "Lumen")], 0, 0);
        assert_eq!(chat_title(&party, me, en), "Party Chat");
        assert_eq!(chat_title(&party, me, Language::Turkish), "Parti Sohbeti");
    }

    // --- party voice ---------------------------------------------------------------------------

    use crate::screens::rows::Change;
    use anyps5_core::online::control::{VoiceLevel, VoicePeer, VoiceState};
    use anyps5_core::online::{DeviceKind, Devices, Problem, VoiceMode, VoicePrefs};

    fn voice_online(leader: u64) -> Online {
        let mut online = online();
        online.social.apply_party(Some(party(
            leader,
            &[(1, "Mira"), (2, "Lumen"), (3, "Juno")],
        )));
        online
    }

    fn switch_on(online: &mut Online) {
        online.voice.load(VoicePrefs {
            enabled: true,
            ..VoicePrefs::default()
        });
        online.voice.state = VoiceState::On;
    }

    fn speaking(id: u64) -> VoicePeer {
        VoicePeer {
            account: AccountId(id),
            level: VoiceLevel {
                level: 50,
                speaking: true,
            },
            link: None,
        }
    }

    fn change(panel: &mut OnlinePanel, online: &Online, change: Change) -> Vec<OnlineAction> {
        let mut actions = Vec::new();
        panel.handle(
            &egui::Context::default(),
            &change,
            &data(online),
            &mut actions,
        );
        actions
    }

    #[test]
    fn the_party_page_has_a_voice_group_between_the_chat_and_the_members() {
        let online = voice_online(1);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            ids(&rows),
            [
                "partyChat",
                "inviteFriends",
                "voiceCaption",
                "voiceProvider",
                "voiceOn",
                "voiceSettings",
                "membersCaption",
                "member",
                "partyMember",
                "partyMember",
                "leaveParty"
            ]
        );
        // Off: the toggle says so, and nothing else about the microphone shows.
        let Control::Toggle(on) = rows[4].control else {
            panic!("not a toggle");
        };
        assert!(!on);
        assert_eq!(rows[4].detail.as_deref(), Some("Off"));
    }

    #[test]
    fn with_voice_on_the_page_shows_the_microphone_the_mode_and_a_note() {
        let mut online = voice_online(1);
        switch_on(&mut online);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let voice: Vec<_> = ids(&rows)
            .into_iter()
            .skip_while(|id| *id != "voiceOn")
            .take_while(|id| *id != "membersCaption")
            .collect();
        assert_eq!(
            voice,
            [
                "voiceOn",
                "micState",
                "voiceMode",
                "voiceNote",
                "voiceSettings"
            ]
        );
        let mic = rows.iter().find(|r| r.id == "micState").unwrap();
        assert!(matches!(&mic.control, Control::Info(Some(v)) if v == "Muted · hold V to talk"));
        assert_eq!(mic.icon, Some(Icon::MICROPHONE_SLASH));
        let note = rows.iter().find(|r| r.id == "voiceNote").unwrap();
        assert_eq!(
            note.title,
            "Push to talk works while the launcher is in front."
        );
        // A running game has the keyboard: the note says what to do instead.
        let playing = OnlineData {
            game_running: true,
            ..data(&online)
        };
        let rows = panel.rows(&playing).2;
        let note = rows.iter().find(|r| r.id == "voiceNote").unwrap();
        assert!(note.title.contains("Switch to Voice Activity"));
        // Voice activity has its own note and no talk key on the voice page.
        online.set_voice_mode(VoiceMode::VoiceActivity);
        let rows = panel.rows(&data(&online)).2;
        let note = rows.iter().find(|r| r.id == "voiceNote").unwrap();
        assert!(
            note.title
                .starts_with("The microphone opens whenever you speak")
        );
        panel.go(Page::Voice, &data(&online));
        assert!(!ids(&panel.rows(&data(&online)).2).contains(&"talkKey"));
    }

    #[test]
    fn the_toggle_and_the_mode_become_actions() {
        let online = voice_online(1);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        assert_eq!(
            change(
                &mut panel,
                &online,
                Change::Toggle {
                    row: "voiceOn",
                    on: true
                }
            ),
            [OnlineAction::Voice(true)]
        );
        assert_eq!(
            change(
                &mut panel,
                &online,
                Change::Choice {
                    row: "voiceMode",
                    index: 1
                }
            ),
            [OnlineAction::VoiceMode(VoiceMode::VoiceActivity)]
        );
        assert_eq!(
            change(
                &mut panel,
                &online,
                Change::Choice {
                    row: "voiceMode",
                    index: 0
                }
            ),
            [OnlineAction::VoiceMode(VoiceMode::PushToTalk)]
        );
    }

    #[test]
    fn the_state_of_voice_is_said_in_words() {
        let mut online = voice_online(1);
        let mut panel = OnlinePanel::default();
        let detail = |panel: &mut OnlinePanel, online: &Online| {
            panel.go(Page::Party, &data(online));
            panel
                .rows(&data(online))
                .2
                .iter()
                .find(|r| r.id == "voiceOn")
                .unwrap()
                .detail
                .clone()
        };
        online.voice.load(VoicePrefs {
            enabled: true,
            ..VoicePrefs::default()
        });
        assert_eq!(detail(&mut panel, &online).as_deref(), Some("Connecting…"));
        online.voice.state = VoiceState::On;
        assert_eq!(detail(&mut panel, &online).as_deref(), Some("On"));
        online.social.apply_party(None);
        let home = {
            panel.go(Page::Party, &data(&online));
            panel.rows(&data(&online)).2
        };
        let row = home.iter().find(|r| r.id == "voice").unwrap();
        assert_eq!(
            row.detail.as_deref(),
            Some("Starts when you are in a party")
        );
    }

    #[test]
    fn a_service_that_gave_up_says_what_to_do() {
        let mut online = voice_online(1);
        online.voice.problem = Some(Problem {
            message: "access denied".into(),
            denied: true,
        });
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let ids = ids(&rows);
        assert!(ids.windows(2).any(|w| w == ["voiceOn", "voiceFailed"]));
        assert!(!ids.contains(&"voiceMode"));
        let note = rows.iter().find(|r| r.id == "voiceFailed").unwrap();
        assert!(note.title.contains("did not allow microphone access"));
        online.voice.problem = Some(Problem {
            message: "no input device".into(),
            denied: false,
        });
        let rows = panel.rows(&data(&online)).2;
        let note = rows.iter().find(|r| r.id == "voiceFailed").unwrap();
        assert!(note.title.starts_with("Voice could not start"));
    }

    #[test]
    fn members_who_speak_or_are_muted_say_so_in_words_and_icons() {
        let mut online = voice_online(2);
        switch_on(&mut online);
        online.voice.peers = vec![speaking(2), speaking(3)];
        online.set_peer_mute(AccountId(3), true);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let members: Vec<_> = rows
            .iter()
            .filter(|r| matches!(r.id, "member" | "partyMember"))
            .collect();
        assert_eq!(members.len(), 3);
        // The player, silent and in voice: nothing to add (they have no presence of their own).
        assert_eq!(members[0].detail.as_deref(), None);
        // Lumen speaks and leads; the party's voice flag says nothing is wrong, so no "Not in voice".
        assert_eq!(
            members[1].detail.as_deref(),
            Some("Speaking · Leader · Online")
        );
        assert_eq!(members[1].icon, Some(Icon::SPEAKER_HIGH));
        // Juno speaks too, but the player muted Juno: not heard, and said so.
        assert_eq!(members[2].detail.as_deref(), Some("Muted · Online"));
        assert_eq!(members[2].icon, Some(Icon::SPEAKER_SLASH));
    }

    #[test]
    fn a_member_with_voice_off_is_said_not_to_be_in_voice() {
        let mut online = voice_online(1);
        switch_on(&mut online);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let lumen = rows.iter().find(|r| r.id == "partyMember").unwrap();
        assert_eq!(lumen.detail.as_deref(), Some("Not in voice · Online"));
        // With voice off nobody is called that.
        online.voice.load(VoicePrefs::default());
        let rows = panel.rows(&data(&online)).2;
        let lumen = rows.iter().find(|r| r.id == "partyMember").unwrap();
        assert_eq!(lumen.detail.as_deref(), Some("Online"));
    }

    #[test]
    fn every_member_can_be_turned_down_or_muted_but_only_the_leader_manages() {
        let online = voice_online(2);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let mut actions = Vec::new();
        panel.run("partyMember", &data(&online), &mut actions);
        assert_eq!(panel.page, Page::PartyMember(AccountId(2)));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(ids(&rows), ["peerVolume", "peerMute"]);
        assert!(matches!(
            &rows[0].control,
            Control::Stepper { value: 100, min: 0, max: 200, step: 10, text } if text == "100%"
        ));
        assert_eq!(
            change(
                &mut panel,
                &online,
                Change::Stepper {
                    row: "peerVolume",
                    value: 140
                }
            ),
            [OnlineAction::PeerVolume(AccountId(2), 140)]
        );
        assert_eq!(
            change(
                &mut panel,
                &online,
                Change::Toggle {
                    row: "peerMute",
                    on: true
                }
            ),
            [OnlineAction::PeerMute(AccountId(2), true)]
        );
        // The page stays when the player is no leader.
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::PartyMember(AccountId(2)));
        // The leader gets the management rows below.
        let online = voice_online(1);
        panel.go(Page::PartyMember(AccountId(2)), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            ids(&rows),
            ["peerVolume", "peerMute", "makeLeader", "removeMember"]
        );
        // A member who left takes the page with them.
        let mut gone = voice_online(1);
        gone.social.apply_party(Some(Party {
            revision: 2,
            ..party(1, &[(1, "Mira"), (3, "Juno")])
        }));
        panel.follow(&data(&gone));
        assert_eq!(panel.page, Page::Party);
    }

    #[test]
    fn the_member_page_shows_what_was_chosen() {
        let mut online = voice_online(1);
        online.set_peer_volume(AccountId(2), 130);
        online.set_peer_mute(AccountId(2), true);
        let mut panel = OnlinePanel::default();
        panel.go(Page::PartyMember(AccountId(2)), &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert!(
            matches!(&rows[0].control, Control::Stepper { value: 130, text, .. } if text == "130%")
        );
        assert!(matches!(rows[1].control, Control::Toggle(true)));
        assert_eq!(
            rows[1].detail.as_deref(),
            Some("Only you stop hearing Lumen")
        );
    }

    #[test]
    fn the_voice_page_offers_the_key_and_the_devices() {
        let mut online = voice_online(1);
        online.voice.load(VoicePrefs {
            enabled: true,
            input: Some("Headset".into()),
            ..VoicePrefs::default()
        });
        let mut panel = OnlinePanel::default();
        let mut actions = Vec::new();
        panel.go(Page::Party, &data(&online));
        panel.run("voiceSettings", &data(&online), &mut actions);
        assert_eq!(actions, [OnlineAction::RefreshVoiceDevices]);
        assert_eq!(panel.page, Page::Voice);
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(
            ids(&rows),
            [
                "voiceProvider",
                "voiceOn",
                "voiceMode",
                "voiceNote",
                "talkKey",
                "voiceInput",
                "voiceOutput",
                "refreshDevices",
                "devicesNote"
            ]
        );
        assert!(
            matches!(&rows[4].control, Control::Keys { sources, capturing: false } if sources == &["KEY:V"])
        );
        let input = rows.iter().find(|r| r.id == "voiceInput").unwrap();
        assert_eq!(input.detail.as_deref(), Some("Headset"));
        let output = rows.iter().find(|r| r.id == "voiceOutput").unwrap();
        assert_eq!(output.detail.as_deref(), Some("System Default"));
        // The devices are not known until the service answers.
        assert_eq!(
            rows.last().unwrap().title,
            "Devices appear once the net service answers."
        );
        // Back leads to where the page was opened.
        assert_eq!(panel.back_page(&data(&online)), Some(Page::Party));
        panel.go(Page::Party, &data(&online));
        panel.run("voice", &data(&online), &mut actions);
        assert_eq!(panel.page, Page::Voice);
        assert_eq!(panel.back_page(&data(&online)), Some(Page::Party));
    }

    #[test]
    fn a_pc_without_a_microphone_or_sound_says_so() {
        let mut online = voice_online(1);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Voice, &data(&online));
        let note = |panel: &OnlinePanel, online: &Online| {
            panel.rows(&data(online)).2.last().unwrap().title.clone()
        };
        online.voice.devices = Some(Devices {
            output: vec!["Speakers".into()],
            ..Devices::default()
        });
        assert!(note(&panel, &online).starts_with("No microphone found"));
        online.voice.devices = Some(Devices::default());
        assert!(note(&panel, &online).starts_with("The net service reports no sound devices"));
        online.voice.devices = Some(Devices {
            input: vec!["Mic".into()],
            output: vec!["Speakers".into()],
            ..Devices::default()
        });
        let rows = panel.rows(&data(&online)).2;
        assert_eq!(rows.last().unwrap().id, "refreshDevices");
    }

    #[test]
    fn a_device_is_picked_from_a_list_that_starts_with_the_default() {
        let mut online = voice_online(1);
        online.voice.devices = Some(Devices {
            input: vec!["Headset".into(), "Built-in".into()],
            output: vec!["Speakers".into()],
            ..Devices::default()
        });
        online.voice.load(VoicePrefs {
            input: Some("Old Mic".into()),
            ..VoicePrefs::default()
        });
        let mut panel = OnlinePanel::default();
        let mut actions = Vec::new();
        panel.go(Page::Voice, &data(&online));
        panel.run("voiceInput", &data(&online), &mut actions);
        assert_eq!(panel.page, Page::VoiceDevices(DeviceKind::Input));
        let (title, subtitle, rows) = panel.rows(&data(&online));
        assert_eq!(
            (title.as_str(), subtitle.as_deref()),
            ("Microphone", Some("Choose the microphone"))
        );
        assert_eq!(
            ids(&rows),
            [
                "deviceDefault",
                "deviceName",
                "deviceName",
                "deviceName",
                "devicesNote"
            ]
        );
        // A chosen device that is not plugged in stays visible, and says so.
        assert_eq!(rows[3].title, "Old Mic");
        assert_eq!(
            rows[3].detail.as_deref(),
            Some("This device is not connected right now.")
        );
        assert_eq!(rows[0].detail, None);
        // Choosing the second one.
        panel.side.nav.row = 2;
        panel.run("deviceName", &data(&online), &mut actions);
        assert_eq!(
            actions,
            [OnlineAction::VoiceDevice(
                DeviceKind::Input,
                Some("Built-in".into())
            )]
        );
        assert_eq!(panel.page, Page::Voice);
        // And the default.
        let mut actions = Vec::new();
        panel.go(Page::VoiceDevices(DeviceKind::Output), &data(&online));
        panel.run("deviceDefault", &data(&online), &mut actions);
        assert_eq!(
            actions,
            [OnlineAction::VoiceDevice(DeviceKind::Output, None)]
        );
        assert_eq!(panel.page, Page::Voice);
    }

    #[test]
    fn the_talk_key_is_captured_and_esc_keeps_the_old_one() {
        use crate::ui::input::{Command, InputEvent};
        let mut online = voice_online(1);
        online.voice.load(VoicePrefs {
            enabled: true,
            ..VoicePrefs::default()
        });
        let mut panel = OnlinePanel::default();
        panel.go(Page::Voice, &data(&online));
        assert!(!panel.capturing);
        change(&mut panel, &online, Change::Capture { row: "talkKey" });
        assert!(panel.capturing);
        let rows = panel.rows(&data(&online)).2;
        let key = rows.iter().find(|r| r.id == "talkKey").unwrap();
        assert_eq!(
            key.detail.as_deref(),
            Some("Press a key. Esc keeps the current one.")
        );
        let pressed = |key| InputEvent::CapturedKey {
            key,
            physical_key: None,
            modifiers: egui::Modifiers::NONE,
        };
        // A key the launcher reads itself is ignored; the row keeps waiting.
        let mut actions = Vec::new();
        panel.capture(
            &[pressed(egui::Key::Space), pressed(egui::Key::ArrowUp)],
            &mut actions,
        );
        assert!(actions.is_empty() && panel.capturing);
        panel.capture(&[pressed(egui::Key::Escape)], &mut actions);
        assert!(actions.is_empty() && !panel.capturing);
        // A pad's back cancels too.
        panel.capturing = true;
        panel.capture(&[InputEvent::Command(Command::Back)], &mut actions);
        assert!(!panel.capturing);
        panel.capturing = true;
        panel.capture(&[pressed(egui::Key::F5)], &mut actions);
        assert_eq!(actions, [OnlineAction::VoiceKey("F5".into())]);
        assert!(!panel.capturing);
        // Leaving the page ends a capture that was never answered.
        panel.capturing = true;
        panel.go(Page::Home, &data(&online));
        assert!(!panel.capturing);
    }

    #[test]
    fn the_chosen_key_shows_as_a_key_cap_and_in_the_microphone_text() {
        let mut online = voice_online(1);
        online.voice.load(VoicePrefs {
            enabled: true,
            key: Some("F5".into()),
            ..VoicePrefs::default()
        });
        let mut panel = OnlinePanel::default();
        panel.go(Page::Party, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        let mic = rows.iter().find(|r| r.id == "micState").unwrap();
        assert!(matches!(&mic.control, Control::Info(Some(v)) if v == "Muted · hold F5 to talk"));
        panel.go(Page::Voice, &data(&online));
        let rows = panel.rows(&data(&online)).2;
        assert!(
            matches!(&rows.iter().find(|r| r.id == "talkKey").unwrap().control,
            Control::Keys { sources, .. } if sources == &["KEY:F5"])
        );
        // A stored name that is no key falls back to the default.
        online.voice.load(VoicePrefs {
            key: Some("Space".into()),
            ..VoicePrefs::default()
        });
        assert_eq!(super::super::talk_key(&online), egui::Key::V);
    }

    #[test]
    fn the_voice_page_follows_the_service_when_the_player_signs_out() {
        let mut online = voice_online(1);
        let mut panel = OnlinePanel::default();
        panel.go(Page::Voice, &data(&online));
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Voice);
        online.account = None;
        panel.follow(&data(&online));
        assert_eq!(panel.page, Page::Start);
    }
}
