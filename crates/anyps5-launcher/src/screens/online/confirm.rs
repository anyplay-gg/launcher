//! Confirmations (docs/DESIGN.md 17, "Confirmations"): the page that asks before something that
//! cannot be taken back. Block, remove a friend, unsend, leave a party or a conversation and
//! remove a member run only from here. The page is a headline that asks, one line that says what
//! it ends, `Cancel` (first, so it holds the focus) and the confirming button in the danger ink.
//! Back leads where the player came from, like Cancel.

use anyps5_core::online::api::AccountId;
use anyps5_core::online::others;

use super::{OnlineAction, OnlineData, OnlinePanel, Page};
use crate::screens::online_confirm_strings::ConfirmText as C;
use crate::screens::online_social_strings::SocialText as S;
use crate::screens::online_strings::OnlineText as T;
use crate::screens::rows::Row;
use crate::ui::i18n::{Arg, Key};
use crate::ui::icons::Icon;

/// What is being asked about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// Block a player (from their friend page or their request).
    Block(AccountId),
    RemoveFriend(AccountId),
    /// Block the other of a direct conversation.
    BlockChat(String),
    /// Block the sender of a message request and drop it.
    BlockRequest(String),
    LeaveConversation(String),
    Unsend {
        conversation: String,
        seq: u64,
    },
    LeaveParty,
    RemoveMember(AccountId),
}

impl Ask {
    /// The question has nothing left to ask about: the thing is gone.
    fn is_stale(&self, data: &OnlineData<'_>) -> bool {
        let online = data.online;
        let social = &online.social;
        match self {
            Ask::Block(_) => false,
            Ask::RemoveFriend(id) => online.friend(*id).is_none(),
            Ask::BlockChat(id) | Ask::BlockRequest(id) | Ask::LeaveConversation(id) => {
                social.get(id).is_none()
            }
            Ask::Unsend {
                conversation, seq, ..
            } => !social.messages(conversation).iter().any(|m| m.seq == *seq),
            Ask::LeaveParty => social.party.is_none(),
            Ask::RemoveMember(id) => !social.in_party(*id) || !social.leads(OnlinePanel::me(data)),
        }
    }
}

impl OnlinePanel {
    /// Shows the question; Back and Cancel lead to the page it was asked on.
    pub(super) fn ask(&mut self, ask: Ask, data: &OnlineData<'_>) {
        let from = self.page.clone();
        self.go(Page::Confirm(ask, Box::new(from)), data);
    }

    /// The page on screen is a confirmation.
    pub fn asks(&self) -> bool {
        matches!(self.page, Page::Confirm(..))
    }

    /// The player's name for someone, for a sentence.
    fn who(data: &OnlineData<'_>, id: AccountId) -> String {
        data.online
            .known(id)
            .map(|a| a.handle.clone())
            .unwrap_or_default()
    }

    /// The other of a conversation, for a sentence.
    fn who_in(data: &OnlineData<'_>, conversation: &str) -> String {
        let me = Self::me(data);
        data.online
            .social
            .get(conversation)
            .and_then(|c| others(c, me).first().map(|a| a.handle.clone()))
            .unwrap_or_default()
    }

