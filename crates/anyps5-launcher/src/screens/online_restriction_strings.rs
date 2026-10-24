//! Texts for what limits an account: a mute, a new account's limits, a ban. English and Turkish
//! like the rest. Row titles are in title case, notes and toasts in sentence case. The words are
//! calm and say what is limited, until when and what to do; they never blame.

use anyps5_core::online::control::Restriction;
use anyps5_core::online::{Blocked, in_force};

use super::format;
use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RestrictionText {
    // Titles of the kinds
    MutedTitle,
    NewAccountTitle,
    BannedTitle,
    OtherTitle,
    FriendRequestsPaused,
    MessagesPaused,
    // Time
    /// `%@` = the day and time.
    Until,
    UntilFurtherNotice,
    // Captions
    Restrictions,
    WhatThisMeans,
    Reason,
    ModeratorNote,
    Appeal,
    Rule,
    // What it means
    MutedExplain,
    MutedExplainForGood,
    /// `%@` = when it opens up.
    NewAccountExplain,
    NewAccountExplainUnknown,
    /// `%@` = when it ends.
    BannedExplain,
    BannedExplainForGood,
    OtherExplain,
    // Appeal
    /// `%@` = the last day to appeal.
    AppealAvailable,
    AppealField,
    SendAppeal,
    AppealSent,
    AppealTooLong,
    AppealOpen,
    AppealUpheld,
    /// `%@` = the day the time ended.
    AppealExpired,
    AppealInBrowser,
    AppealInBrowserNote,
    // Toasts
    /// `%@` = when it ends.
    ToastMuted,
    ToastMutedForGood,
    /// `%@` = when it opens up.
    ToastNewAccountFriend,
    ToastNewAccountStranger,
    ToastNewAccount,
    /// `%@` = when it ends.
    ToastBanned,
    ToastBannedForGood,
    ToastOther,
    /// When the net service said nothing about what is limited.
    ToastUnknown,
}

