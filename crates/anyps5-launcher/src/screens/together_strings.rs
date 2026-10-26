//! Texts of Play Together (docs/DESIGN.md 17, "Play Together"): the button, the prompt, what
//! stops a join, the presence line. English and Turkish like the rest; the Turkish was written for
//! this change and has not been read by a native speaker yet. Headlines say what is the case in
//! sentence case; the line under it says what to do.

use anyps5_core::play_together::{Blocker, Line};

use crate::ui::i18n::{Arg, Language, substitute};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TogetherText {
    // The button
    /// The host button on a game's page and on the party page.
    PlayTogether,
    /// `%@` = the game. The party page's row detail.
    PlayTogetherDetail,
    // The prompt
    /// `%@` = who, `%@` = the game.
    OfferHeadline,
    /// `%@` = places taken, `%@` = places in all.
    OfferPlaces,
    /// Under the headline when the places are not known.
    OfferNoPlaces,
    Join,
    Decline,
    Close,
    /// The secondary answer on a version blocker: join although the versions do not fit.
    TryAnyway,
    SignIn,
    SwitchOn,
    // What stops a join
    SignedOutHeadline,
    SignedOutLine,
    OfflineHeadline,
    OfflineLine,
    OnlineOffHeadline,
    OnlineOffLine,
    /// `%@` = the game.
    NotOwnedHeadline,
    NotOwnedLine,
    /// `%@` = the game, the version needed, the version the player has.
    WrongVersionHeadline,
    WrongVersionLine,
    /// `%@` = the game, `%@` = the version. You are ahead of the session.
    WrongVersionLineAhead,
    /// Said under the version blocker's line, where "Try Anyway" is offered.
    TryAnywayNote,
    /// `%@` = the toolkit needed, the toolkit the player has.
    WrongBuildHeadline,
    WrongBuildLine,
    FullHeadline,
    FullLine,
    BusyHeadline,
    BusyLine,
    EndedHeadline,
    EndedLine,
    /// `%@` = who. The friend plays the game, but opened no session.
    NoSessionHeadline,
    /// `%@` = the game.
    NoSessionLine,
    /// `%@` = who.
    NotJoinableHeadline,
    NotJoinableLine,
    /// `%@` = the game.
    NotSupportedHeadline,
    NotSupportedLine,
    // Presence
    /// `%@` = the game, places taken, places in all.
    PresenceTogether,
    // Toasts
    /// `%@` = who. The game keeps running.
    ToastSessionEnded,
    /// The game started, but no session opened.
    ToastHostFailed,
    /// The net service did not answer in time: the game started alone.
    ToastHostTimedOut,
    /// `%@` = the game. Unlocking before the join starts.
    ToastUnlocking,
    /// The unlock did not finish: no join.
    ToastUnlockFailed,
    /// `%@` = the game. The button on the page when the game is not unlocked.
    UnlockFirstNote,
}

