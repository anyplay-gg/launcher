//! Texts of the profile screen (docs/DESIGN.md 17, "Profile screen"). English and Turkish like the
//! rest. Titles of sections and buttons are in title case, notes and details in sentence case.
//! Grades, times and the four reactions come from `online_feed_strings`; this file holds only what
//! the screen adds.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProfileText {
    // Relation chips
    Friend,
    RequestSent,
    RequestReceived,
    // Stat labels, singular and plural
    StatGame,
    StatGames,
    StatHour,
    StatHours,
    StatMinute,
    StatMinutes,
    StatTrophy,
    StatTrophies,
    // Sections
    SectionGames,
    SectionTrophies,
    SectionActivity,
    // Rows
    /// `%@` = when ("2 h ago", "Oct 21").
    LastPlayed,
    NeverPlayed,
    Favourite,
    NotInLibraryDetail,
    /// The accessible name of a progress: `%@` = earned, `%@` = all.
    TrophyProgress,
    /// `%@` = the grade (Gold).
    GradeTrophy,
    InAGame,
    /// The activity line of a profile: `%@` = the subject, `%@` = the sentence.
    ActivityLine,
    // Buttons
    FriendsOnline,
    /// `%@` = how many messages wait.
    FriendsOnlineUnread,
    FriendOptions,
    AnswerRequest,
    AddFriend,
    // States
    LoadingNote,
    OfflineTitle,
    OfflineNote,
    FailedTitle,
    FailedNote,
    PrivateTitle,
    /// `%@` = the handle.
    PrivateNote,
    EmptyTitle,
    /// `%@` = the handle.
    EmptyNote,
    // The player's own empty sections
    OwnGamesEmpty,
    OwnTrophiesEmpty,
    OwnActivityEmpty,
    /// Under the header of the player's own profile while the cloud is out of reach.
    OwnOfflineNote,
    /// On a friend's profile: `%@` = how many of their games the player has too.
    SharedGames,
    /// `%@` = the handle.
    ScreenName,
    /// `%@` = the figure, `%@` = the label ("12 hours").
    StatName,
}

