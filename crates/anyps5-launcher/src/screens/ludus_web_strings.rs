//! Texts of LudusCtrl's browser client on the Phone as Controller page (docs/DESIGN.md 18,
//! "LudusCtrl browser client"). English and Turkish like the rest; the status title is
//! [`PhoneText::Status`](crate::screens::phone_strings::PhoneText::Status).

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BrowserText {
    /// The title of the browser client's status row, under the app's.
    Status,
    /// The bridge could not start; the reason is in the log.
    StartFailed,
    /// Status values: no browser yet, and a browser that holds the pad (`%@` = its address; with
    /// `ConnectedDevice` first what it runs on, then the address).
    Waiting,
    ConnectedTo,
    ConnectedDevice,
    /// The QR code: its accessible name and the line beside it.
    QrLabel,
    QrHint,
    /// The link, and the row that copies it.
    Link,
    Copy,
    CopyDetail,
    /// The links of this PC's other addresses: the caption and the note under it.
    Other,
    OtherNote,
    /// The phone's warning about a certificate this PC signs itself.
    CertNote,
    /// The certificate was made again: one more warning.
    Reissued,
    /// The port was busy: `%d` the wanted port, `%d` the one the link uses.
    PortBusy,
    /// A new token.
    NewLink,
    NewLinkDetail,
    /// Under New Link: the link is a key, so a shared screen calls for a new one.
    ShareNote,
    /// This PC has no address a phone could open.
    NoAddress,
    /// Toasts.
    ToastConnected,
    ToastCopied,
    /// Top bar: the pill's name and its accessible name (`%@` = the browser's address).
    BarName,
    BarLabel,
}

impl BrowserText {
    pub const ALL: [BrowserText; 23] = [
        BrowserText::Status,
        BrowserText::StartFailed,
        BrowserText::Waiting,
        BrowserText::ConnectedTo,
        BrowserText::ConnectedDevice,
        BrowserText::QrLabel,
        BrowserText::QrHint,
        BrowserText::Link,
        BrowserText::Copy,
        BrowserText::CopyDetail,
        BrowserText::Other,
        BrowserText::OtherNote,
        BrowserText::CertNote,
        BrowserText::Reissued,
        BrowserText::PortBusy,
        BrowserText::NewLink,
        BrowserText::NewLinkDetail,
        BrowserText::ShareNote,
        BrowserText::NoAddress,
        BrowserText::ToastConnected,
        BrowserText::ToastCopied,
        BrowserText::BarName,
        BrowserText::BarLabel,
    ];

