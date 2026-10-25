//! Texts of the run reports ("How did it run?") and of the friends line on the game page.
//! English and Turkish like the rest.

use anyps5_core::online::api::Verdict;

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RunText {
    /// The question on the game page.
    Question,
    /// The four answers, as buttons and as the community verdict.
    NoStart,
    Menus,
    InGame,
    Playable,
    /// The button that closes the question without an answer.
    Dismiss,
    /// The overline of the fact tile.
    Runs,
    /// The tile's value: `%@` = the verdict, `%@` = how many players reported.
    Verdict,
    /// Under Play: `%@` = a friend's name.
    OnePlaying,
    /// `%@` and `%@` = two friends' names.
    TwoPlaying,
    /// `%@` = how many friends.
    ManyPlaying,
}

impl RunText {
    pub const fn english(self) -> &'static str {
        match self {
            RunText::Question => "How did it run?",
            RunText::NoStart => "Doesn't start",
            RunText::Menus => "Menus",
            RunText::InGame => "In game",
            RunText::Playable => "Playable",
            RunText::Dismiss => "Dismiss",
            RunText::Runs => "Runs",
            RunText::Verdict => "%@ \u{b7} %@ reports",
            RunText::OnePlaying => "%@ is playing this",
            RunText::TwoPlaying => "%@ and %@ are playing this",
            RunText::ManyPlaying => "%@ friends are playing this",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            RunText::Question => "Nas\u{131}l \u{e7}al\u{131}\u{15f}t\u{131}?",
            RunText::NoStart => "Ba\u{15f}lam\u{131}yor",
            RunText::Menus => "Men\u{fc}ler",
            RunText::InGame => "Oyunda",
            RunText::Playable => "Oynanabilir",
            RunText::Dismiss => "Kapat",
            RunText::Runs => "\u{c7}al\u{131}\u{15f}ma",
            RunText::Verdict => "%@ \u{b7} %@ rapor",
            RunText::OnePlaying => "%@ bunu oynuyor",
            RunText::TwoPlaying => "%@ ve %@ bunu oynuyor",
            RunText::ManyPlaying => "%@ arkada\u{15f} bunu oynuyor",
        }
    }

    pub fn text(self, language: Language) -> &'static str {
        match language {
            Language::English => self.english(),
            Language::Turkish => self.turkish(),
        }
    }

    pub fn format(self, language: Language, args: &[Arg<'_>]) -> String {
        substitute(self.text(language), args)
    }

    /// The label of a verdict.
    pub fn of(verdict: Verdict) -> RunText {
        match verdict {
            Verdict::NoStart => RunText::NoStart,
            Verdict::Menus => RunText::Menus,
            Verdict::InGame => RunText::InGame,
            Verdict::Playable => RunText::Playable,
        }
    }

    pub const ALL: [RunText; 11] = [
        RunText::Question,
        RunText::NoStart,
        RunText::Menus,
        RunText::InGame,
        RunText::Playable,
        RunText::Dismiss,
        RunText::Runs,
        RunText::Verdict,
        RunText::OnePlaying,
        RunText::TwoPlaying,
        RunText::ManyPlaying,
    ];
}

/// The line under Play for the friends who play the game now; `None` without any.
pub fn friends_line(names: &[&str], language: Language) -> Option<String> {
    Some(match names {
        [] => return None,
        [one] => RunText::OnePlaying.format(language, &[Arg::Str(one)]),
        [a, b] => RunText::TwoPlaying.format(language, &[Arg::Str(a), Arg::Str(b)]),
        many => RunText::ManyPlaying.format(language, &[Arg::Int(many.len() as i64)]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_exists_in_both_languages_with_the_same_placeholders() {
        for text in RunText::ALL {
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

    #[test]
    fn the_friends_line_names_one_or_two_and_counts_more() {
        let en = Language::English;
        assert_eq!(friends_line(&[], en), None);
        assert_eq!(
            friends_line(&["Mira"], en).as_deref(),
            Some("Mira is playing this")
        );
        assert_eq!(
            friends_line(&["Mira", "Sam"], en).as_deref(),
            Some("Mira and Sam are playing this")
        );
        assert_eq!(
            friends_line(&["Mira", "Sam", "Lu"], en).as_deref(),
            Some("3 friends are playing this")
        );
    }
}
