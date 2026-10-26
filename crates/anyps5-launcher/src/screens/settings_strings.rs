//! Captions and the few lines of the global settings that have no other home: the group headings
//! that tell a long page apart (docs/DESIGN.md 5.7). English and Turkish like the rest; captions
//! in title case.

use crate::ui::i18n::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettingsText {
    /// Behavior: sounds and music.
    Sound,
    /// Behavior (Windows): the clips.
    Captures,
    /// Behavior: the pad.
    Controller,
    /// Behavior: Discord, Steam and links from websites.
    ConnectedApps,
    /// Language: the fonts games use.
    Fonts,
    /// Advanced: what the network recording is for.
    Diagnostics,
    /// Advanced (Linux): the video codec.
    ShareScreen,
    /// Toolkit: the update check.
    Updates,
    /// Toolkit: the releases nothing uses.
    Unused,
    /// Toolkit: the header row.
    Installed,
    /// Toolkit: the toolkit was never looked up.
    NotCheckedYet,
    /// Advanced: under the screenshots rows.
    ScreenshotHint,
}

impl SettingsText {
    pub const ALL: [SettingsText; 12] = [
        SettingsText::Sound,
        SettingsText::Captures,
        SettingsText::Controller,
        SettingsText::ConnectedApps,
        SettingsText::Fonts,
        SettingsText::Diagnostics,
        SettingsText::ShareScreen,
        SettingsText::Updates,
        SettingsText::Unused,
        SettingsText::Installed,
        SettingsText::NotCheckedYet,
        SettingsText::ScreenshotHint,
    ];

    pub const fn english(self) -> &'static str {
        match self {
            SettingsText::Sound => "Sound",
            SettingsText::Captures => "Captures",
            SettingsText::Controller => "Controller",
            SettingsText::ConnectedApps => "Connected Apps",
            SettingsText::Fonts => "Fonts",
            SettingsText::Diagnostics => "Diagnostics",
            SettingsText::ShareScreen => "Share Screen",
            SettingsText::Updates => "Updates",
            SettingsText::Unused => "Not Used by Any Game",
            SettingsText::Installed => "Installed Toolkit",
            SettingsText::NotCheckedYet => "Not checked yet",
            SettingsText::ScreenshotHint => "F12 in a game saves a screenshot.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            SettingsText::Sound => "Ses",
            SettingsText::Captures => "Yakalamalar",
            SettingsText::Controller => "Kol",
            SettingsText::ConnectedApps => "Bağlı Uygulamalar",
            SettingsText::Fonts => "Yazı Tipleri",
            SettingsText::Diagnostics => "Tanılama",
            SettingsText::ShareScreen => "Ekran Paylaşımı",
            SettingsText::Updates => "Güncellemeler",
            SettingsText::Unused => "Hiçbir Oyunun Kullanmadığı Sürümler",
            SettingsText::Installed => "Kurulu Araç Seti",
            SettingsText::NotCheckedYet => "Henüz denetlenmedi",
            SettingsText::ScreenshotHint => "Oyunda F12 bir ekran görüntüsü kaydeder.",
        }
    }

    pub const fn text(self, language: Language) -> &'static str {
        match language {
            Language::Turkish => self.turkish(),
            Language::English => self.english(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_is_there_in_both_languages() {
        for text in SettingsText::ALL {
            for language in Language::ALL {
                assert!(
                    !text.text(language).trim().is_empty(),
                    "{text:?} {language:?}"
                );
            }
        }
    }
}
