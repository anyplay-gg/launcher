//! Ported from utkuhalis/AnyPS5-ARM (Localization.swift, GPL-2.0-only) and changed since; see README "Credits".
//! Localisation (docs/DESIGN.md section 8): English and Turkish, 213 keys generated from the
//! macOS launcher's `Localization.swift` by `tools/gen-i18n.mjs`, with the Windows replacements
//! of section 9.3 already applied.
//!
//! Lookup is `current language table, then English, then the key name`. Because both tables are
//! complete `match`es the fallbacks can never fire; they are kept in [`Language::text`] only as the
//! documented contract.

include!("i18n_table.rs");

/// Interface languages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Language {
    Turkish,
    #[default]
    English,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::Turkish, Language::English];

    /// Preference value (`"tr"` / `"en"`).
    pub const fn code(self) -> &'static str {
        match self {
            Language::Turkish => "tr",
            Language::English => "en",
        }
    }

    pub fn from_code(code: &str) -> Option<Language> {
        match code {
            "tr" => Some(Language::Turkish),
            "en" => Some(Language::English),
            _ => None,
        }
    }

    /// Name in its own language.
    pub const fn display_name(self) -> &'static str {
        match self {
            Language::Turkish => "Türkçe",
            Language::English => "English",
        }
    }

    /// Two-letter tag shown on the onboarding language cards.
    pub const fn tag(self) -> &'static str {
        match self {
            Language::Turkish => "TR",
            Language::English => "EN",
        }
    }

    /// The console language a game starts in when its own setting follows the launcher.
    pub const fn console_language(self) -> u32 {
        match self {
            Language::Turkish => 19,
            Language::English => 1,
        }
    }

    /// Turkish when the OS UI language starts with `tr`, else English.
    pub fn system() -> Language {
        match sys_locale::get_locale() {
            Some(locale) if locale.to_ascii_lowercase().starts_with("tr") => Language::Turkish,
            _ => Language::English,
        }
    }

    /// Looks a key up in this language.
    pub const fn text(self, key: Key) -> &'static str {
        if !cfg!(windows)
            && let Some(text) = elsewhere(self, key)
        {
            return text;
        }
        if let Some(text) = ours(self, key) {
            return text;
        }
        match self {
            Language::Turkish => key.turkish(),
            Language::English => key.english(),
        }
    }

    /// Looks a key up and substitutes `%d` / `%@` in order. `%%` yields a percent sign.
    pub fn format(self, key: Key, args: &[Arg<'_>]) -> String {
        substitute(self.text(key), args)
    }

    /// Locale tag for dates (`en-US` / `tr-TR`).
    pub const fn locale(self) -> &'static str {
        match self {
            Language::Turkish => "tr-TR",
            Language::English => "en-US",
        }
    }
}

/// Strings AnyPlay words its own way, in place of the generated table's: they speak of the
/// player's games and game dumps, never of a platform's name.
const fn ours(language: Language, key: Key) -> Option<&'static str> {
    Some(match (language, key) {
        (Language::English, Key::WelcomeSubtitle) => {
            "Play your games on your PC, and with your friends. A few quick steps first."
        }
        (Language::English, Key::ChooseFolderSubtitle) => {
            "Choose the folder that holds your game dumps. Subfolders are scanned too, and new games show up on their own."
        }
        (Language::English, Key::EmptyLibraryText) => {
            "Put game dumps in your games folder and they show up here on their own."
        }
        (Language::English, Key::AboutText) => {
            "AnyPlay Launcher runs your games on Windows with AnyPS5."
        }
        (Language::Turkish, Key::WelcomeSubtitle) => {
            "Oyunlarını PC'nde ve arkadaşlarınla oyna. Önce birkaç kısa adım."
        }
        (Language::Turkish, Key::ChooseFolderSubtitle) => {
            "Oyun dump'larının olduğu klasörü seç. Alt klasörleri de tararım, yeni oyunları kendim bulurum."
        }
        (Language::Turkish, Key::EmptyLibraryText) => {
            "Oyun klasörüne oyun dump'larını koy, burada kendiliğinden görünsünler."
        }
        (Language::Turkish, Key::AboutText) => {
            "AnyPlay Launcher oyunlarını AnyPS5 ile Windows'ta çalıştırır."
        }
        (Language::English, Key::DiagnosisUnsupported) => {
            "Not supported yet. Send a report from Options > Log so it can be added. Function: %@"
        }
        (Language::English, Key::DiagnosisGpuLost) => {
            "The GPU stopped responding. Update your graphics driver, then start the game again."
        }
        (Language::English, Key::DiagnosisOutOfMemory) => {
            "The game ran out of memory. Close other programs, then start the game again."
        }
        (Language::English, Key::DiagnosisEngineError) => {
            "Engine error. Start the game again. If it keeps happening, send a report from Options > Log. Details: %@"
        }
        (Language::English, Key::DiagnosisCrashed) => {
            "The game crashed. Start the game again. If it keeps happening, send a report from Options > Log. (%@)"
        }
        (Language::English, Key::ReportFailed) => {
            "Could not save the report. Check that the disk has space, then try again."
        }
        (Language::Turkish, Key::DiagnosisUnsupported) => {
            "Henüz desteklenmiyor. Eklenebilmesi için Seçenekler > Günlük'ten rapor gönder. Fonksiyon: %@"
        }
        (Language::Turkish, Key::DiagnosisGpuLost) => {
            "GPU ile bağlantı koptu. Grafik sürücünü güncelle, sonra oyunu yeniden başlat."
        }
        (Language::Turkish, Key::DiagnosisOutOfMemory) => {
            "Bellek yetersiz kaldı. Diğer programları kapat, sonra oyunu yeniden başlat."
        }
        (Language::Turkish, Key::DiagnosisEngineError) => {
            "Motor hatası. Oyunu yeniden başlat. Tekrarlarsa Seçenekler > Günlük'ten rapor gönder. Ayrıntı: %@"
        }
        (Language::Turkish, Key::DiagnosisCrashed) => {
            "Oyun çöktü. Oyunu yeniden başlat. Tekrarlarsa Seçenekler > Günlük'ten rapor gönder. (%@)"
        }
        (Language::Turkish, Key::ReportFailed) => {
            "Rapor kaydedilemedi. Diskte yer olduğunu kontrol edip yeniden dene."
        }
        _ => return None,
    })
}

