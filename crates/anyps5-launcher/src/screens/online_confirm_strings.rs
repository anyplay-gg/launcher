//! Texts of the confirmations of the online area (docs/DESIGN.md 17, "Confirmations"). English and
//! Turkish like the rest. The headline is the question and ends with a question mark; the line
//! under it says what the action ends and whether it can be taken back, in sentence case. The
//! Turkish was written for this change and has not been read by a native speaker yet.

use crate::ui::i18n::{Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConfirmText {
    /// `%@` = who.
    BlockTitle,
    /// `%@` = who.
    BlockNote,
    /// `%@` = who.
    BlockRequestNote,
    /// `%@` = who.
    RemoveFriendTitle,
    RemoveFriendNote,
    UnsendTitle,
    UnsendNote,
    LeavePartyTitle,
    LeavePartyNote,
    /// `%@` = who.
    RemoveMemberTitle,
    RemoveMemberNote,
    LeaveConversationTitle,
    LeaveConversationNote,
}

impl ConfirmText {
    pub const fn english(self) -> &'static str {
        match self {
            ConfirmText::BlockTitle => "Block %@?",
            ConfirmText::BlockNote => {
                "%@ can no longer message you or send you friend requests. You can unblock in Settings."
            }
            ConfirmText::BlockRequestNote => {
                "%@ can no longer message you or send you friend requests, and this request is removed. You can unblock in Settings."
            }
            ConfirmText::RemoveFriendTitle => "Remove %@ as a friend?",
            ConfirmText::RemoveFriendNote => {
                "You stop seeing each other's status. Either of you can send a new request later."
            }
            ConfirmText::UnsendTitle => "Unsend this message?",
            ConfirmText::UnsendNote => {
                "It is removed for everyone in the conversation. This cannot be undone."
            }
            ConfirmText::LeavePartyTitle => "Leave the party?",
            ConfirmText::LeavePartyNote => "You need a new invitation to come back.",
            ConfirmText::RemoveMemberTitle => "Remove %@ from the party?",
            ConfirmText::RemoveMemberNote => "They need a new invitation to come back.",
            ConfirmText::LeaveConversationTitle => "Leave this conversation?",
            ConfirmText::LeaveConversationNote => {
                "You stop getting its messages. Someone has to add you again to bring you back."
            }
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            ConfirmText::BlockTitle => "%@ engellensin mi?",
            ConfirmText::BlockNote => {
                "%@ artık sana mesaj ya da arkadaşlık isteği gönderemez. Engeli Ayarlar'dan kaldırabilirsin."
            }
            ConfirmText::BlockRequestNote => {
                "%@ artık sana mesaj ya da arkadaşlık isteği gönderemez ve bu istek kaldırılır. Engeli Ayarlar'dan kaldırabilirsin."
            }
            ConfirmText::RemoveFriendTitle => "%@ arkadaşlıktan çıkarılsın mı?",
            ConfirmText::RemoveFriendNote => {
                "Birbirinizin durumunu görmezsiniz. İkinizden biri sonra yeni istek gönderebilir."
            }
            ConfirmText::UnsendTitle => "Mesaj geri alınsın mı?",
            ConfirmText::UnsendNote => "Konuşmadaki herkes için kaldırılır. Bu geri alınamaz.",
            ConfirmText::LeavePartyTitle => "Partiden ayrılınsın mı?",
            ConfirmText::LeavePartyNote => "Geri dönmek için yeni bir davet gerekir.",
            ConfirmText::RemoveMemberTitle => "%@ partiden çıkarılsın mı?",
            ConfirmText::RemoveMemberNote => "Geri dönmesi için yeni bir davet gerekir.",
            ConfirmText::LeaveConversationTitle => "Sohbetten ayrılınsın mı?",
            ConfirmText::LeaveConversationNote => {
                "Mesajlarını artık almazsın. Geri dönmen için biri seni yeniden eklemeli."
            }
        }
    }

    pub const fn text(self, language: Language) -> &'static str {
        match language {
            Language::Turkish => self.turkish(),
            Language::English => self.english(),
        }
    }

    pub fn format(self, language: Language, args: &[crate::ui::i18n::Arg<'_>]) -> String {
        substitute(self.text(language), args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::i18n::Arg;

    #[test]
    fn placeholders_match_between_languages() {
        for text in [
            ConfirmText::BlockTitle,
            ConfirmText::BlockNote,
            ConfirmText::BlockRequestNote,
            ConfirmText::RemoveFriendTitle,
            ConfirmText::RemoveMemberTitle,
            ConfirmText::UnsendTitle,
            ConfirmText::LeavePartyTitle,
        ] {
            let count = |s: &str| s.matches("%@").count();
            assert_eq!(count(text.english()), count(text.turkish()), "{text:?}");
        }
    }

    #[test]
    fn a_title_asks_and_a_note_ends_in_a_full_stop() {
        for title in [
            ConfirmText::BlockTitle,
            ConfirmText::RemoveFriendTitle,
            ConfirmText::UnsendTitle,
            ConfirmText::LeavePartyTitle,
            ConfirmText::RemoveMemberTitle,
            ConfirmText::LeaveConversationTitle,
        ] {
            assert!(title.english().ends_with('?'), "{title:?}");
            assert!(title.turkish().ends_with('?'), "{title:?}");
        }
        for note in [
            ConfirmText::BlockNote,
            ConfirmText::BlockRequestNote,
            ConfirmText::RemoveFriendNote,
            ConfirmText::UnsendNote,
            ConfirmText::LeavePartyNote,
            ConfirmText::RemoveMemberNote,
            ConfirmText::LeaveConversationNote,
        ] {
            assert!(note.english().ends_with('.'), "{note:?}");
            assert!(note.turkish().ends_with('.'), "{note:?}");
        }
        assert_eq!(
            ConfirmText::BlockTitle.format(Language::English, &[Arg::Str("Lumen")]),
            "Block Lumen?"
        );
    }
}
