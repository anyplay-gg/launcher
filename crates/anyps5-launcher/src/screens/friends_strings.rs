//! Texts of the Friends screen (docs/DESIGN.md 17, "Friends screen"): the sections of its rail,
//! the sections of the friends list and the states around it. English and Turkish like the rest.
//! Titles of sections and buttons are in title case, notes and details in sentence case.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FriendsText {
    /// The accessible name of the list of sections.
    Sections,
    Activity,
    Settings,
    /// The title of the page behind Settings.
    OnlineSettings,
    // Sections of the friends list
    PlayingNow,
    OnlineCaption,
    OfflineCaption,
    // The line under the handle
    /// `%@` = how many.
    CountMany,
    CountOne,
    CountNone,
    // Actions on a friend's row
    ActionJoin,
    ActionMessage,
    /// The accessible name of the row's own press: the friend's page.
    MoreAbout,
    // States
    ConnectingNote,
    OfflineNote,
    TryAgain,
    NotInParty,
    NotInPartyNote,
    /// The rail's name for an item with something waiting: `%@` = the item, `%@` = how many.
    WaitingName,
}

impl FriendsText {
    pub const fn english(self) -> &'static str {
        match self {
            FriendsText::Sections => "Sections",
            FriendsText::Activity => "Activity",
            FriendsText::Settings => "Settings",
            FriendsText::OnlineSettings => "Online Settings",
            FriendsText::PlayingNow => "Playing Now",
            FriendsText::OnlineCaption => "Online",
            FriendsText::OfflineCaption => "Offline",
            FriendsText::CountMany => "%@ friends online",
            FriendsText::CountOne => "1 friend online",
            FriendsText::CountNone => "No friends online",
            FriendsText::ActionJoin => "Join",
            FriendsText::ActionMessage => "Message",
            FriendsText::MoreAbout => "Options",
            FriendsText::ConnectingNote => "Connecting to the network…",
            FriendsText::OfflineNote => {
                "You are offline. Friends and messages update when the connection is back."
            }
            FriendsText::TryAgain => "Try Again",
            FriendsText::NotInParty => "You are not in a party",
            FriendsText::NotInPartyNote => {
                "A party keeps you together across games, with chat and voice."
            }
            FriendsText::WaitingName => "%@, %@ new",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            FriendsText::Sections => "Bölümler",
            FriendsText::Activity => "Etkinlik",
            FriendsText::Settings => "Ayarlar",
            FriendsText::OnlineSettings => "Çevrimiçi Ayarları",
            FriendsText::PlayingNow => "Şimdi Oynayanlar",
            FriendsText::OnlineCaption => "Çevrimiçi",
            FriendsText::OfflineCaption => "Çevrimdışı",
            FriendsText::CountMany => "%@ arkadaş çevrimiçi",
            FriendsText::CountOne => "1 arkadaş çevrimiçi",
            FriendsText::CountNone => "Çevrimiçi arkadaş yok",
            FriendsText::ActionJoin => "Katıl",
            FriendsText::ActionMessage => "Mesaj",
            FriendsText::MoreAbout => "Seçenekler",
            FriendsText::ConnectingNote => "Ağa bağlanılıyor…",
            FriendsText::OfflineNote => {
                "Çevrimdışısın. Arkadaşlar ve mesajlar bağlantı gelince güncellenir."
            }
            FriendsText::TryAgain => "Tekrar Dene",
            FriendsText::NotInParty => "Bir partide değilsin",
            FriendsText::NotInPartyNote => {
                "Parti, sohbet ve sesle oyunlar arasında seni arkadaşlarınla bir arada tutar."
            }
            FriendsText::WaitingName => "%@, %@ yeni",
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

    #[test]
    fn placeholders_match_between_languages() {
        for text in [
            FriendsText::CountMany,
            FriendsText::CountOne,
            FriendsText::WaitingName,
            FriendsText::OfflineNote,
        ] {
            assert_eq!(
                text.english().matches("%@").count(),
                text.turkish().matches("%@").count(),
                "{text:?}"
            );
        }
    }
}
