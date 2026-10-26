//! Texts of the Captures area (docs/DESIGN.md 9.10). English and Turkish like the rest.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CapturesText {
    /// The area's name: the title, the top bar button, the Home button panel's row.
    Title,
    FilterAll,
    FilterScreenshots,
    FilterClips,
    /// Accessible name of a tile: `%@` = the game, `%@` = when.
    Screenshot,
    Clip,
    /// The first line of the loading state.
    Loading,
    /// Nothing in the folder.
    EmptyTitle,
    /// How to take a screenshot, and a clip where the player can.
    EmptyBody,
    EmptyBodyShotsOnly,
    OpenClipsSettings,
    /// The filter hides everything.
    NoScreenshots,
    NoClips,
    NoClipsBody,
    ShowAll,
    /// The folder is not there. `%@` = its path.
    MissingTitle,
    MissingBody,
    OpenFolderSettings,
    /// The folder is there and cannot be read. `%@` = its path.
    FailedTitle,
    FailedBody,
    TryAgain,
    /// A game's group when the library does not know the game.
    OtherGame,
    /// The full view.
    OpenInPlayer,
    ShowInFolder,
    CopyImage,
    Delete,
    Previous,
    Next,
    /// `%@` = place, `%@` = how many.
    Position,
    /// Under a clip that has no picture.
    ClipNote,
    /// The question before a delete.
    DeleteScreenshotTitle,
    DeleteClipTitle,
    /// `%@` = the file name.
    DeleteBody,
    DeleteScreenshot,
    DeleteClip,
    Cancel,
    /// Toasts. `%@` = the file name.
    Deleted,
    DeleteFailed,
    Copied,
    CopyFailed,
    PlayerFailed,
    FolderFailed,
}

