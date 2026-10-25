use anyps5_core::online::api::{Account, Party, PartyMember};
use anyps5_core::online::control::{
    Event, ScreenHosting, ScreenOffer, ScreenPhase, ScreenStats, ScreenViewer, ScreenWatching,
};
use anyps5_core::online::{Cloud, Line, Output, SignedIn, Status};
use egui::Color32;

use super::*;
use crate::screens::online::Page;
use crate::screens::rows::Control;

fn account(id: u64, handle: &str) -> Account {
    Account {
        id: AccountId(id),
        handle: handle.into(),
    }
}

fn online() -> Online {
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
        leader: AccountId(1),
        members: [(1, "Mira"), (2, "Lumen"), (3, "Kit")]
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
        conversation: "pc".into(),
        created: 0,
        revision: 1,
        discord: None,
    }));
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
        accent: Color32::WHITE,
        now: 1_792_670_400,
        utc_offset: Some(0),
        game_running: true,
    }
}

fn offer(id: u64, title: &str) -> ScreenOffer {
    ScreenOffer {
        account: AccountId(id),
        title: title.into(),
        link: None,
    }
}

fn viewer(id: u64, watching: bool, link: Link, rtt: u32, loss: u8) -> ScreenViewer {
    ScreenViewer {
        account: AccountId(id),
        watching,
        link: Some(link),
        rtt_ms: Some(rtt),
        loss_percent: loss,
    }
}

fn watching(id: u64, phase: ScreenPhase, link: Option<Link>, rtt: Option<u32>) -> ScreenWatching {
    ScreenWatching {
        account: AccountId(id),
        title: "Astro Bot".into(),
        phase,
        link,
        rtt_ms: rtt,
        stats: ScreenStats::default(),
        shown_fps: 0,
    }
}

fn hosting(phase: ScreenPhase, viewers: Vec<ScreenViewer>) -> ScreenHosting {
    ScreenHosting {
        phase,
        title: "Astro Bot".into(),
        codec: None,
        stats: ScreenStats::default(),
        viewers,
    }
}

fn report(
    online: &mut Online,
    can_share: bool,
    can_watch: bool,
    hosting: Option<ScreenHosting>,
    offers: Vec<ScreenOffer>,
    watching: Option<ScreenWatching>,
) {
    online.apply(Line::Output(Output::Event(Event::Screen {
        can_share,
        can_watch,
        hosting,
        offers,
        watching,
    })));
}

fn rows_of(online: &Online, page: Page) -> Vec<Row> {
    let mut panel = OnlinePanel::default();
    panel.go(page, &data(online));
    panel.rows(&data(online)).2
}

fn party_rows(online: &Online) -> Vec<Row> {
    rows_of(online, Page::Party)
}

fn ids(rows: &[Row]) -> Vec<&'static str> {
    rows.iter().map(|r| r.id).collect()
}

fn row<'a>(rows: &'a [Row], id: &str) -> &'a Row {
    rows.iter().find(|r| r.id == id).expect(id)
}

/// Presses the `nth` row with this id on the party page and returns what the panel asks for.
fn press(online: &Online, id: &'static str, nth: usize) -> Vec<OnlineAction> {
    let mut panel = OnlinePanel::default();
    panel.go(Page::Party, &data(online));
    let rows = panel.rows(&data(online)).2;
    panel.side.nav.row = crate::screens::rows::position(&rows, id).unwrap() + nth;
    let mut actions = Vec::new();
    panel.run(id, &data(online), &mut actions);
    actions
}

#[test]
fn nothing_shows_while_nothing_is_possible_and_nothing_is_shared() {
    let mut online = online();
    let before = party_rows(&online);
    report(&mut online, false, false, None, vec![], None);
    assert_eq!(party_rows(&online), before);
    // Able to watch, but no stream: still no caption over an empty group.
    report(&mut online, false, true, None, vec![], None);
    assert_eq!(party_rows(&online), before);
    assert!(!ids(&before).contains(&"screenCaption"));
    assert!(!ids(&rows_of(&online, Page::Home)).contains(&"screenShare"));
}