impl TogetherText {
    pub const fn english(self) -> &'static str {
        match self {
            TogetherText::PlayTogether => "Play Together",
            TogetherText::PlayTogetherDetail => "Start %@ and let friends join",
            TogetherText::OfferHeadline => "%@ is playing %@. Join?",
            TogetherText::OfferPlaces => "%@ of %@ players",
            TogetherText::OfferNoPlaces => "Your game starts in their session",
            TogetherText::Join => "Join",
            TogetherText::Decline => "Decline",
            TogetherText::Close => "Close",
            TogetherText::TryAnyway => "Try Anyway",
            TogetherText::SignIn => "Sign In",
            TogetherText::SwitchOn => "Switch On",
            TogetherText::SignedOutHeadline => "Sign in to use Play Together",
            TogetherText::SignedOutLine => "Play Together uses your AnyPlay account.",
            TogetherText::OfflineHeadline => "You are offline",
            TogetherText::OfflineLine => "Play Together needs a connection. Try again in a moment.",
            TogetherText::OnlineOffHeadline => "Online play is switched off",
            TogetherText::OnlineOffLine => "Switch it on to play together.",
            TogetherText::NotOwnedHeadline => "You need %@ to join",
            TogetherText::NotOwnedLine => "It is not in your library. Add it, then join again.",
            TogetherText::WrongVersionHeadline => "You need %@ %@; you have %@",
            TogetherText::WrongVersionLine => "Update the game, then join again.",
            TogetherText::WrongVersionLineAhead => "Ask your friend to update, or install %@ %@.",
            TogetherText::TryAnywayNote => "Trying anyway may not connect or may fall out of sync.",
            TogetherText::WrongBuildHeadline => "You need toolkit %@; you have %@",
            TogetherText::WrongBuildLine => {
                "Choose it under Toolkit for This Game, then join again."
            }
            TogetherText::FullHeadline => "This game is full",
            TogetherText::FullLine => "Try again when someone leaves.",
            TogetherText::BusyHeadline => "Another game is running",
            TogetherText::BusyLine => "Close it, then join.",
            TogetherText::EndedHeadline => "That session has ended",
            TogetherText::EndedLine => "Ask your friend to start a new one.",
            TogetherText::NoSessionHeadline => "%@ has nothing to join yet",
            TogetherText::NoSessionLine => {
                "Their online play is off, or they started %@ without Play Together."
            }
            TogetherText::NotJoinableHeadline => "%@ is not in a game you can join",
            TogetherText::NotJoinableLine => "Try again when they start one.",
            TogetherText::NotSupportedHeadline => "%@ cannot be played together yet",
            TogetherText::NotSupportedLine => "Only some games can be played together.",
            TogetherText::PresenceTogether => "Playing %@ together (%@/%@)",
            TogetherText::ToastSessionEnded => {
                "%@\u{2019}s session ended. Your game keeps running."
            }
            TogetherText::ToastHostFailed => {
                "Nobody can join: Play Together could not start. Your game runs anyway."
            }
            TogetherText::ToastHostTimedOut => {
                "Nobody can join: the network did not answer. Your game runs anyway."
            }
            TogetherText::ToastUnlocking => "Unlocking %@, then joining",
            TogetherText::ToastUnlockFailed => "Unlocking did not finish, so you did not join",
            TogetherText::UnlockFirstNote => "%@ is unlocked first",
        }
    }

    pub const fn turkish(self) -> &'static str {
        match self {
            TogetherText::PlayTogether => "Birlikte oyna",
            TogetherText::PlayTogetherDetail => "%@ oyununu başlat, arkadaşların katılsın",
            TogetherText::OfferHeadline => "%@ %@ oynuyor. Katılmak ister misin?",
            TogetherText::OfferPlaces => "%@ / %@ oyuncu",
            TogetherText::OfferNoPlaces => "Oyunun onların oturumunda açılır",
            TogetherText::Join => "Katıl",
            TogetherText::Decline => "Reddet",
            TogetherText::Close => "Kapat",
            TogetherText::TryAnyway => "Yine de Dene",
            TogetherText::SignIn => "Giriş Yap",
            TogetherText::SwitchOn => "Aç",
            TogetherText::SignedOutHeadline => "Birlikte oynamak için giriş yap",
            TogetherText::SignedOutLine => "Birlikte oyna, AnyPlay hesabını kullanır.",
            TogetherText::OfflineHeadline => "Çevrimdışısın",
            TogetherText::OfflineLine => {
                "Birlikte oyna için bağlantı gerekir. Birazdan tekrar dene."
            }
            TogetherText::OnlineOffHeadline => "Çevrimiçi oyun kapalı",
            TogetherText::OnlineOffLine => "Birlikte oynamak için aç.",
            TogetherText::NotOwnedHeadline => "Katılmak için %@ gerekir",
            TogetherText::NotOwnedLine => "Kitaplığında yok. Ekle, sonra tekrar katıl.",
            TogetherText::WrongVersionHeadline => "%@ %@ sürümü gerekir; sende %@ var",
            TogetherText::WrongVersionLine => "Oyunu güncelle, sonra tekrar katıl.",
            TogetherText::WrongVersionLineAhead => {
                "Arkadaşından güncellemesini iste ya da %@ %@ sürümünü kur."
            }
            TogetherText::TryAnywayNote => {
                "Yine de denersen bağlanamayabilir ya da senkron dışı kalabilir."
            }
            TogetherText::WrongBuildHeadline => "%@ araç takımı gerekir; sende %@ var",
            TogetherText::WrongBuildLine => {
                "Oyunun ayarlarında Bu Oyun İçin Araç Takımı'ndan seç, sonra tekrar katıl."
            }
            TogetherText::FullHeadline => "Bu oyun dolu",
            TogetherText::FullLine => "Biri ayrılınca tekrar dene.",
            TogetherText::BusyHeadline => "Başka bir oyun çalışıyor",
            TogetherText::BusyLine => "Onu kapat, sonra katıl.",
            TogetherText::EndedHeadline => "Bu oturum sona erdi",
            TogetherText::EndedLine => "Arkadaşından yeni bir oturum açmasını iste.",
            TogetherText::NoSessionHeadline => "%@ için katılacak bir şey yok",
            TogetherText::NoSessionLine => {
                "Çevrimiçi oyunu kapalı ya da %@ oyununu Birlikte oyna olmadan başlattı."
            }
            TogetherText::NotJoinableHeadline => "%@ katılabileceğin bir oyunda değil",
            TogetherText::NotJoinableLine => "Yeni bir oyun başlatınca tekrar dene.",
            TogetherText::NotSupportedHeadline => "%@ henüz birlikte oynanamıyor",
            TogetherText::NotSupportedLine => "Yalnızca bazı oyunlar birlikte oynanabilir.",
            TogetherText::PresenceTogether => "%@ birlikte oynuyor (%@/%@)",
            TogetherText::ToastSessionEnded => {
                "%@ kullanıcısının oturumu sona erdi. Oyunun çalışmaya devam ediyor."
            }
            TogetherText::ToastHostFailed => {
                "Kimse katılamaz: Birlikte oyna başlatılamadı. Oyunun yine de çalışıyor."
            }
            TogetherText::ToastHostTimedOut => {
                "Kimse katılamaz: ağ yanıt vermedi. Oyunun yine de çalışıyor."
            }
            TogetherText::ToastUnlocking => "%@ açılıyor, sonra katılınacak",
            TogetherText::ToastUnlockFailed => "Açma tamamlanmadı, bu yüzden katılmadın",
            TogetherText::UnlockFirstNote => "%@ önce açılır",
        }
    }

    pub fn text(self, language: Language) -> &'static str {
        match language {
            Language::Turkish => self.turkish(),
            Language::English => self.english(),
        }
    }

    pub fn format(self, language: Language, args: &[Arg<'_>]) -> String {
        substitute(self.text(language), args)
    }
}

