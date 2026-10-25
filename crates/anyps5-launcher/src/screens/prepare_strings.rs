//! The words of preparing a game in the background and of unlocking in general: the quiet tile
//! label, the time left, the failure sentences that name the next step, and the setting
//! (docs/DESIGN.md 5.4). English and Turkish like the rest.

use anyps5_core::unlock::FailureKind;

use crate::ui::i18n::{Arg, Language, substitute};

/// The label under a tile that is being prepared: `percent` is known once the work has started.
/// "Preparing…", "Preparing… 40%", "Paused · 40%".
pub fn tile_label(percent: Option<u32>, paused: bool, language: Language) -> String {
    match (language, paused, percent) {
        (Language::English, false, None) => "Preparing\u{2026}".to_owned(),
        (Language::English, false, Some(p)) => format!("Preparing\u{2026} {p}%"),
        (Language::English, true, None) => "Paused".to_owned(),
        (Language::English, true, Some(p)) => format!("Paused \u{b7} {p}%"),
        (Language::Turkish, false, None) => "Haz\u{131}rlan\u{131}yor\u{2026}".to_owned(),
        (Language::Turkish, false, Some(p)) => format!("Haz\u{131}rlan\u{131}yor\u{2026} %{p}"),
        (Language::Turkish, true, None) => "Duraklat\u{131}ld\u{131}".to_owned(),
        (Language::Turkish, true, Some(p)) => format!("Duraklat\u{131}ld\u{131} \u{b7} %{p}"),
    }
}

/// The state line of a game that is being prepared (`Ready to play` and its kin).
pub fn state_label(paused: bool, language: Language) -> &'static str {
    match (language, paused) {
        (Language::English, false) => "Preparing",
        (Language::English, true) => "Paused",
        (Language::Turkish, false) => "Haz\u{131}rlan\u{131}yor",
        (Language::Turkish, true) => "Duraklat\u{131}ld\u{131}",
    }
}

/// What the hero says next to the bar of a game being prepared on the launcher's own account.
pub fn hero_note(paused: bool, language: Language) -> &'static str {
    match (language, paused) {
        (Language::English, false) => {
            "Getting this game ready in the background. Play moves it to the front."
        }
        (Language::English, true) => {
            "Waiting for the PC to be free. It goes on by itself, or at once when you press Play."
        }
        (Language::Turkish, false) => {
            "Bu oyun arka planda haz\u{131}rlan\u{131}yor. Oyna'ya bas\u{131}nca \u{f6}ne al\u{131}n\u{131}r."
        }
        (Language::Turkish, true) => {
            "PC'nin bo\u{15f} olmas\u{131} bekleniyor. Kendili\u{11f}inden devam eder; Oyna'ya bas\u{131}nca hemen ba\u{15f}lar."
        }
    }
}

/// The time left, from the seconds a calm estimate gives: "Under a minute left", "About 3 min left".
pub fn time_left(seconds: f32, language: Language) -> String {
    let minutes = (seconds / 60.0).round() as i64;
    let hours = (seconds / 3600.0).round() as i64;
    match language {
        Language::English => {
            if seconds < 45.0 {
                "Under a minute left".to_owned()
            } else if minutes < 90 {
                substitute("About %d min left", &[Arg::Int(minutes.max(1))])
            } else {
                substitute("About %d h left", &[Arg::Int(hours)])
            }
        }
        Language::Turkish => {
            if seconds < 45.0 {
                "Bir dakikadan az kald\u{131}".to_owned()
            } else if minutes < 90 {
                substitute(
                    "Yakla\u{15f}\u{131}k %d dk kald\u{131}",
                    &[Arg::Int(minutes.max(1))],
                )
            } else {
                substitute("Yakla\u{15f}\u{131}k %d sa kald\u{131}", &[Arg::Int(hours)])
            }
        }
    }
}

