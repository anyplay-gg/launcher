use anyps5_core::online::api::{
    Account, Count, Friend, FriendLists, Grade, Presence, Tally, TrophyCompare,
};
use anyps5_core::online::{Cloud, CompareEntry, SignedIn, Status};
use anyps5_core::trophies::{Grade as CoreGrade, Trophy as FileTrophy};

use super::*;
use crate::screens::online::OwnedGame;

const TITLE: &str = "GAME00001";
const AT: i64 = 1_792_238_400;

fn online() -> Online {
    let mut online = Online::default();
    online.status = Status::Running;
    online.cloud = Some(Cloud::Online);
    online.account = Some(SignedIn {
        id: AccountId(1),
        handle: "Mira".into(),
        email: String::new(),
        appear_offline: false,
    });
    online.friends = FriendLists {
        friends: vec![Friend {
            account: Account {
                id: AccountId(2),
                handle: "Lumen".into(),
            },
            since: 0,
            presence: Presence {
                online: true,
                activity: None,
                revision: 0,
            },
        }],
        ..FriendLists::default()
    };
    online
}

fn record(number: u16, grade: Grade, at: i64) -> Record {
    Record {
        number,
        grade,
        earned: at as u64,
    }
}

fn side(account: u64, records: Vec<Record>) -> CompareSide {
    let mut counts = Count::default();
    for r in &records {
        match r.grade {
            Grade::Platinum => counts.platinum += 1,
            Grade::Gold => counts.gold += 1,
            Grade::Silver => counts.silver += 1,
            Grade::Bronze => counts.bronze += 1,
        }
    }
    let tally = Tally::of(counts);
    CompareSide {
        account,
        trophies: records,
        title: tally,
        total: tally,
        level: TrophyLevel::of(tally.points),
    }
}

fn with_answer(online: &mut Online, you: Vec<Record>, them: Vec<Record>) {
    online.compares.insert(
        (AccountId(2), TITLE.to_owned()),
        CompareEntry {
            data: Some(TrophyCompare {
                title_id: TITLE.into(),
                you: side(1, you),
                them: side(2, them),
            }),
            ..CompareEntry::default()
        },
    );
}

fn file_trophy(number: u32, grade: CoreGrade, hidden: bool, unlocked: Option<i64>) -> FileTrophy {
    FileTrophy {
        service_label: 0,
        number,
        grade,
        hidden,
        name: format!("Name {number}"),
        detail: format!("Detail {number}"),
        icon: None,
        unlocked: unlocked.map(|s| s * 1_000_000),
        progress: None,
    }
}

fn set() -> TrophySet {
    TrophySet {
        title: "Ember Drift".into(),
        icon: None,
        trophies: vec![
            file_trophy(0, CoreGrade::Platinum, false, None),
            file_trophy(1, CoreGrade::Bronze, false, Some(AT)),
            file_trophy(2, CoreGrade::Silver, false, None),
            file_trophy(3, CoreGrade::Gold, true, None),
        ],
    }
}

fn local() -> Local {
    let mut local = Local::default();
    local.games.insert(
        TITLE.into(),
        OwnedGame {
            name: "Ember Drift".into(),
            icon: None,
        },
    );
    local
}

fn inputs<'a>(
    online: &'a Online,
    local: &'a Local,
    set: Option<&'a TrophySet>,
    revealed: &'a HashSet<u32>,
) -> Inputs<'a> {
    Inputs {
        online,
        local,
        set,
        account: AccountId(2),
        title_id: TITLE,
        show_hidden: false,
        revealed,
        language: Language::English,
        utc_offset: Some(0),
    }
}

fn ready(view: CompareView) -> Ready {
    match view.body {
        Body::Ready(ready) => *ready,
        other => panic!("not ready: {other:?}"),
    }
}

#[test]
fn rows_follow_the_players_own_trophy_file() {
    let mut online = online();
    with_answer(
        &mut online,
        vec![record(1, Grade::Bronze, AT)],
        vec![
            record(1, Grade::Bronze, AT + 86_400),
            record(2, Grade::Silver, AT + 90_000),
        ],
    );
    let (local, set, revealed) = (local(), set(), HashSet::new());
    let view = view(&inputs(&online, &local, Some(&set), &revealed));
    assert_eq!(view.game, "Ember Drift");
    assert_eq!(view.handle, "Lumen");
    let ready = ready(view);
    assert!(ready.note.is_none(), "the game is in the library");
    let names: Vec<&str> = ready.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Name 0", "Name 1", "Name 2", "Hidden Trophy"]);
    assert_eq!(ready.rows[1].you, Cell::Earned("Oct 17, 2026".into()));
    assert_eq!(ready.rows[1].them, Cell::Earned("Oct 18, 2026".into()));
    assert_eq!(ready.rows[2].you, Cell::NotYet);
    assert_eq!(ready.rows[2].them, Cell::Earned("Oct 18, 2026".into()));
    assert_eq!(ready.rows[1].detail, "Bronze · Detail 1");
    assert_eq!(ready.rows[1].press, Press::None);
}

#[test]
fn a_secret_stays_masked_until_shown() {
    let mut online = online();
    with_answer(&mut online, Vec::new(), Vec::new());
    let (local, set) = (local(), set());
    let mut revealed = HashSet::new();
    let masked = ready(view(&inputs(&online, &local, Some(&set), &revealed)));
    let row = &masked.rows[3];
    assert!(row.secret);
    assert_eq!(row.name, "Hidden Trophy");
    assert_eq!(row.detail, "Shows what it is once you earn it.");
    assert_eq!(row.press, Press::Reveal(3));
    assert!(!row.name.contains("Name 3") && !row.label.contains("Name 3"));

    revealed.insert(3);
    let shown = ready(view(&inputs(&online, &local, Some(&set), &revealed)));
    assert!(!shown.rows[3].secret);
    assert_eq!(shown.rows[3].name, "Name 3");
    assert_eq!(shown.rows[3].press, Press::None);

    // "Always Show Hidden Trophies" shows it too.
    let none = HashSet::new();
    let mut everything = inputs(&online, &local, Some(&set), &none);
    everything.show_hidden = true;
    assert!(!ready(view(&everything)).rows[3].secret);
}

