use anyps5_core::online::api::{Account, AccountId, Party, PartyMember};
use anyps5_core::online::{Cloud, Online, SignedIn, Status};
use egui::Color32;

use super::*;
use crate::ui::i18n::Language;

fn account(id: u64, handle: &str) -> Account {
    Account {
        id: AccountId(id),
        handle: handle.into(),
    }
}

fn online(members: &[(u64, &str)], leader: u64) -> Online {
    let mut online = Online::default();
    online.status = Status::Running;
    online.cloud = Some(Cloud::Online);
    online.account = Some(SignedIn {
        id: AccountId(1),
        handle: "Mira".into(),
        email: "mira@example.org".into(),
        appear_offline: false,
    });
    online.social.apply_party(Some(Party {
        id: "p".into(),
        leader: AccountId(leader),
        members: members
            .iter()
            .map(|(id, handle)| PartyMember {
                account: account(*id, handle),
                joined: 0,
                endpoint: String::new(),
                relay: None,
                voice: false,
            })
            .collect(),
        max_members: 8,
        conversation: "c".into(),
        created: 0,
        revision: 1,
        discord: None,
    }));
    online
}

fn data(online: &Online) -> OnlineData<'_> {
    OnlineData {
        online,
        cloud: Some("https://cloud.example"),
        custom_cloud: false,
        official_cloud: true,
        relay_only: false,
        language: Language::English,
        accent: Color32::from_rgb(90, 140, 255),
        now: 0,
        utc_offset: Some(0),
        game_running: false,
        play_together: None,
    }
}

#[test]
fn a_party_of_one_starts_on_inviting_and_has_one_primary_action() {
    let online = online(&[(1, "Mira")], 1);
    let room = Room::new(&data(&online));
    assert_eq!(room.primary, Some(Stop::Invite));
    let plan = room.plan(4);
    assert_eq!(plan.rows[0], [Stop::Member(0), Stop::Seat]);
    let start = plan.start();
    assert_eq!(plan.stop(start), Some(Stop::Invite));
}

#[test]
fn members_stand_in_rows_of_the_grid_and_the_focus_walks_them() {
    let online = online(
        &[
            (1, "Mira"),
            (2, "Lumen"),
            (3, "Kit"),
            (4, "Juno"),
            (5, "Rook"),
        ],
        1,
    );
    let room = Room::new(&data(&online));
    let plan = room.plan(4);
    assert_eq!(plan.rows[0].len(), 4);
    assert_eq!(plan.rows[1][..2], [Stop::Member(4), Stop::Seat]);
    let at = |row, col| RoomFocus { row, col };
    // Down from the last column of the first row lands on the second row's last stop.
    assert_eq!(
        step(&plan, at(0, 3), Command::Down),
        Step::Moved(at(1, 1), true)
    );
    // Left at the start stays: the screen takes it to the rail.
    assert_eq!(
        step(&plan, at(0, 0), Command::Left),
        Step::Moved(at(0, 0), false)
    );
    // Right at the end of a row stays.
    let last = plan.rows[0].len() - 1;
    assert_eq!(
        step(&plan, at(0, last), Command::Right),
        Step::Moved(at(0, last), false)
    );
    assert_eq!(
        step(&plan, at(0, 1), Command::Select),
        Step::Press(Stop::Member(1))
    );
    assert_eq!(step(&plan, at(0, 1), Command::Back), Step::Back);
}

#[test]
fn only_the_leader_has_the_seat_and_the_invite_button() {
    let online = online(&[(2, "Lumen"), (1, "Mira")], 2);
    let room = Room::new(&data(&online));
    assert!(!room.seat);
    assert!(room.actions.iter().all(|b| b.stop != Stop::Invite));
    assert_eq!(room.primary, None);
}

#[test]
fn voice_off_offers_to_turn_it_on_and_voice_on_offers_mute_and_deafen() {
    let mut online = online(&[(1, "Mira"), (2, "Lumen")], 1);
    let room = Room::new(&data(&online));
    assert_eq!(room.voice[0].stop, Stop::VoiceOn);
    online.voice.load(anyps5_core::online::VoicePrefs {
        enabled: true,
        ..Default::default()
    });
    let room = Room::new(&data(&online));
    let stops: Vec<Stop> = room.voice.iter().map(|b| b.stop).collect();
    assert_eq!(stops, [Stop::Mute, Stop::Deafen, Stop::VoiceSettings]);
    online.voice.muted = true;
    online.voice.deafened = true;
    let room = Room::new(&data(&online));
    assert_eq!(room.tiles[0].mark, Some(Mark::Deafened));
    assert_eq!(room.tiles[0].state, "You are deafened");
    assert_eq!(room.voice[0].label, "Unmute");
    assert_eq!(room.voice[1].label, "Undeafen");
}

#[test]
fn without_a_party_the_page_has_one_primary_action() {
    let mut online = online(&[(1, "Mira")], 1);
    online.social.apply_party(None);
    let room = Room::new(&data(&online));
    assert!(!room.in_party);
    assert_eq!(room.primary, Some(Stop::Start));
    let plan = room.plan(4);
    assert_eq!(plan.rows[0], [Stop::Start]);
    assert_eq!(plan.start(), RoomFocus { row: 0, col: 0 });
}

#[test]
fn a_focus_kept_from_a_bigger_party_is_clamped() {
    let online = online(&[(1, "Mira")], 1);
    let plan = Room::new(&data(&online)).plan(4);
    let clamped = plan.clamp(RoomFocus { row: 9, col: 9 });
    assert!(plan.stop(clamped).is_some());
}

#[test]
fn play_together_is_the_primary_card_of_a_party_with_company() {
    let online = online(&[(1, "Mira"), (2, "Lumen")], 1);
    let mut data = data(&online);
    data.play_together = Some("Ember Drift");
    let room = Room::new(&data);
    assert!(matches!(
        room.cards[0].action,
        Some((_, CardAction::PlayTogether))
    ));
    assert_eq!(room.primary, Some(Stop::Card(0)));
    let plan = room.plan(5);
    assert_eq!(plan.stop(plan.start()), Some(Stop::Card(0)));
}

#[test]
fn leaving_is_the_last_stop_and_the_only_one_that_ends_something() {
    let online = online(&[(1, "Mira"), (2, "Lumen")], 1);
    let room = Room::new(&data(&online));
    // The page draws it alone at the far end; the focus still reaches it last, after the others.
    let last = room.actions.last().expect("the party has actions");
    assert_eq!(last.stop, Stop::Leave);
    assert!(last.destructive);
    let ending: Vec<Stop> = room
        .voice
        .iter()
        .chain(&room.actions)
        .filter(|b| b.destructive)
        .map(|b| b.stop)
        .collect();
    assert_eq!(ending, [Stop::Leave]);
    // The one that ends something is never the primary action.
    assert_ne!(room.primary, Some(Stop::Leave));
}
