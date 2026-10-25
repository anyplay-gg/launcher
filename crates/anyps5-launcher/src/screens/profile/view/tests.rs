use anyps5_core::online::api::{
    Account, Feed, FeedEvent, FeedItem, FriendLists, Grade, Presence, Profile, ProfilePage,
    ReactionCounts, Request, TitlePlaytime,
};
use anyps5_core::online::{Cloud, ProfileEntry, SignedIn, Status};
use egui::TextureId;

use super::*;
use crate::screens::online::OwnedGame as LocalGame;

const NOW: i64 = 1_792_670_400;

fn who(id: u64, handle: &str) -> Account {
    Account {
        id: AccountId(id),
        handle: handle.into(),
    }
}

/// Mira (1) is signed in; Lumen (2) is a friend; Vale (6) asked; Ash (7) was asked.
fn online() -> Online {
    let mut online = Online::default();
    online.status = Status::Running;
    online.cloud = Some(Cloud::Online);
    online.account = Some(SignedIn {
        id: AccountId(1),
        handle: "Mira".into(),
        email: String::new(),
    });
    online.friends = FriendLists {
        friends: vec![anyps5_core::online::api::Friend {
            account: who(2, "Lumen"),
            since: 0,
            presence: Presence {
                online: true,
                activity: None,
                revision: 0,
            },
        }],
        incoming: vec![Request {
            account: who(6, "Vale"),
            since: 0,
        }],
        outgoing: vec![Request {
            account: who(7, "Ash"),
            since: 0,
        }],
        blocked: Vec::new(),
    };
    online
}

fn page(account: Account) -> ProfilePage {
    ProfilePage {
        profile: Profile {
            account,
            about: None,
            favourites: None,
            avatar: None,
            playtime: None,
        },
        trophies: None,
        links: Vec::new(),
    }
}

fn set(online: &mut Online, page: ProfilePage) {
    let id = page.profile.account.id;
    online.profiles.insert(
        id,
        ProfileEntry {
            page: Some(page),
            failed: false,
        },
    );
}

fn played(title_id: &str, minutes: u64) -> TitlePlaytime {
    TitlePlaytime {
        title_id: title_id.into(),
        minutes,
    }
}

fn item(id: u64, account: Account, event: FeedEvent) -> FeedItem {
    FeedItem {
        id,
        account,
        at: NOW as u64 - 3_600,
        event,
        reactions: ReactionCounts {
            nice: 0,
            gg: 3,
            wow: 0,
            same: 1,
        },
        mine: None,
    }
}

fn trophy(id: u64, account: Account, title: &str, number: u16) -> FeedItem {
    item(
        id,
        account,
        FeedEvent::Trophy {
            title_id: title.into(),
            number,
            grade: Grade::Gold,
            rarity: Some(2.3),
        },
    )
}

fn local() -> Local {
    let mut local = Local::default();
    local.games.insert(
        "GAME00001".into(),
        LocalGame {
            name: "Ember Drift".into(),
            icon: Some(TextureId::default()),
        },
    );
    local
        .trophy_names
        .insert(("GAME00001".into(), 4), "First Light".into());
    local
}

fn inputs<'a>(
    online: &'a Online,
    local: &'a Local,
    library: Option<&'a OwnLibrary>,
    account: u64,
) -> Inputs<'a> {
    Inputs {
        online,
        local,
        library,
        account: AccountId(account),
        language: Language::English,
        now: NOW,
        utc_offset: Some(0),
        unread: 0,
    }
}

fn ready(view: ProfileView) -> Box<Ready> {
    match view.body {
        Body::Ready(ready) => ready,
        other => panic!("expected a profile, got {other:?}"),
    }
}

fn own_game(title: &str, name: &str, seconds: u64, last: Option<i64>) -> OwnGame {
    OwnGame {
        title_id: title.into(),
        name: name.into(),
        art: None,
        seconds,
        last_played: last,
        trophies: None,
    }
}