#[test]
fn a_game_that_runs_offers_one_button_between_voice_and_members() {
    let mut online = online();
    report(&mut online, true, true, None, vec![], None);
    let rows = party_rows(&online);
    let order = ids(&rows);
    let caption = order.iter().position(|i| *i == "screenCaption").unwrap();
    assert_eq!(order[caption - 1], "voiceSettings");
    assert_eq!(order[caption + 1], "shareScreen");
    assert_eq!(order[caption + 2], "membersCaption");
    let button = row(&rows, "shareScreen");
    assert_eq!(button.title, "Share Screen");
    assert_eq!(
        button.detail.as_deref(),
        Some("Show your game to the party")
    );
    assert_eq!(
        press(&online, "shareScreen", 0),
        [OnlineAction::Screen(ScreenCommand::Start)]
    );
}

#[test]
fn an_offer_is_one_quiet_row_with_a_watch_button() {
    let mut online = online();
    report(
        &mut online,
        false,
        true,
        None,
        vec![offer(2, "Astro Bot")],
        None,
    );
    let rows = party_rows(&online);
    let watch = row(&rows, "watchScreen");
    assert_eq!(watch.title, "Watch Lumen");
    assert_eq!(watch.detail.as_deref(), Some("Sharing Astro Bot"));
    assert!(!ids(&rows).contains(&"shareScreen"));
    assert_eq!(
        press(&online, "watchScreen", 0),
        [OnlineAction::Screen(ScreenCommand::Watch(AccountId(2)))]
    );
}

#[test]
fn each_sharer_has_a_row_and_the_button_watches_that_one() {
    let mut online = online();
    report(
        &mut online,
        false,
        true,
        None,
        vec![offer(2, "Astro Bot"), offer(3, "Paper Moon")],
        None,
    );
    let rows = party_rows(&online);
    let titles: Vec<_> = rows
        .iter()
        .filter(|r| r.id == "watchScreen")
        .map(|r| r.title.as_str())
        .collect();
    assert_eq!(titles, ["Watch Lumen", "Watch Kit"]);
    assert_eq!(
        press(&online, "watchScreen", 1),
        [OnlineAction::Screen(ScreenCommand::Watch(AccountId(3)))]
    );
}

#[test]
fn a_build_that_cannot_show_a_stream_says_so_instead_of_offering_it() {
    let mut online = online();
    report(
        &mut online,
        false,
        false,
        None,
        vec![offer(2, "Astro Bot")],
        None,
    );
    let rows = party_rows(&online);
    assert!(!ids(&rows).contains(&"watchScreen"));
    assert_eq!(
        row(&rows, "screenOffer").title,
        "Lumen is sharing Astro Bot"
    );
    assert_eq!(
        row(&rows, "screenOfferNote").title,
        "This version cannot show it yet."
    );
    assert!(row(&rows, "screenOfferNote").attached);
}

#[test]
fn the_host_sees_who_watches_and_how_in_words() {
    let mut online = online();
    let live = hosting(
        ScreenPhase::Live,
        vec![
            viewer(2, true, Link::Direct, 24, 0),
            viewer(3, true, Link::Relay, 80, 9),
        ],
    );
    report(&mut online, true, true, Some(live), vec![], None);
    let rows = party_rows(&online);
    assert_eq!(
        row(&rows, "screenShared").title,
        "Your screen is shared with 2 people"
    );
    let game = row(&rows, "screenGame");
    assert_eq!(game.title, "Astro Bot");
    assert_eq!(game.control, Control::Info(Some("Live".into())));
    let viewers: Vec<_> = rows
        .iter()
        .filter(|r| r.id == "screenViewer")
        .map(|r| (r.title.as_str(), r.control.clone(), r.detail.as_deref()))
        .collect();
    assert_eq!(
        viewers,
        [
            ("Lumen", Control::Info(Some("Direct, 24 ms".into())), None),
            (
                "Kit",
                Control::Info(Some("Through a relay, 80 ms".into())),
                Some("Some of the picture is lost")
            ),
        ]
    );
    // One way to end it, and no second button to start another.
    assert!(!ids(&rows).contains(&"shareScreen"));
    assert_eq!(row(&rows, "stopSharing").title, "Stop Sharing");
    assert_eq!(
        press(&online, "stopSharing", 0),
        [OnlineAction::Screen(ScreenCommand::Stop)]
    );
}

