//! Mapping between the core's preference enums and the UI's tokens. The core knows nothing of the
//! UI, the UI nothing of the files; screens convert at the edge with these functions.

use anyps5_core::settings as core;

use crate::ui::i18n::Language;
use crate::ui::theme::{Accent, Grade, Theme};

pub fn language(value: core::Language) -> Language {
    match value {
        core::Language::Turkish => Language::Turkish,
        core::Language::English => Language::English,
    }
}

pub fn language_to_core(value: Language) -> core::Language {
    match value {
        Language::Turkish => core::Language::Turkish,
        Language::English => core::Language::English,
    }
}

pub fn vibration(value: core::Vibration) -> crate::ui::haptics::Level {
    use crate::ui::haptics::Level;
    match value {
        core::Vibration::Off => Level::Off,
        core::Vibration::Light => Level::Light,
        core::Vibration::Medium => Level::Medium,
        core::Vibration::Strong => Level::Strong,
    }
}

pub fn vibration_to_core(index: usize) -> core::Vibration {
    match index {
        0 => core::Vibration::Off,
        1 => core::Vibration::Light,
        3 => core::Vibration::Strong,
        _ => core::Vibration::Medium,
    }
}

pub fn vibration_index(value: core::Vibration) -> usize {
    match value {
        core::Vibration::Off => 0,
        core::Vibration::Light => 1,
        core::Vibration::Medium => 2,
        core::Vibration::Strong => 3,
    }
}

pub fn theme(value: core::Theme) -> Theme {
    match value {
        core::Theme::Cosmic => Theme::Cosmic,
        core::Theme::Aurora => Theme::Aurora,
        core::Theme::Sunset => Theme::Sunset,
    }
}

pub fn theme_to_core(value: Theme) -> core::Theme {
    match value {
        Theme::Cosmic => core::Theme::Cosmic,
        Theme::Aurora => core::Theme::Aurora,
        Theme::Sunset => core::Theme::Sunset,
    }
}

pub fn accent(value: core::Accent) -> Accent {
    match value {
        core::Accent::Blue => Accent::Blue,
        core::Accent::Mint => Accent::Mint,
        core::Accent::Coral => Accent::Coral,
        core::Accent::Violet => Accent::Violet,
        core::Accent::Gold => Accent::Gold,
        core::Accent::White => Accent::White,
    }
}

pub fn accent_to_core(value: Accent) -> core::Accent {
    match value {
        Accent::Blue => core::Accent::Blue,
        Accent::Mint => core::Accent::Mint,
        Accent::Coral => core::Accent::Coral,
        Accent::Violet => core::Accent::Violet,
        Accent::Gold => core::Accent::Gold,
        Accent::White => core::Accent::White,
    }
}

pub fn grade(value: anyps5_core::trophies::Grade) -> Grade {
    use anyps5_core::trophies::Grade as Core;
    match value {
        Core::Platinum => Grade::Platinum,
        Core::Gold => Grade::Gold,
        Core::Silver => Grade::Silver,
        Core::Bronze => Grade::Bronze,
    }
}

/// The accent a preferences file stands for: the user's pick, else the theme's default.
pub fn effective_accent(preferences: &core::Preferences) -> Accent {
    preferences
        .accent
        .map(accent)
        .unwrap_or_else(|| theme(preferences.theme).default_accent())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enums_round_trip() {
        for t in Theme::ALL {
            assert_eq!(theme(theme_to_core(t)), t);
        }
        for a in Accent::ALL {
            assert_eq!(accent(accent_to_core(a)), a);
        }
        for l in Language::ALL {
            assert_eq!(language(language_to_core(l)), l);
        }
    }

    #[test]
    fn the_accent_follows_the_theme_unless_chosen() {
        let mut prefs = core::Preferences::first_run();
        prefs.theme = core::Theme::Sunset;
        prefs.accent = None;
        assert_eq!(effective_accent(&prefs), Accent::Coral);
        prefs.accent = Some(core::Accent::Violet);
        assert_eq!(effective_accent(&prefs), Accent::Violet);
    }
}