impl RestrictionText {
    pub const fn english(self) -> &'static str {
        match self {
            Self::MutedTitle => "Messages and Friend Requests Paused",
            Self::NewAccountTitle => "New Account Limits",
            Self::BannedTitle => "Account Banned",
            Self::OtherTitle => "Account Limited",
            Self::FriendRequestsPaused => "Friend Requests Paused",
            Self::MessagesPaused => "Messages Paused",
            Self::Until => "Until %@",
            Self::UntilFurtherNotice => "Until further notice",
            Self::Restrictions => "Restrictions",
            Self::WhatThisMeans => "What This Means",
            Self::Reason => "Reason",
            Self::ModeratorNote => "Note From the Moderator",
            Self::Appeal => "Appeal",
            Self::Rule => "Rule",
            Self::MutedExplain => {
                "You cannot send messages or friend requests while this lasts. Everything else works as before."
            }
            Self::MutedExplainForGood => {
                "You cannot send messages or friend requests. Everything else works as before."
            }
            Self::NewAccountExplain => {
                "New accounts can send a few friend requests and cannot write to people who are not friends yet. This opens up on %@. Messages to friends always work."
            }
            Self::NewAccountExplainUnknown => {
                "New accounts can send a few friend requests and cannot write to people who are not friends yet. Messages to friends always work."
            }
            Self::BannedExplain => "This account is signed out and cannot sign in again until %@.",
            Self::BannedExplainForGood => "This account is signed out and cannot sign in.",
            Self::OtherExplain => "Part of this account is limited right now.",
            Self::AppealAvailable => {
                "If you think this is a mistake, tell us why. A person reads every appeal. You can appeal until %@."
            }
            Self::AppealField => "Why do you think this is a mistake?",
            Self::SendAppeal => "Send Appeal",
            Self::AppealSent => "Appeal sent.",
            Self::AppealTooLong => "Shorten it to 1,000 characters",
            Self::AppealOpen => {
                "Your appeal was sent. A person will read it, and the answer shows here."
            }
            Self::AppealUpheld => "A moderator read your appeal and the restriction stands.",
            Self::AppealExpired => "The time to appeal ended on %@.",
            Self::AppealInBrowser => "Appeal in Browser",
            Self::AppealInBrowserNote => {
                "Sign in with this account in your browser. The page shows why it was banned and lets you appeal."
            }
            Self::ToastMuted => "Messages and friend requests are paused until %@.",
            Self::ToastMutedForGood => "Messages and friend requests are paused.",
            Self::ToastNewAccountFriend => {
                "New accounts can send a few friend requests. You can send more after %@."
            }
            Self::ToastNewAccountStranger => {
                "New accounts can write to people who are not friends after %@."
            }
            Self::ToastNewAccount => "This opens up for new accounts after %@.",
            Self::ToastBanned => {
                "This account is banned until %@. Open Online to read why and how to appeal."
            }
            Self::ToastBannedForGood => {
                "This account is banned. Open Online to read why and how to appeal."
            }
            Self::ToastOther => "This account is limited until %@.",
            Self::ToastUnknown => "This account cannot do that right now.",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            Self::MutedTitle => "Mesajlar ve Arkadaşlık İstekleri Durduruldu",
            Self::NewAccountTitle => "Yeni Hesap Sınırları",
            Self::BannedTitle => "Hesap Engellendi",
            Self::OtherTitle => "Hesap Kısıtlandı",
            Self::FriendRequestsPaused => "Arkadaşlık İstekleri Durduruldu",
            Self::MessagesPaused => "Mesajlar Durduruldu",
            Self::Until => "%@ tarihine kadar",
            Self::UntilFurtherNotice => "Başka bir duyuruya kadar",
            Self::Restrictions => "Kısıtlamalar",
            Self::WhatThisMeans => "Bunun Anlamı",
            Self::Reason => "Sebep",
            Self::ModeratorNote => "Moderatörün Notu",
            Self::Appeal => "İtiraz",
            Self::Rule => "Kural",
            Self::MutedExplain => {
                "Bu sürerken mesaj ve arkadaşlık isteği gönderemezsin. Geri kalan her şey eskisi gibi çalışır."
            }
            Self::MutedExplainForGood => {
                "Mesaj ve arkadaşlık isteği gönderemezsin. Geri kalan her şey eskisi gibi çalışır."
            }
            Self::NewAccountExplain => {
                "Yeni hesaplar birkaç arkadaşlık isteği gönderebilir, arkadaş olmadığı kişilere henüz yazamaz. %@ tarihinde açılır. Arkadaşlara mesaj her zaman çalışır."
            }
            Self::NewAccountExplainUnknown => {
                "Yeni hesaplar birkaç arkadaşlık isteği gönderebilir, arkadaş olmadığı kişilere henüz yazamaz. Arkadaşlara mesaj her zaman çalışır."
            }
            Self::BannedExplain => {
                "Bu hesabın oturumu kapalı, %@ tarihine kadar yeniden giriş yapamaz."
            }
            Self::BannedExplainForGood => "Bu hesabın oturumu kapalı, giriş yapamaz.",
            Self::OtherExplain => "Bu hesabın bir bölümü şu an kısıtlı.",
            Self::AppealAvailable => {
                "Bunun bir hata olduğunu düşünüyorsan nedenini yaz. Her itirazı bir kişi okur. %@ tarihine kadar itiraz edebilirsin."
            }
            Self::AppealField => "Bunun neden hata olduğunu düşünüyorsun?",
            Self::SendAppeal => "İtirazı Gönder",
            Self::AppealSent => "İtiraz gönderildi.",
            Self::AppealTooLong => "En fazla 1.000 karakter olsun",
            Self::AppealOpen => "İtirazın gönderildi. Bir kişi okuyacak, yanıt burada görünür.",
            Self::AppealUpheld => "Bir moderatör itirazını okudu, kısıtlama geçerli kalıyor.",
            Self::AppealExpired => "İtiraz süresi %@ tarihinde bitti.",
            Self::AppealInBrowser => "Tarayıcıda İtiraz Et",
            Self::AppealInBrowserNote => {
                "Tarayıcında bu hesapla giriş yap. Sayfa neden engellendiğini gösterir ve itiraz etmeni sağlar."
            }
            Self::ToastMuted => "Mesajlar ve arkadaşlık istekleri %@ tarihine kadar durduruldu.",
            Self::ToastMutedForGood => "Mesajlar ve arkadaşlık istekleri durduruldu.",
            Self::ToastNewAccountFriend => {
                "Yeni hesaplar birkaç arkadaşlık isteği gönderebilir. %@ sonrasında daha fazlasını gönderebilirsin."
            }
            Self::ToastNewAccountStranger => {
                "Yeni hesaplar arkadaş olmadığı kişilere %@ sonrasında yazabilir."
            }
            Self::ToastNewAccount => "Bu, yeni hesaplar için %@ sonrasında açılır.",
            Self::ToastBanned => {
                "Bu hesap %@ tarihine kadar engellendi. Nedenini ve nasıl itiraz edeceğini görmek için Çevrimiçi'yi aç."
            }
            Self::ToastBannedForGood => {
                "Bu hesap engellendi. Nedenini ve nasıl itiraz edeceğini görmek için Çevrimiçi'yi aç."
            }
            Self::ToastOther => "Bu hesap %@ tarihine kadar kısıtlı.",
            Self::ToastUnknown => "Bu hesap şu an bunu yapamaz.",
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

type T = RestrictionText;

/// The day and time `unix` seconds name, in the player's time zone. `offset` is the local UTC
/// offset in seconds; `None` asks the system.
pub fn when(unix: u64, offset: Option<i32>, language: Language) -> String {
    let micros = i64::try_from(unix).unwrap_or(i64::MAX / 1_000_000) * 1_000_000;
    let offset = offset.unwrap_or_else(|| format::local_offset_seconds(micros));
    format::date_text(micros, offset, language)
}

/// "Until Oct 25, 2026 at 9:14 PM", or "Until further notice" for no end.
pub fn until_line(until: Option<u64>, offset: Option<i32>, language: Language) -> String {
    match until {
        Some(until) => T::Until.format(language, &[Arg::Str(&when(until, offset, language))]),
        None => T::UntilFurtherNotice.text(language).to_owned(),
    }
}

/// The title of a restriction, as a row names it.
pub fn title(restriction: &Restriction, language: Language) -> &'static str {
    match restriction.kind.as_str() {
        Restriction::MUTED => T::MutedTitle,
        Restriction::NEW_ACCOUNT => T::NewAccountTitle,
        Restriction::BANNED => T::BannedTitle,
        _ => T::OtherTitle,
    }
    .text(language)
}

/// What a restriction means for the player, in a sentence or two.
pub fn explanation(restriction: &Restriction, offset: Option<i32>, language: Language) -> String {
    let at = restriction.until.map(|until| when(until, offset, language));
    match (restriction.kind.as_str(), at) {
        (Restriction::MUTED, Some(_)) => T::MutedExplain.text(language).to_owned(),
        (Restriction::MUTED, None) => T::MutedExplainForGood.text(language).to_owned(),
        (Restriction::NEW_ACCOUNT, Some(at)) => {
            T::NewAccountExplain.format(language, &[Arg::Str(&at)])
        }
        (Restriction::NEW_ACCOUNT, None) => T::NewAccountExplainUnknown.text(language).to_owned(),
        (Restriction::BANNED, Some(at)) => T::BannedExplain.format(language, &[Arg::Str(&at)]),
        (Restriction::BANNED, None) => T::BannedExplainForGood.text(language).to_owned(),
        _ => T::OtherExplain.text(language).to_owned(),
    }
}

/// The name of a rule of the terms, as the cloud names it (`harassment`).
pub fn rule_name(rule: &str, language: Language) -> String {
    let (en, tr) = match rule {
        "cheating" => ("Cheating", "Hile"),
        "harassment" => ("Harassment", "Taciz"),
        "spam" => ("Spam", "Spam"),
        "name" => ("Name", "İsim"),
        "illegal" => ("Illegal content", "Yasa dışı içerik"),
        _ => ("Other", "Diğer"),
    };
    match language {
        Language::English => en,
        Language::Turkish => tr,
    }
    .to_owned()
}

/// The row that stands where the blocked action would be: "Messages Paused" and the like.
pub fn paused_title(what: Blocked, language: Language) -> &'static str {
    match what {
        Blocked::FriendRequest => T::FriendRequestsPaused,
        Blocked::Stranger | Blocked::Message => T::MessagesPaused,
    }
    .text(language)
}

