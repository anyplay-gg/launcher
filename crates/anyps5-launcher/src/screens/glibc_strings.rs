//! The sentence for a system whose glibc is older than the one the engine's libraries ask for
//! (Linux; docs/DESIGN.md 5.20). One sentence, said the same in Settings, at Unlock and at Play:
//! what is missing, what the system has and what to do about it (a release that ships the glibc
//! needed). Settings adds a row that opens the install guide.

use anyps5_core::glibc::{Shortfall, Version};

use crate::ui::i18n::{Arg, Language, substitute};

/// Releases of common distributions and the glibc each ships, oldest first.
const RELEASES: [(&str, Version); 5] = [
    ("Ubuntu 22.04", Version::new(2, 35)),
    ("Debian 12", Version::new(2, 36)),
    ("Fedora 39", Version::new(2, 38)),
    ("Ubuntu 24.04", Version::new(2, 39)),
    ("Debian 13", Version::new(2, 41)),
];

/// The two oldest releases above that have at least `needed`, as "Fedora 39, Ubuntu 24.04".
fn releases_with(needed: Version) -> Option<String> {
    let names: Vec<&str> = RELEASES
        .iter()
        .filter(|(_, glibc)| *glibc >= needed)
        .map(|(name, _)| *name)
        .take(2)
        .collect();
    (!names.is_empty()).then(|| names.join(", "))
}

const fn template(language: Language) -> &'static str {
    match language {
        Language::English => "Games need glibc %@ or newer; this system has %@.",
        Language::Turkish => "Oyunlar glibc %@ veya daha yenisini ister; bu sistemde %@ var.",
    }
}

/// What to do: update to a release that has the glibc, or, when none is known to, to a newer one.
fn next_step(needed: Version, language: Language) -> String {
    match (releases_with(needed), language) {
        (Some(names), Language::English) => format!("Update to {names} or newer."),
        (Some(names), Language::Turkish) => format!("Güncelle: {names} veya daha yenisi."),
        (None, Language::English) => "Update to a newer release of your distribution.".to_owned(),
        (None, Language::Turkish) => "Dağıtımını daha yeni bir sürüme güncelle.".to_owned(),
    }
}

/// "Games need glibc 2.38 or newer; this system has 2.35. Update to Fedora 39, Ubuntu 24.04 or
/// newer."
pub fn shortfall_text(shortfall: &Shortfall, language: Language) -> String {
    let what = substitute(
        template(language),
        &[
            Arg::Str(&shortfall.needed.to_string()),
            Arg::Str(&shortfall.system.to_string()),
        ],
    );
    format!("{what} {}", next_step(shortfall.needed, language))
}

/// The row under the sentence in Settings: the install guide says which systems work.
pub const fn guide_title(language: Language) -> &'static str {
    match language {
        Language::English => "Read the Install Guide",
        Language::Turkish => "Kurulum Kılavuzunu Oku",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short(needed: (u32, u32), system: (u32, u32)) -> Shortfall {
        Shortfall {
            needed: Version::new(needed.0, needed.1),
            system: Version::new(system.0, system.1),
        }
    }

    #[test]
    fn the_sentence_names_both_versions_and_a_next_step_in_both_languages() {
        let s = short((2, 38), (2, 35));
        assert_eq!(
            shortfall_text(&s, Language::English),
            "Games need glibc 2.38 or newer; this system has 2.35. Update to Fedora 39, Ubuntu 24.04 or newer."
        );
        assert_eq!(
            shortfall_text(&s, Language::Turkish),
            "Oyunlar glibc 2.38 veya daha yenisini ister; bu sistemde 2.35 var. Güncelle: Fedora 39, Ubuntu 24.04 veya daha yenisi."
        );
    }

    #[test]
    fn the_releases_named_always_have_the_glibc_needed() {
        for (needed, names) in [
            ((2, 35), "Ubuntu 22.04, Debian 12"),
            ((2, 37), "Fedora 39, Ubuntu 24.04"),
            ((2, 39), "Ubuntu 24.04, Debian 13"),
            ((2, 40), "Debian 13"),
        ] {
            assert_eq!(
                releases_with(Version::new(needed.0, needed.1)).as_deref(),
                Some(names)
            );
        }
        // Newer than any release known: no names, but still a step.
        assert_eq!(releases_with(Version::new(2, 42)), None);
        let text = shortfall_text(&short((2, 42), (2, 39)), Language::English);
        assert!(text.ends_with("Update to a newer release of your distribution."));
    }
}
