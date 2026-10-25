use anyps5_core::online::api::{Account, AccountId, Invitation, PartyInvitation, Request};
use anyps5_core::online::{Cloud, Online, SignedIn, Status};
use egui::Color32;

use super::*;

fn account(id: u64, handle: &str) -> Account {
    Account {
        id: AccountId(id),
        handle: handle.into(),
    }
}

fn signed_in() -> Online {
    let mut online = Online::default();
    online.status = Status::Running;
    online.cloud = Some(Cloud::Online);
    online.account = Some(SignedIn {
        id: AccountId(1),
        handle: "Mira".into(),
        email: "mira@example.org".into(),
    });
    online
}

fn data(online: &Online) -> OnlineData<'_> {
    OnlineData {
        online,
        cloud: Some("https://cloud.example.org"),
        custom_cloud: false,
        official_cloud: true,
        relay_only: false,
        language: Language::English,
        accent: Color32::from_rgb(0, 112, 209),
        now: 0,
        utc_offset: Some(0),
        game_running: false,
    }
}

#[test]
fn the_rail_has_the_sections_then_your_profile() {
    let online = signed_in();
    let entries = rail_entries(&data(&online), Some(Section::Party), Language::English);
    let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Friends",
            "Party",
            "Messages",
            "Activity",
            "Settings",
            "Your Profile"
        ]
    );
    assert_eq!(entries.len(), RAIL);
    // Only the section on screen is marked, and only the last entry is an action.
    let current: Vec<bool> = entries.iter().map(|e| e.current).collect();
    assert_eq!(current, [false, true, false, false, false, false]);
    let actions: Vec<bool> = entries.iter().map(|e| e.action).collect();
    assert_eq!(actions, [false, false, false, false, false, true]);
    assert_eq!(PROFILE, entries.len() - 1);
}

#[test]
fn the_rail_counts_what_waits_in_each_section() {
    let mut online = signed_in();
    online.friends.incoming = vec![
        Request {
            account: account(2, "Lumen"),
            since: 0,
        },
        Request {
            account: account(3, "Juno"),
            since: 0,
        },
    ];
    online.invitations = vec![Invitation {
        id: "i".into(),
        from: account(4, "Kit"),
        session: "s".into(),
        title_id: "TEST00001".into(),
        title: "Test Game".into(),
        version: "01.000".into(),
        expires: 0,
    }];
    online.social.add_party_invitation(PartyInvitation {
        id: "pi".into(),
        from: account(5, "Rook"),
        party: "p".into(),
        members: 2,
        expires: 0,
        call: false,
    });
    let entries = rail_entries(&data(&online), None, Language::English);
    let waiting: Vec<u32> = entries.iter().map(|e| e.waiting).collect();
    // Requests and invitations on Friends, the party's invitation on Party.
    assert_eq!(waiting, [3, 1, 0, 0, 0, 0]);
    assert_eq!(entries[0].name, "Friends, 3 new");
    assert_eq!(entries[2].name, "Messages", "nothing waits: just the name");
}

#[test]
fn the_rail_speaks_turkish_too() {
    let online = signed_in();
    let entries = rail_entries(&data(&online), None, Language::Turkish);
    assert_eq!(entries[0].label, "Arkadaşlar");
    assert_eq!(entries[4].label, "Ayarlar");
}

#[test]
fn a_section_has_one_place_in_the_rail() {
    for (n, section) in Section::ALL.into_iter().enumerate() {
        assert_eq!(index_of(section), n);
    }
}

#[test]
fn the_screen_opens_on_the_page_with_the_rail_on_its_section() {
    let online = signed_in();
    let mut screen = FriendsScreen::default();
    screen.show_page(Page::Voice, &data(&online));
    assert_eq!(screen.focus().zone, Zone::Page);
    assert_eq!(screen.focus().rail, index_of(Section::Party));
    screen.open(&data(&online));
    assert_eq!(screen.focus().rail, index_of(Section::Friends));
    assert_eq!(screen.page(), &Page::Home);
}

#[test]
fn back_is_the_pages_and_leaves_only_from_the_first_page_of_a_section() {
    let online = signed_in();
    let mut screen = FriendsScreen::default();
    screen.open(&data(&online));
    assert!(screen.panel.back_closes(&data(&online)));
    screen.show_page(Page::Friend(AccountId(2)), &data(&online));
    assert!(!screen.panel.back_closes(&data(&online)));
}