#[test]
fn a_profile_that_is_not_there_is_loading_offline_or_failed() {
    let mut online = online();
    let local = local();
    let body = |online: &Online| view(&inputs(online, &local, None, 2)).body;
    assert!(matches!(body(&online), Body::Loading));
    online.cloud = Some(Cloud::Offline);
    assert!(matches!(body(&online), Body::Offline));
    online.cloud = Some(Cloud::Online);
    online.profiles.insert(
        AccountId(2),
        ProfileEntry {
            page: None,
            failed: true,
        },
    );
    assert!(matches!(body(&online), Body::Failed));
    // A page that was read once stays on screen when the next read fails.
    let mut known = page(who(2, "Lumen"));
    known.profile.about = Some("Hi".into());
    online.profiles.insert(
        AccountId(2),
        ProfileEntry {
            page: Some(known),
            failed: true,
        },
    );
    assert!(matches!(body(&online), Body::Ready(_)));
}

#[test]
fn a_stranger_with_nothing_shared_is_private_and_a_friend_is_just_new() {
    let mut online = online();
    let local = local();
    set(&mut online, page(who(12, "Tarn")));
    set(&mut online, page(who(2, "Lumen")));
    let stranger = view(&inputs(&online, &local, None, 12));
    assert!(matches!(stranger.body, Body::Private));
    assert_eq!(stranger.handle, "Tarn");
    assert!(matches!(
        view(&inputs(&online, &local, None, 2)).body,
        Body::Empty
    ));
}

#[test]
fn a_friend_shows_what_they_share_and_nothing_where_a_part_is_hidden() {
    let mut online = online();
    let local = local();
    let mut p = page(who(2, "Lumen"));
    p.profile.about = Some("Speedruns.".into());
    p.profile.favourites = Some(vec!["GAME00009".into()]);
    set(&mut online, p);
    let ready = ready(view(&inputs(&online, &local, None, 2)));
    assert_eq!(ready.relation, Relation::Friend);
    assert_eq!(ready.chip.as_deref(), Some("Friend"));
    assert_eq!(ready.status.as_ref().unwrap().text, "Online");
    assert_eq!(ready.about.as_deref(), Some("Speedruns."));
    // Hidden playtime and trophies: no hours, no trophies, no placeholder, and the one figure
    // that is left would only repeat the caption.
    assert!(ready.stats.is_empty());
    assert!(ready.trophies.rows.is_empty() && ready.trophies.empty.is_none());
    assert!(!ready.trophies.shown() && !ready.activity.shown());
    // A game the player does not have: its title ID, no art, nothing to open.
    let game = &ready.games.rows[0];
    assert_eq!(game.name, "GAME00009");
    assert!(game.art.is_none() && !game.owned);
    assert_eq!(game.detail, "Favourite · Not in your library");
    assert_eq!(game.press, Press::None);
}

#[test]
fn names_and_art_come_only_from_the_players_own_copy() {
    let mut online = online();
    let local = local();
    let mut p = page(who(2, "Lumen"));
    p.profile.playtime = Some(vec![
        played("GAME00002", 600),
        played("GAME00001", 125),
        played("GAME00003", 0),
    ]);
    p.profile.favourites = Some(vec!["GAME00002".into()]);
    set(&mut online, p);
    let ready = ready(view(&inputs(&online, &local, None, 2)));
    let names: Vec<&str> = ready.games.rows.iter().map(|g| g.name.as_str()).collect();
    // Favourites first, then by time.
    assert_eq!(names, ["GAME00002", "Ember Drift", "GAME00003"]);
    let owned = &ready.games.rows[1];
    assert!(owned.art.is_some() && owned.owned);
    assert_eq!(owned.trailing.as_deref(), Some("2 h 5 min"));
    assert_eq!(owned.press, Press::OpenGame("GAME00001".into()));
    assert!(ready.games.rows[2].trailing.is_none(), "no time, no fact");
    // Three titles, eleven hours: the figures.
    let figures: Vec<(&str, &str)> = ready
        .stats
        .iter()
        .map(|s| (s.figure.as_str(), s.label.as_str()))
        .collect();
    assert_eq!(figures, [("3", "Games"), ("12", "Hours")]);
}

