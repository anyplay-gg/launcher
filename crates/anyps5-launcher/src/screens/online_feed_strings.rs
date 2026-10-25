//! Texts of the profile page and the feed in the online area. English and Turkish like the rest.
//! Row titles are in title case, notes and the sentences of the feed in sentence case.
//!
//! The sentences of the feed are told from the account they are about: "earned a gold trophy in
//! Nebula Drift" after "Lumen". For the player's own items the subject is "You", and Turkish
//! needs its own verb forms there (`*Own`). Arguments are filled in order; where Turkish wants
//! another order, the caller passes them in that order (see `feed::sentence`).

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FeedText {
    // Entry rows
    /// The row on Home and the page's title.
    Feed,
    FeedDetail,
    FeedSubtitle,
    /// The row on Home that opens the player's own profile.
    YourProfile,
    YourProfileDetail,
    /// The row on a friend's page.
    ViewProfile,
    // Feed states
    Loading,
    Empty,
    EmptyNote,
    Offline,
    OfflineNote,
    LoadFailedTitle,
    LoadFailed,
    TryAgain,
    ShowOlder,
    /// The "Show Older Activity" row after a refusal.
    OlderFailed,
    /// The profile page's title.
    Profile,
    EndOfFeed,
    // Sentences. The subject is the handle before them.
    /// `%@` = the grade (gold), `%@` = the title of the trophy, `%@` = the game.
    TrophyNamed,
    TrophyNamedOwn,
    /// `%@` = the grade, `%@` = the game.
    Trophy,
    TrophyOwn,
    /// `%@` = the grade.
    TrophyUnknownGame,
    TrophyUnknownGameOwn,
    /// `%@` = who.
    Friend,
    FriendOwn,
    /// The player is the one the account became friends with.
    FriendWithYou,
    /// `%@` = the game.
    FirstPlayed,
    FirstPlayedOwn,
    FirstPlayedUnknown,
    FirstPlayedUnknownOwn,
    /// `%@` = who.
    Party,
    PartyOwn,
    PartyAlone,
    PartyAloneOwn,
    ListTwo,
    ListThree,
    /// `%@` = first, `%@` = second, `%@` = how many more.
    ListMore,
    // Trophy facts
    GradePlatinum,
    GradeGold,
    GradeSilver,
    GradeBronze,
    RarityExtreme,
    RarityRare,
    RarityUncommon,
    RarityCommon,
    /// `%@` = the word (Rare), `%@` = the percent (2.3).
    RarityFact,
    // Time
    AgoNow,
    /// `%@` = how many.
    AgoMinutes,
    AgoHours,
    AgoDays,
    AgoYesterday,
    // Reactions
    ReactNice,
    ReactGg,
    ReactWow,
    ReactSame,
    /// `%@` = the reaction (Nice), `%@` = how many.
    ReactionCount,
    /// The accessible name of the player's own reaction: `%@` = the reaction.
    ReactionMine,
    /// Toast: a reaction did not go through.
    ReactFailed,
    // Profile page
    /// Shown for the player's own profile.
    OwnSubtitle,
    ProfileLoading,
    ProfileFailed,
    Favourites,
    /// A favourite the player does not have: `%@` = the title ID.
    NotInLibrary,
    MostPlayed,
    LinkedAccounts,
    Discord,
    Steam,
    /// A connection whose name the provider did not give.
    Linked,
    TrophiesPrivate,
    EditProfile,
    EditProfileDetail,
    OwnEmpty,
    /// `%@` = hours, `%@` = minutes.
    HoursMinutes,
    /// `%@` = hours.
    Hours,
    /// `%@` = minutes.
    Minutes,
    /// The accessible name of a banner: `%@` = the handle.
    BannerName,
}

