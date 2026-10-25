//! Texts of the trophy comparison (docs/DESIGN.md 17, "Trophy compare"). English and Turkish like
//! the rest. Titles are in title case, notes and details in sentence case. Grades, "Hidden Trophy"
//! and "Show" come from the shared strings; this file holds only what the screen adds.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CompareText {
    /// The small caption over the game's name.
    Overline,
    /// Under the game's name: `%@` = the other player's handle.
    With,
    /// The player's own column.
    You,
    /// `%@` = the level.
    Level,
    /// `%@` = the points.
    Points,
    /// `%@` = the points of the trophies in this game.
    GamePoints,
    /// A trophy the player has not earned.
    NotYet,
    /// `%@` = the trophy number, for a game the player does not have.
    NumberedTrophy,
    /// Under the header when the game is not in the player's library.
    NotOwnedNote,
    LoadingNote,
    OfflineTitle,
    OfflineNote,
    FailedTitle,
    FailedNote,
    HiddenTitle,
    /// `%@` = the handle.
    HiddenNote,
    EmptyTitle,
    EmptyNote,
    /// A column with no trophies in the game, in its accessible name.
    NoneInGame,
    /// The accessible name of a column: `%@` handle, `%@` level, `%@` points in all, `%@` the
    /// game's trophies by grade, `%@` points in this game.
    SideName,
    /// The accessible name of a row: `%@` trophy, `%@` your state, `%@` handle, `%@` their state.
    RowName,
    /// `%@` = the date.
    EarnedOn,
    /// The accessible name of the screen: `%@` = the game, `%@` = the handle.
    ScreenName,
}

impl CompareText {
    pub const fn english(self) -> &'static str {
        match self {
            CompareText::Overline => "Compare Trophies",
            CompareText::With => "With %@",
            CompareText::You => "You",
            CompareText::Level => "Level %@",
            CompareText::Points => "%@ points",
            CompareText::GamePoints => "%@ points in this game",
            CompareText::NotYet => "Not yet",
            CompareText::NoneInGame => "no trophies",
            CompareText::NumberedTrophy => "Trophy %@",
            CompareText::NotOwnedNote => {
                "This game is not in your library, so its trophies show by number."
            }
            CompareText::LoadingNote => "Loading the comparison…",
            CompareText::OfflineTitle => "You are offline",
            CompareText::OfflineNote => "Trophies are compared when you are back online.",
            CompareText::FailedTitle => "The comparison did not load",
            CompareText::FailedNote => "Check your connection and try again.",
            CompareText::HiddenTitle => "Their trophies are hidden",
            CompareText::HiddenNote => "%@ does not share trophies with you.",
            CompareText::EmptyTitle => "Nothing to compare yet",
            CompareText::EmptyNote => "Neither of you has earned a trophy in this game.",
            CompareText::SideName => "%@: level %@, %@ points, %@, %@ points in this game",
            CompareText::RowName => "%@. %@: %@. %@: %@",
            CompareText::EarnedOn => "Earned %@",
            CompareText::ScreenName => "Trophies of %@ compared with %@",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            CompareText::Overline => "Kupaları Karşılaştır",
            CompareText::With => "%@ ile",
            CompareText::You => "Sen",
            CompareText::Level => "Seviye %@",
            CompareText::Points => "%@ puan",
            CompareText::GamePoints => "Bu oyunda %@ puan",
            CompareText::NotYet => "Henüz yok",
            CompareText::NoneInGame => "kupa yok",
            CompareText::NumberedTrophy => "%@. kupa",
            CompareText::NotOwnedNote => {
                "Bu oyun kitaplığında yok, bu yüzden kupalar numarayla görünür."
            }
            CompareText::LoadingNote => "Karşılaştırma yükleniyor…",
            CompareText::OfflineTitle => "Çevrimdışısın",
            CompareText::OfflineNote => "Kupalar tekrar çevrimiçi olunca karşılaştırılır.",
            CompareText::FailedTitle => "Karşılaştırma yüklenemedi",
            CompareText::FailedNote => "Bağlantını kontrol edip tekrar dene.",
            CompareText::HiddenTitle => "Onun kupaları gizli",
            CompareText::HiddenNote => "%@ kupalarını seninle paylaşmıyor.",
            CompareText::EmptyTitle => "Henüz karşılaştıracak bir şey yok",
            CompareText::EmptyNote => "İkinizin de bu oyunda kazandığı bir kupa yok.",
            CompareText::SideName => "%@: seviye %@, %@ puan, %@, bu oyunda %@ puan",
            CompareText::RowName => "%@. %@: %@. %@: %@",
            CompareText::EarnedOn => "%@ tarihinde kazanıldı",
            CompareText::ScreenName => "%@ kupaları, %@ ile karşılaştırma",
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
            CompareText::With,
            CompareText::Level,
            CompareText::Points,
            CompareText::GamePoints,
            CompareText::NumberedTrophy,
            CompareText::HiddenNote,
            CompareText::SideName,
            CompareText::RowName,
            CompareText::EarnedOn,
            CompareText::ScreenName,
            CompareText::NotOwnedNote,
        ] {
            assert_eq!(
                text.english().matches("%@").count(),
                text.turkish().matches("%@").count(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn no_text_names_the_platform() {
        for text in [
            CompareText::Overline,
            CompareText::NotOwnedNote,
            CompareText::HiddenTitle,
            CompareText::EmptyNote,
        ] {
            for words in [text.english(), text.turkish()] {
                assert!(!words.contains("PSN") && !words.contains("PlayStation"));
            }
        }
    }
}