/// The toast for a refused action or a new restriction. `what` is the action that was refused,
/// when it is known.
pub fn toast(
    restriction: &Restriction,
    what: Option<Blocked>,
    offset: Option<i32>,
    language: Language,
) -> String {
    let at = restriction.until.map(|until| when(until, offset, language));
    let with = |text: T, at: &str| text.format(language, &[Arg::Str(at)]);
    match (restriction.kind.as_str(), at) {
        (Restriction::MUTED, Some(at)) => with(T::ToastMuted, &at),
        (Restriction::MUTED, None) => T::ToastMutedForGood.text(language).to_owned(),
        (Restriction::NEW_ACCOUNT, Some(at)) => match what {
            Some(Blocked::FriendRequest) => with(T::ToastNewAccountFriend, &at),
            Some(Blocked::Stranger) => with(T::ToastNewAccountStranger, &at),
            _ => with(T::ToastNewAccount, &at),
        },
        (Restriction::BANNED, Some(at)) => with(T::ToastBanned, &at),
        (Restriction::BANNED, None) => T::ToastBannedForGood.text(language).to_owned(),
        (_, Some(at)) => with(T::ToastOther, &at),
        _ => T::ToastUnknown.text(language).to_owned(),
    }
}

/// True when the appeal of `restriction` can be filed at `now`: it names an action and the
/// service says an appeal is still open for filing.
pub fn can_appeal(restriction: &Restriction, now: u64) -> bool {
    restriction.action.is_some()
        && restriction
            .appeal
            .as_ref()
            .is_some_and(|a| a.status == "available" && now < a.deadline)
}

