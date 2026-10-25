//! Texts of share screen in the online area (docs/DESIGN.md 17, "Share screen"). English and
//! Turkish like the rest. The Turkish was written for this change and has not been read by a
//! native speaker yet. Row titles are in title case, notes and toasts in sentence case. The
//! numbers are said in plain words: where the picture comes from and how long it takes, never
//! a protocol's names.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScreenText {
    /// The caption of the group on the party page and the title of the Home row.
    Screen,
    /// The button that starts a stream, and what it does.
    ShareScreen,
    ShareDetail,
    /// The sentence over the player's own stream.
    SharedNobody,
    SharedOne,
    /// `%@` = how many.
    SharedMany,
    /// The detail of the sentence: `%@` = the game.
    HostStarting,
    HostLive,
    HostStalled,
    StopSharing,
    // Those who watch
    ViewerNotWatching,
    ViewerLosing,
    // Offers
    /// `%@` = who.
    Watch,
    /// `%@` = the game.
    SharingTitle,
    /// `%@` = who, `%@` = the game.
    IsSharing,
    CannotShow,
    // The stream being watched
    /// `%@` = who.
    Watching,
    PhaseConnecting,
    PhaseLive,
    PhaseStalled,
    StopWatching,
    WatchNote,
    // Where the picture comes from
    /// `%@` = milliseconds.
    DirectMs,
    RelayMs,
    // Home row
    /// `%@` = who.
    HomeWatching,
    // Toasts
    /// `%@` = who, `%@` = the game.
    ToastOffered,
    /// `%@` = who.
    ToastWatchEnded,
    ToastShareEnded,
    ErrorNoGame,
    ErrorNotInParty,
    ErrorBusy,
    ErrorNoEncoder,
    ErrorNotFound,
    /// A party member whose name is not known.
    SomeoneUnknown,
}

