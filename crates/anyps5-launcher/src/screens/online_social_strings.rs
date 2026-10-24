//! Texts of parties and messages in the online area. English and Turkish like the rest. Row
//! titles are in title case, notes and toasts in sentence case, as everywhere in the launcher.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SocialText {
    // Party
    Party,
    /// A row in Home for a player without a party.
    StartParty,
    StartPartyDetail,
    /// `%@` members of `%@`.
    PartySubtitle,
    PartyMembers,
    Leader,
    PartyChat,
    InviteFriends,
    InviteToParty,
    LeaveParty,
    TakeParty,
    MakeLeader,
    RemoveFromParty,
    JoinParty,
    /// The detail of Join Party while the player has a party of their own.
    LeaveFirst,
    /// `%@` = how many members.
    PartyInvitationDetail,
    NobodyToInvite,
    /// `%@` = who.
    MakeLeaderDetail,
    /// `%@` = who.
    RemoveDetail,
    PickFirst,
    PartyFull,
    // Messages
    Messages,
    /// `%@` = how many.
    MessagesUnread,
    MessagesQuiet,
    /// `%@` = how many.
    RequestsCount,
    MessageRequests,
    Conversations,
    NoConversations,
    SendMessage,
    NoMessages,
    Loading,
    /// `%@` = how many.
    NewCount,
    WantsToMessage,
    Opening,
    ShowEarlier,
    MessageField,
    Send,
    TooLong,
    LeaveConversation,
    /// `%@` = how many people.
    GroupPeople,
    Seen,
    CopyText,
    Copied,
    RequestNote,
    // Toasts
    /// `%@` = who.
    PartyInvitationArrived,
    /// `%@` = who, `%@` = the start of the message.
    MessageArrived,
    /// `%@` = who.
    MessageRequestArrived,
    /// The profile button's name while messages wait; `%@` = how many.
    ProfileUnread,
}