/// True when `restriction` still counts at `now`.
pub fn counts(restriction: &Restriction, now: u64) -> bool {
    in_force(restriction, now)
}

#[cfg(test)]
mod tests {
    use anyps5_core::online::control::Appeal;

    use super::*;

    const ALL: &[RestrictionText] = &[
        T::MutedTitle,
        T::NewAccountTitle,
        T::BannedTitle,
        T::OtherTitle,
        T::FriendRequestsPaused,
        T::MessagesPaused,
        T::Until,
        T::UntilFurtherNotice,
        T::Restrictions,
        T::WhatThisMeans,
        T::Reason,
        T::ModeratorNote,
        T::Appeal,
        T::Rule,
        T::MutedExplain,
        T::MutedExplainForGood,
        T::NewAccountExplain,
        T::NewAccountExplainUnknown,
        T::BannedExplain,
        T::BannedExplainForGood,
        T::OtherExplain,
        T::AppealAvailable,
        T::AppealField,
        T::SendAppeal,
        T::AppealSent,
        T::AppealTooLong,
        T::AppealOpen,
        T::AppealUpheld,
        T::AppealExpired,
        T::AppealInBrowser,
        T::AppealInBrowserNote,
        T::ToastMuted,
        T::ToastMutedForGood,
        T::ToastNewAccountFriend,
        T::ToastNewAccountStranger,
        T::ToastNewAccount,
        T::ToastBanned,
        T::ToastBannedForGood,
        T::ToastOther,
        T::ToastUnknown,
    ];

    #[test]
    fn placeholders_match_between_languages() {
        for text in ALL {
            let count = |s: &str| s.matches("%@").count();
            assert_eq!(count(text.english()), count(text.turkish()), "{text:?}");
        }
    }