/// The few strings that name the system. The table is generated with Windows in them; on every
/// other system these take their place.
const fn elsewhere(language: Language, key: Key) -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        return match (language, key) {
            (Language::English, Key::StartingText) => Some(
                "The game is opening, please wait. The first start can take a few minutes while Rosetta translates the game and its shaders compile.",
            ),
            (Language::English, Key::StepRelink) => Some("Converting for macOS"),
            (Language::English, Key::ShowInFinder) => Some("Show in Finder"),
            (Language::English, Key::AboutText) => {
                Some("AnyPlay Launcher runs your games on macOS with AnyPS5, Rosetta and MoltenVK.")
            }
            (Language::Turkish, Key::StartingText) => Some(
                "Oyun açılıyor, lütfen bekle. İlk açılış, Rosetta oyunu çevirirken ve gölgelendiriciler derlenirken birkaç dakika sürebilir.",
            ),
            (Language::Turkish, Key::StepRelink) => Some("macOS için çevriliyor"),
            (Language::Turkish, Key::ShowInFinder) => Some("Finder’da göster"),
            (Language::Turkish, Key::AboutText) => Some(
                "AnyPlay Launcher oyunlarını AnyPS5, Rosetta ve MoltenVK ile macOS'ta çalıştırır.",
            ),
            _ => None,
        };
    }
    Some(match (language, key) {
        (Language::English, Key::StartingText) => {
            "The game is opening, please wait. The first start can take a few minutes while the game gets ready for Linux."
        }
        (Language::English, Key::StepRelink) => "Converting for Linux",
        (Language::English, Key::ShowInFinder) => "Show in file manager",
        (Language::English, Key::AboutText) => {
            "AnyPlay Launcher runs your games on Linux with AnyPS5."
        }
        (Language::Turkish, Key::StartingText) => {
            "Oyun açılıyor, lütfen bekle. İlk açılış, oyun Linux için hazırlanırken birkaç dakika sürebilir."
        }
        (Language::Turkish, Key::StepRelink) => "Linux için çevriliyor",
        (Language::Turkish, Key::ShowInFinder) => "Dosya yöneticisinde göster",
        (Language::Turkish, Key::AboutText) => {
            "AnyPlay Launcher oyunlarını AnyPS5 ile Linux'ta çalıştırır."
        }
        _ => return None,
    })
}

/// One format argument.
#[derive(Clone, Copy, Debug)]
pub enum Arg<'a> {
    Int(i64),
    Str(&'a str),
}

impl From<i64> for Arg<'_> {
    fn from(value: i64) -> Self {
        Arg::Int(value)
    }
}

impl From<usize> for Arg<'_> {
    fn from(value: usize) -> Self {
        Arg::Int(value as i64)
    }
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(value: &'a str) -> Self {
        Arg::Str(value)
    }
}

/// Replaces `%d` and `%@` placeholders with `args` in order.
pub fn substitute(template: &str, args: &[Arg<'_>]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut next = args.iter();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('d') | Some('@') => {
                chars.next();
                match next.next() {
                    Some(Arg::Int(v)) => out.push_str(&v.to_string()),
                    Some(Arg::Str(s)) => out.push_str(s),
                    None => {}
                }
            }
            Some('%') => {
                chars.next();
                out.push('%');
            }
            _ => out.push('%'),
        }
    }
    out
}

impl Key {
    /// Keys that only make sense on macOS (Rosetta, move to Applications). They stay in the table
    /// for completeness but no Windows screen shows them.
    pub const fn is_macos_only(self) -> bool {
        matches!(
            self,
            Key::RosettaMissing
                | Key::RosettaInstall
                | Key::RosettaInstalled
                | Key::MoveToApplications
                | Key::MoveToApplicationsText
                | Key::MoveNow
                | Key::Later
        )
    }
}