impl ProfileText {
    pub const fn english(self) -> &'static str {
        match self {
            ProfileText::Friend => "Friend",
            ProfileText::RequestSent => "Request sent",
            ProfileText::RequestReceived => "Wants to be friends",
            ProfileText::StatGame => "Game",
            ProfileText::StatGames => "Games",
            ProfileText::StatHour => "Hour",
            ProfileText::StatHours => "Hours",
            ProfileText::StatMinute => "Minute",
            ProfileText::StatMinutes => "Minutes",
            ProfileText::StatTrophy => "Trophy",
            ProfileText::StatTrophies => "Trophies",
            ProfileText::SectionGames => "Games",
            ProfileText::SectionTrophies => "Recent Trophies",
            ProfileText::SectionActivity => "Recent Activity",
            ProfileText::LastPlayed => "Last played %@",
            ProfileText::NeverPlayed => "Not played yet",
            ProfileText::Favourite => "Favourite",
            ProfileText::NotInLibraryDetail => "Not in your library",
            ProfileText::TrophyProgress => "%@ of %@ trophies",
            ProfileText::GradeTrophy => "%@ trophy",
            ProfileText::InAGame => "In a game",
            ProfileText::ActivityLine => "%@ %@",
            ProfileText::FriendsOnline => "Friends & Online",
            ProfileText::FriendsOnlineUnread => "Friends & Online (%@)",
            ProfileText::FriendOptions => "Friend Options",
            ProfileText::AnswerRequest => "Answer Request",
            ProfileText::AddFriend => "Add Friend",
            ProfileText::LoadingNote => "Loading profile…",
            ProfileText::OfflineTitle => "You are offline",
            ProfileText::OfflineNote => "Profiles of other players load when you are back online.",
            ProfileText::FailedTitle => "This profile did not load",
            ProfileText::FailedNote => "Check your connection and try again.",
            ProfileText::PrivateTitle => "This profile is private",
            ProfileText::PrivateNote => "%@ shares their profile with friends only.",
            ProfileText::EmptyTitle => "Nothing to show yet",
            ProfileText::EmptyNote => "%@ has not played or shared anything yet.",
            ProfileText::OwnGamesEmpty => "Games in your library show up here with your time.",
            ProfileText::OwnTrophiesEmpty => "Earn a trophy in a game and it shows up here.",
            ProfileText::OwnActivityEmpty => "Trophies, new friends and first games show up here.",
            ProfileText::OwnOfflineNote => {
                "You are offline. Your activity comes back when you are online again."
            }
            ProfileText::SharedGames => "You have %@ of their games",
            ProfileText::ScreenName => "Profile of %@",
            ProfileText::StatName => "%@ %@",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            ProfileText::Friend => "Arkadaş",
            ProfileText::RequestSent => "İstek gönderildi",
            ProfileText::RequestReceived => "Arkadaş olmak istiyor",
            ProfileText::StatGame => "Oyun",
            ProfileText::StatGames => "Oyun",
            ProfileText::StatHour => "Saat",
            ProfileText::StatHours => "Saat",
            ProfileText::StatMinute => "Dakika",
            ProfileText::StatMinutes => "Dakika",
            ProfileText::StatTrophy => "Kupa",
            ProfileText::StatTrophies => "Kupa",
            ProfileText::SectionGames => "Oyunlar",
            ProfileText::SectionTrophies => "Son Kupalar",
            ProfileText::SectionActivity => "Son Etkinlik",
            ProfileText::LastPlayed => "Son oynama: %@",
            ProfileText::NeverPlayed => "Henüz oynanmadı",
            ProfileText::Favourite => "Favori",
            ProfileText::NotInLibraryDetail => "Kitaplığında yok",
            ProfileText::TrophyProgress => "%@ / %@ kupa",
            ProfileText::GradeTrophy => "%@ kupa",
            ProfileText::InAGame => "Bir oyunda",
            ProfileText::ActivityLine => "%@ %@",
            ProfileText::FriendsOnline => "Arkadaşlar ve Çevrimiçi",
            ProfileText::FriendsOnlineUnread => "Arkadaşlar ve Çevrimiçi (%@)",
            ProfileText::FriendOptions => "Arkadaş Seçenekleri",
            ProfileText::AnswerRequest => "İsteği Yanıtla",
            ProfileText::AddFriend => "Arkadaş Ekle",
            ProfileText::LoadingNote => "Profil yükleniyor…",
            ProfileText::OfflineTitle => "Çevrimdışısın",
            ProfileText::OfflineNote => {
                "Diğer oyuncuların profilleri tekrar çevrimiçi olunca yüklenir."
            }
            ProfileText::FailedTitle => "Bu profil yüklenemedi",
            ProfileText::FailedNote => "Bağlantını kontrol edip tekrar dene.",
            ProfileText::PrivateTitle => "Bu profil gizli",
            ProfileText::PrivateNote => "%@ profilini yalnızca arkadaşlarıyla paylaşıyor.",
            ProfileText::EmptyTitle => "Henüz gösterilecek bir şey yok",
            ProfileText::EmptyNote => "%@ henüz bir şey oynamadı veya paylaşmadı.",
            ProfileText::OwnGamesEmpty => "Kitaplığındaki oyunlar süreleriyle burada görünür.",
            ProfileText::OwnTrophiesEmpty => "Bir oyunda kupa kazan, burada görünsün.",
            ProfileText::OwnActivityEmpty => {
                "Kupalar, yeni arkadaşlar ve ilk oyunlar burada görünür."
            }
            ProfileText::OwnOfflineNote => {
                "Çevrimdışısın. Etkinliğin tekrar çevrimiçi olunca geri gelir."
            }
            ProfileText::SharedGames => "Onların oyunlarından %@ tanesi sende de var",
            ProfileText::ScreenName => "%@ Kullanıcısının Profili",
            ProfileText::StatName => "%@ %@",
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

    fn placeholders(text: &str) -> usize {
        text.matches("%@").count()
    }

    #[test]
    fn placeholders_match_between_languages() {
        for text in [
            ProfileText::LastPlayed,
            ProfileText::TrophyProgress,
            ProfileText::GradeTrophy,
            ProfileText::ActivityLine,
            ProfileText::FriendsOnlineUnread,
            ProfileText::PrivateNote,
            ProfileText::EmptyNote,
            ProfileText::SharedGames,
            ProfileText::ScreenName,
            ProfileText::StatName,
            ProfileText::OwnGamesEmpty,
        ] {
            assert_eq!(
                placeholders(text.english()),
                placeholders(text.turkish()),
                "{text:?}"
            );
        }
    }
}
