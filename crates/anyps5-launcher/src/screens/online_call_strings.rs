//! Texts of calls and of the voice provider in the online area (docs/DESIGN.md 17, "Calls").
//! English and Turkish like the rest. The Turkish was written for this change and has not been
//! read by a native speaker yet. Row titles are in title case, notes, details and toasts in
//! sentence case.

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallText {
    // The ringing call
    /// `%@` = who. The accessible name of the card.
    IncomingName,
    /// Under the caller's name.
    IncomingLine,
    /// Under the caller's name while the player's voice is on Discord.
    IncomingDiscordLine,
    Accept,
    Decline,
    // The call in progress
    InCall,
    /// `%@` = who.
    Calling,
    Cancel,
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    Leave,
    /// After a name that is speaking.
    SpeakingSuffix,
    /// The state line: your microphone is muted.
    YouMuted,
    YouDeafened,
    /// Shown while nobody else is in the call yet.
    WaitingForOthers,
    // Discord
    VoiceOnDiscord,
    /// Accepting a call with the voice set to Discord and the party has a link.
    AnswerOnDiscord,
    OpenInDiscord,
    /// No link: tell the player where the talking happens.
    DiscordSetNote,
    // Missed calls
    /// `%@` = how long ago.
    MissedAgo,
    MissedNow,
    // The friend page
    CallFriend,
    CallFriendDetail,
    AddToCall,
    CallNeedsLeader,
    CallFriendBusy,
    // The voice provider
    VoiceOn,
    ProviderAnyPlay,
    ProviderDiscord,
    ProviderDiscordNote,
    ProviderAnyPlayNote,
    StatusDiscord,
    // The party's Discord link
    DiscordLink,
    DiscordLinkNone,
    DiscordLinkField,
    SetLink,
    RemoveLink,
    LinkNote,
    LinkInvalid,
    // Toasts
    /// `%@` = who.
    ToastMissed,
    ToastNoAnswer,
    ToastDeclined,
    ToastUnreachable,
    ToastNeedsLeader,
}

