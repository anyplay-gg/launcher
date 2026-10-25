//! Texts of Clips: the settings rows and the toasts. English and Turkish like the rest.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClipsText {
    /// Settings row: how long a clip is.
    Title,
    Detail,
    Off,
    /// The lengths, in the order of `anyps5_core::clips::LENGTHS` after Off.
    Seconds15,
    Seconds30,
    Minute1,
    Minutes2,
    /// Settings row: the key that saves a clip.
    Key,
    /// Shown on the key row while it waits for a key.
    KeyPrompt,
    /// Settings row: opens the folder. Detail: how many clips are in it.
    Show,
    CountNone,
    CountOne,
    /// `%@` = how many.
    CountMany,
    /// Toast after a clip was written.
    Saved,
    /// Toast: a save did not work.
    SaveFailed,
    /// Toast: pressed before the first seconds were kept.
    NothingYet,
    /// Toast: Create was pressed in a game while Clips is off.
    IsOff,
    /// Toast: the worker has no hardware video encoder.
    NoEncoder,
    NoWindow,
    /// Toast: the recorder stopped in the middle of a game.
    Stopped,
    NoWorker,
    /// Toast: `%@` = the key, which another program has.
    KeyTaken,
}

impl ClipsText {
    pub const fn english(self) -> &'static str {
        match self {
            ClipsText::Title => "Clips",
            ClipsText::Detail => "Keeps the last seconds of a running game, without sound",
            ClipsText::Off => "Off",
            ClipsText::Seconds15 => "15 seconds",
            ClipsText::Seconds30 => "30 seconds",
            ClipsText::Minute1 => "1 minute",
            ClipsText::Minutes2 => "2 minutes",
            ClipsText::Key => "Clip Key",
            ClipsText::KeyPrompt => "Press the keys to use. Esc keeps the current ones.",
            ClipsText::Show => "Show Clips",
            ClipsText::CountNone => "No clips yet",
            ClipsText::CountOne => "1 clip",
            ClipsText::CountMany => "%@ clips",
            ClipsText::Saved => "Clip saved",
            ClipsText::SaveFailed => "The clip could not be saved.",
            ClipsText::NothingYet => "Nothing recorded yet. Try again in a moment.",
            ClipsText::IsOff => "Clips are off. Turn them on in Settings.",
            ClipsText::NoEncoder => "Clips need a graphics card with a video encoder.",
            ClipsText::NoWindow => "Clips could not find the game's window.",
            ClipsText::Stopped => "Clips stopped. Start the game again to record.",
            ClipsText::NoWorker => "Clips are not part of this build.",
            ClipsText::KeyTaken => "Another program uses %@. Choose another clip key.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            ClipsText::Title => "Klipler",
            ClipsText::Detail => "Çalışan oyunun son saniyelerini sessiz olarak tutar",
            ClipsText::Off => "Kapalı",
            ClipsText::Seconds15 => "15 saniye",
            ClipsText::Seconds30 => "30 saniye",
            ClipsText::Minute1 => "1 dakika",
            ClipsText::Minutes2 => "2 dakika",
            ClipsText::Key => "Klip tuşu",
            ClipsText::KeyPrompt => "Kullanılacak tuşlara bas. Esc mevcut tuşları korur.",
            ClipsText::Show => "Klipleri göster",
            ClipsText::CountNone => "Henüz klip yok",
            ClipsText::CountOne => "1 klip",
            ClipsText::CountMany => "%@ klip",
            ClipsText::Saved => "Klip kaydedildi",
            ClipsText::SaveFailed => "Klip kaydedilemedi.",
            ClipsText::NothingYet => "Henüz bir şey kaydedilmedi. Biraz sonra tekrar dene.",
            ClipsText::IsOff => "Klipler kapalı. Ayarlar'dan aç.",
            ClipsText::NoEncoder => "Klipler için video kodlayıcısı olan bir ekran kartı gerekir.",
            ClipsText::NoWindow => "Klipler oyunun penceresini bulamadı.",
            ClipsText::Stopped => "Klipler durdu. Kayıt için oyunu yeniden başlat.",
            ClipsText::NoWorker => "Klipler bu sürümde yok.",
            ClipsText::KeyTaken => {
                "%@ tuşunu başka bir program kullanıyor. Başka bir klip tuşu seç."
            }
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

    pub const ALL: [ClipsText; 22] = [
        ClipsText::Title,
        ClipsText::Detail,
        ClipsText::Off,
        ClipsText::Seconds15,
        ClipsText::Seconds30,
        ClipsText::Minute1,
        ClipsText::Minutes2,
        ClipsText::Key,
        ClipsText::KeyPrompt,
        ClipsText::Show,
        ClipsText::CountNone,
        ClipsText::CountOne,
        ClipsText::CountMany,
        ClipsText::Saved,
        ClipsText::SaveFailed,
        ClipsText::NothingYet,
        ClipsText::IsOff,
        ClipsText::NoEncoder,
        ClipsText::NoWindow,
        ClipsText::Stopped,
        ClipsText::NoWorker,
        ClipsText::KeyTaken,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_exists_in_both_languages_with_the_same_placeholders() {
        for text in ClipsText::ALL {
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
    fn no_visible_text_names_a_platform() {
        for text in ClipsText::ALL {
            for language in Language::ALL {
                let shown = text.text(language).to_lowercase();
                assert!(
                    !shown.contains("psn") && !shown.contains("playstation"),
                    "{text:?}"
                );
            }
        }
    }
}