/// The sentence of a failed unlock: what stopped it and the next step, in one line.
pub fn failure(kind: FailureKind, language: Language) -> &'static str {
    match (language, kind) {
        (Language::English, FailureKind::ToolkitMissing) => {
            "No toolkit is installed. Install it under Settings > Toolkit, then unlock again."
        }
        (Language::English, FailureKind::DiskFull) => {
            "The drive of your games folder is full. Free some space on it, then try again."
        }
        (Language::English, FailureKind::FileInUse) => {
            "Another program holds a file of the game. Close it or pause its scan of the games folder, then try again."
        }
        (Language::English, FailureKind::BadDump) => {
            "This dump cannot be used. Dump the game again from your console and replace the folder."
        }
        (Language::English, FailureKind::Converter) => {
            "The converter stopped. Try again. If it stops again, Options > Log shows why, and Report a Problem sends it to us."
        }
        (Language::English, FailureKind::Other) => {
            "Try again. If it stops again, Options > Log shows why, and Report a Problem sends it to us."
        }
        (Language::Turkish, FailureKind::ToolkitMissing) => {
            "Ara\u{e7} seti kurulu de\u{11f}il. Ayarlar > Ara\u{e7} seti b\u{f6}l\u{fc}m\u{fc}nden kur, sonra yeniden kilidi a\u{e7}."
        }
        (Language::Turkish, FailureKind::DiskFull) => {
            "Oyun klas\u{f6}r\u{fc}n\u{fc}n s\u{fc}r\u{fc}c\u{fc}s\u{fc} dolu. Yer a\u{e7}, sonra yeniden dene."
        }
        (Language::Turkish, FailureKind::FileInUse) => {
            "Ba\u{15f}ka bir program oyunun bir dosyas\u{131}n\u{131} tutuyor. Onu kapat ya da oyun klas\u{f6}r\u{fc}n\u{fc} taramas\u{131}n\u{131} duraklat, sonra yeniden dene."
        }
        (Language::Turkish, FailureKind::BadDump) => {
            "Bu d\u{f6}k\u{fc}m kullan\u{131}lamaz. Oyunu konsolundan yeniden d\u{f6}k ve klas\u{f6}r\u{fc} de\u{11f}i\u{15f}tir."
        }
        (Language::Turkish, FailureKind::Converter) => {
            "D\u{f6}n\u{fc}\u{15f}t\u{fc}r\u{fc}c\u{fc} durdu. Yeniden dene. Yine durursa Se\u{e7}enekler > G\u{fc}nl\u{fc}k nedenini g\u{f6}sterir; Sorun bildir bunu bize g\u{f6}nderir."
        }
        (Language::Turkish, FailureKind::Other) => {
            "Yeniden dene. Yine durursa Se\u{e7}enekler > G\u{fc}nl\u{fc}k nedenini g\u{f6}sterir; Sorun bildir bunu bize g\u{f6}nderir."
        }
    }
}

/// The headline of a failed unlock (the sentence under it says what to do).
pub fn failure_headline(language: Language) -> &'static str {
    match language {
        Language::English => "Unlocking stopped.",
        Language::Turkish => "Kilit a\u{e7}ma durdu.",
    }
}

/// The toast when a game could not be prepared in the background.
pub fn prepare_failed(language: Language) -> &'static str {
    match language {
        Language::English => "Could not prepare",
        Language::Turkish => "Haz\u{131}rlanamad\u{131}",
    }
}

/// Settings > Behaviour: the switch for preparing new games by itself.
pub fn setting(language: Language) -> &'static str {
    match language {
        Language::English => "Prepare New Games Automatically",
        Language::Turkish => "Yeni oyunlar\u{131} otomatik haz\u{131}rla",
    }
}

pub fn setting_detail(language: Language) -> &'static str {
    match language {
        Language::English => {
            "Unlocks new games in the background while the PC is idle, so they are ready when you want to play. Stops whenever a game starts."
        }
        Language::Turkish => {
            "PC bo\u{15f}tayken yeni oyunlar\u{131}n kilidini arka planda a\u{e7}ar; oynamak istedi\u{11f}inde haz\u{131}r olurlar. Bir oyun ba\u{15f}lay\u{131}nca durur."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [FailureKind; 6] = [
        FailureKind::ToolkitMissing,
        FailureKind::DiskFull,
        FailureKind::FileInUse,
        FailureKind::BadDump,
        FailureKind::Converter,
        FailureKind::Other,
    ];

    #[test]
    fn every_failure_names_a_next_step_in_both_languages() {
        for language in Language::ALL {
            for kind in KINDS {
                let text = failure(kind, language);
                assert!(text.ends_with('.'), "{kind:?} {language:?}");
                assert!(text.len() > 30, "{kind:?} {language:?}");
            }
        }
    }

    #[test]
    fn the_tile_label_is_quiet_and_honest() {
        let en = Language::English;
        assert_eq!(tile_label(None, false, en), "Preparing\u{2026}");
        assert_eq!(tile_label(Some(40), false, en), "Preparing\u{2026} 40%");
        assert_eq!(tile_label(Some(40), true, en), "Paused \u{b7} 40%");
        assert_eq!(
            tile_label(Some(7), false, Language::Turkish),
            "Haz\u{131}rlan\u{131}yor\u{2026} %7"
        );
    }

    #[test]
    fn the_time_left_rounds_calmly() {
        let en = Language::English;
        assert_eq!(time_left(20.0, en), "Under a minute left");
        assert_eq!(time_left(50.0, en), "About 1 min left");
        assert_eq!(time_left(170.0, en), "About 3 min left");
        assert_eq!(time_left(3.0 * 3600.0, en), "About 3 h left");
        assert_eq!(
            time_left(100.0, Language::Turkish),
            "Yakla\u{15f}\u{131}k 2 dk kald\u{131}"
        );
    }

    #[test]
    fn all_texts_exist_in_both_languages() {
        for language in Language::ALL {
            assert!(!state_label(false, language).is_empty());
            assert!(!state_label(true, language).is_empty());
            assert!(!hero_note(false, language).is_empty());
            assert!(!hero_note(true, language).is_empty());
            assert!(!failure_headline(language).is_empty());
            assert!(!prepare_failed(language).is_empty());
            assert!(!setting(language).is_empty());
            assert!(!setting_detail(language).is_empty());
        }
    }
}