#[test]
fn a_friends_trophies_come_from_their_feed_and_are_named_only_when_owned() {
    let mut online = online();
    let local = local();
    set(&mut online, page(who(2, "Lumen")));
    online.feed.merge_newest(Feed {
        items: vec![
            trophy(9, who(2, "Lumen"), "GAME00001", 4),
            trophy(8, who(2, "Lumen"), "GAME00001", 5),
            trophy(7, who(2, "Lumen"), "GAME00002", 1),
            trophy(6, who(3, "Juno"), "GAME00001", 4),
            item(
                5,
                who(2, "Lumen"),
                FeedEvent::FirstPlayed {
                    title_id: "GAME00002".into(),
                },
            ),
        ],
        next: None,
    });
    let ready = ready(view(&inputs(&online, &local, None, 2)));
    let rows: Vec<(&str, &str)> = ready
        .trophies
        .rows
        .iter()
        .map(|r| (r.title.as_str(), r.detail.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("First Light", "Gold · Ember Drift · 1 h ago · Rare, 2.3%"),
            ("Gold trophy", "Ember Drift · 1 h ago · Rare, 2.3%"),
            ("Gold trophy", "In a game · 1 h ago · Rare, 2.3%"),
        ]
    );
    assert_eq!(ready.trophies.rows[0].press, Press::OpenFeed(9));
    // The activity is their other items, not the trophies, and not Juno's.
    assert_eq!(ready.activity.rows.len(), 1);
    assert!(ready.activity.rows[0].text.starts_with("Lumen "));
    assert_eq!(ready.activity.rows[0].reactions, "GG 3 · Same 1");
}

#[test]
fn a_friend_says_what_they_play_without_promising_a_join_and_what_you_share() {
    let mut online = online();
    let local = local();
    online.friends.friends[0].presence.activity = Some(anyps5_core::online::api::Activity {
        title_id: "GAME00001".into(),
        title: "Ember Drift".into(),
        version: "01.000".into(),
        session: Some(anyps5_core::online::api::SessionRef {
            id: "s".into(),
            joinable: true,
            members: 1,
            max_members: 4,
        }),
    });
    let mut p = page(who(2, "Lumen"));
    p.profile.playtime = Some(vec![played("GAME00001", 90), played("GAME00002", 90)]);
    set(&mut online, p);
    let ready = ready(view(&inputs(&online, &local, None, 2)));
    assert_eq!(ready.status.as_ref().unwrap().text, "Playing Ember Drift");
    assert_eq!(ready.note.as_deref(), Some("You have 1 of their games"));
}

#[test]
fn a_request_shows_its_chip_and_the_button_that_answers_it() {
    let mut online = online();
    let local = local();
    let mut p = page(who(6, "Vale"));
    p.profile.about = Some("Hello".into());
    set(&mut online, p);
    let ready = ready(view(&inputs(&online, &local, None, 6)));
    assert_eq!(ready.relation, Relation::Invited);
    assert_eq!(ready.chip.as_deref(), Some("Wants to be friends"));
    assert_eq!(
        ready
            .actions
            .iter()
            .map(|a| a.press.clone())
            .collect::<Vec<_>>(),
        [Press::AnswerRequest(AccountId(6), true)]
    );
    let mut p = page(who(7, "Ash"));
    p.profile.about = Some("Hello".into());
    set(&mut online, p);
    let ready = self::ready(view(&inputs(&online, &local, None, 7)));
    assert_eq!(ready.chip.as_deref(), Some("Request sent"));
    assert!(ready.actions.is_empty());
    // A stranger can be added, when the cloud answers.
    let mut p = page(who(12, "Tarn"));
    p.profile.about = Some("Hello".into());
    set(&mut online, p);
    let stranger = self::ready(view(&inputs(&online, &local, None, 12)));
    assert_eq!(stranger.chip, None);
    assert_eq!(
        stranger
            .actions
            .iter()
            .map(|a| a.press.clone())
            .collect::<Vec<_>>(),
        [Press::AddFriend("Tarn".into())]
    );
}

