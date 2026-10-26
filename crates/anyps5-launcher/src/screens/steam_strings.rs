//! Texts of Add to Steam: the two settings rows and the toasts. English and Turkish like the rest.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SteamText {
    /// Settings row: puts the launcher into the Steam library.
    Add,
    AddDetail,
    /// Settings row: one Steam entry for each game.
    AddGames,
    AddGamesDetail,
    /// Toast: the launcher's entry was written.
    Added,
    /// Toast: the launcher's entry was there and is up to date now.
    Updated,
    /// Toast: one game's entry was written.
    GameAdded,
    /// Toast: `%@` = how many games.
    GamesAdded,
    /// Toast: Steam runs, so nothing was written.
    Running,
    NoSteam,
    /// Toast: Steam has no account folder yet.
    NoUsers,
    /// Toast: the shortcut list is in a form the launcher does not know and was left alone.
    Unreadable,
    Failed,
    NoGames,
}

impl SteamText {
    pub const fn english(self) -> &'static str {
        match self {
            SteamText::Add => "Add to Steam",
            SteamText::AddDetail => {
                "Adds the launcher to your Steam library for Big Picture and the Steam Deck. Close Steam first."
            }
            SteamText::AddGames => "Add My Games to Steam",
            SteamText::AddGamesDetail => {
                "Adds each game in your library to Steam, with its own artwork. Close Steam first."
            }
            SteamText::Added => "Added to Steam. Start Steam again to see it.",
            SteamText::Updated => "Updated in Steam. Start Steam again to see the change.",
            SteamText::GameAdded => "1 game is in Steam. Start Steam again to see it.",
            SteamText::GamesAdded => "%@ games are in Steam. Start Steam again to see them.",
            SteamText::Running => "Steam is running. Close Steam, then try again.",
            SteamText::NoSteam => {
                "Steam was not found on this PC. Install Steam and sign in once, then try again."
            }
            SteamText::NoUsers => {
                "Steam has no account on this PC yet. Sign in to Steam once, then try again."
            }
            SteamText::Unreadable => {
                "Could not read Steam's shortcut list, so nothing was changed."
            }
            SteamText::Failed => {
                "Could not save to your Steam folder. Check that it is not read-only, then try again."
            }
            SteamText::NoGames => "No games in the library yet.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            SteamText::Add => "Steam'e Ekle",
            SteamText::AddDetail => {
                "Başlatıcıyı Big Picture ve Steam Deck için Steam kitaplığına ekler. Önce Steam'i kapat."
            }
            SteamText::AddGames => "Oyunlarımı Steam'e Ekle",
            SteamText::AddGamesDetail => {
                "Kitaplıktaki her oyunu kendi görseliyle Steam'e ekler. Önce Steam'i kapat."
            }
            SteamText::Added => "Steam'e eklendi. Görmek için Steam'i yeniden başlat.",
            SteamText::Updated => {
                "Steam'de güncellendi. Değişikliği görmek için Steam'i yeniden başlat."
            }
            SteamText::GameAdded => "1 oyun Steam'de. Görmek için Steam'i yeniden başlat.",
            SteamText::GamesAdded => "%@ oyun Steam'de. Görmek için Steam'i yeniden başlat.",
            SteamText::Running => "Steam çalışıyor. Steam'i kapat, sonra tekrar dene.",
            SteamText::NoSteam => {
                "Bu bilgisayarda Steam bulunamadı. Steam'i kur ve bir kez giriş yap, sonra tekrar dene."
            }
            SteamText::NoUsers => {
                "Steam'de bu bilgisayarda henüz hesap yok. Steam'e bir kez giriş yap, sonra tekrar dene."
            }
            SteamText::Unreadable => {
                "Steam'in kısayol listesi okunamadı, bu yüzden hiçbir şey değiştirilmedi."
            }
            SteamText::Failed => {
                "Steam klasörüne kaydedilemedi. Salt okunur olmadığını kontrol et, sonra tekrar dene."
            }
            SteamText::NoGames => "Kitaplıkta henüz oyun yok.",
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

    pub const ALL: [SteamText; 14] = [
        SteamText::Add,
        SteamText::AddDetail,
        SteamText::AddGames,
        SteamText::AddGamesDetail,
        SteamText::Added,
        SteamText::Updated,
        SteamText::GameAdded,
        SteamText::GamesAdded,
        SteamText::Running,
        SteamText::NoSteam,
        SteamText::NoUsers,
        SteamText::Unreadable,
        SteamText::Failed,
        SteamText::NoGames,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_exists_in_both_languages_with_the_same_placeholders() {
        for text in SteamText::ALL {
            for language in Language::ALL {
                assert!(!text.text(language).is_empty(), "{text:?} {language:?}");
            }
            assert_eq!(
                text.english().matches("%@").count(),
                text.turkish().matches("%@").count(),
                "{text:?}"
            );
        }
    }
}
