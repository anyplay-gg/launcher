//! Texts of LudusCtrl on the Phone as Controller page (docs/DESIGN.md 18, "LudusCtrl"). English
//! and Turkish like the rest. The words the page shares with the phone server (status, the pairing
//! question, Remove, the toasts) are [`PhoneText`](crate::screens::phone_strings::PhoneText).

use crate::ui::i18n::{Arg, Language, substitute};

/// The project page of LudusCtrl, which explains the app and links where to get it. The Google
/// Play listing (`https://play.google.com/store/apps/details?id=com.parlov.ludusctrl`) opens only
/// for testers until the app is in production, so the page links this one.
pub const PROJECT_PAGE: &str = "https://github.com/parl0v/ludusctrl";
/// The address as the row's detail shows it.
pub const PROJECT_PAGE_SHOWN: &str = "github.com/parl0v/ludusctrl";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LudusText {
    /// The switch and its detail.
    Title,
    SwitchDetail,
    /// The server could not start.
    StartFailed,
    /// Status: a phone holds the pad with no controller connected to it (`%@` = its name).
    NoController,
    /// The caption over the paired LudusCtrl phones, and the line while there is none.
    Paired,
    PairedNone,
    /// Where the app comes from: the note and the row that opens the project page.
    AppNote,
    AppOpen,
    /// Top bar: the accessible name of the status, `%@` = the phone's name.
    BarLabel,
}

impl LudusText {
    pub const ALL: [LudusText; 9] = [
        LudusText::Title,
        LudusText::SwitchDetail,
        LudusText::StartFailed,
        LudusText::NoController,
        LudusText::Paired,
        LudusText::PairedNone,
        LudusText::AppNote,
        LudusText::AppOpen,
        LudusText::BarLabel,
    ];

    pub const fn english(self) -> &'static str {
        match self {
            LudusText::Title => "LudusCtrl",
            LudusText::SwitchDetail => {
                "Play with a controller connected to your phone, through the LudusCtrl app. Each phone has to be paired first."
            }
            LudusText::StartFailed => {
                "Could not start. Another program, such as the LudusCtrl receiver, may be using its network port."
            }
            LudusText::NoController => "Connected to %@, no controller",
            LudusText::Paired => "Paired LudusCtrl Phones",
            LudusText::PairedNone => {
                "No phone is paired yet. Open the LudusCtrl app on your phone and start the link."
            }
            LudusText::AppNote => {
                "LudusCtrl runs on Android phones and tablets. Pair your controller with the phone over Bluetooth, then open the app."
            }
            LudusText::AppOpen => "Open the LudusCtrl Page",
            LudusText::BarLabel => "LudusCtrl connected: %@",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            LudusText::Title => "LudusCtrl",
            LudusText::SwitchDetail => {
                "Telefonuna bağlı bir kumandayla LudusCtrl uygulaması üzerinden oyna. Her telefon önce eşleştirilmelidir."
            }
            LudusText::StartFailed => {
                "Başlatılamadı. LudusCtrl alıcısı gibi başka bir program ağ bağlantı noktasını kullanıyor olabilir."
            }
            LudusText::NoController => "%@ bağlı, kumanda yok",
            LudusText::Paired => "Eşleştirilmiş LudusCtrl Telefonları",
            LudusText::PairedNone => {
                "Henüz eşleştirilmiş telefon yok. Telefonunda LudusCtrl uygulamasını açıp bağlantıyı başlat."
            }
            LudusText::AppNote => {
                "LudusCtrl, Android telefon ve tabletlerde çalışır. Kumandanı telefonla Bluetooth üzerinden eşleştir, sonra uygulamayı aç."
            }
            LudusText::AppOpen => "LudusCtrl Sayfasını Aç",
            LudusText::BarLabel => "LudusCtrl bağlı: %@",
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
    fn every_text_is_there_in_both_languages() {
        for text in LudusText::ALL {
            for language in Language::ALL {
                assert!(
                    !text.text(language).trim().is_empty(),
                    "{text:?} {language:?}"
                );
            }
        }
    }

    #[test]
    fn the_name_reaches_the_bar_label_in_both_languages() {
        for language in Language::ALL {
            let text = LudusText::BarLabel.format(language, &[Arg::Str("Pixel 8")]);
            assert!(
                text.contains("Pixel 8") && text.contains("LudusCtrl"),
                "{text}"
            );
            assert!(!text.contains("%@"), "{text}");
        }
    }

    #[test]
    fn the_words_name_no_console_brand() {
        for text in LudusText::ALL {
            for language in Language::ALL {
                let words = text.text(language).to_lowercase();
                for brand in [
                    "playstation",
                    "dualsense",
                    "dualshock",
                    "xbox",
                    "ps5",
                    "sony",
                ] {
                    assert!(!words.contains(brand), "{text:?} {language:?}: {brand}");
                }
            }
        }
    }

    #[test]
    fn the_project_page_is_secure_and_matches_what_is_shown() {
        assert!(PROJECT_PAGE.starts_with("https://github.com/"));
        assert_eq!(
            PROJECT_PAGE.trim_start_matches("https://"),
            PROJECT_PAGE_SHOWN
        );
    }
}