#[test]
fn the_host_hears_of_nobody_one_viewer_and_a_member_who_only_knows() {
    let mut online = online();
    let starting = hosting(
        ScreenPhase::Connecting,
        vec![viewer(2, false, Link::Direct, 20, 0)],
    );
    report(&mut online, true, true, Some(starting), vec![], None);
    let rows = party_rows(&online);
    assert_eq!(row(&rows, "screenShared").title, "Your screen is shared");
    assert_eq!(
        row(&rows, "screenGame").control,
        Control::Info(Some("Starting…".into()))
    );
    assert_eq!(
        row(&rows, "screenViewer").control,
        Control::Info(Some("Not watching".into()))
    );

    let stalled = hosting(
        ScreenPhase::Stalled,
        vec![viewer(2, true, Link::Direct, 20, 0)],
    );
    report(&mut online, true, true, Some(stalled), vec![], None);
    let rows = party_rows(&online);
    assert_eq!(
        row(&rows, "screenShared").title,
        "Your screen is shared with 1 person"
    );
    assert_eq!(
        row(&rows, "screenGame").control,
        Control::Info(Some("The picture has stopped".into()))
    );
}

#[test]
fn the_viewer_sees_what_they_watch_its_state_and_the_way_it_comes() {
    let mut online = online();
    let states = [
        (
            ScreenPhase::Connecting,
            None,
            None,
            "Connecting…",
            "Connecting…",
        ),
        (
            ScreenPhase::Live,
            Some(Link::Direct),
            Some(24),
            "Live",
            "Direct, 24 ms",
        ),
        (
            ScreenPhase::Stalled,
            Some(Link::Relay),
            Some(80),
            "Waiting for the picture",
            "Through a relay, 80 ms",
        ),
    ];
    for (phase, link, rtt, said, path) in states {
        report(
            &mut online,
            false,
            true,
            None,
            vec![offer(2, "Astro Bot")],
            Some(watching(2, phase, link, rtt)),
        );
        let rows = party_rows(&online);
        let shown = row(&rows, "screenWatching");
        assert_eq!(shown.title, "Watching Lumen");
        assert_eq!(shown.control, Control::Info(Some(said.to_owned())));
        assert_eq!(
            shown.detail.as_deref(),
            Some(format!("Astro Bot · {path}").as_str())
        );
        assert_eq!(
            row(&rows, "screenNote").title,
            "The picture opens in its own window."
        );
        // The stream watched is not offered a second time.
        assert!(!ids(&rows).contains(&"watchScreen"));
    }
    assert_eq!(
        press(&online, "stopWatching", 0),
        [OnlineAction::Screen(ScreenCommand::Leave)]
    );
}

#[test]
fn home_names_the_stream_only_while_there_is_one_and_leads_to_the_party() {
    let mut online = online();
    report(&mut online, true, true, None, vec![], None);
    assert!(!ids(&rows_of(&online, Page::Home)).contains(&"screenShare"));

    report(
        &mut online,
        false,
        true,
        None,
        vec![offer(2, "Astro Bot")],
        None,
    );
    let rows = rows_of(&online, Page::Home);
    assert_eq!(
        row(&rows, "screenShare").detail.as_deref(),
        Some("Lumen is sharing Astro Bot")
    );

    report(
        &mut online,
        false,
        true,
        None,
        vec![offer(2, "Astro Bot")],
        Some(watching(2, ScreenPhase::Live, Some(Link::Direct), Some(24))),
    );
    assert_eq!(
        row(&rows_of(&online, Page::Home), "screenShare")
            .detail
            .as_deref(),
        Some("Watching Lumen")
    );

    let mut panel = OnlinePanel::default();
    panel.go(Page::Home, &data(&online));
    let mut actions = Vec::new();
    panel.run("screenShare", &data(&online), &mut actions);
    assert_eq!(panel.page, Page::Party);
    assert!(actions.is_empty());
}

#[test]
fn the_path_is_said_without_the_protocols_names() {
    let en = Language::English;
    assert_eq!(path_text(Some(Link::Direct), Some(24), en), "Direct, 24 ms");
    assert_eq!(
        path_text(Some(Link::Relay), Some(80), en),
        "Through a relay, 80 ms"
    );
    assert_eq!(path_text(Some(Link::Direct), None, en), "Direct");
    assert_eq!(
        path_text(Some(Link::Connecting), Some(5), en),
        "Connecting…"
    );
    assert_eq!(path_text(None, None, en), "Connecting…");
    assert_eq!(
        path_text(Some(Link::Direct), Some(24), Language::Turkish),
        "Doğrudan, 24 ms"
    );
}

#[test]
fn an_unknown_member_is_called_someone() {
    let online = Online::default();
    assert_eq!(name_of(&online, AccountId(9), Language::English), "Someone");
}
