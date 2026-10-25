//! Texts of the Join button on Discord and of the join links that open the launcher
//! (docs/DESIGN.md 7.4 and 9.6). English and Turkish like the rest.

use anyps5_core::online::Failure;
use anyps5_core::online::api::ErrorCode;

use crate::ui::i18n::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiscordText {
    /// Settings row: the button on the Discord status, and what it does.
    Button,
    ButtonDetail,
    /// Toast: a join link arrived while nobody is signed in.
    SignIn,
    /// Toasts for the answers to a join link.
    Gone,
    Full,
    Already,
    Own,
}

impl DiscordText {
    pub const fn english(self) -> &'static str {
        match self {
            DiscordText::Button => "Show a Join Button on Discord",
            DiscordText::ButtonDetail => {
                "While you play online, people who see your Discord status can get an invitation. They choose whether to join."
            }
            DiscordText::SignIn => "Sign in to AnyPlay to use this join link.",
            DiscordText::Gone => {
                "This join link has expired, or the player left. Ask for a new one."
            }
            DiscordText::Full => "That game or party is full. Try again later.",
            DiscordText::Already => "You are already there, or in another party.",
            DiscordText::Own => "This is your own join link. Send it to a friend.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            DiscordText::Button => "Discord'da Katıl Düğmesini Göster",
            DiscordText::ButtonDetail => {
                "Çevrimiçi oynarken Discord durumunu görenler davet alabilir. Katılıp katılmayacaklarına kendileri karar verir."
            }
            DiscordText::SignIn => "Bu katılma bağlantısı için önce AnyPlay'e giriş yap.",
            DiscordText::Gone => {
                "Bu katılma bağlantısının süresi dolmuş ya da oyuncu ayrılmış. Yenisini iste."
            }
            DiscordText::Full => "Bu oyun ya da parti dolu. Sonra tekrar dene.",
            DiscordText::Already => "Zaten buradasın ya da başka bir partidesin.",
            DiscordText::Own => "Bu senin kendi katılma bağlantın. Bir arkadaşına gönder.",
        }
    }

    pub const fn text(self, language: Language) -> &'static str {
        match language {
            Language::Turkish => self.turkish(),
            Language::English => self.english(),
        }
    }

    pub const ALL: [DiscordText; 7] = [
        DiscordText::Button,
        DiscordText::ButtonDetail,
        DiscordText::SignIn,
        DiscordText::Gone,
        DiscordText::Full,
        DiscordText::Already,
        DiscordText::Own,
    ];
}

/// What a refused join link tells the player, or none for a failure with no text of its own
/// (the caller then uses the general one: offline, rate limited, restricted).
pub fn redeem_failure(failure: &Failure, language: Language) -> Option<&'static str> {
    let text = if failure.is(ErrorCode::NotFound) {
        DiscordText::Gone
    } else if failure.is(ErrorCode::Full) {
        DiscordText::Full
    } else if failure.is(ErrorCode::Limit) {
        DiscordText::Already
    } else if failure.is(ErrorCode::BadRequest) {
        DiscordText::Own
    } else {
        return None;
    };
    Some(text.text(language))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_exists_in_both_languages() {
        for text in DiscordText::ALL {
            for language in Language::ALL {
                assert!(!text.text(language).is_empty(), "{text:?} {language:?}");
            }
        }
    }

    #[test]
    fn the_names_are_ours() {
        for text in DiscordText::ALL {
            for language in Language::ALL {
                let shown = text.text(language);
                assert!(
                    !shown.contains("PlayStation") && !shown.contains("PS5"),
                    "{shown}"
                );
            }
        }
    }

    #[test]
    fn each_answer_of_the_cloud_has_a_sentence() {
        for (code, expected) in [
            ("not_found", DiscordText::Gone),
            ("full", DiscordText::Full),
            ("limit", DiscordText::Already),
            ("bad_request", DiscordText::Own),
        ] {
            let failure = Failure::new(code, "x");
            assert_eq!(
                redeem_failure(&failure, Language::English),
                Some(expected.english()),
                "{code}"
            );
        }
        for code in ["rate_limited", Failure::OFFLINE, "internal"] {
            assert_eq!(
                redeem_failure(&Failure::new(code, "x"), Language::English),
                None,
                "{code}"
            );
        }
    }
}
