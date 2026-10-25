//! Texts of the launcher's own update: the About rows, the notice on the home screen and the
//! panel with what is new (docs/DESIGN.md 9.5). English and Turkish like the rest. The Turkish was
//! written for this change and has not been read by a native speaker yet. Rows and buttons are in
//! title case, notes in sentence case. Errors say what happened and the one thing to do next.

use crate::ui::i18n::{Arg, Language, substitute};

/// Where a launcher that cannot update itself is downloaded. The launcher opens only this page
/// from here.
pub const RELEASES: &str = "https://github.com/anyplaynet/launcher/releases/latest";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpdateText {
    /// The About row that tells where the update stands.
    Row,
    Checking,
    UpToDate,
    /// `%@` = the percentage, e.g. `42%`.
    Downloading,
    Verifying,
    /// `%@` = the version.
    Ready,
    WaitingForGame,
    /// `%@` = the version.
    Available,
    NotReachable,
    NotInstalled,
    // Notes under the row
    /// `%@` = the version.
    NoteDownloading,
    NoteReady,
    /// `%@` = the version.
    NoteHolding,
    NoteReadOnly,
    /// This launcher cannot install updates itself (a Linux `.tar.gz`).
    NoteManual,
    NoteOffline,
    NoteRefused,
    NoteOther,
    // Buttons and rows that act
    CancelRestart,
    Later,
    /// `%@` = the version.
    WhatsNewIn,
    Download,
    // The notice on the home screen
    /// `%@` = the version.
    NoticeReady,
    /// On the notice while a game runs and the restart waits.
    NoticeHolding,
    /// The accessible name of the notice. `%@` = the version.
    NoticeName,
    /// The same while the restart waits for a game.
    NoticeHoldingName,
    /// The accessible name of the settings button while an update waits. `%@` = "Settings".
    SettingsBadge,
    // The "Update required" screen (docs/DESIGN.md 9.5.1)
    RequiredTitle,
    /// The one-line reason. `%@` = the version to update to.
    RequiredReason,
    /// The same when the version is not known yet.
    RequiredReasonUnknown,
    /// The reason when the player asked for the online area while the update is required.
    RequiredOnline,
    UpdateNow,
    PlayOffline,
    TryAgain,
    /// Required, and GitHub could not be reached.
    RequiredOffline,
    /// Required, but the check found no newer release (the cloud turned the launcher away first).
    RequiredNoNewer,
    /// Required and this launcher cannot install it itself.
    RequiredNoteManual,
}

impl UpdateText {
    pub const ALL: [UpdateText; 37] = [
        UpdateText::Row,
        UpdateText::Checking,
        UpdateText::UpToDate,
        UpdateText::Downloading,
        UpdateText::Verifying,
        UpdateText::Ready,
        UpdateText::WaitingForGame,
        UpdateText::Available,
        UpdateText::NotReachable,
        UpdateText::NotInstalled,
        UpdateText::NoteDownloading,
        UpdateText::NoteReady,
        UpdateText::NoteHolding,
        UpdateText::NoteReadOnly,
        UpdateText::NoteManual,
        UpdateText::NoteOffline,
        UpdateText::NoteRefused,
        UpdateText::NoteOther,
        UpdateText::CancelRestart,
        UpdateText::Later,
        UpdateText::WhatsNewIn,
        UpdateText::Download,
        UpdateText::NoticeReady,
        UpdateText::NoticeHolding,
        UpdateText::NoticeName,
        UpdateText::NoticeHoldingName,
        UpdateText::SettingsBadge,
        UpdateText::RequiredTitle,
        UpdateText::RequiredReason,
        UpdateText::RequiredReasonUnknown,
        UpdateText::RequiredOnline,
        UpdateText::UpdateNow,
        UpdateText::PlayOffline,
        UpdateText::TryAgain,
        UpdateText::RequiredOffline,
        UpdateText::RequiredNoNewer,
        UpdateText::RequiredNoteManual,
    ];

