//! Texts and pages of the About page: where AnyPlay lives, its rules and who it builds on.
//! English and Turkish like the rest; row titles in title case.

use crate::ui::i18n::Language;

/// The pages the About page opens. The launcher opens no other address from there.
pub const WEBSITE: &str = "https://anyplaynet.com";
pub const DISCORD: &str = "https://discord.gg/9zYqf8fEsh";
pub const PRIVACY: &str = "https://anyplaynet.com/privacy";
pub const TERMS: &str = "https://anyplaynet.com/terms";
pub const PAGES: [&str; 4] = [WEBSITE, DISCORD, PRIVACY, TERMS];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AboutText {
    Website,
    Discord,
    /// Under the Discord row: what it is for.
    DiscordDetail,
    Privacy,
    Terms,
    /// Who the launcher builds on.
    Credits,
}

impl AboutText {
    pub const ALL: [AboutText; 6] = [
        AboutText::Website,
        AboutText::Discord,
        AboutText::DiscordDetail,
        AboutText::Privacy,
        AboutText::Terms,
        AboutText::Credits,
    ];

    pub const fn english(self) -> &'static str {
        match self {
            AboutText::Website => "AnyPlay Website",
            AboutText::Discord => "AnyPlay Discord",
            AboutText::DiscordDetail => "News, help and other players",
            AboutText::Privacy => "Privacy Policy",
            AboutText::Terms => "Terms of Service",
            AboutText::Credits => {
                "Built on AnyPS5 by boykopovar. The launcher began as utkuhalis' AnyPS5-ARM launcher for macOS."
            }
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            AboutText::Website => "AnyPlay Web Sitesi",
            AboutText::Discord => "AnyPlay Discord",
            AboutText::DiscordDetail => "Haberler, yardım ve diğer oyuncular",
            AboutText::Privacy => "Gizlilik Politikası",
            AboutText::Terms => "Hizmet Şartları",
            AboutText::Credits => {
                "boykopovar'ın AnyPS5'i üzerine kurulu. Launcher, utkuhalis'in macOS için AnyPS5-ARM launcher'ı olarak başladı."
            }
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
        for text in AboutText::ALL {
            for language in Language::ALL {
                assert!(
                    !text.text(language).trim().is_empty(),
                    "{text:?} {language:?}"
                );
            }
        }
    }

    #[test]
    fn the_pages_are_ours_and_secure() {
        for page in PAGES {
            assert!(page.starts_with("https://"), "{page}");
            assert!(
                page.starts_with("https://anyplaynet.com")
                    || page.starts_with("https://discord.gg/"),
                "{page}"
            );
        }
    }
}