impl FeedText {
    pub const fn english(self) -> &'static str {
        match self {
            FeedText::Feed => "Feed",
            FeedText::FeedDetail => "What your friends are up to",
            FeedText::FeedSubtitle => "Recent activity from you and your friends",
            FeedText::YourProfile => "Your Profile",
            FeedText::YourProfileDetail => "What other players see",
            FeedText::ViewProfile => "View Profile",
            FeedText::Loading => "Loading…",
            FeedText::Empty => "Nothing here yet",
            FeedText::EmptyNote => {
                "Trophies, new friends, first games and parties show up here, for you and your friends."
            }
            FeedText::Offline => "You are offline",
            FeedText::OfflineNote => "The feed comes back when you are online again.",
            FeedText::LoadFailedTitle => "The feed did not load",
            FeedText::LoadFailed => "Check your connection and try again.",
            FeedText::TryAgain => "Try Again",
            FeedText::ShowOlder => "Show Older Activity",
            FeedText::OlderFailed => "Could not load. Select to try again.",
            FeedText::Profile => "Profile",
            FeedText::EndOfFeed => "You are all caught up.",
            FeedText::TrophyNamed => "earned the %@ trophy “%@” in %@",
            FeedText::TrophyNamedOwn => "earned the %@ trophy “%@” in %@",
            FeedText::Trophy => "earned a %@ trophy in %@",
            FeedText::TrophyOwn => "earned a %@ trophy in %@",
            FeedText::TrophyUnknownGame => "earned a %@ trophy in a game",
            FeedText::TrophyUnknownGameOwn => "earned a %@ trophy in a game",
            FeedText::Friend => "became friends with %@",
            FeedText::FriendOwn => "became friends with %@",
            FeedText::FriendWithYou => "became friends with you",
            FeedText::FirstPlayed => "played %@ for the first time",
            FeedText::FirstPlayedOwn => "played %@ for the first time",
            FeedText::FirstPlayedUnknown => "played a game for the first time",
            FeedText::FirstPlayedUnknownOwn => "played a game for the first time",
            FeedText::Party => "started a party with %@",
            FeedText::PartyOwn => "started a party with %@",
            FeedText::PartyAlone => "started a party",
            FeedText::PartyAloneOwn => "started a party",
            FeedText::ListTwo => "%@ and %@",
            FeedText::ListThree => "%@, %@ and %@",
            FeedText::ListMore => "%@, %@ and %@ more",
            FeedText::GradePlatinum => "platinum",
            FeedText::GradeGold => "gold",
            FeedText::GradeSilver => "silver",
            FeedText::GradeBronze => "bronze",
            FeedText::RarityExtreme => "Extremely rare",
            FeedText::RarityRare => "Rare",
            FeedText::RarityUncommon => "Uncommon",
            FeedText::RarityCommon => "Common",
            FeedText::RarityFact => "%@, %@%%",
            FeedText::AgoNow => "Just now",
            FeedText::AgoMinutes => "%@ min ago",
            FeedText::AgoHours => "%@ h ago",
            FeedText::AgoDays => "%@ d ago",
            FeedText::AgoYesterday => "Yesterday",
            FeedText::ReactNice => "Nice",
            FeedText::ReactGg => "GG",
            FeedText::ReactWow => "Wow",
            FeedText::ReactSame => "Same",
            FeedText::ReactionCount => "%@: %@",
            FeedText::ReactionMine => "%@: %@, your reaction",
            FeedText::ReactFailed => "The reaction did not go through. Try again.",
            FeedText::OwnSubtitle => "This is what other players see",
            FeedText::ProfileLoading => "Loading…",
            FeedText::ProfileFailed => {
                "Unable to load this profile. Check your connection and try again."
            }
            FeedText::Favourites => "Favourite Games",
            FeedText::NotInLibrary => "%@, not in your library",
            FeedText::MostPlayed => "Most Played",
            FeedText::LinkedAccounts => "Linked Accounts",
            FeedText::Discord => "Discord",
            FeedText::Steam => "Steam",
            FeedText::Linked => "Linked",
            FeedText::TrophiesPrivate => "Trophies are private",
            FeedText::EditProfile => "Edit Profile",
            FeedText::EditProfileDetail => "Opens your account on the web",
            FeedText::OwnEmpty => {
                "Your profile is empty. Add a short text, favourite games and a picture on the web."
            }
            FeedText::HoursMinutes => "%@ h %@ min",
            FeedText::Hours => "%@ h",
            FeedText::Minutes => "%@ min",
            FeedText::BannerName => "Profile of %@",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            FeedText::Feed => "Akış",
            FeedText::FeedDetail => "Arkadaşların neler yapıyor",
            FeedText::FeedSubtitle => "Senin ve arkadaşlarının son hareketleri",
            FeedText::YourProfile => "Profilin",
            FeedText::YourProfileDetail => "Diğer oyuncuların gördüğü profil",
            FeedText::ViewProfile => "Profili Gör",
            FeedText::Loading => "Yükleniyor…",
            FeedText::Empty => "Henüz bir şey yok",
            FeedText::EmptyNote => {
                "Kupalar, yeni arkadaşlar, ilk oynanan oyunlar ve partiler burada görünür."
            }
            FeedText::Offline => "Çevrimdışısın",
            FeedText::OfflineNote => "Tekrar çevrimiçi olunca akış geri gelir.",
            FeedText::LoadFailedTitle => "Akış yüklenemedi",
            FeedText::LoadFailed => "Bağlantını kontrol edip tekrar dene.",
            FeedText::TryAgain => "Tekrar Dene",
            FeedText::ShowOlder => "Eski Hareketleri Göster",
            FeedText::OlderFailed => "Yüklenemedi. Tekrar denemek için seç.",
            FeedText::Profile => "Profil",
            FeedText::EndOfFeed => "Hepsini gördün.",
            FeedText::TrophyNamed => "%@ oyununda %@ kupa kazandı: “%@”",
            FeedText::TrophyNamedOwn => "%@ oyununda %@ kupa kazandın: “%@”",
            FeedText::Trophy => "%@ oyununda %@ kupa kazandı",
            FeedText::TrophyOwn => "%@ oyununda %@ kupa kazandın",
            FeedText::TrophyUnknownGame => "Bir oyunda %@ kupa kazandı",
            FeedText::TrophyUnknownGameOwn => "Bir oyunda %@ kupa kazandın",
            FeedText::Friend => "%@ ile arkadaş oldu",
            FeedText::FriendOwn => "%@ ile arkadaş oldun",
            FeedText::FriendWithYou => "seninle arkadaş oldu",
            FeedText::FirstPlayed => "%@ oyununu ilk kez oynadı",
            FeedText::FirstPlayedOwn => "%@ oyununu ilk kez oynadın",
            FeedText::FirstPlayedUnknown => "Bir oyunu ilk kez oynadı",
            FeedText::FirstPlayedUnknownOwn => "Bir oyunu ilk kez oynadın",
            FeedText::Party => "%@ ile parti kurdu",
            FeedText::PartyOwn => "%@ ile parti kurdun",
            FeedText::PartyAlone => "Parti kurdu",
            FeedText::PartyAloneOwn => "Parti kurdun",
            FeedText::ListTwo => "%@ ve %@",
            FeedText::ListThree => "%@, %@ ve %@",
            FeedText::ListMore => "%@, %@ ve %@ kişi daha",
            FeedText::GradePlatinum => "platin",
            FeedText::GradeGold => "altın",
            FeedText::GradeSilver => "gümüş",
            FeedText::GradeBronze => "bronz",
            FeedText::RarityExtreme => "Çok nadir",
            FeedText::RarityRare => "Nadir",
            FeedText::RarityUncommon => "Seyrek",
            FeedText::RarityCommon => "Yaygın",
            FeedText::RarityFact => "%@, %%%@",
            FeedText::AgoNow => "Az önce",
            FeedText::AgoMinutes => "%@ dk önce",
            FeedText::AgoHours => "%@ sa önce",
            FeedText::AgoDays => "%@ gün önce",
            FeedText::AgoYesterday => "Dün",
            FeedText::ReactNice => "Güzel",
            FeedText::ReactGg => "GG",
            FeedText::ReactWow => "Vay",
            FeedText::ReactSame => "Ben de",
            FeedText::ReactionCount => "%@: %@",
            FeedText::ReactionMine => "%@: %@, senin tepkin",
            FeedText::ReactFailed => "Tepki gönderilemedi. Tekrar dene.",
            FeedText::OwnSubtitle => "Diğer oyuncular profilini böyle görür",
            FeedText::ProfileLoading => "Yükleniyor…",
            FeedText::ProfileFailed => "Profil yüklenemedi. Bağlantını kontrol edip tekrar dene.",
            FeedText::Favourites => "Favori Oyunlar",
            FeedText::NotInLibrary => "%@, kitaplığında yok",
            FeedText::MostPlayed => "En Çok Oynanan",
            FeedText::LinkedAccounts => "Bağlı Hesaplar",
            FeedText::Discord => "Discord",
            FeedText::Steam => "Steam",
            FeedText::Linked => "Bağlı",
            FeedText::TrophiesPrivate => "Kupalar gizli",
            FeedText::EditProfile => "Profili Düzenle",
            FeedText::EditProfileDetail => "Hesabını webde açar",
            FeedText::OwnEmpty => {
                "Profilin boş. Kısa bir yazı, favori oyunlar ve bir resim eklemek için web sitesini aç."
            }
            FeedText::HoursMinutes => "%@ sa %@ dk",
            FeedText::Hours => "%@ sa",
            FeedText::Minutes => "%@ dk",
            FeedText::BannerName => "%@ kullanıcısının profili",
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
            FeedText::TrophyNamed,
            FeedText::TrophyNamedOwn,
            FeedText::Trophy,
            FeedText::TrophyOwn,
            FeedText::TrophyUnknownGame,
            FeedText::TrophyUnknownGameOwn,
            FeedText::Friend,
            FeedText::FriendOwn,
            FeedText::FirstPlayed,
            FeedText::FirstPlayedOwn,
            FeedText::Party,
            FeedText::PartyOwn,
            FeedText::ListTwo,
            FeedText::ListThree,
            FeedText::ListMore,
            FeedText::RarityFact,
            FeedText::AgoMinutes,
            FeedText::AgoHours,
            FeedText::AgoDays,
            FeedText::ReactionCount,
            FeedText::ReactionMine,
            FeedText::NotInLibrary,
            FeedText::HoursMinutes,
            FeedText::Hours,
            FeedText::Minutes,
            FeedText::BannerName,
        ] {
            let count = |s: &str| s.matches("%@").count();
            assert_eq!(count(text.english()), count(text.turkish()), "{text:?}");
        }
    }

    #[test]
    fn the_rarity_fact_reads_as_the_example() {
        assert_eq!(
            FeedText::RarityFact.format(Language::English, &[Arg::Str("Rare"), Arg::Str("2.3")]),
            "Rare, 2.3%"
        );
        assert_eq!(
            FeedText::RarityFact.format(Language::Turkish, &[Arg::Str("Nadir"), Arg::Str("2.3")]),
            "Nadir, %2.3"
        );
    }

    #[test]
    fn row_titles_are_in_title_case() {
        for title in [
            FeedText::Feed,
            FeedText::YourProfile,
            FeedText::ViewProfile,
            FeedText::TryAgain,
            FeedText::ShowOlder,
            FeedText::Profile,
            FeedText::Favourites,
            FeedText::MostPlayed,
            FeedText::LinkedAccounts,
            FeedText::EditProfile,
        ] {
            for word in title.english().split(' ') {
                let first = word.chars().next().unwrap();
                assert!(first.is_uppercase(), "{title:?}: {word}");
            }
        }
    }
}