#[test]
fn the_own_profile_comes_from_this_pc() {
    let mut online = online();
    let local = local();
    let mut p = page(who(1, "Mira"));
    p.profile.about = Some("Co-op.".into());
    set(&mut online, p);
    let library = OwnLibrary {
        games: vec![
            own_game("GAME00002", "Paper Moon", 3 * 3_600 + 600, None),
            OwnGame {
                trophies: Some((9, 40)),
                ..own_game("GAME00001", "Ember Drift", 42 * 3_600, Some(NOW - 7_200))
            },
            own_game("GAME00003", "Cinder Run", 0, None),
        ],
        trophies: vec![OwnTrophy {
            title_id: "GAME00001".into(),
            game: "Ember Drift".into(),
            name: "First Light".into(),
            grade: crate::ui::theme::Grade::Gold,
            at: NOW - 600,
        }],
        grades: [0, 1, 0, 2],
    };
    let ready = ready(view(&inputs(&online, &local, Some(&library), 1)));
    assert_eq!(ready.relation, Relation::Own);
    assert_eq!(
        ready
            .actions
            .iter()
            .map(|a| a.press.clone())
            .collect::<Vec<_>>(),
        [Press::EditProfile, Press::FriendsOnline]
    );
    let figures: Vec<(&str, &str)> = ready
        .stats
        .iter()
        .map(|s| (s.figure.as_str(), s.label.as_str()))
        .collect();
    assert_eq!(
        figures,
        [("3", "Games"), ("45", "Hours"), ("3", "Trophies")]
    );
    assert_eq!(ready.stats[2].grades, Some([0, 1, 0, 2]));
    // Last played first, then the most time, the unplayed last.
    let names: Vec<&str> = ready.games.rows.iter().map(|g| g.name.as_str()).collect();
    assert_eq!(names, ["Ember Drift", "Paper Moon", "Cinder Run"]);
    let first = &ready.games.rows[0];
    assert_eq!(first.detail, "42 h · Last played 2 h ago");
    assert_eq!(first.trailing.as_deref(), Some("9/40"));
    assert_eq!(first.progress, Some(0.225));
    assert_eq!(ready.games.rows[2].detail, "Not played yet");
    assert_eq!(ready.trophies.rows[0].title, "First Light");
    assert_eq!(
        ready.trophies.rows[0].detail,
        "Gold · Ember Drift · 10 min ago"
    );
}

#[test]
fn the_own_profile_names_what_an_empty_section_waits_for_and_a_friends_stays_out() {
    let mut online = online();
    let local = local();
    set(&mut online, page(who(1, "Mira")));
    let library = OwnLibrary::default();
    let own = ready(view(&inputs(&online, &local, Some(&library), 1)));
    assert!(own.games.empty.is_some() && own.trophies.empty.is_some());
    assert!(own.activity.empty.is_some());
    assert!(own.games.shown() && own.trophies.shown() && own.activity.shown());
    // A new player has no figures: zeros say nothing.
    assert!(own.stats.is_empty());
}

#[test]
fn the_own_profile_says_when_the_cloud_is_out_of_reach_and_counts_unread_messages() {
    let mut online = online();
    let local = local();
    online.cloud = Some(Cloud::Offline);
    let library = OwnLibrary::default();
    let mut input = inputs(&online, &local, Some(&library), 1);
    input.unread = 3;
    let ready = ready(view(&input));
    assert!(
        ready
            .note
            .as_deref()
            .unwrap()
            .starts_with("You are offline")
    );
    assert_eq!(ready.actions[1].label, "Friends (3)");
    // The own profile never waits for the cloud: it is on this PC.
    assert!(ready.games.shown());
}

#[test]
fn stamps_and_times_read_as_the_launcher_writes_them() {
    assert_eq!(parse_stamp("2026-10-22T09:30:00Z"), Some(1_792_661_400));
    assert_eq!(parse_stamp("1970-01-01T00:00:01Z"), None, "before 2000");
    assert_eq!(parse_stamp("nonsense"), None);
    let en = Language::English;
    assert_eq!(playtime_text(35, en), "35 min");
    assert_eq!(playtime_text(260, en), "4 h 20 min");
    assert_eq!(playtime_text(240, en), "4 h");
    assert_eq!(playtime_text(60 * 12 + 30, en), "12 h");
    assert_eq!(playtime_text(60 * 12 + 30, Language::Turkish), "12 sa");
}
