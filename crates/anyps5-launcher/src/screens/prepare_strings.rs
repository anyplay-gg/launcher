//! The words of preparing a game in the background and of unlocking in general: the quiet tile
//! label, the time left, the failure sentences that name the next step, and the setting
//! (docs/DESIGN.md 5.4). English and Turkish like the rest.

use anyps5_core::unlock::FailureKind;

use crate::screens::share_strings::Share;
use crate::screens::strings::Extra;
use crate::screens::toolkit_pin::Pin;
use crate::ui::i18n::{Arg, Key, Language, substitute};

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

/// Beside the bar of a prepared game that stands still.
pub fn paused_note(language: Language) -> &'static str {
    match language {
        Language::English => "Paused. Goes on when the PC is free.",
        Language::Turkish => "Duraklatıldı. PC boş olunca devam eder.",
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

/// The sentence of a failed unlock: what stopped it and the next step. The places it names are
/// written from the labels the player will find there, so the words cannot drift.
pub fn failure(kind: FailureKind, language: Language) -> String {
    let t = |key: Key| language.text(key);
    let (settings, options) = (t(Key::Settings), t(Key::Options));
    let (toolkit, library) = (Extra::Toolkit.text(language), t(Key::Storage));
    let (log, report) = (t(Key::OpenLog), Share::Report.text(language));
    let template = match (language, kind) {
        (Language::English, FailureKind::ToolkitMissing) => {
            "No toolkit is installed. Install it under %@ > %@, then unlock again."
        }
        (Language::English, FailureKind::DiskFull) => {
            "The drive for your unlocked games is full. Free some space on it, then try again."
        }
        (Language::English, FailureKind::LocationMissing) => {
            "The folder for unlocked games is not there. Connect its drive, or pick another folder in %@ > %@."
        }
        (Language::English, FailureKind::FileInUse) => {
            "Another program holds a file of the game. Close it or pause its scan of the games folder, then try again."
        }
        (Language::English, FailureKind::BadDump) => {
            "This dump cannot be used. Dump the game again from your console and replace the folder."
        }
        (Language::English, FailureKind::Converter) => {
            "The converter stopped. Try again. If it stops again, %@ > %@ shows why, and %@ sends it to us."
        }
        (Language::English, FailureKind::Other) => {
            "Try again. If it stops again, %@ > %@ shows why, and %@ sends it to us."
        }
        (Language::Turkish, FailureKind::ToolkitMissing) => {
            "Araç seti kurulu değil. %@ > %@ bölümünden kur, sonra yeniden kilidi aç."
        }
        (Language::Turkish, FailureKind::DiskFull) => {
            "Kilidi açılan oyunların sürücüsü dolu. Yer aç, sonra yeniden dene."
        }
        (Language::Turkish, FailureKind::LocationMissing) => {
            "Kilidi açılan oyunların klasörü bulunamadı. Sürücüyü tak ya da %@ > %@ bölümünden başka bir klasör seç."
        }
        (Language::Turkish, FailureKind::FileInUse) => {
            "Başka bir program oyunun bir dosyasını tutuyor. Onu kapat ya da oyun klasörünü taramasını duraklat, sonra yeniden dene."
        }
        (Language::Turkish, FailureKind::BadDump) => {
            "Bu döküm kullanılamaz. Oyunu konsolundan yeniden dök ve klasörü değiştir."
        }
        (Language::Turkish, FailureKind::Converter) => {
            "Dönüştürücü durdu. Yeniden dene. Yine durursa %@ > %@ nedenini gösterir; %@ bunu bize gönderir."
        }
        (Language::Turkish, FailureKind::Other) => {
            "Yeniden dene. Yine durursa %@ > %@ nedenini gösterir; %@ bunu bize gönderir."
        }
    };
    let names: &[&str] = match kind {
        FailureKind::ToolkitMissing => &[settings, toolkit],
        FailureKind::LocationMissing => &[settings, library],
        FailureKind::Converter | FailureKind::Other => &[options, log, report],
        FailureKind::DiskFull | FailureKind::FileInUse | FailureKind::BadDump => &[],
    };
    let args: Vec<Arg<'_>> = names.iter().map(|n| Arg::Str(n)).collect();
    substitute(template, &args)
}

/// The sentence when the toolkit a game stays on is gone: the choice is under that game's
/// Options, not in Settings.
pub fn pinned_toolkit_missing(language: Language) -> String {
    let (options, row) = (language.text(Key::Options), Pin::Title.text(language));
    let template = match language {
        Language::English => {
            "The toolkit this game stays on is gone. Pick another under %@ > %@, then unlock again."
        }
        Language::Turkish => {
            "Bu oyunun kullandığı araç seti yok. %@ > %@ bölümünden başka birini seç, sonra yeniden kilidi aç."
        }
    };
    substitute(template, &[Arg::Str(options), Arg::Str(row)])
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

    const KINDS: [FailureKind; 7] = [
        FailureKind::LocationMissing,
        FailureKind::ToolkitMissing,
        FailureKind::DiskFull,
        FailureKind::FileInUse,
        FailureKind::BadDump,
        FailureKind::Converter,
        FailureKind::Other,
    ];

    /// The game page gives a failure 520 dp and three lines of the detail type.
    #[test]
    fn every_failure_fits_the_game_page_in_three_lines() {
        let ctx = egui::Context::default();
        crate::ui::theme::install(&ctx);
        let mut texts = Vec::new();
        for language in Language::ALL {
            for kind in KINDS {
                texts.push(failure(kind, language));
            }
            texts.push(pinned_toolkit_missing(language));
        }
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter().clone();
            for text in &texts {
                let galley = crate::screens::wrapped(
                    &painter,
                    crate::ui::theme::ty::DETAIL_LARGE,
                    text,
                    egui::Color32::WHITE,
                    520.0,
                    3,
                );
                assert!(!galley.elided, "{text}");
            }
        });
        output.textures_delta.clear();
    }

    #[test]
    fn the_places_a_failure_names_carry_the_labels_the_player_finds() {
        for language in Language::ALL {
            let log = language.text(Key::OpenLog);
            for kind in [FailureKind::Converter, FailureKind::Other] {
                assert!(
                    failure(kind, language).contains(log),
                    "{kind:?} {language:?}"
                );
            }
            let place = language.text(Key::Storage);
            assert!(failure(FailureKind::LocationMissing, language).contains(place));
            assert!(
                failure(FailureKind::ToolkitMissing, language)
                    .contains(Extra::Toolkit.text(language))
            );
        }
    }

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
    fn no_text_shows_an_escape_it_forgot_to_resolve() {
        for language in Language::ALL {
            for text in [
                paused_note(language),
                hero_note(false, language),
                hero_note(true, language),
                state_label(true, language),
            ] {
                assert!(!text.contains('{') && !text.contains('}'), "{text}");
            }
        }
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