/// What a blocker says: the headline, and under it what to do.
pub fn blocker_text(blocker: &Blocker, language: Language) -> (String, String) {
    use TogetherText as T;
    let one = |head: T, line: T, args: &[Arg<'_>]| {
        (head.format(language, args), line.format(language, args))
    };
    match blocker {
        Blocker::SignedOut => one(T::SignedOutHeadline, T::SignedOutLine, &[]),
        Blocker::Offline => one(T::OfflineHeadline, T::OfflineLine, &[]),
        Blocker::OnlineOff => one(T::OnlineOffHeadline, T::OnlineOffLine, &[]),
        Blocker::NotOwned { title } => {
            one(T::NotOwnedHeadline, T::NotOwnedLine, &[Arg::Str(title)])
        }
        Blocker::WrongVersion { title, need, have } => {
            // Versions are fixed-width ("01.044.000"): the text order is the version order.
            let line = if need.as_str() > have.as_str() {
                T::WrongVersionLine
            } else {
                T::WrongVersionLineAhead
            };
            (
                T::WrongVersionHeadline
                    .format(language, &[Arg::Str(title), Arg::Str(need), Arg::Str(have)]),
                // The note belongs to the "Try Anyway" button that sits under this line.
                format!(
                    "{} {}",
                    line.format(language, &[Arg::Str(title), Arg::Str(need)]),
                    T::TryAnywayNote.text(language)
                ),
            )
        }
        Blocker::WrongBuild { need, have } => one(
            T::WrongBuildHeadline,
            T::WrongBuildLine,
            &[Arg::Str(need), Arg::Str(have)],
        ),
        Blocker::Full => one(T::FullHeadline, T::FullLine, &[]),
        Blocker::Busy => one(T::BusyHeadline, T::BusyLine, &[]),
        Blocker::Ended => one(T::EndedHeadline, T::EndedLine, &[]),
        Blocker::NoSession { host, title } => (
            T::NoSessionHeadline.format(language, &[Arg::Str(host)]),
            T::NoSessionLine.format(language, &[Arg::Str(title)]),
        ),
        Blocker::NotJoinable { host } => (
            T::NotJoinableHeadline.format(language, &[Arg::Str(host)]),
            T::NotJoinableLine.text(language).to_owned(),
        ),
        Blocker::NotSupported { title } => (
            T::NotSupportedHeadline.format(language, &[Arg::Str(title)]),
            T::NotSupportedLine.text(language).to_owned(),
        ),
    }
}

/// How a friend's activity reads in a list, a profile or the party: "Playing Minecraft together
/// (2/4)" for a `lan` session. `None` for a line the caller words the way it always did.
pub fn presence_line(line: &Line<'_>, language: Language) -> Option<String> {
    match line {
        Line::Together {
            title,
            taken,
            total,
        } => Some(TogetherText::PresenceTogether.format(
            language,
            &[
                Arg::Str(title),
                Arg::Int(i64::from(*taken)),
                Arg::Int(i64::from(*total)),
            ],
        )),
        Line::Joinable { .. } | Line::Playing { .. } => None,
    }
}