impl CapturesText {
    pub const fn english(self) -> &'static str {
        match self {
            CapturesText::Title => "Captures",
            CapturesText::FilterAll => "All",
            CapturesText::FilterScreenshots => "Screenshots",
            CapturesText::FilterClips => "Clips",
            CapturesText::Screenshot => "Screenshot",
            CapturesText::Clip => "Clip",
            CapturesText::Loading => "Loading captures",
            CapturesText::EmptyTitle => "No captures yet",
            CapturesText::EmptyBody => {
                "Press F12 in a game to take a screenshot. Turn on Clips to keep the last seconds of play as a video."
            }
            CapturesText::EmptyBodyShotsOnly => "Press F12 in a game to take a screenshot.",
            CapturesText::OpenClipsSettings => "Open Clips Settings",
            CapturesText::NoScreenshots => "No screenshots yet",
            CapturesText::NoClips => "No clips yet",
            CapturesText::NoClipsBody => {
                "Turn on Clips in Settings to keep the last seconds of a game as a video."
            }
            CapturesText::ShowAll => "Show All Captures",
            CapturesText::MissingTitle => "Screenshots folder not found",
            CapturesText::MissingBody => {
                "%@ is missing. Take a screenshot in a game to create it, or choose another folder in Settings."
            }
            CapturesText::OpenFolderSettings => "Open Folder Settings",
            CapturesText::FailedTitle => "Captures could not be read",
            CapturesText::FailedBody => {
                "AnyPlay could not read %@. Check that the folder is open to you, then try again."
            }
            CapturesText::TryAgain => "Try Again",
            CapturesText::OtherGame => "Other Captures",
            CapturesText::OpenInPlayer => "Open in Player",
            CapturesText::ShowInFolder => "Show in Folder",
            CapturesText::CopyImage => "Copy Image",
            CapturesText::Delete => "Delete",
            CapturesText::Previous => "Previous capture",
            CapturesText::Next => "Next capture",
            CapturesText::Position => "%@ of %@",
            CapturesText::ClipNote => "Plays in your video player",
            CapturesText::DeleteScreenshotTitle => "Delete this screenshot?",
            CapturesText::DeleteClipTitle => "Delete this clip?",
            CapturesText::DeleteBody => "%@ is removed from your PC. This cannot be undone.",
            CapturesText::DeleteScreenshot => "Delete Screenshot",
            CapturesText::DeleteClip => "Delete Clip",
            CapturesText::Cancel => "Cancel",
            CapturesText::Deleted => "Deleted %@",
            CapturesText::DeleteFailed => "%@ could not be deleted.",
            CapturesText::Copied => "Image copied",
            CapturesText::CopyFailed => "The image could not be copied.",
            CapturesText::PlayerFailed => "The video player could not be opened.",
            CapturesText::FolderFailed => "The folder could not be opened.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            CapturesText::Title => "Yakalamalar",
            CapturesText::FilterAll => "Tümü",
            CapturesText::FilterScreenshots => "Ekran Görüntüleri",
            CapturesText::FilterClips => "Klipler",
            CapturesText::Screenshot => "Ekran görüntüsü",
            CapturesText::Clip => "Klip",
            CapturesText::Loading => "Yakalamalar yükleniyor",
            CapturesText::EmptyTitle => "Henüz yakalama yok",
            CapturesText::EmptyBody => {
                "Ekran görüntüsü almak için oyunda F12'ye bas. Oynayışın son saniyelerini video olarak tutmak için Klipleri aç."
            }
            CapturesText::EmptyBodyShotsOnly => "Ekran görüntüsü almak için oyunda F12'ye bas.",
            CapturesText::OpenClipsSettings => "Klip Ayarlarını Aç",
            CapturesText::NoScreenshots => "Henüz ekran görüntüsü yok",
            CapturesText::NoClips => "Henüz klip yok",
            CapturesText::NoClipsBody => {
                "Bir oyunun son saniyelerini video olarak tutmak için Ayarlar'dan Klipleri aç."
            }
            CapturesText::ShowAll => "Tüm Yakalamaları Göster",
            CapturesText::MissingTitle => "Ekran görüntüsü klasörü bulunamadı",
            CapturesText::MissingBody => {
                "%@ yok. Oluşturmak için bir oyunda ekran görüntüsü al ya da Ayarlar'dan başka bir klasör seç."
            }
            CapturesText::OpenFolderSettings => "Klasör Ayarlarını Aç",
            CapturesText::FailedTitle => "Yakalamalar okunamadı",
            CapturesText::FailedBody => {
                "AnyPlay %@ klasörünü okuyamadı. Klasöre erişimin olduğundan emin ol, sonra tekrar dene."
            }
            CapturesText::TryAgain => "Tekrar Dene",
            CapturesText::OtherGame => "Diğer Yakalamalar",
            CapturesText::OpenInPlayer => "Oynatıcıda Aç",
            CapturesText::ShowInFolder => "Klasörde Göster",
            CapturesText::CopyImage => "Görüntüyü Kopyala",
            CapturesText::Delete => "Sil",
            CapturesText::Previous => "Önceki yakalama",
            CapturesText::Next => "Sonraki yakalama",
            CapturesText::Position => "%@ / %@",
            CapturesText::ClipNote => "Video oynatıcında açılır",
            CapturesText::DeleteScreenshotTitle => "Bu ekran görüntüsü silinsin mi?",
            CapturesText::DeleteClipTitle => "Bu klip silinsin mi?",
            CapturesText::DeleteBody => "%@ bilgisayarından kaldırılır. Bu geri alınamaz.",
            CapturesText::DeleteScreenshot => "Ekran Görüntüsünü Sil",
            CapturesText::DeleteClip => "Klibi Sil",
            CapturesText::Cancel => "Vazgeç",
            CapturesText::Deleted => "%@ silindi",
            CapturesText::DeleteFailed => "%@ silinemedi.",
            CapturesText::Copied => "Görüntü kopyalandı",
            CapturesText::CopyFailed => "Görüntü kopyalanamadı.",
            CapturesText::PlayerFailed => "Video oynatıcısı açılamadı.",
            CapturesText::FolderFailed => "Klasör açılamadı.",
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

    pub const ALL: [CapturesText; 42] = [
        CapturesText::Title,
        CapturesText::FilterAll,
        CapturesText::FilterScreenshots,
        CapturesText::FilterClips,
        CapturesText::Screenshot,
        CapturesText::Clip,
        CapturesText::Loading,
        CapturesText::EmptyTitle,
        CapturesText::EmptyBody,
        CapturesText::EmptyBodyShotsOnly,
        CapturesText::OpenClipsSettings,
        CapturesText::NoScreenshots,
        CapturesText::NoClips,
        CapturesText::NoClipsBody,
        CapturesText::ShowAll,
        CapturesText::MissingTitle,
        CapturesText::MissingBody,
        CapturesText::OpenFolderSettings,
        CapturesText::FailedTitle,
        CapturesText::FailedBody,
        CapturesText::TryAgain,
        CapturesText::OtherGame,
        CapturesText::OpenInPlayer,
        CapturesText::ShowInFolder,
        CapturesText::CopyImage,
        CapturesText::Delete,
        CapturesText::Previous,
        CapturesText::Next,
        CapturesText::Position,
        CapturesText::ClipNote,
        CapturesText::DeleteScreenshotTitle,
        CapturesText::DeleteClipTitle,
        CapturesText::DeleteBody,
        CapturesText::DeleteScreenshot,
        CapturesText::DeleteClip,
        CapturesText::Cancel,
        CapturesText::Deleted,
        CapturesText::DeleteFailed,
        CapturesText::Copied,
        CapturesText::CopyFailed,
        CapturesText::PlayerFailed,
        CapturesText::FolderFailed,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_has_both_languages() {
        for text in CapturesText::ALL {
            assert!(!text.english().is_empty());
            assert!(!text.turkish().is_empty());
            assert_eq!(
                text.english().matches("%@").count(),
                text.turkish().matches("%@").count(),
                "{text:?}"
            );
        }
    }
}
