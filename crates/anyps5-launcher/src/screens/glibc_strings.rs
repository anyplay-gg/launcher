//! The sentence for a system whose glibc is older than the one the engine's libraries ask for
//! (Linux; docs/DESIGN.md 5.20). One sentence, said the same in Settings, at Unlock and at Play.

use anyps5_core::glibc::Shortfall;

use crate::ui::i18n::{Arg, Language, substitute};

const fn template(language: Language) -> &'static str {
    match language {
        Language::English => "Games need glibc %@ or newer; this system has %@.",
        Language::Turkish => "Oyunlar glibc %@ veya daha yenisini ister; bu sistemde %@ var.",
    }
}

/// "Games need glibc 2.38 or newer; this system has 2.35."
pub fn shortfall_text(shortfall: &Shortfall, language: Language) -> String {
    substitute(
        template(language),
        &[
            Arg::Str(&shortfall.needed.to_string()),
            Arg::Str(&shortfall.system.to_string()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::glibc::Version;

    #[test]
    fn the_sentence_names_both_versions_in_both_languages() {
        let short = Shortfall {
            needed: Version::new(2, 38),
            system: Version::new(2, 35),
        };
        assert_eq!(
            shortfall_text(&short, Language::English),
            "Games need glibc 2.38 or newer; this system has 2.35."
        );
        assert_eq!(
            shortfall_text(&short, Language::Turkish),
            "Oyunlar glibc 2.38 veya daha yenisini ister; bu sistemde 2.35 var."
        );
    }
}