/// The caption of the player's own session in the panel: "Playing Minecraft together (2/4)" for a
/// Play Together session, `None` for any other.
pub fn own_session_line(
    session: &anyps5_core::online::control::Session,
    language: Language,
) -> Option<String> {
    (session.kind == anyps5_core::online::api::SessionKind::Lan).then(|| {
        TogetherText::PresenceTogether.format(
            language,
            &[
                Arg::Str(&session.title),
                Arg::Int(session.members.len() as i64),
                Arg::Int(i64::from(session.max_members)),
            ],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_match_between_languages() {
        let all = [
            TogetherText::PlayTogetherDetail,
            TogetherText::OfferHeadline,
            TogetherText::OfferPlaces,
            TogetherText::NotOwnedHeadline,
            TogetherText::WrongVersionHeadline,
            TogetherText::WrongVersionLineAhead,
            TogetherText::WrongBuildHeadline,
            TogetherText::NoSessionHeadline,
            TogetherText::NoSessionLine,
            TogetherText::NotJoinableHeadline,
            TogetherText::NotSupportedHeadline,
            TogetherText::PresenceTogether,
            TogetherText::ToastSessionEnded,
            TogetherText::ToastUnlocking,
            TogetherText::UnlockFirstNote,
            TogetherText::SignedOutHeadline,
            TogetherText::FullHeadline,
        ];
        for text in all {
            let count = |s: &str| s.matches("%@").count();
            assert_eq!(count(text.english()), count(text.turkish()), "{text:?}");
        }
    }

    #[test]
    fn a_wrong_version_says_what_the_owner_wants_to_read() {
        let blocker = Blocker::WrongVersion {
            title: "Minecraft".into(),
            need: "01.044.000".into(),
            have: "01.008.000".into(),
        };
        let (head, line) = blocker_text(&blocker, Language::English);
        assert_eq!(head, "You need Minecraft 01.044.000; you have 01.008.000");
        assert!(line.contains("Update the game"));
        assert!(line.ends_with("may not connect or may fall out of sync."));
        let ahead = Blocker::WrongVersion {
            title: "Minecraft".into(),
            need: "01.008.000".into(),
            have: "01.044.000".into(),
        };
        let (_, line) = blocker_text(&ahead, Language::English);
        assert_eq!(
            line,
            "Ask your friend to update, or install Minecraft 01.008.000. Trying anyway may not connect or may fall out of sync."
        );
    }

    #[test]
    fn every_blocker_has_a_headline_and_a_line_with_no_placeholder_left() {
        let blockers = [
            Blocker::SignedOut,
            Blocker::Offline,
            Blocker::OnlineOff,
            Blocker::NotOwned {
                title: "Minecraft".into(),
            },
            Blocker::WrongVersion {
                title: "Minecraft".into(),
                need: "2".into(),
                have: "1".into(),
            },
            Blocker::WrongBuild {
                need: "v0.1.5".into(),
                have: "v0.1.4".into(),
            },
            Blocker::Full,
            Blocker::Busy,
            Blocker::Ended,
            Blocker::NoSession {
                host: "Mira".into(),
                title: "Minecraft".into(),
            },
            Blocker::NotJoinable {
                host: "Mira".into(),
            },
            Blocker::NotSupported {
                title: "Minecraft".into(),
            },
        ];
        for language in [Language::English, Language::Turkish] {
            for blocker in &blockers {
                let (head, line) = blocker_text(blocker, language);
                assert!(!head.is_empty() && !line.is_empty(), "{blocker:?}");
                assert!(!head.contains("%@") && !line.contains("%@"), "{blocker:?}");
            }
        }
    }

    #[test]
    fn a_missing_bridge_names_the_friend_and_the_game() {
        let (head, line) = blocker_text(
            &Blocker::NoSession {
                host: "Mira".into(),
                title: "Minecraft".into(),
            },
            Language::English,
        );
        assert_eq!(head, "Mira has nothing to join yet");
        assert!(line.contains("Minecraft") && line.contains("Play Together"));
    }

    #[test]
    fn presence_reads_together_with_the_places() {
        let line = Line::Together {
            title: "Minecraft",
            taken: 2,
            total: 4,
        };
        assert_eq!(
            presence_line(&line, Language::English).as_deref(),
            Some("Playing Minecraft together (2/4)")
        );
        assert_eq!(
            presence_line(&Line::Playing { title: "X" }, Language::English),
            None
        );
        assert_eq!(
            presence_line(&Line::Joinable { title: "X" }, Language::English),
            None
        );
    }

    #[test]
    fn the_prompt_is_the_sentence_the_owner_wrote() {
        assert_eq!(
            TogetherText::OfferHeadline.format(
                Language::English,
                &[Arg::Str("Mira"), Arg::Str("Minecraft")]
            ),
            "Mira is playing Minecraft. Join?"
        );
    }
}