#[test]
fn a_secret_the_player_earned_is_not_one() {
    let mut online = online();
    with_answer(&mut online, vec![record(3, Grade::Gold, AT)], Vec::new());
    let (local, set, revealed) = (local(), set(), HashSet::new());
    let ready = ready(view(&inputs(&online, &local, Some(&set), &revealed)));
    assert!(!ready.rows[3].secret);
    assert_eq!(ready.rows[3].name, "Name 3");
}

#[test]
fn a_game_the_player_does_not_have_shows_numbers_and_says_so_quietly() {
    let mut online = online();
    with_answer(
        &mut online,
        vec![record(4, Grade::Gold, AT)],
        vec![record(2, Grade::Silver, AT), record(4, Grade::Gold, AT)],
    );
    let (mut local, revealed) = (local(), HashSet::new());
    local.games.clear();
    let view = view(&inputs(&online, &local, None, &revealed));
    assert_eq!(view.game, TITLE, "the title ID stands in for the name");
    let ready = ready(view);
    let names: Vec<&str> = ready.rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Trophy 2", "Trophy 4"]);
    assert_eq!(ready.rows[0].detail, "Silver");
    assert_eq!(ready.rows[0].you, Cell::NotYet);
    assert_eq!(ready.rows[1].you, Cell::Earned("Oct 17, 2026".into()));
    assert!(ready.note.is_some());
    assert!(ready.rows.iter().all(|r| r.index.is_none() && !r.secret));
}

#[test]
fn the_sides_carry_the_clouds_numbers_and_nothing_else() {
    let mut online = online();
    with_answer(
        &mut online,
        vec![record(1, Grade::Bronze, AT), record(3, Grade::Gold, AT)],
        vec![record(1, Grade::Bronze, AT)],
    );
    let (local, set, revealed) = (local(), set(), HashSet::new());
    let ready = ready(view(&inputs(&online, &local, Some(&set), &revealed)));
    // 10 + 60 points for the player, 10 for Lumen: level 1 for both, no more.
    assert_eq!(ready.you.name, "You");
    assert_eq!(ready.you.grades, [0, 1, 0, 1]);
    assert_eq!(ready.you.points_text, "70 points");
    assert_eq!(ready.you.game_points_text, "70 points in this game");
    assert_eq!(ready.you.level_text, "Level 1");
    assert_eq!(ready.them.name, "Lumen");
    assert_eq!(ready.them.grades, [0, 0, 0, 1]);
    assert_eq!(ready.them.points_text, "10 points");
    assert!((ready.you.fraction - 70.0 / 72.0).abs() < 1e-6);
}

#[test]
fn nothing_yet_is_said_when_there_is_nothing_to_list() {
    let mut online = online();
    with_answer(&mut online, Vec::new(), Vec::new());
    let (local, revealed) = (local(), HashSet::new());
    let view = view(&inputs(&online, &local, None, &revealed));
    assert!(matches!(view.body, Body::Empty));
}

#[test]
fn the_states_before_an_answer() {
    let (local, revealed) = (local(), HashSet::new());
    let mut online = online();
    let body = |online: &Online| view(&inputs(online, &local, None, &revealed)).body;
    assert!(matches!(body(&online), Body::Loading));

    online.compares.insert(
        (AccountId(2), TITLE.to_owned()),
        CompareEntry {
            failed: true,
            ..CompareEntry::default()
        },
    );
    assert!(matches!(body(&online), Body::Failed));

    online.compares.insert(
        (AccountId(2), TITLE.to_owned()),
        CompareEntry {
            hidden: true,
            ..CompareEntry::default()
        },
    );
    assert!(matches!(body(&online), Body::Hidden));

    online.compares.clear();
    online.cloud = Some(Cloud::Offline);
    assert!(matches!(body(&online), Body::Offline));
}

#[test]
fn an_answer_that_is_shown_stays_when_the_cloud_goes_away() {
    let mut online = online();
    with_answer(&mut online, vec![record(1, Grade::Bronze, AT)], Vec::new());
    online.cloud = Some(Cloud::Offline);
    let (local, set, revealed) = (local(), set(), HashSet::new());
    assert!(matches!(
        view(&inputs(&online, &local, Some(&set), &revealed)).body,
        Body::Ready(_)
    ));
}

#[test]
fn a_friend_who_hides_their_trophies_wins_over_an_older_answer() {
    let mut online = online();
    with_answer(&mut online, Vec::new(), Vec::new());
    online
        .compares
        .get_mut(&(AccountId(2), TITLE.to_owned()))
        .unwrap()
        .hidden = true;
    let (local, set, revealed) = (local(), set(), HashSet::new());
    assert!(matches!(
        view(&inputs(&online, &local, Some(&set), &revealed)).body,
        Body::Hidden
    ));
}

#[test]
fn signing_out_leaves_nothing_to_show() {
    let mut online = online();
    with_answer(&mut online, Vec::new(), Vec::new());
    online.account = None;
    let (local, set, revealed) = (local(), set(), HashSet::new());
    assert!(matches!(
        view(&inputs(&online, &local, Some(&set), &revealed)).body,
        Body::Offline
    ));
}
