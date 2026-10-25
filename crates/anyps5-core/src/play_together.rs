//! Play Together: which games can be played together, and whether this player can join one now.
//!
//! The decision and the flow are in the network repository (`docs/adr/0030`, `spec/play-together.md`).
//! This module is the launcher's half of it, and it is pure: it reads nothing, starts nothing and
//! knows no screen. Its interface is five things.
//!
//! * [`lan_title`]: is this game in the LAN table (`data/lan-titles.v1.json`)?
//! * [`host`]: may the player host it, and with which plan for the net service?
//! * [`check`]: the join check. An [`Offer`] (what a friend's session or an invitation says) against a
//!   [`World`] (what the player has) is either [`Go`] or the first thing that does not hold, a
//!   [`Blocker`], in the order of the spec's table.
//! * [`Blocker::from_failure`]: the same blockers for what the net service answers (`version_mismatch`,
//!   `full`, `not_found`, `offline`), so a screen has one thing to word.
//! * [`presence`]: how a friend's activity reads, "playing together" for a `lan` session.
//!
//! Wording is not here: a [`Blocker`] carries the facts and the launcher's strings say them.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use anyplay_control::Host;

use crate::online::api::{Activity, Friend, Invitation, SessionKind, Visibility};
use crate::online::{Failure, api::ErrorCode};

/// The table as shipped in the exe.
pub const EMBEDDED: &str = include_str!("../../../data/lan-titles.v1.json");

/// The most players a table entry may allow, and the least that makes a game worth hosting.
const MAX_PLAYERS: u16 = 64;
const MIN_PLAYERS: u16 = 2;

// ---------------------------------------------------------------------------------------------
// The table

/// A game that can be played together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanTitle {
    pub name: String,
    /// The most players in one world, the host included.
    pub max_players: u16,
}

#[derive(serde::Deserialize)]
struct File {
    schema: u32,
    titles: BTreeMap<String, Entry>,
}

#[derive(serde::Deserialize)]
struct Entry {
    name: String,
    mode: String,
    max_players: u16,
}

/// Reads a table. An entry with a mode this launcher does not know is left out; a file that
/// is not the schema this launcher reads, or an entry that makes no sense, is refused.
pub fn parse(json: &str) -> Result<BTreeMap<String, LanTitle>, String> {
    let file: File = serde_json::from_str(json).map_err(|e| e.to_string())?;
    if file.schema != 1 {
        return Err(format!("schema {} is not known", file.schema));
    }
    let mut titles = BTreeMap::new();
    for (id, entry) in file.titles {
        if entry.mode != "lan" {
            continue;
        }
        if id.is_empty() || entry.name.trim().is_empty() {
            return Err(format!("entry {id:?} has no name"));
        }
        if !(MIN_PLAYERS..=MAX_PLAYERS).contains(&entry.max_players) {
            return Err(format!(
                "{id}: max_players {} is outside {MIN_PLAYERS} to {MAX_PLAYERS}",
                entry.max_players
            ));
        }
        titles.insert(
            id,
            LanTitle {
                name: entry.name,
                max_players: entry.max_players,
            },
        );
    }
    Ok(titles)
}

fn table() -> &'static BTreeMap<String, LanTitle> {
    static TABLE: OnceLock<BTreeMap<String, LanTitle>> = OnceLock::new();
    TABLE.get_or_init(|| parse(EMBEDDED).unwrap_or_default())
}

/// The table entry of a game, by its title ID.
pub fn lan_title(title_id: &str) -> Option<&'static LanTitle> {
    table().get(title_id)
}

// ---------------------------------------------------------------------------------------------
// The player's world

/// A game of the player's library that has the offer's title ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owned {
    pub version: String,
    /// Unlocked, so it can start now. A game that is not is unlocked first.
    pub unlocked: bool,
}

/// What the player has, as far as joining or hosting depends on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct World {
    pub signed_in: bool,
    /// The cloud can be reached.
    pub online: bool,
    /// The player has the online features on.
    pub online_play: bool,
    /// The library's game for the title in question.
    pub owned: Option<Owned>,
    /// The toolkit build the game would start with.
    pub build: String,
    /// Another game is starting or running.
    pub busy: bool,
}