    #[test]
    fn no_text_blames_the_player() {
        for text in ALL {
            let english = text.english().to_lowercase();
            for word in ["you did", "your fault", "violat", "oops", "!"] {
                assert!(!english.contains(word), "{text:?}: {word}");
            }
        }
    }

    // 2026-10-25 21:14 UTC.
    const AT: u64 = 1_792_962_840;

    fn mute(until: Option<u64>) -> Restriction {
        Restriction::new(Restriction::MUTED, until)
    }

    #[test]
    fn the_end_shows_in_the_players_time_zone() {
        assert_eq!(
            until_line(Some(AT), Some(0), Language::English),
            "Until Oct 25, 2026 at 9:14 PM"
        );
        assert_eq!(
            until_line(Some(AT), Some(2 * 3600), Language::English),
            "Until Oct 25, 2026 at 11:14 PM"
        );
        // Past midnight in the zone, the day moves on.
        assert_eq!(
            until_line(Some(AT), Some(3 * 3600), Language::English),
            "Until Oct 26, 2026 at 12:14 AM"
        );
        assert_eq!(
            until_line(Some(AT), Some(3 * 3600), Language::Turkish),
            "26 Eki 2026 00:14 tarihine kadar"
        );
        assert_eq!(
            until_line(None, Some(0), Language::English),
            "Until further notice"
        );
    }

    #[test]
    fn a_toast_says_what_is_paused_and_until_when() {
        assert_eq!(
            toast(&mute(Some(AT)), None, Some(0), Language::English),
            "Messages and friend requests are paused until Oct 25, 2026 at 9:14 PM."
        );
        assert_eq!(
            toast(&mute(None), None, Some(0), Language::English),
            "Messages and friend requests are paused."
        );
        let new = Restriction::new(Restriction::NEW_ACCOUNT, Some(AT));
        assert_eq!(
            toast(
                &new,
                Some(Blocked::FriendRequest),
                Some(0),
                Language::English
            ),
            "New accounts can send a few friend requests. You can send more after Oct 25, 2026 at 9:14 PM."
        );
        assert_eq!(
            toast(&new, Some(Blocked::Stranger), Some(0), Language::English),
            "New accounts can write to people who are not friends after Oct 25, 2026 at 9:14 PM."
        );
        let ban = Restriction::new(Restriction::BANNED, Some(AT));
        assert!(
            toast(&ban, None, Some(0), Language::English)
                .starts_with("This account is banned until Oct 25, 2026 at 9:14 PM.")
        );
    }

    #[test]
    fn a_kind_this_version_does_not_know_still_gets_its_end() {
        let other = Restriction::new("quiet_hours", Some(AT));
        assert_eq!(
            toast(&other, None, Some(0), Language::English),
            "This account is limited until Oct 25, 2026 at 9:14 PM."
        );
        assert_eq!(title(&other, Language::English), "Account Limited");
        assert_eq!(
            toast(
                &Restriction::new("quiet_hours", None),
                None,
                None,
                Language::English
            ),
            "This account cannot do that right now."
        );
    }

    #[test]
    fn rules_have_names_and_unknown_ones_read_as_other() {
        assert_eq!(rule_name("harassment", Language::English), "Harassment");
        assert_eq!(rule_name("illegal", Language::English), "Illegal content");
        assert_eq!(rule_name("treason", Language::English), "Other");
    }

    #[test]
    fn an_appeal_can_be_filed_while_it_is_available_and_the_time_runs() {
        let mut mute = mute(Some(AT));
        assert!(!can_appeal(&mute, 10), "no action to appeal");
        mute.action = Some(4);
        mute.appeal = Some(Appeal {
            status: "available".into(),
            deadline: 100,
        });
        assert!(can_appeal(&mute, 99));
        assert!(!can_appeal(&mute, 100));
        mute.appeal.as_mut().unwrap().status = "open".into();
        assert!(!can_appeal(&mute, 10));
    }
}