    pub const fn english(self) -> &'static str {
        match self {
            BrowserText::Status => "Browser",
            BrowserText::StartFailed => {
                "The browser client could not start. The log has the reason."
            }
            BrowserText::Waiting => "Waiting for a browser",
            BrowserText::ConnectedTo => "Connected to a browser on %@",
            BrowserText::ConnectedDevice => "Connected to a browser on %@ (%@)",
            BrowserText::QrLabel => "QR code of the browser link",
            BrowserText::QrHint => {
                "Scan the code with the phone's camera to open the page, then press a button on the controller."
            }
            BrowserText::Link => "Link",
            BrowserText::Copy => "Copy Link",
            BrowserText::CopyDetail => "Open it on a phone on the same network.",
            BrowserText::Other => "Other Addresses",
            BrowserText::OtherNote => {
                "If the phone is on another network of this PC, open the link of that network."
            }
            BrowserText::CertNote => {
                "The phone warns once that the connection is not private, because this PC signs its own certificate. On iPhone choose Show Details, then visit this website; in Chrome on Android choose Advanced, then Proceed."
            }
            BrowserText::Reissued => "This PC's address changed, so the phone will warn once more.",
            BrowserText::PortBusy => {
                "Port %d is in use, maybe by the LudusCtrl receiver. The link uses port %d instead."
            }
            BrowserText::NewLink => "New Link",
            BrowserText::NewLinkDetail => {
                "Makes a new link and disconnects the browser. The old QR code stops working."
            }
            BrowserText::ShareNote => {
                "The link opens the page for anyone who has it. Choose New Link after you share this screen."
            }
            BrowserText::NoAddress => {
                "This PC is not on a network a phone can reach. Connect it to your home network, then choose New Link."
            }
            BrowserText::ToastConnected => "A browser is connected as the controller",
            BrowserText::ToastCopied => "Link copied",
            BrowserText::BarName => "Browser",
            BrowserText::BarLabel => "Browser connected: %@",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            BrowserText::Status => "Tarayıcı",
            BrowserText::StartFailed => "Tarayıcı istemcisi başlatılamadı. Nedeni günlükte yazılı.",
            BrowserText::Waiting => "Bir tarayıcı bekleniyor",
            BrowserText::ConnectedTo => "%@ adresindeki tarayıcı bağlı",
            BrowserText::ConnectedDevice => "%@ üzerindeki tarayıcı bağlı (%@)",
            BrowserText::QrLabel => "Tarayıcı bağlantısının QR kodu",
            BrowserText::QrHint => {
                "Sayfayı açmak için kodu telefonun kamerasıyla tara, sonra kumandada bir tuşa bas."
            }
            BrowserText::Link => "Bağlantı",
            BrowserText::Copy => "Bağlantıyı Kopyala",
            BrowserText::CopyDetail => "Aynı ağdaki bir telefonda aç.",
            BrowserText::Other => "Diğer Adresler",
            BrowserText::OtherNote => {
                "Telefon bu bilgisayarın başka bir ağındaysa o ağın bağlantısını aç."
            }
            BrowserText::CertNote => {
                "Bu bilgisayar sertifikasını kendisi imzaladığı için telefon bir kez bağlantının gizli olmadığı uyarısını gösterir. iPhone'da ayrıntıları açıp siteyi yine de ziyaret et; Android'deki Chrome'da gelişmiş seçenekleri açıp siteye devam et."
            }
            BrowserText::Reissued => {
                "Bu bilgisayarın adresi değişti, bu yüzden telefon bir kez daha uyaracak."
            }
            BrowserText::PortBusy => {
                "%d numaralı bağlantı noktası kullanımda, LudusCtrl alıcısı kullanıyor olabilir. Bağlantı onun yerine %d numaralı noktayı kullanıyor."
            }
            BrowserText::NewLink => "Yeni Bağlantı",
            BrowserText::NewLinkDetail => {
                "Yeni bir bağlantı oluşturur ve tarayıcının bağlantısını keser. Eski QR kodu artık çalışmaz."
            }
            BrowserText::ShareNote => {
                "Bağlantıya sahip olan herkes sayfayı açabilir. Bu ekranı paylaştıktan sonra Yeni Bağlantı'yı seç."
            }
            BrowserText::NoAddress => {
                "Bu bilgisayar bir telefonun ulaşabileceği bir ağda değil. Ev ağına bağla, sonra Yeni Bağlantı'yı seç."
            }
            BrowserText::ToastConnected => "Bir tarayıcı kumanda olarak bağlandı",
            BrowserText::ToastCopied => "Bağlantı kopyalandı",
            BrowserText::BarName => "Tarayıcı",
            BrowserText::BarLabel => "Tarayıcı bağlı: %@",
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
        for text in BrowserText::ALL {
            for language in Language::ALL {
                assert!(
                    !text.text(language).trim().is_empty(),
                    "{text:?} {language:?}"
                );
            }
        }
    }

    #[test]
    fn the_placeholders_match_in_both_languages() {
        for text in BrowserText::ALL {
            let count = |s: &str, p: &str| s.matches(p).count();
            for p in ["%@", "%d"] {
                assert_eq!(
                    count(text.english(), p),
                    count(text.turkish(), p),
                    "{text:?} {p}"
                );
            }
        }
    }

    #[test]
    fn the_ports_reach_the_busy_note_in_order() {
        for language in Language::ALL {
            let text = BrowserText::PortBusy.format(language, &[Arg::Int(45991), Arg::Int(51234)]);
            let (a, b) = (text.find("45991").unwrap(), text.find("51234").unwrap());
            assert!(a < b, "{text}");
            assert!(!text.contains("%d"), "{text}");
        }
    }

    #[test]
    fn the_words_name_no_console_brand() {
        for text in BrowserText::ALL {
            for language in Language::ALL {
                let words = text.text(language).to_lowercase();
                for brand in [
                    "playstation",
                    "dualsense",
                    "dualshock",
                    "xbox",
                    "ps5",
                    "sony",
                ] {
                    assert!(!words.contains(brand), "{text:?} {language:?}: {brand}");
                }
            }
        }
    }

    #[test]
    fn titles_are_title_case_and_sentences_are_not() {
        // DESIGN.md 3.10: row titles, buttons and captions in Title Case.
        for text in [
            BrowserText::Status,
            BrowserText::Link,
            BrowserText::Copy,
            BrowserText::Other,
            BrowserText::NewLink,
        ] {
            for language in Language::ALL {
                for word in text.text(language).split(' ') {
                    let first = word.chars().next().unwrap();
                    assert!(first.is_uppercase(), "{text:?} {language:?}: {word}");
                }
            }
        }
        // Statuses and toasts read as sentences: only the first word is capital.
        for text in [
            BrowserText::Waiting,
            BrowserText::ToastConnected,
            BrowserText::ToastCopied,
        ] {
            for language in Language::ALL {
                let rest: Vec<&str> = text.text(language).split(' ').skip(1).collect();
                assert!(
                    rest.iter().all(|w| !w.starts_with(char::is_uppercase)),
                    "{text:?} {language:?}"
                );
            }
        }
    }
}