    /// The title, the question and the rows of a confirmation.
    pub(super) fn confirm_rows(
        &self,
        ask: &Ask,
        data: &OnlineData<'_>,
        rows: &mut Vec<Row>,
    ) -> (String, Option<String>) {
        let language = data.language;
        let named = |text: C, who: &str| text.format(language, &[Arg::Str(who)]);
        let (title, note, label, icon) = match ask {
            Ask::Block(id) => {
                let who = Self::who(data, *id);
                (
                    named(C::BlockTitle, &who),
                    named(C::BlockNote, &who),
                    T::Block.text(language),
                    Icon::PROHIBIT,
                )
            }
            Ask::BlockChat(id) | Ask::BlockRequest(id) => {
                let who = Self::who_in(data, id);
                (
                    named(C::BlockTitle, &who),
                    named(C::BlockNote, &who),
                    T::Block.text(language),
                    Icon::PROHIBIT,
                )
            }
            Ask::RemoveFriend(id) => (
                named(C::RemoveFriendTitle, &Self::who(data, *id)),
                C::RemoveFriendNote.text(language).to_owned(),
                T::RemoveFriend.text(language),
                Icon::USER_MINUS,
            ),
            Ask::Unsend { .. } => (
                C::UnsendTitle.text(language).to_owned(),
                C::UnsendNote.text(language).to_owned(),
                S::Unsend.text(language),
                Icon::TRASH,
            ),
            Ask::LeaveParty => (
                C::LeavePartyTitle.text(language).to_owned(),
                C::LeavePartyNote.text(language).to_owned(),
                S::LeaveParty.text(language),
                Icon::SIGN_OUT,
            ),
            Ask::RemoveMember(id) => (
                named(C::RemoveMemberTitle, &Self::who(data, *id)),
                C::RemoveMemberNote.text(language).to_owned(),
                S::RemoveFromParty.text(language),
                Icon::USER_MINUS,
            ),
            Ask::LeaveConversation(_) => (
                C::LeaveConversationTitle.text(language).to_owned(),
                C::LeaveConversationNote.text(language).to_owned(),
                S::LeaveConversation.text(language),
                Icon::SIGN_OUT,
            ),
        };
        // Cancel comes first: a page opens on its first row, so the safe answer holds the focus.
        rows.push(Row::command("confirmCancel", language.text(Key::Cancel)).icon(Icon::X));
        rows.push(
            Row::command("confirmYes", label)
                .icon(icon)
                .destructive()
                .group_start(),
        );
        (title, Some(note))
    }

    /// What the two buttons of a confirmation do. Returns true when `row` was one of them.
    pub(super) fn confirm_run(
        &mut self,
        row: &str,
        data: &OnlineData<'_>,
        actions: &mut Vec<OnlineAction>,
    ) -> bool {
        let Page::Confirm(ask, from) = self.page.clone() else {
            return false;
        };
        match row {
            "confirmCancel" => self.go(*from, data),
            "confirmYes" => self.confirmed(ask, data, actions),
            _ => return false,
        }
        true
    }

    /// Does what was asked and goes to the page that shows its result.
    fn confirmed(&mut self, ask: Ask, data: &OnlineData<'_>, actions: &mut Vec<OnlineAction>) {
        let me = Self::me(data);
        let social = &data.online.social;
        let next = match ask {
            Ask::Block(id) => {
                actions.push(OnlineAction::Block(id));
                Page::Home
            }
            Ask::RemoveFriend(id) => {
                actions.push(OnlineAction::RemoveFriend(id));
                Page::Home
            }
            Ask::BlockChat(id) => {
                if let Some(other) = social
                    .get(&id)
                    .and_then(|c| others(c, me).first().map(|a| a.id))
                {
                    actions.push(OnlineAction::Block(other));
                }
                Page::Messages
            }
            Ask::BlockRequest(id) => {
                if let Some(c) = social.get(&id) {
                    for other in others(c, me) {
                        actions.push(OnlineAction::Block(other.id));
                    }
                }
                actions.push(OnlineAction::LeaveConversation(id));
                Page::Messages
            }
            Ask::LeaveConversation(id) => {
                actions.push(OnlineAction::LeaveConversation(id));
                Page::Messages
            }
            Ask::Unsend { conversation, seq } => {
                actions.push(OnlineAction::Unsend {
                    conversation: conversation.clone(),
                    seq,
                });
                Page::Conversation(conversation)
            }
            Ask::LeaveParty => {
                actions.push(OnlineAction::PartyLeave);
                Page::Party
            }
            Ask::RemoveMember(id) => {
                actions.push(OnlineAction::PartyKick(id));
                Page::Party
            }
        };
        self.go(next, data);
    }

    /// Where a confirmation whose subject is gone leads, if it is one.
    pub(super) fn confirm_follow(&self, data: &OnlineData<'_>) -> Option<Page> {
        match &self.page {
            Page::Confirm(ask, from) if ask.is_stale(data) => Some((**from).clone()),
            _ => None,
        }
    }
}