impl SocialText {
    pub const fn english(self) -> &'static str {
        match self {
            SocialText::Party => "Party",
            SocialText::StartParty => "Start a Party",
            SocialText::StartPartyDetail => "Stay together across games",
            SocialText::PartySubtitle => "%@ of %@ members",
            SocialText::PartyMembers => "Members",
            SocialText::Leader => "Leader",
            SocialText::PartyChat => "Party Chat",
            SocialText::InviteFriends => "Invite Friends",
            SocialText::InviteToParty => "Invite to Party",
            SocialText::LeaveParty => "Leave Party",
            SocialText::TakeParty => "Take the Party to My Game",
            SocialText::MakeLeader => "Make Leader",
            SocialText::RemoveFromParty => "Remove from Party",
            SocialText::JoinParty => "Join Party",
            SocialText::LeaveFirst => "Leave your party first to join this one.",
            SocialText::PartyInvitationDetail => "Party of %@",
            SocialText::NobodyToInvite => "None of your friends is online and free to invite.",
            SocialText::MakeLeaderDetail => "%@ will lead the party",
            SocialText::RemoveDetail => "%@ is taken out of the party",
            SocialText::PickFirst => "Pick a friend first",
            SocialText::PartyFull => "The party is full",
            SocialText::Messages => "Messages",
            SocialText::MessagesUnread => "%@ unread",
            SocialText::MessagesQuiet => "Talk to your friends",
            SocialText::RequestsCount => "Requests: %@",
            SocialText::MessageRequests => "Message Requests",
            SocialText::Conversations => "Conversations",
            SocialText::NoConversations => {
                "No conversations yet. Open a friend's page and choose Send Message."
            }
            SocialText::SendMessage => "Send Message",
            SocialText::NoMessages => "No messages yet. Say hello.",
            SocialText::Loading => "Loading…",
            SocialText::NewCount => "%@ new",
            SocialText::WantsToMessage => "Wants to message you",
            SocialText::Opening => "Opening…",
            SocialText::ShowEarlier => "Show Earlier Messages",
            SocialText::MessageField => "Message",
            SocialText::Send => "Send",
            SocialText::TooLong => "Shorten it to 2,000 characters",
            SocialText::LeaveConversation => "Leave Conversation",
            SocialText::GroupPeople => "%@ people",
            SocialText::Seen => "Seen",
            SocialText::CopyText => "Copy Text",
            SocialText::Copied => "Copied",
            SocialText::RequestNote => {
                "They cannot see that you read this. Links in messages do not open."
            }
            SocialText::PartyInvitationArrived => "%@ invites you to a party",
            SocialText::MessageArrived => "%@: %@",
            SocialText::MessageRequestArrived => "%@ wants to message you",
            SocialText::ProfileUnread => "%@ unread",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            SocialText::Party => "Parti",
            SocialText::StartParty => "Parti Kur",
            SocialText::StartPartyDetail => "Oyunlar arasında bir arada kal",
            SocialText::PartySubtitle => "%@ / %@ üye",
            SocialText::PartyMembers => "Üyeler",
            SocialText::Leader => "Lider",
            SocialText::PartyChat => "Parti Sohbeti",
            SocialText::InviteFriends => "Arkadaşları Davet Et",
            SocialText::InviteToParty => "Partiye Davet Et",
            SocialText::LeaveParty => "Partiden Ayrıl",
            SocialText::TakeParty => "Partiyi Oyunuma Götür",
            SocialText::MakeLeader => "Lider Yap",
            SocialText::RemoveFromParty => "Partiden Çıkar",
            SocialText::JoinParty => "Partiye Katıl",
            SocialText::LeaveFirst => "Bu partiye katılmak için önce kendi partinden ayrıl.",
            SocialText::PartyInvitationDetail => "%@ kişilik parti",
            SocialText::MakeLeaderDetail => "%@ partiyi yönetecek",
            SocialText::RemoveDetail => "%@ partiden çıkarılır",
            SocialText::PickFirst => "Önce bir arkadaş seç",
            SocialText::PartyFull => "Parti dolu",
            SocialText::NobodyToInvite => {
                "Arkadaşlarından hiçbiri çevrimiçi değil ya da davet edilebilecek durumda değil."
            }
            SocialText::Messages => "Mesajlar",
            SocialText::MessagesUnread => "%@ okunmamış",
            SocialText::MessagesQuiet => "Arkadaşlarınla konuş",
            SocialText::RequestsCount => "İstekler: %@",
            SocialText::MessageRequests => "Mesaj İstekleri",
            SocialText::Conversations => "Sohbetler",
            SocialText::NoConversations => {
                "Henüz sohbet yok. Bir arkadaşının sayfasını aç ve Mesaj Gönder'i seç."
            }
            SocialText::SendMessage => "Mesaj Gönder",
            SocialText::NoMessages => "Henüz mesaj yok. Merhaba de.",
            SocialText::Loading => "Yükleniyor…",
            SocialText::NewCount => "%@ yeni",
            SocialText::WantsToMessage => "Sana mesaj yazmak istiyor",
            SocialText::Opening => "Açılıyor…",
            SocialText::ShowEarlier => "Önceki Mesajları Göster",
            SocialText::MessageField => "Mesaj",
            SocialText::Send => "Gönder",
            SocialText::TooLong => "En fazla 2.000 karakter olsun",
            SocialText::LeaveConversation => "Sohbetten Ayrıl",
            SocialText::GroupPeople => "%@ kişi",
            SocialText::Seen => "Görüldü",
            SocialText::CopyText => "Metni Kopyala",
            SocialText::Copied => "Kopyalandı",
            SocialText::RequestNote => "Okuduğunu göremezler. Mesajlardaki bağlantılar açılmaz.",
            SocialText::PartyInvitationArrived => "%@ seni bir partiye davet ediyor",
            SocialText::MessageArrived => "%@: %@",
            SocialText::MessageRequestArrived => "%@ sana mesaj yazmak istiyor",
            SocialText::ProfileUnread => "%@ okunmamış",
        }
    }

    pub const fn text(self, language: Language) -> &'static str {
        match language {
            Language::Turkish => self.turkish(),
            Language::English => self.english(),
        }
    }

    pub fn format(self, language: Language, args: &[Arg<'_>]) -> String {
        substitute(self.text(language), args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_ARGUMENTS: &[SocialText] = &[
        SocialText::PartySubtitle,
        SocialText::PartyInvitationDetail,
        SocialText::MakeLeaderDetail,
        SocialText::RemoveDetail,
        SocialText::RequestsCount,
        SocialText::MessagesUnread,
        SocialText::NewCount,
        SocialText::GroupPeople,
        SocialText::PartyInvitationArrived,
        SocialText::MessageArrived,
        SocialText::MessageRequestArrived,
        SocialText::ProfileUnread,
    ];

    #[test]
    fn placeholders_match_between_languages() {
        for text in WITH_ARGUMENTS {
            let count = |s: &str| s.matches("%@").count();
            assert_eq!(count(text.english()), count(text.turkish()), "{text:?}");
        }
    }

    #[test]
    fn row_titles_are_in_title_case_and_notes_in_sentence_case() {
        for title in [
            SocialText::StartParty,
            SocialText::InviteFriends,
            SocialText::TakeParty,
            SocialText::ShowEarlier,
            SocialText::LeaveConversation,
            SocialText::MessageRequests,
        ] {
            for word in title.english().split(' ') {
                let first = word.chars().next().unwrap();
                assert!(
                    first.is_uppercase() || matches!(word, "the" | "to" | "a"),
                    "{title:?}: {word}"
                );
            }
        }
        assert_eq!(
            SocialText::PartySubtitle.format(Language::English, &[Arg::Int(3), Arg::Int(8)]),
            "3 of 8 members"
        );
    }
}