/// Decimal-unit file size (1 GB = 10^9 bytes) with the language's decimal separator, e.g. `8,39 GB`
/// in Turkish and `8.39 GB` in English. Negative input means "not measured yet" and yields an ellipsis.
pub fn byte_text(bytes: i64, language: Language) -> String {
    if bytes < 0 {
        return "…".to_owned();
    }
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        return format!("{bytes} bytes");
    }
    let text = if value >= 100.0 {
        format!("{value:.0}")
    } else if value >= 10.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.2}")
    };
    let text = if language == Language::Turkish {
        text.replace('.', ",")
    } else {
        text
    };
    format!("{text} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_213_keys_exist_in_both_languages() {
        assert_eq!(Key::ALL.len(), 213);
        for key in Key::ALL {
            for language in Language::ALL {
                assert!(
                    !language.text(key).is_empty(),
                    "{} is empty in {language:?}",
                    key.name()
                );
            }
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn system_names_follow_the_system() {
        assert_eq!(
            Language::English.text(Key::StepRelink),
            "Converting for Linux"
        );
        assert_eq!(
            Language::Turkish.text(Key::ShowInFinder),
            "Dosya yöneticisinde göster"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_names_do_not_claim_linux_or_windows_support() {
        assert_eq!(
            Language::English.text(Key::StepRelink),
            "Converting for macOS"
        );
        assert_eq!(Language::English.text(Key::ShowInFinder), "Show in Finder");
    }

    #[cfg(windows)]
    #[test]
    fn windows_replacements_are_applied() {
        assert_eq!(
            Language::English.text(Key::WelcomeSubtitle),
            "Play your games on your PC, and with your friends. A few quick steps first."
        );
        assert_eq!(
            Language::English.text(Key::StepRelink),
            "Converting for Windows"
        );
        assert_eq!(
            Language::Turkish.text(Key::StepRelink),
            "Windows için çevriliyor"
        );
        assert_eq!(
            Language::English.text(Key::ShowInFinder),
            "Show in Explorer"
        );
        assert_eq!(
            Language::English.text(Key::AboutText),
            "AnyPlay Launcher runs your games on Windows with AnyPS5."
        );
    }

    #[test]
    fn no_visible_string_uses_the_old_name() {
        for key in Key::ALL {
            if key.is_macos_only() {
                continue;
            }
            for language in Language::ALL {
                let text = language.text(key);
                assert!(
                    !text.contains("AnyPS5 Launcher"),
                    "{} ({language:?}): {text}",
                    key.name()
                );
                // AnyPS5 is still the engine (the About text) and the store folder of games.
                if !matches!(key, Key::AboutText | Key::InstalledSize) {
                    assert!(
                        !text.contains("AnyPS5"),
                        "{} ({language:?}) names AnyPS5: {text}",
                        key.name()
                    );
                }
            }
        }
    }

    #[test]
    fn no_visible_string_names_the_platform() {
        for key in Key::ALL {
            for language in Language::ALL {
                let text = language.text(key).replace("AnyPS5", "");
                assert!(
                    !text.contains("PS5") && !text.contains("PlayStation"),
                    "{} ({language:?}) names the platform: {text}",
                    key.name()
                );
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn no_visible_string_still_names_the_mac() {
        for key in Key::ALL {
            if key.is_macos_only() {
                continue;
            }
            for language in Language::ALL {
                let text = language.text(key);
                for word in ["Mac", "Finder", "Rosetta", "APFS", "MoltenVK"] {
                    assert!(
                        !text.contains(word),
                        "{} ({language:?}) still says {word}: {text}",
                        key.name()
                    );
                }
            }
        }
    }

    #[test]
    fn format_substitutes_in_order() {
        assert_eq!(
            Language::English.format(Key::GamesFound, &[3usize.into()]),
            "3 games found"
        );
        assert_eq!(
            Language::Turkish.format(Key::GamesFound, &[3usize.into()]),
            "3 oyun bulundu"
        );
        assert_eq!(
            Language::English.format(Key::DiagnosisMissingModule, &["libX.dll".into()]),
            "Missing module: libX.dll. Unlocking the game again may fix it."
        );
        assert_eq!(substitute("100%% of %d", &[Arg::Int(7)]), "100% of 7");
    }

    #[test]
    fn language_codes_round_trip() {
        for language in Language::ALL {
            assert_eq!(Language::from_code(language.code()), Some(language));
        }
        assert_eq!(Language::from_code("de"), None);
    }

    #[test]
    fn byte_text_uses_decimal_units() {
        assert_eq!(byte_text(8_390_000_000, Language::Turkish), "8,39 GB");
        assert_eq!(byte_text(8_390_000_000, Language::English), "8.39 GB");
        assert_eq!(byte_text(-1, Language::English), "…");
        assert_eq!(byte_text(999, Language::English), "999 bytes");
        assert_eq!(byte_text(12_300_000, Language::English), "12.3 MB");
    }
}
