//! Texts of the sharing features: layouts (export, import, community) and "Report to AnyPS5".
//! English and Turkish like the rest; what a report holds (the form's fields) stays English, as
//! the upstream form is.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Share {
    /// Layout choice: `%@` = the layout's name.
    LayoutCommunity,
    LayoutExport,
    LayoutImport,
    /// Toast after the file was written.
    LayoutExported,
    LayoutNotSaved,
    LayoutImported,
    /// Toast: `%@` = the reason (a line of the file, or that it cannot be read).
    LayoutImportFailed,
    LayoutUnreadable,
    /// Toast: the file was read, but lines of it have no row here.
    LayoutImportedPartly,
    /// Game settings row.
    Report,
    ReportDetail,
    /// The review: what happens, and what to do when the report is long.
    ReportHint,
    ReportPasteHint,
    ReportOpen,
    ReportOpenedPaste,
    ReportOpened,
    ReportOpenFailed,
}

impl Share {
    pub const fn english(self) -> &'static str {
        match self {
            Share::LayoutCommunity => "Community: %@",
            Share::LayoutExport => "Export Layout",
            Share::LayoutImport => "Import Layout",
            Share::LayoutExported => "Layout saved",
            Share::LayoutNotSaved => "Could not save the layout.",
            Share::LayoutImported => "Layout imported",
            Share::LayoutImportFailed => "Layout not imported. %@",
            Share::LayoutUnreadable => "The file cannot be read.",
            Share::LayoutImportedPartly => {
                "Layout imported. Some actions are not shown in this list."
            }
            Share::Report => "Report a Problem",
            Share::ReportDetail => "Review the report, then save it or open it on GitHub",
            Share::ReportHint => {
                "This is the report. Save it as a file, or open it on GitHub, filled in. Nothing is sent until you submit it there."
            }
            Share::ReportPasteHint => {
                "The log is too long for the link. Open on GitHub copies the full report; paste it into the form."
            }
            Share::ReportOpen => "Open on GitHub",
            Share::ReportOpenedPaste => "Report copied. Paste it into the GitHub form.",
            Share::ReportOpened => "The form is open in your browser",
            Share::ReportOpenFailed => "Could not open the browser. The link was copied instead.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            Share::LayoutCommunity => "Topluluk: %@",
            Share::LayoutExport => "Düzeni Dışa Aktar",
            Share::LayoutImport => "Düzeni İçe Aktar",
            Share::LayoutExported => "Düzen kaydedildi",
            Share::LayoutNotSaved => "Düzen kaydedilemedi.",
            Share::LayoutImported => "Düzen içe aktarıldı",
            Share::LayoutImportFailed => "Düzen içe aktarılamadı. %@",
            Share::LayoutUnreadable => "Dosya okunamıyor.",
            Share::LayoutImportedPartly => {
                "Düzen içe aktarıldı. Bazı eylemler bu listede gösterilmiyor."
            }
            Share::Report => "Sorun Bildir",
            Share::ReportDetail => "Raporu gözden geçir, sonra kaydet ya da GitHub'da aç",
            Share::ReportHint => {
                "Bu rapordur. Dosya olarak kaydet ya da doldurulmuş hâliyle GitHub'da aç. GitHub'da sen gönderene kadar hiçbir şey gönderilmez."
            }
            Share::ReportPasteHint => {
                "Günlük bağlantı için çok uzun. GitHub'da aç, raporun tamamını panoya kopyalar; forma yapıştır."
            }
            Share::ReportOpen => "GitHub'da Aç",
            Share::ReportOpenedPaste => "Rapor kopyalandı. GitHub formuna yapıştır.",
            Share::ReportOpened => "Form tarayıcında açıldı",
            Share::ReportOpenFailed => "Tarayıcı açılamadı. Bağlantı panoya kopyalandı.",
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

    pub const ALL: [Share; 17] = [
        Share::LayoutCommunity,
        Share::LayoutExport,
        Share::LayoutImport,
        Share::LayoutExported,
        Share::LayoutNotSaved,
        Share::LayoutImported,
        Share::LayoutImportFailed,
        Share::LayoutUnreadable,
        Share::LayoutImportedPartly,
        Share::Report,
        Share::ReportDetail,
        Share::ReportHint,
        Share::ReportPasteHint,
        Share::ReportOpen,
        Share::ReportOpenedPaste,
        Share::ReportOpened,
        Share::ReportOpenFailed,
    ];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_text_exists_in_both_languages_with_the_same_placeholders() {
        for share in Share::ALL {
            for language in Language::ALL {
                assert!(!share.text(language).is_empty(), "{share:?} {language:?}");
            }
            assert_eq!(
                share.english().matches("%@").count(),
                share.turkish().matches("%@").count(),
                "{share:?}"
            );
        }
    }
}
