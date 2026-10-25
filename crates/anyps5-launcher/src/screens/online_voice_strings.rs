//! Texts of party voice in the online area (docs/DESIGN.md 17, "Party voice"). English and
//! Turkish like the rest. The Turkish was written for this change and has not been read by a
//! native speaker yet. Row titles are in title case, notes and toasts in sentence case.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum VoiceText {
    /// The page, the Home row and the caption on the party page.
    Voice,
    VoiceSubtitle,
    /// The toggle.
    PartyVoice,
    StatusOff,
    StatusNoParty,
    StatusConnecting,
    StatusOn,
    /// The service switched voice off: the cause, in a note.
    FailedGeneric,
    FailedDenied,
    /// The service's own reason; `%@` = what it said.
    FailedReason,
    // The microphone row
    Microphone,
    MicOff,
    /// `%@` = the key.
    MicMuted,
    MicOpen,
    MicListening,
    MicTalking,
    // Mode
    Mode,
    PushToTalk,
    VoiceActivity,
    /// Push to talk reaches only the launcher's own window.
    PttNote,
    /// A game is running and push to talk would not reach the launcher.
    PttInGameNote,
    ActivityNote,
    CallModeNote,
    // The key
    TalkKey,
    /// The detail while the key row waits for a key.
    TalkKeyNote,
    // Devices
    Speakers,
    SystemDefault,
    InUse,
    RefreshDevices,
    DevicesNote,
    NoMicrophone,
    NoSound,
    DevicesUnknown,
    /// `%@` = the kind of device the page lists.
    PickMicrophone,
    PickSpeakers,
    DeviceGone,
    // Members
    Volume,
    /// `%@` = the percent.
    VolumeValue,
    Mute,
    /// `%@` = who.
    MuteDetail,
    Speaking,
    MutedByYou,
    NotInVoice,
    VoiceSettings,
    // Toasts
    ToastFailed,
    ToastDenied,
    ToastNoDevice,
}