    pub const fn english(self) -> &'static str {
        match self {
            UpdateText::Row => "Launcher Update",
            UpdateText::Checking => "Checking…",
            UpdateText::UpToDate => "Up to date",
            UpdateText::Downloading => "Downloading %@",
            UpdateText::Verifying => "Verifying",
            UpdateText::Ready => "%@ ready",
            UpdateText::WaitingForGame => "Waiting for your game",
            UpdateText::Available => "%@ available",
            UpdateText::NotReachable => "Not reachable",
            UpdateText::NotInstalled => "Not installed",
            UpdateText::NoteDownloading => "Launcher %@ downloads in the background.",
            UpdateText::NoteReady => {
                "Restart now, or the update installs the next time you close the launcher."
            }
            UpdateText::NoteHolding => {
                "Launcher %@ restarts when your game closes. Your game keeps running."
            }
            UpdateText::NoteReadOnly => {
                "The launcher cannot update itself in this read-only folder. Download it from GitHub."
            }
            UpdateText::NoteManual => {
                "This launcher cannot update itself. Download the new version from GitHub, or use the AppImage, which updates itself."
            }
            UpdateText::NoteOffline => "Could not reach GitHub. The launcher tries again later.",
            UpdateText::NoteRefused => {
                "The update could not be verified, so it was not installed. Download the latest launcher from GitHub."
            }
            UpdateText::NoteOther => {
                "The update could not be installed. Download the latest launcher from GitHub."
            }
            UpdateText::CancelRestart => "Cancel Restart",
            UpdateText::Later => "Later",
            UpdateText::WhatsNewIn => "What's New in %@",
            UpdateText::Download => "Download from GitHub",
            UpdateText::NoticeReady => "Launcher %@ is ready",
            UpdateText::NoticeHolding => "Restarts when your game closes",
            UpdateText::NoticeName => "Launcher %@ is ready to install",
            UpdateText::NoticeHoldingName => "Launcher %@ restarts when your game closes",
            UpdateText::SettingsBadge => "%@, update ready",
            UpdateText::RequiredTitle => "Update required",
            UpdateText::RequiredReason => "AnyPlay Launcher %@ is needed to play online.",
            UpdateText::RequiredReasonUnknown => {
                "This version of AnyPlay Launcher can no longer play online."
            }
            UpdateText::RequiredOnline => {
                "The online area needs the latest launcher. Your games still play offline."
            }
            UpdateText::UpdateNow => "Update Now",
            UpdateText::PlayOffline => "Play Offline",
            UpdateText::TryAgain => "Try Again",
            UpdateText::RequiredOffline => {
                "Could not reach GitHub. Check your connection, then try again."
            }
            UpdateText::RequiredNoNewer => {
                "No newer launcher was found yet. Try again in a moment."
            }
            UpdateText::RequiredNoteManual => {
                "Download the new version from GitHub and install it."
            }
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            UpdateText::Row => "Başlatıcı Güncellemesi",
            UpdateText::Checking => "Denetleniyor…",
            UpdateText::UpToDate => "Güncel",
            UpdateText::Downloading => "İndiriliyor %@",
            UpdateText::Verifying => "Doğrulanıyor",
            UpdateText::Ready => "%@ hazır",
            UpdateText::WaitingForGame => "Oyunun kapanması bekleniyor",
            UpdateText::Available => "%@ var",
            UpdateText::NotReachable => "Ulaşılamadı",
            UpdateText::NotInstalled => "Kurulmadı",
            UpdateText::NoteDownloading => "Başlatıcı %@ arka planda indiriliyor.",
            UpdateText::NoteReady => {
                "Şimdi yeniden başlat ya da güncelleme, başlatıcıyı bir sonraki kapatışında kurulur."
            }
            UpdateText::NoteHolding => {
                "Başlatıcı %@, oyun kapanınca yeniden başlar. Oyunun açık kalır."
            }
            UpdateText::NoteReadOnly => {
                "Başlatıcı bu salt okunur klasörde kendini güncelleyemez. GitHub'dan indir."
            }
            UpdateText::NoteManual => {
                "Bu başlatıcı kendini güncelleyemez. Yeni sürümü GitHub'dan indir ya da kendini güncelleyen AppImage'i kullan."
            }
            UpdateText::NoteOffline => "GitHub'a ulaşılamadı. Başlatıcı daha sonra yeniden dener.",
            UpdateText::NoteRefused => {
                "Güncelleme doğrulanamadı, bu yüzden kurulmadı. Son sürümü GitHub'dan indir."
            }
            UpdateText::NoteOther => "Güncelleme kurulamadı. Son sürümü GitHub'dan indir.",
            UpdateText::CancelRestart => "Yeniden başlatmayı iptal et",
            UpdateText::Later => "Sonra",
            UpdateText::WhatsNewIn => "%@ sürümünde yenilikler",
            UpdateText::Download => "GitHub'dan indir",
            UpdateText::NoticeReady => "Başlatıcı %@ hazır",
            UpdateText::NoticeHolding => "Oyun kapanınca yeniden başlar",
            UpdateText::NoticeName => "Başlatıcı %@ kurulmaya hazır",
            UpdateText::NoticeHoldingName => "Başlatıcı %@, oyun kapanınca yeniden başlar",
            UpdateText::SettingsBadge => "%@, güncelleme hazır",
            UpdateText::RequiredTitle => "Güncelleme gerekli",
            UpdateText::RequiredReason => "Çevrimiçi oynamak için AnyPlay Başlatıcı %@ gerekir.",
            UpdateText::RequiredReasonUnknown => {
                "AnyPlay Başlatıcı'nın bu sürümüyle artık çevrimiçi oynanamaz."
            }
            UpdateText::RequiredOnline => {
                "Çevrimiçi alan en yeni başlatıcıyı gerektirir. Oyunlarını çevrimdışı oynamaya devam edebilirsin."
            }
            UpdateText::UpdateNow => "Şimdi Güncelle",
            UpdateText::PlayOffline => "Çevrimdışı Oyna",
            UpdateText::TryAgain => "Tekrar Dene",
            UpdateText::RequiredOffline => {
                "GitHub'a ulaşılamadı. Bağlantını denetle, sonra tekrar dene."
            }
            UpdateText::RequiredNoNewer => {
                "Henüz daha yeni bir başlatıcı bulunamadı. Biraz sonra tekrar dene."
            }
            UpdateText::RequiredNoteManual => "Yeni sürümü GitHub'dan indir ve kur.",
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
        for text in UpdateText::ALL {
            for language in Language::ALL {
                assert!(
                    !text.text(language).trim().is_empty(),
                    "{text:?} {language:?}"
                );
            }
        }
    }

    #[test]
    fn the_list_names_every_text_once() {
        let unique: std::collections::HashSet<_> = UpdateText::ALL.into_iter().collect();
        assert_eq!(unique.len(), UpdateText::ALL.len());
    }

    #[test]
    fn the_texts_that_carry_a_version_or_share_take_it() {
        for text in [
            UpdateText::Downloading,
            UpdateText::Ready,
            UpdateText::Available,
            UpdateText::NoteDownloading,
            UpdateText::NoteHolding,
            UpdateText::WhatsNewIn,
            UpdateText::NoticeReady,
            UpdateText::NoticeName,
            UpdateText::NoticeHoldingName,
            UpdateText::SettingsBadge,
            UpdateText::RequiredReason,
        ] {
            for language in Language::ALL {
                assert!(text.text(language).contains("%@"), "{text:?} {language:?}");
            }
        }
    }

    #[test]
    fn the_download_page_is_ours_and_secure() {
        assert!(RELEASES.starts_with("https://github.com/anyplaynet/"));
    }
}