impl CallText {
    pub const fn english(self) -> &'static str {
        match self {
            CallText::IncomingName => "Incoming call from %@",
            CallText::IncomingLine => "is calling you",
            CallText::IncomingDiscordLine => "Your voice is set to Discord",
            CallText::Accept => "Accept",
            CallText::Decline => "Decline",
            CallText::InCall => "In call",
            CallText::Calling => "Calling %@",
            CallText::Cancel => "Cancel",
            CallText::Mute => "Mute",
            CallText::Unmute => "Unmute",
            CallText::Deafen => "Deafen",
            CallText::Undeafen => "Undeafen",
            CallText::Leave => "Leave call",
            CallText::SpeakingSuffix => "speaking",
            CallText::YouMuted => "You are muted",
            CallText::YouDeafened => "You are deafened",
            CallText::WaitingForOthers => "Waiting for others",
            CallText::VoiceOnDiscord => "Voice on Discord",
            CallText::AnswerOnDiscord => "Answer on Discord",
            CallText::OpenInDiscord => "Open in Discord",
            CallText::DiscordSetNote => {
                "Your voice is set to Discord. Join the call there; AnyPlay does not open your microphone."
            }
            CallText::MissedAgo => "Missed call · %@",
            CallText::MissedNow => "Missed call · just now",
            CallText::CallFriend => "Call",
            CallText::CallFriendDetail => "Rings for 30 seconds",
            CallText::AddToCall => "Add to Call",
            CallText::CallNeedsLeader => "Only the one who started the call can add people.",
            CallText::CallFriendBusy => "In a party already",
            CallText::VoiceOn => "Voice On",
            CallText::ProviderAnyPlay => "AnyPlay",
            CallText::ProviderDiscord => "Discord",
            CallText::ProviderDiscordNote => {
                "On Discord, AnyPlay never opens your microphone for parties or calls. Calls still ring here."
            }
            CallText::ProviderAnyPlayNote => "Parties and calls use AnyPlay's own voice.",
            CallText::StatusDiscord => "On Discord",
            CallText::DiscordLink => "Discord Link",
            CallText::DiscordLinkNone => "Not set",
            CallText::DiscordLinkField => "discord.com/channels/… or discord.gg/…",
            CallText::SetLink => "Save Link",
            CallText::RemoveLink => "Remove Link",
            CallText::LinkNote => {
                "Everyone in the party sees this link. Only the leader can change it."
            }
            CallText::LinkInvalid => {
                "That is not a Discord link. Use an address from discord.com/channels or discord.gg."
            }
            CallText::ToastMissed => "Missed call from %@",
            CallText::ToastNoAnswer => "%@ did not answer.",
            CallText::ToastDeclined => "%@ declined the call.",
            CallText::ToastUnreachable => "%@ cannot take a call right now.",
            CallText::ToastNeedsLeader => "Only the one who started the call can add people.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            CallText::IncomingName => "%@ seni arıyor",
            CallText::IncomingLine => "seni arıyor",
            CallText::IncomingDiscordLine => "Sesin Discord'a ayarlı",
            CallText::Accept => "Kabul Et",
            CallText::Decline => "Reddet",
            CallText::InCall => "Aramada",
            CallText::Calling => "%@ aranıyor",
            CallText::Cancel => "Vazgeç",
            CallText::Mute => "Sessize Al",
            CallText::Unmute => "Sesi Aç",
            CallText::Deafen => "Kulaklığı Kapat",
            CallText::Undeafen => "Kulaklığı Aç",
            CallText::Leave => "Aramadan Çık",
            CallText::SpeakingSuffix => "konuşuyor",
            CallText::YouMuted => "Mikrofonun kapalı",
            CallText::YouDeafened => "Kimseyi duymuyorsun",
            CallText::WaitingForOthers => "Diğerleri bekleniyor",
            CallText::VoiceOnDiscord => "Ses Discord'da",
            CallText::AnswerOnDiscord => "Discord'da Yanıtla",
            CallText::OpenInDiscord => "Discord'da Aç",
            CallText::DiscordSetNote => {
                "Sesin Discord'a ayarlı. Aramaya orada katıl; AnyPlay mikrofonunu açmaz."
            }
            CallText::MissedAgo => "Cevapsız arama · %@",
            CallText::MissedNow => "Cevapsız arama · az önce",
            CallText::CallFriend => "Ara",
            CallText::CallFriendDetail => "30 saniye çalar",
            CallText::AddToCall => "Aramaya Ekle",
            CallText::CallNeedsLeader => "Yalnızca aramayı başlatan kişi ekleyebilir.",
            CallText::CallFriendBusy => "Zaten bir partide",
            CallText::VoiceOn => "Ses",
            CallText::ProviderAnyPlay => "AnyPlay",
            CallText::ProviderDiscord => "Discord",
            CallText::ProviderDiscordNote => {
                "Discord seçiliyken AnyPlay parti ve aramalar için mikrofonunu hiç açmaz. Aramalar yine burada çalar."
            }
            CallText::ProviderAnyPlayNote => {
                "Partiler ve aramalar AnyPlay'in kendi sesini kullanır."
            }
            CallText::StatusDiscord => "Discord'da",
            CallText::DiscordLink => "Discord Bağlantısı",
            CallText::DiscordLinkNone => "Ayarlanmadı",
            CallText::DiscordLinkField => "discord.com/channels/… veya discord.gg/…",
            CallText::SetLink => "Bağlantıyı Kaydet",
            CallText::RemoveLink => "Bağlantıyı Kaldır",
            CallText::LinkNote => {
                "Partideki herkes bu bağlantıyı görür. Yalnızca lider değiştirebilir."
            }
            CallText::LinkInvalid => {
                "Bu bir Discord bağlantısı değil. discord.com/channels veya discord.gg adresi kullan."
            }
            CallText::ToastMissed => "%@ seni aradı, açamadın",
            CallText::ToastNoAnswer => "%@ yanıt vermedi.",
            CallText::ToastDeclined => "%@ aramayı reddetti.",
            CallText::ToastUnreachable => "%@ şu an arama alamıyor.",
            CallText::ToastNeedsLeader => "Yalnızca aramayı başlatan kişi ekleyebilir.",
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

/// How long ago a missed call was: "just now", "5 min ago", "2 h ago", "3 d ago".
pub fn ago(seconds: u64, language: Language) -> String {
    let (n, unit) = match seconds {
        0..=59 => return CallText::MissedNow.text(language).to_owned(),
        60..=3599 => (seconds / 60, "min"),
        3600..=86_399 => (seconds / 3600, "h"),
        _ => (seconds / 86_400, "d"),
    };
    let how = match language {
        Language::English => format!("{n} {unit} ago"),
        Language::Turkish => format!(
            "{n} {} önce",
            match unit {
                "min" => "dk",
                "h" => "sa",
                _ => "gün",
            }
        ),
    };
    CallText::MissedAgo.format(language, &[Arg::Str(&how)])
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_ARGUMENT: [CallText; 6] = [
        CallText::IncomingName,
        CallText::Calling,
        CallText::MissedAgo,
        CallText::ToastMissed,
        CallText::ToastNoAnswer,
        CallText::ToastDeclined,
    ];

    #[test]
    fn placeholders_match_between_languages() {
        for text in WITH_ARGUMENT {
            let count = |s: &str| s.matches("%@").count();
            assert_eq!(count(text.english()), 1, "{text:?}");
            assert_eq!(count(text.turkish()), 1, "{text:?}");
        }
        for text in [CallText::ToastUnreachable] {
            assert_eq!(text.english().matches("%@").count(), 1);
            assert_eq!(text.turkish().matches("%@").count(), 1);
        }
    }

    #[test]
    fn a_missed_call_says_how_long_ago() {
        assert_eq!(ago(10, Language::English), "Missed call · just now");
        assert_eq!(ago(300, Language::English), "Missed call · 5 min ago");
        assert_eq!(ago(7300, Language::English), "Missed call · 2 h ago");
        assert_eq!(ago(3 * 86_400, Language::English), "Missed call · 3 d ago");
        assert_eq!(ago(300, Language::Turkish), "Cevapsız arama · 5 dk önce");
    }
}