impl VoiceText {
    pub const fn english(self) -> &'static str {
        match self {
            VoiceText::Voice => "Voice",
            VoiceText::VoiceSubtitle => "Talk with your party",
            VoiceText::PartyVoice => "Party Voice",
            VoiceText::StatusOff => "Off",
            VoiceText::StatusNoParty => "Starts when you are in a party",
            VoiceText::StatusConnecting => "Connecting…",
            VoiceText::StatusOn => "On",
            VoiceText::FailedGeneric => {
                "Voice could not start. Check your microphone and speakers, then switch it on again."
            }
            VoiceText::FailedDenied => {
                "The system did not allow microphone access. Allow it in the system's privacy settings, then switch voice on again."
            }
            VoiceText::FailedReason => "Reason: %@",
            VoiceText::Microphone => "Microphone",
            VoiceText::MicOff => "Off",
            VoiceText::MicMuted => "Muted · hold %@ to talk",
            VoiceText::MicOpen => "Open",
            VoiceText::MicListening => "Listening",
            VoiceText::MicTalking => "Talking",
            VoiceText::Mode => "Mode",
            VoiceText::PushToTalk => "Push to Talk",
            VoiceText::VoiceActivity => "Voice Activity",
            VoiceText::PttNote => "Push to talk works while the launcher is in front.",
            VoiceText::PttInGameNote => {
                "A game has the keyboard, so the key does not reach the launcher. Switch to Voice Activity to talk while you play."
            }
            VoiceText::ActivityNote => {
                "The microphone opens whenever you speak. Without headphones, others hear themselves back."
            }
            VoiceText::CallModeNote => {
                "On a call, the microphone opens whenever you speak. Push to Talk is back when the call ends."
            }
            VoiceText::TalkKey => "Talk Key",
            VoiceText::TalkKeyNote => "Press a key. Esc keeps the current one.",
            VoiceText::Speakers => "Speakers",
            VoiceText::SystemDefault => "System Default",
            VoiceText::InUse => "In use",
            VoiceText::RefreshDevices => "Refresh Devices",
            VoiceText::DevicesNote => "A change restarts voice for a moment.",
            VoiceText::NoMicrophone => {
                "No microphone found. Connect one or check the system's sound settings, then refresh."
            }
            VoiceText::NoSound => {
                "The net service reports no sound devices. Connect a microphone and speakers, or check the system's sound settings."
            }
            VoiceText::DevicesUnknown => "Devices appear once the net service answers.",
            VoiceText::PickMicrophone => "Choose the microphone",
            VoiceText::PickSpeakers => "Choose the speakers",
            VoiceText::DeviceGone => "This device is not connected right now.",
            VoiceText::Volume => "Volume",
            VoiceText::VolumeValue => "%@%",
            VoiceText::Mute => "Mute",
            VoiceText::MuteDetail => "Only you stop hearing %@",
            VoiceText::Speaking => "Speaking",
            VoiceText::MutedByYou => "Muted",
            VoiceText::NotInVoice => "Not in voice",
            VoiceText::VoiceSettings => "Voice Settings",
            VoiceText::ToastFailed => "Voice could not start. Check your microphone and speakers.",
            VoiceText::ToastDenied => "The system did not allow the microphone.",
            VoiceText::ToastNoDevice => "That device is not available. Refresh the list.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            VoiceText::Voice => "Ses",
            VoiceText::VoiceSubtitle => "Partinle konuş",
            VoiceText::PartyVoice => "Parti Sesi",
            VoiceText::StatusOff => "Kapalı",
            VoiceText::StatusNoParty => "Bir partiye girince başlar",
            VoiceText::StatusConnecting => "Bağlanıyor…",
            VoiceText::StatusOn => "Açık",
            VoiceText::FailedGeneric => {
                "Ses başlatılamadı. Mikrofonunu ve hoparlörlerini kontrol et, sonra tekrar aç."
            }
            VoiceText::FailedDenied => {
                "Sistem mikrofon erişimine izin vermedi. Sistemin gizlilik ayarlarından izin ver, sonra sesi tekrar aç."
            }
            VoiceText::FailedReason => "Neden: %@",
            VoiceText::Microphone => "Mikrofon",
            VoiceText::MicOff => "Kapalı",
            VoiceText::MicMuted => "Sessiz · konuşmak için %@ tuşuna basılı tut",
            VoiceText::MicOpen => "Açık",
            VoiceText::MicListening => "Dinliyor",
            VoiceText::MicTalking => "Konuşuyor",
            VoiceText::Mode => "Mod",
            VoiceText::PushToTalk => "Bas ve Konuş",
            VoiceText::VoiceActivity => "Ses Algılama",
            VoiceText::PttNote => "Bas ve konuş, başlatıcı öndeyken çalışır.",
            VoiceText::PttInGameNote => {
                "Klavye oyunda olduğu için tuş başlatıcıya ulaşmaz. Oynarken konuşmak için Ses Algılama'ya geç."
            }
            VoiceText::ActivityNote => {
                "Mikrofon, konuştuğunda açılır. Kulaklık yoksa diğerleri kendi seslerini geri duyar."
            }
            VoiceText::CallModeNote => {
                "Aramada mikrofon, konuştuğunda açılır. Arama bitince Bas ve Konuş geri gelir."
            }
            VoiceText::TalkKey => "Konuşma Tuşu",
            VoiceText::TalkKeyNote => "Bir tuşa bas. Esc mevcut tuşu korur.",
            VoiceText::Speakers => "Hoparlör",
            VoiceText::SystemDefault => "Sistem Varsayılanı",
            VoiceText::InUse => "Kullanımda",
            VoiceText::RefreshDevices => "Cihazları Yenile",
            VoiceText::DevicesNote => "Bir değişiklik sesi kısa süre yeniden başlatır.",
            VoiceText::NoMicrophone => {
                "Mikrofon bulunamadı. Bir mikrofon tak ya da sistemin ses ayarlarına bak, sonra yenile."
            }
            VoiceText::NoSound => {
                "Ağ hizmeti hiç ses cihazı bildirmiyor. Mikrofon ve hoparlör tak ya da sistemin ses ayarlarına bak."
            }
            VoiceText::DevicesUnknown => "Cihazlar, ağ hizmeti yanıt verince görünür.",
            VoiceText::PickMicrophone => "Mikrofonu seç",
            VoiceText::PickSpeakers => "Hoparlörü seç",
            VoiceText::DeviceGone => "Bu cihaz şu an bağlı değil.",
            VoiceText::Volume => "Ses Düzeyi",
            VoiceText::VolumeValue => "%@%",
            VoiceText::Mute => "Sustur",
            VoiceText::MuteDetail => "Yalnızca sen %@ kişisini duymazsın",
            VoiceText::Speaking => "Konuşuyor",
            VoiceText::MutedByYou => "Susturuldu",
            VoiceText::NotInVoice => "Seste değil",
            VoiceText::VoiceSettings => "Ses Ayarları",
            VoiceText::ToastFailed => {
                "Ses başlatılamadı. Mikrofonunu ve hoparlörlerini kontrol et."
            }
            VoiceText::ToastDenied => "Sistem mikrofona izin vermedi.",
            VoiceText::ToastNoDevice => "Bu cihaz kullanılamıyor. Listeyi yenile.",
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

    const WITH_ARGUMENTS: &[VoiceText] = &[
        VoiceText::FailedReason,
        VoiceText::MicMuted,
        VoiceText::VolumeValue,
        VoiceText::MuteDetail,
    ];

    #[test]
    fn placeholders_match_between_languages() {
        for text in WITH_ARGUMENTS {
            let count = |s: &str| s.matches("%@").count();
            assert_eq!(count(text.english()), 1, "{text:?}");
            assert_eq!(count(text.turkish()), 1, "{text:?}");
        }
    }

    #[test]
    fn row_titles_are_in_title_case() {
        for title in [
            VoiceText::PartyVoice,
            VoiceText::PushToTalk,
            VoiceText::VoiceActivity,
            VoiceText::TalkKey,
            VoiceText::SystemDefault,
            VoiceText::RefreshDevices,
            VoiceText::VoiceSettings,
        ] {
            for word in title.english().split(' ') {
                let first = word.chars().next().unwrap();
                assert!(first.is_uppercase() || word == "to", "{title:?}: {word}");
            }
        }
    }

    #[test]
    fn the_values_read_in_a_row() {
        assert_eq!(
            VoiceText::MicMuted.format(Language::English, &[Arg::Str("V")]),
            "Muted · hold V to talk"
        );
        assert_eq!(
            VoiceText::VolumeValue.format(Language::English, &[Arg::Int(150)]),
            "150%"
        );
    }
}