impl ScreenText {
    pub const fn english(self) -> &'static str {
        match self {
            ScreenText::Screen => "Screen",
            ScreenText::ShareScreen => "Share Screen",
            ScreenText::ShareDetail => "Show your game to the party",
            ScreenText::SharedNobody => "Your screen is shared",
            ScreenText::SharedOne => "Your screen is shared with 1 person",
            ScreenText::SharedMany => "Your screen is shared with %@ people",
            ScreenText::HostStarting => "Starting…",
            ScreenText::HostLive => "Live",
            ScreenText::HostStalled => "The picture has stopped",
            ScreenText::StopSharing => "Stop Sharing",
            ScreenText::ViewerNotWatching => "Not watching",
            ScreenText::ViewerLosing => "Some of the picture is lost",
            ScreenText::Watch => "Watch %@",
            ScreenText::SharingTitle => "Sharing %@",
            ScreenText::IsSharing => "%@ is sharing %@",
            ScreenText::CannotShow => "This version cannot show it yet.",
            ScreenText::Watching => "Watching %@",
            ScreenText::PhaseConnecting => "Connecting…",
            ScreenText::PhaseLive => "Live",
            ScreenText::PhaseStalled => "Waiting for the picture",
            ScreenText::StopWatching => "Stop Watching",
            ScreenText::WatchNote => "The picture opens in its own window.",
            ScreenText::DirectMs => "Direct, %@ ms",
            ScreenText::RelayMs => "Through a relay, %@ ms",
            ScreenText::HomeWatching => "Watching %@",
            ScreenText::ToastOffered => "%@ is sharing %@",
            ScreenText::ToastWatchEnded => "%@ stopped sharing.",
            ScreenText::ToastShareEnded => "Your screen is no longer shared.",
            ScreenText::ErrorNoGame => "Start a game first, then share its screen.",
            ScreenText::ErrorNotInParty => "Join a party to share your screen.",
            ScreenText::ErrorBusy => "You are already sharing or watching a screen.",
            ScreenText::ErrorNoEncoder => "This PC cannot share its screen.",
            ScreenText::ErrorNotFound => "That screen is not shared any more.",
            ScreenText::SomeoneUnknown => "Someone",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            ScreenText::Screen => "Ekran",
            ScreenText::ShareScreen => "Ekranı Paylaş",
            ScreenText::ShareDetail => "Oyununu partiye göster",
            ScreenText::SharedNobody => "Ekranın paylaşılıyor",
            ScreenText::SharedOne => "Ekranın 1 kişiyle paylaşılıyor",
            ScreenText::SharedMany => "Ekranın %@ kişiyle paylaşılıyor",
            ScreenText::HostStarting => "Başlıyor…",
            ScreenText::HostLive => "Canlı",
            ScreenText::HostStalled => "Görüntü gitmiyor",
            ScreenText::StopSharing => "Paylaşımı Durdur",
            ScreenText::ViewerNotWatching => "İzlemiyor",
            ScreenText::ViewerLosing => "Görüntünün bir kısmı kayboluyor",
            ScreenText::Watch => "İzle: %@",
            ScreenText::SharingTitle => "Paylaşılan: %@",
            ScreenText::IsSharing => "%@ şunu paylaşıyor: %@",
            ScreenText::CannotShow => "Bu sürüm bunu henüz gösteremiyor.",
            ScreenText::Watching => "İzlenen: %@",
            ScreenText::PhaseConnecting => "Bağlanıyor…",
            ScreenText::PhaseLive => "Canlı",
            ScreenText::PhaseStalled => "Görüntü bekleniyor",
            ScreenText::StopWatching => "İzlemeyi Bırak",
            ScreenText::WatchNote => "Görüntü kendi penceresinde açılır.",
            ScreenText::DirectMs => "Doğrudan, %@ ms",
            ScreenText::RelayMs => "Aktarıcı üzerinden, %@ ms",
            ScreenText::HomeWatching => "İzlenen: %@",
            ScreenText::ToastOffered => "%@ şunu paylaşıyor: %@",
            ScreenText::ToastWatchEnded => "%@ paylaşımı bıraktı.",
            ScreenText::ToastShareEnded => "Ekranın artık paylaşılmıyor.",
            ScreenText::ErrorNoGame => "Önce bir oyun başlat, sonra ekranını paylaş.",
            ScreenText::ErrorNotInParty => "Ekranını paylaşmak için bir partiye katıl.",
            ScreenText::ErrorBusy => "Zaten bir ekran paylaşıyor ya da izliyorsun.",
            ScreenText::ErrorNoEncoder => "Bu bilgisayar ekranını paylaşamıyor.",
            ScreenText::ErrorNotFound => "Bu ekran artık paylaşılmıyor.",
            ScreenText::SomeoneUnknown => "Biri",
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

    const ONE_ARGUMENT: &[ScreenText] = &[
        ScreenText::SharedMany,
        ScreenText::Watch,
        ScreenText::SharingTitle,
        ScreenText::Watching,
        ScreenText::DirectMs,
        ScreenText::RelayMs,
        ScreenText::HomeWatching,
        ScreenText::ToastWatchEnded,
    ];
    const TWO_ARGUMENTS: &[ScreenText] = &[ScreenText::IsSharing, ScreenText::ToastOffered];

    #[test]
    fn placeholders_match_between_languages() {
        let count = |s: &str| s.matches("%@").count();
        for text in ONE_ARGUMENT {
            assert_eq!(count(text.english()), 1, "{text:?}");
            assert_eq!(count(text.turkish()), 1, "{text:?}");
        }
        for text in TWO_ARGUMENTS {
            assert_eq!(count(text.english()), 2, "{text:?}");
            assert_eq!(count(text.turkish()), 2, "{text:?}");
        }
    }

    #[test]
    fn row_titles_are_in_title_case() {
        for title in [
            ScreenText::Screen,
            ScreenText::ShareScreen,
            ScreenText::StopSharing,
            ScreenText::StopWatching,
        ] {
            for word in title.english().split(' ') {
                assert!(word.chars().next().unwrap().is_uppercase(), "{title:?}");
            }
        }
    }

    #[test]
    fn the_sentences_read_in_plain_words() {
        let en = Language::English;
        assert_eq!(
            ScreenText::SharedMany.format(en, &[Arg::Int(2)]),
            "Your screen is shared with 2 people"
        );
        assert_eq!(
            ScreenText::IsSharing.format(en, &[Arg::Str("Alec"), Arg::Str("Astro Bot")]),
            "Alec is sharing Astro Bot"
        );
        assert_eq!(
            ScreenText::DirectMs.format(en, &[Arg::Int(24)]),
            "Direct, 24 ms"
        );
        assert_eq!(
            ScreenText::RelayMs.format(en, &[Arg::Int(80)]),
            "Through a relay, 80 ms"
        );
    }

    #[test]
    fn no_text_uses_the_protocol_s_words() {
        for text in [
            ScreenText::DirectMs,
            ScreenText::RelayMs,
            ScreenText::PhaseStalled,
            ScreenText::HostStalled,
            ScreenText::ViewerLosing,
        ] {
            let said = text.english().to_lowercase();
            for jargon in ["rtt", "quic", "packet", "latency", "stalled"] {
                assert!(!said.contains(jargon), "{text:?}: {jargon}");
            }
        }
    }
}