// ---------------------------------------------------------------------------------------------
// The offer

/// What a friend's session or an invitation offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub session: String,
    pub title_id: String,
    pub title: String,
    /// The host's version of the game.
    pub version: String,
    pub kind: SessionKind,
    /// The host's handle.
    pub host: String,
    /// Places taken and places in all, when known (an invitation does not say).
    pub places: Option<(u16, u16)>,
    /// The session's AnyPS5 build, when known. Only the net service learns it.
    pub build: Option<String>,
}

impl Offer {
    /// From an invitation.
    pub fn of_invitation(invitation: &Invitation) -> Offer {
        Offer {
            session: invitation.session.clone(),
            title_id: invitation.title_id.clone(),
            title: invitation.title.clone(),
            version: invitation.version.clone(),
            kind: invitation.kind,
            host: invitation.from.handle.clone(),
            places: None,
            build: None,
        }
    }

    /// From a friend's presence: the session they are in, if friends may join it. Else the reason
    /// there is nothing to join.
    pub fn of_friend(friend: &Friend) -> Result<Offer, Blocker> {
        let host = friend.account.handle.clone();
        let Some(activity) = friend.presence.activity.as_ref() else {
            return Err(Blocker::NotJoinable { host });
        };
        match activity.session.as_ref() {
            Some(session) if session.joinable => Ok(Offer {
                session: session.id.clone(),
                title_id: activity.title_id.clone(),
                title: activity.title.clone(),
                version: activity.version.clone(),
                kind: session.kind,
                host,
                places: Some((session.members, session.max_members)),
                build: None,
            }),
            Some(session) if session.members >= session.max_members => Err(Blocker::Full),
            Some(_) => Err(Blocker::NotJoinable { host }),
            // The friend plays a game that finds others on the LAN, but opened no session: their
            // online play is off, or they pressed Play and not Play Together.
            None if lan_title(&activity.title_id).is_some() => Err(Blocker::NoSession {
                host,
                title: activity.title.clone(),
            }),
            None => Err(Blocker::NotJoinable { host }),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The check

/// The join may go ahead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Go {
    /// The game is owned but not unlocked: unlock it, then start it in the session.
    pub unlock_first: bool,
}

/// What stops Play Together, one at a time and in the order the spec lists them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Blocker {
    /// The launcher is signed out.
    SignedOut,
    /// The cloud cannot be reached.
    Offline,
    /// The player switched the online features off.
    OnlineOff,
    /// No game of the library has the title ID.
    NotOwned { title: String },
    /// The library's version differs from the host's.
    WrongVersion {
        title: String,
        need: String,
        have: String,
    },
    /// The AnyPS5 build the game would start with differs from the session's.
    WrongBuild { need: String, have: String },
    /// The session has no free place.
    Full,
    /// Another game is starting or running.
    Busy,
    /// The cloud no longer knows the session.
    Ended,
    /// The friend plays a LAN game with no session: their online play is off, or they did not
    /// start it with Play Together.
    NoSession { host: String, title: String },
    /// The friend is in nothing a friend may join.
    NotJoinable { host: String },
    /// Hosting only: the game is not in the table.
    NotSupported { title: String },
}

impl Blocker {
    /// What the net service's answer to `check_join` or `game_start` says, in the same terms.
    /// `None` for a failure that is not about the session (the caller keeps its generic text).
    /// `have` is the player's own version and build, to tell which of the two differs.
    pub fn from_failure(
        failure: &Failure,
        title: &str,
        have_version: &str,
        have_build: &str,
    ) -> Option<Blocker> {
        if failure.is(ErrorCode::VersionMismatch) {
            let expected = failure.expected.as_ref()?;
            return Some(if expected.version != have_version {
                Blocker::WrongVersion {
                    title: title.to_owned(),
                    need: expected.version.clone(),
                    have: have_version.to_owned(),
                }
            } else {
                Blocker::WrongBuild {
                    need: expected.build.clone(),
                    have: have_build.to_owned(),
                }
            });
        }
        if failure.is(ErrorCode::Full) {
            return Some(Blocker::Full);
        }
        if failure.is(ErrorCode::NotFound) {
            return Some(Blocker::Ended);
        }
        match failure.code.as_str() {
            Failure::OFFLINE => Some(Blocker::Offline),
            Failure::NOT_SIGNED_IN => Some(Blocker::SignedOut),
            Failure::BUSY => Some(Blocker::Busy),
            _ => None,
        }
    }
}

/// The first of the world's gates, shared by hosting and joining.
fn connected(world: &World) -> Result<(), Blocker> {
    if !world.signed_in {
        return Err(Blocker::SignedOut);
    }
    if !world.online {
        return Err(Blocker::Offline);
    }
    if !world.online_play {
        return Err(Blocker::OnlineOff);
    }
    Ok(())
}

/// The join check: the offer against the player's world. The first of signed in and online,
/// online play, owned, version, toolkit, place and not busy that does not hold. "Still there" is
/// the net service's, and comes back through [`Blocker::from_failure`].
pub fn check(offer: &Offer, world: &World) -> Result<Go, Blocker> {
    connected(world)?;
    let Some(owned) = &world.owned else {
        return Err(Blocker::NotOwned {
            title: offer.title.clone(),
        });
    };
    if owned.version != offer.version {
        return Err(Blocker::WrongVersion {
            title: offer.title.clone(),
            need: offer.version.clone(),
            have: owned.version.clone(),
        });
    }
    if let Some(build) = offer.build.as_ref().filter(|b| **b != world.build) {
        return Err(Blocker::WrongBuild {
            need: build.clone(),
            have: world.build.clone(),
        });
    }
    if offer.places.is_some_and(|(taken, total)| taken >= total) {
        return Err(Blocker::Full);
    }
    if world.busy {
        return Err(Blocker::Busy);
    }
    Ok(Go {
        unlock_first: !owned.unlocked,
    })
}

/// May the player host `title_id` now, and with which plan? The plan is `game_start`'s `host`:
/// the table's player count, friends may join, and the party comes along when there is one.
pub fn host(title_id: &str, title: &str, in_party: bool, world: &World) -> Result<Host, Blocker> {
    let Some(entry) = lan_title(title_id) else {
        return Err(Blocker::NotSupported {
            title: title.to_owned(),
        });
    };
    connected(world)?;
    if world.busy {
        return Err(Blocker::Busy);
    }
    Ok(Host {
        max_members: entry.max_players,
        visibility: Visibility::Friends,
        party: in_party,
    })
}

// ---------------------------------------------------------------------------------------------
// Presence

/// How a friend's activity reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line<'a> {
    /// In a Play Together session: "Playing Minecraft together (2/4)".
    Together {
        title: &'a str,
        taken: u16,
        total: u16,
    },
    /// In a session of a game that friends may join.
    Joinable { title: &'a str },
    /// Playing, with nothing to join.
    Playing { title: &'a str },
}

/// The presence line of an activity. A `lan` session reads as playing together, joinable or not
/// (a full world is still being played together); any other session keeps its wording.
pub fn presence(activity: &Activity) -> Line<'_> {
    let title = activity.title.as_str();
    match &activity.session {
        Some(session) if session.kind == SessionKind::Lan => Line::Together {
            title,
            taken: session.members,
            total: session.max_members,
        },
        Some(session) if session.joinable => Line::Joinable { title },
        _ => Line::Playing { title },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::online::api::{Account, AccountId, Expected, Presence, SessionRef};

    const MC: &str = "PPSA17221";

    fn offer() -> Offer {
        Offer {
            session: "s1".into(),
            title_id: MC.into(),
            title: "Minecraft".into(),
            version: "01.044.000".into(),
            kind: SessionKind::Lan,
            host: "Mira".into(),
            places: Some((2, 4)),
            build: None,
        }
    }

    fn world() -> World {
        World {
            signed_in: true,
            online: true,
            online_play: true,
            owned: Some(Owned {
                version: "01.044.000".into(),
                unlocked: true,
            }),
            build: "v0.1.4".into(),
            busy: false,
        }
    }

    fn account(name: &str) -> Account {
        Account {
            id: AccountId(7),
            handle: name.into(),
        }
    }

    #[test]
    fn the_shipped_table_is_valid_and_has_minecraft() {
        let titles = parse(EMBEDDED).expect("the shipped file parses");
        assert!(!titles.is_empty());
        let minecraft = lan_title(MC).expect("Minecraft is in the table");
        assert_eq!(minecraft.name, "Minecraft");
        assert_eq!(minecraft.max_players, 8);
        assert!(lan_title("GAME00001").is_none());
        for (id, title) in &titles {
            assert!(id.len() >= 8 && id.chars().all(|c| c.is_ascii_alphanumeric()));
            assert!((MIN_PLAYERS..=MAX_PLAYERS).contains(&title.max_players));
        }
    }

    #[test]
    fn an_unknown_mode_is_ignored_and_nonsense_is_refused() {
        let json = r#"{"schema":1,"updated":"x","titles":{
            "AAAA00001":{"name":"A","mode":"lan","max_players":4},
            "BBBB00002":{"name":"B","mode":"matchmaking","max_players":4}}}"#;
        let titles = parse(json).expect("parses");
        assert_eq!(titles.len(), 1);
        assert!(titles.contains_key("AAAA00001"));
        assert!(parse(r#"{"schema":2,"titles":{}}"#).is_err());
        assert!(
            parse(r#"{"schema":1,"titles":{"A":{"name":"A","mode":"lan","max_players":1}}}"#)
                .is_err()
        );
        assert!(parse("not json").is_err());
    }

    #[test]
    fn a_fitting_world_may_join() {
        assert_eq!(
            check(&offer(), &world()),
            Ok(Go {
                unlock_first: false
            })
        );
    }

    #[test]
    fn a_game_that_is_owned_but_locked_is_unlocked_first() {
        let mut w = world();
        w.owned.as_mut().unwrap().unlocked = false;
        assert_eq!(check(&offer(), &w), Ok(Go { unlock_first: true }));
    }

    #[test]
    fn every_blocker_is_found() {
        let mut w = world();
        w.signed_in = false;
        assert_eq!(check(&offer(), &w), Err(Blocker::SignedOut));
        let mut w = world();
        w.online = false;
        assert_eq!(check(&offer(), &w), Err(Blocker::Offline));
        let mut w = world();
        w.online_play = false;
        assert_eq!(check(&offer(), &w), Err(Blocker::OnlineOff));
        let mut w = world();
        w.owned = None;
        assert_eq!(
            check(&offer(), &w),
            Err(Blocker::NotOwned {
                title: "Minecraft".into()
            })
        );
        let mut w = world();
        w.owned.as_mut().unwrap().version = "01.008.000".into();
        assert_eq!(
            check(&offer(), &w),
            Err(Blocker::WrongVersion {
                title: "Minecraft".into(),
                need: "01.044.000".into(),
                have: "01.008.000".into()
            })
        );
        let mut o = offer();
        o.build = Some("v0.1.5".into());
        assert_eq!(
            check(&o, &world()),
            Err(Blocker::WrongBuild {
                need: "v0.1.5".into(),
                have: "v0.1.4".into()
            })
        );
        let mut o = offer();
        o.places = Some((4, 4));
        assert_eq!(check(&o, &world()), Err(Blocker::Full));
        let mut w = world();
        w.busy = true;
        assert_eq!(check(&offer(), &w), Err(Blocker::Busy));
    }

    #[test]
    fn the_same_build_and_unknown_places_pass() {
        let mut o = offer();
        o.build = Some("v0.1.4".into());
        o.places = None;
        assert!(check(&o, &world()).is_ok());
    }

    #[test]
    fn the_first_failing_check_wins_in_the_order_of_the_table() {
        // Everything is wrong at once; each step mends the blocker just named.
        let mut o = offer();
        o.build = Some("v0.1.5".into());
        o.places = Some((4, 4));
        let mut w = World {
            signed_in: false,
            online: false,
            online_play: false,
            owned: None,
            build: "v0.1.4".into(),
            busy: true,
        };
        let name = |o: &Offer, w: &World| match check(o, w) {
            Err(Blocker::SignedOut) => "signed_out",
            Err(Blocker::Offline) => "offline",
            Err(Blocker::OnlineOff) => "online_off",
            Err(Blocker::NotOwned { .. }) => "not_owned",
            Err(Blocker::WrongVersion { .. }) => "version",
            Err(Blocker::WrongBuild { .. }) => "build",
            Err(Blocker::Full) => "full",
            Err(Blocker::Busy) => "busy",
            Err(_) => "other",
            Ok(_) => "ok",
        };
        assert_eq!(name(&o, &w), "signed_out");
        w.signed_in = true;
        assert_eq!(name(&o, &w), "offline");
        w.online = true;
        assert_eq!(name(&o, &w), "online_off");
        w.online_play = true;
        assert_eq!(name(&o, &w), "not_owned");
        w.owned = Some(Owned {
            version: "9".into(),
            unlocked: true,
        });
        assert_eq!(name(&o, &w), "version");
        w.owned.as_mut().unwrap().version = "01.044.000".into();
        assert_eq!(name(&o, &w), "build");
        o.build = None;
        assert_eq!(name(&o, &w), "full");
        o.places = Some((1, 4));
        assert_eq!(name(&o, &w), "busy");
        w.busy = false;
        assert_eq!(name(&o, &w), "ok");
    }

    fn failure(code: &str, expected: Option<(&str, &str)>) -> Failure {
        let mut f = Failure::new(code, "message");
        f.expected = expected.map(|(version, build)| Expected {
            version: version.into(),
            build: build.into(),
        });
        f
    }

    #[test]
    fn the_net_services_failures_become_the_same_blockers() {
        let from = |f: Failure| Blocker::from_failure(&f, "Minecraft", "01.008.000", "v0.1.4");
        assert_eq!(
            from(failure("version_mismatch", Some(("01.044.000", "v0.1.4")))),
            Some(Blocker::WrongVersion {
                title: "Minecraft".into(),
                need: "01.044.000".into(),
                have: "01.008.000".into()
            })
        );
        assert_eq!(
            from(failure("version_mismatch", Some(("01.008.000", "v0.1.9")))),
            Some(Blocker::WrongBuild {
                need: "v0.1.9".into(),
                have: "v0.1.4".into()
            })
        );
        assert_eq!(from(failure("version_mismatch", None)), None);
        assert_eq!(from(failure("full", None)), Some(Blocker::Full));
        assert_eq!(from(failure("not_found", None)), Some(Blocker::Ended));
        assert_eq!(from(failure("offline", None)), Some(Blocker::Offline));
        assert_eq!(
            from(failure("not_signed_in", None)),
            Some(Blocker::SignedOut)
        );
        assert_eq!(from(failure("busy", None)), Some(Blocker::Busy));
        assert_eq!(from(failure("rate_limited", None)), None);
    }

    fn friend(activity: Option<Activity>) -> Friend {
        Friend {
            account: account("Mira"),
            since: 0,
            presence: Presence {
                online: true,
                activity,
                revision: 0,
            },
        }
    }

    fn activity(title_id: &str, session: Option<SessionRef>) -> Activity {
        Activity {
            title_id: title_id.into(),
            title: "Minecraft".into(),
            version: "01.044.000".into(),
            session,
        }
    }

    fn session(kind: SessionKind, joinable: bool, members: u16, max: u16) -> SessionRef {
        SessionRef {
            id: "s1".into(),
            joinable,
            members,
            max_members: max,
            kind,
        }
    }

    #[test]
    fn a_friends_joinable_session_is_an_offer() {
        let f = friend(Some(activity(
            MC,
            Some(session(SessionKind::Lan, true, 2, 4)),
        )));
        let offer = Offer::of_friend(&f).expect("an offer");
        assert_eq!(offer.session, "s1");
        assert_eq!(offer.kind, SessionKind::Lan);
        assert_eq!(offer.places, Some((2, 4)));
        assert_eq!(offer.host, "Mira");
    }

    #[test]
    fn a_friend_with_the_title_but_no_session_has_their_bridge_off() {
        let f = friend(Some(activity(MC, None)));
        assert_eq!(
            Offer::of_friend(&f),
            Err(Blocker::NoSession {
                host: "Mira".into(),
                title: "Minecraft".into()
            })
        );
        // A game that is no LAN game with no session is simply not joinable.
        let f = friend(Some(activity("GAME00001", None)));
        assert_eq!(
            Offer::of_friend(&f),
            Err(Blocker::NotJoinable {
                host: "Mira".into()
            })
        );
        assert_eq!(
            Offer::of_friend(&friend(None)),
            Err(Blocker::NotJoinable {
                host: "Mira".into()
            })
        );
    }

    #[test]
    fn a_full_session_is_full_and_a_closed_one_is_not_joinable() {
        let f = friend(Some(activity(
            MC,
            Some(session(SessionKind::Lan, false, 4, 4)),
        )));
        assert_eq!(Offer::of_friend(&f), Err(Blocker::Full));
        let f = friend(Some(activity(
            MC,
            Some(session(SessionKind::Lan, false, 1, 4)),
        )));
        assert_eq!(
            Offer::of_friend(&f),
            Err(Blocker::NotJoinable {
                host: "Mira".into()
            })
        );
    }

    #[test]
    fn hosting_takes_the_tables_players_and_the_party() {
        let plan = host(MC, "Minecraft", true, &world()).expect("a plan");
        assert_eq!(plan.max_members, 8);
        assert_eq!(plan.visibility, Visibility::Friends);
        assert!(plan.party);
        assert!(!host(MC, "Minecraft", false, &world()).unwrap().party);
    }

    #[test]
    fn hosting_is_stopped_by_the_same_gates() {
        let mut w = world();
        w.signed_in = false;
        assert_eq!(host(MC, "Minecraft", false, &w), Err(Blocker::SignedOut));
        let mut w = world();
        w.online_play = false;
        assert_eq!(host(MC, "Minecraft", false, &w), Err(Blocker::OnlineOff));
        let mut w = world();
        w.busy = true;
        assert_eq!(host(MC, "Minecraft", false, &w), Err(Blocker::Busy));
        assert_eq!(
            host("GAME00001", "Other", false, &world()),
            Err(Blocker::NotSupported {
                title: "Other".into()
            })
        );
    }

    #[test]
    fn presence_reads_together_for_a_lan_session_only() {
        let lan = activity(MC, Some(session(SessionKind::Lan, true, 2, 4)));
        assert_eq!(
            presence(&lan),
            Line::Together {
                title: "Minecraft",
                taken: 2,
                total: 4
            }
        );
        let full = activity(MC, Some(session(SessionKind::Lan, false, 4, 4)));
        assert!(matches!(presence(&full), Line::Together { taken: 4, .. }));
        let game = activity("GAME00001", Some(session(SessionKind::Game, true, 1, 4)));
        assert_eq!(presence(&game), Line::Joinable { title: "Minecraft" });
        assert_eq!(
            presence(&activity(MC, None)),
            Line::Playing { title: "Minecraft" }
        );
    }

    #[test]
    fn an_invitation_keeps_its_kind() {
        let invitation = Invitation {
            id: "i1".into(),
            from: account("Mira"),
            session: "s1".into(),
            kind: SessionKind::Lan,
            title_id: MC.into(),
            title: "Minecraft".into(),
            version: "01.044.000".into(),
            expires: 0,
        };
        let offer = Offer::of_invitation(&invitation);
        assert_eq!(offer.kind, SessionKind::Lan);
        assert_eq!(offer.places, None);
        assert_eq!(offer.host, "Mira");
    }
}
