//! LudusCtrl on the "Phone as Controller" page (docs/DESIGN.md 18, "LudusCtrl"): the switch, the
//! status of the app and of the browser client, the pairing question, the browser client's rows
//! ([`crate::screens::ludus_web`]), the paired LudusCtrl phones and where the app comes from. The
//! virtual controller's rows are the page's own ([`crate::screens::phone::page`]).

use anyps5_core::settings::Preferences;

use crate::app::ludus::LudusState;
use crate::screens::ludus_strings::{LudusText, PROJECT_PAGE_SHOWN};
use crate::screens::ludus_web;
use crate::screens::phone::{PairRows, pairing_rows};
use crate::screens::phone_strings::PhoneText;
use crate::screens::rows::{Change, Row};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;

/// The ids of the "Remove" rows, one per paired LudusCtrl phone.
pub const REMOVE_ROWS: [&str; 8] = [
    "ludus0", "ludus1", "ludus2", "ludus3", "ludus4", "ludus5", "ludus6", "ludus7",
];

/// LudusCtrl's pairing question.
pub const PAIR_ROWS: PairRows = PairRows {
    ask: "ludusAsk",
    note: "ludusNote",
    allow: "ludusAllow",
    deny: "ludusDeny",
};

/// What the host has to do after a change in the LudusCtrl rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LudusAction {
    /// The answer to the pairing question.
    Respond(bool),
    /// Forget this paired phone (its id).
    Forget(String),
    /// Open the LudusCtrl project page.
    OpenProjectPage,
    /// Put this browser link on the clipboard (the host does it and says so).
    CopyLink(String),
    /// A new browser link: the old one stops working.
    NewLink,
}

/// The LudusCtrl group, an option of Phone as Controller: the page shows it only while the phone
/// server's switch is on. With the switch on, the app and the browser client both run: their
/// status rows, then the browser's QR code and link (the first thing a new player needs, as the
/// app is not in the store yet), then the app's address, paired phones and where it comes from.
/// A pairing question goes right under the status, as the phone server's does: it waits a minute
/// for an answer. `address` is this PC's address with the server's port, for typing into the
/// app; `windows`: the firewall note applies.
pub fn rows(
    prefs: &Preferences,
    ludus: &LudusState,
    address: Option<String>,
    windows: bool,
    language: Language,
) -> Vec<Row> {
    let t = |text: LudusText| text.text(language).to_owned();
    let p = |text: PhoneText| text.text(language).to_owned();
    let mut rows = vec![
        Row::toggle("ludusctrl", t(LudusText::Title), prefs.phone_ludusctrl)
            .detail(t(LudusText::SwitchDetail))
            .icon(Icon::GAME_CONTROLLER)
            .group_start(),
    ];
    if !prefs.phone_ludusctrl {
        return rows;
    }

    // The app's status, then the browser's, in one group.
    let app = if ludus.failed {
        Row::info("ludusFailed", t(LudusText::StartFailed), None)
    } else {
        let status = match &ludus.connected {
            Some(name) if ludus.controller => {
                PhoneText::ConnectedTo.format(language, &[Arg::Str(name)])
            }
            Some(name) => LudusText::NoController.format(language, &[Arg::Str(name)]),
            None => p(PhoneText::Waiting),
        };
        Row::info("ludusStatus", t(LudusText::AppStatus), Some(status)).icon(Icon::DEVICE_MOBILE)
    };
    rows.push(app.group_start());
    rows.extend(ludus_web::status_row(&ludus.browser, language));

    // The pairing question, as the phone server asks it.
    if let Some(request) = ludus.pairing.as_ref().filter(|_| !ludus.failed) {
        rows.extend(pairing_rows(
            PAIR_ROWS,
            &request.phone_name,
            &request.code,
            language,
        ));
    }

    // The browser client: the QR code, the link, the other addresses, New Link.
    rows.extend(ludus_web::link_rows(&ludus.browser, windows, language));

    if !ludus.failed {
        // The address for typing it in.
        if let Some(address) = address {
            rows.push(
                Row::info("ludusAddress", p(PhoneText::Address), Some(address)).group_start(),
            );
            rows.push(Row::info("ludusAddressNote", p(PhoneText::AddressNote), None).attached());
        }

        // The paired phones.
        rows.push(Row::caption("ludusPaired", t(LudusText::Paired)).group_start());
        if ludus.paired.is_empty() {
            rows.push(Row::info("ludusPairedNone", t(LudusText::PairedNone), None));
        }
        for (id, phone) in REMOVE_ROWS.iter().zip(&ludus.paired) {
            rows.push(
                Row::command(
                    id,
                    PhoneText::Remove.format(language, &[Arg::Str(&phone.name)]),
                )
                .detail(PhoneText::PairedOn.format(language, &[Arg::Str(date_of(&phone.added))]))
                .icon(Icon::X_CIRCLE)
                .destructive(),
            );
        }
    }

    // Where the app comes from.
    rows.push(Row::info("ludusAppNote", t(LudusText::AppNote), None).group_start());
    rows.push(
        Row::command("ludusApp", t(LudusText::AppOpen))
            .detail(PROJECT_PAGE_SHOWN)
            .icon(Icon::GITHUB_LOGO),
    );
    rows
}

/// "2026-10-24T18:14:03Z" as the date part.
fn date_of(stamp: &str) -> &str {
    stamp.get(..10).unwrap_or(stamp)
}

/// What a run of one of the LudusCtrl rows asks of the host.
pub fn action(ludus: &LudusState, change: &Change) -> Option<LudusAction> {
    let Change::Run { row } = change else {
        return None;
    };
    match *row {
        "ludusAllow" => Some(LudusAction::Respond(true)),
        "ludusDeny" => Some(LudusAction::Respond(false)),
        "ludusApp" => Some(LudusAction::OpenProjectPage),
        other => {
            let at = REMOVE_ROWS.iter().position(|id| *id == other)?;
            ludus
                .paired
                .get(at)
                .map(|phone| LudusAction::Forget(phone.id.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ludus::{LudusPairing, LudusPhone};
    use crate::app::ludus_web::{BrowserInfo, BrowserState, Certificate, LUDUS_WEB_PORT};
    use crate::screens::rows::Control;

    fn on() -> Preferences {
        Preferences {
            phone_remote: true,
            phone_ludusctrl: true,
            ..Preferences::first_run()
        }
    }

    const LINK: &str = "https://192.168.178.20:45991/#t=q8Zk3VtW0bHn5sLc2pRj1A";

    fn browser(urls: &[&str]) -> BrowserState {
        BrowserState {
            info: Some(BrowserInfo {
                urls: urls.iter().map(|u| (*u).to_owned()).collect(),
                port: LUDUS_WEB_PORT,
                preferred_port: true,
                certificate: Certificate::Kept,
                fingerprint: String::new(),
            }),
            ..BrowserState::default()
        }
    }

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.id).collect()
    }

    fn phone(id: &str, name: &str) -> LudusPhone {
        LudusPhone {
            id: id.into(),
            name: name.into(),
            added: "2026-10-24T18:14:03Z".into(),
        }
    }

    #[test]
    fn off_is_the_switch_alone() {
        let rows = rows(
            &Preferences::first_run(),
            &LudusState::default(),
            None,
            true,
            Language::English,
        );
        assert_eq!(ids(&rows), ["ludusctrl"]);
        assert_eq!(rows[0].title, "LudusCtrl");
        let detail = rows[0].detail.as_deref().unwrap();
        assert!(detail.contains("LudusCtrl app") && detail.contains("paired"));
        assert!(detail.contains("scan the code"), "the browser client too");
        assert!(
            rows[0].group_start,
            "its own group after the phone server's rows"
        );
    }

    #[test]
    fn a_failed_start_says_so_and_leaves_the_apps_rows_out() {
        let state = LudusState {
            failed: true,
            pairing: Some(LudusPairing {
                id: 5,
                phone_name: "Pixel 8".into(),
                code: "260329".into(),
            }),
            paired: vec![phone("a", "Pixel 8")],
            ..LudusState::default()
        };
        let address = Some("192.168.178.20:45989".into());
        assert_eq!(
            ids(&rows(&on(), &state, address, true, Language::English)),
            ["ludusctrl", "ludusFailed", "ludusAppNote", "ludusApp"]
        );
    }

    #[test]
    fn without_the_app_the_browser_client_still_shows_its_code() {
        let state = LudusState {
            failed: true,
            browser: browser(&[LINK]),
            ..LudusState::default()
        };
        let rows = rows(&on(), &state, None, false, Language::English);
        let list = ids(&rows);
        assert_eq!(
            &list[..4],
            ["ludusctrl", "ludusFailed", "browserStatus", "browserQr"]
        );
        assert!(
            rows[1].title.contains("LudusCtrl app"),
            "the note says which part failed"
        );
        assert!(!list.contains(&"ludusPaired") && !list.contains(&"ludusStatus"));
    }

    #[test]
    fn a_failed_browser_client_is_a_note_under_the_apps_status() {
        let state = LudusState {
            browser: BrowserState {
                failed: true,
                ..BrowserState::default()
            },
            ..LudusState::default()
        };
        let list = ids(&rows(&on(), &state, None, true, Language::English));
        assert_eq!(&list[..3], ["ludusctrl", "ludusStatus", "browserFailed"]);
        assert!(list.contains(&"ludusPaired") && !list.contains(&"browserQr"));
    }

    #[test]
    fn the_group_reads_status_code_then_the_app() {
        let state = LudusState {
            pairing: Some(LudusPairing {
                id: 5,
                phone_name: "Pixel 8".into(),
                code: "260329".into(),
            }),
            paired: vec![phone("a", "Pixel 8")],
            browser: browser(&[LINK, "https://10.0.0.7:45991/#t=q8Zk3VtW0bHn5sLc2pRj1A"]),
            ..LudusState::default()
        };
        let address = Some("192.168.178.20:45989".into());
        let rows = rows(&on(), &state, address, true, Language::English);
        assert_eq!(
            ids(&rows),
            [
                "ludusctrl",
                "ludusStatus",
                "browserStatus",
                "ludusAsk",
                "ludusNote",
                "ludusAllow",
                "ludusDeny",
                "browserQr",
                "browserLink",
                "browserCopy",
                "browserCertNote",
                "browserOther",
                "browserOtherNote",
                "browserAddr0",
                "browserNewLink",
                "browserFirewall",
                "ludusAddress",
                "ludusAddressNote",
                "ludusPaired",
                "ludus0",
                "ludusAppNote",
                "ludusApp",
            ]
        );
        let at = |id: &str| rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!(at("ludusStatus").title, "App");
        assert_eq!(at("browserStatus").title, "Browser");
        assert!(
            !at("browserStatus").group_start,
            "one group with the app's status"
        );
        assert!(at("ludusStatus").group_start && at("browserQr").group_start);
        assert_eq!(
            at("browserStatus").control,
            Control::Info(Some("Waiting for a browser".into()))
        );
    }

    #[test]
    fn waiting_shows_the_status_the_empty_list_and_the_app() {
        let rows = rows(&on(), &LudusState::default(), None, true, Language::English);
        assert_eq!(
            ids(&rows),
            [
                "ludusctrl",
                "ludusStatus",
                "ludusPaired",
                "ludusPairedNone",
                "ludusAppNote",
                "ludusApp"
            ]
        );
        let status = &rows[1];
        assert_eq!(
            status.control,
            Control::Info(Some("Waiting for a phone".into()))
        );
        let app = rows.iter().find(|r| r.id == "ludusApp").unwrap();
        assert_eq!(app.title, "Open the LudusCtrl Page");
        assert_eq!(app.detail.as_deref(), Some("github.com/parl0v/ludusctrl"));
        assert_eq!(
            action(&LudusState::default(), &Change::Run { row: "ludusApp" }),
            Some(LudusAction::OpenProjectPage)
        );
    }

    #[test]
    fn the_pairing_question_shows_six_digits_in_two_groups() {
        let state = LudusState {
            pairing: Some(LudusPairing {
                id: 5,
                phone_name: "Pixel 8".into(),
                code: "260329".into(),
            }),
            ..LudusState::default()
        };
        let rows = rows(&on(), &state, None, true, Language::English);
        let ask = rows.iter().find(|r| r.id == "ludusAsk").unwrap();
        assert_eq!(ask.title, "Pair Pixel 8?");
        assert_eq!(ask.control, Control::Info(Some("260 329".into())));
        let list = ids(&rows);
        let at = |id: &str| list.iter().position(|r| *r == id).unwrap();
        assert!(at("ludusStatus") < at("ludusAsk"));
        assert!(at("ludusAsk") < at("ludusNote") && at("ludusNote") < at("ludusAllow"));
        assert!(at("ludusAllow") < at("ludusDeny"));
        assert_eq!(
            action(&state, &Change::Run { row: "ludusAllow" }),
            Some(LudusAction::Respond(true))
        );
        assert_eq!(
            action(&state, &Change::Run { row: "ludusDeny" }),
            Some(LudusAction::Respond(false))
        );
        assert_eq!(action(&state, &Change::Run { row: "pairAllow" }), None);
    }

    #[test]
    fn a_live_phone_and_every_paired_phone_with_remove() {
        let state = LudusState {
            connected: Some("Pixel 8".into()),
            controller: true,
            paired: vec![phone("a", "Pixel 8"), phone("b", "Galaxy S23")],
            ..LudusState::default()
        };
        let address = Some("192.168.178.20:45989".into());
        let rows = rows(&on(), &state, address, true, Language::English);
        let status = rows.iter().find(|r| r.id == "ludusStatus").unwrap();
        assert_eq!(
            status.control,
            Control::Info(Some("Connected to Pixel 8".into()))
        );
        let address = rows.iter().find(|r| r.id == "ludusAddress").unwrap();
        assert_eq!(
            address.control,
            Control::Info(Some("192.168.178.20:45989".into()))
        );
        let first = rows.iter().find(|r| r.id == "ludus0").unwrap();
        assert_eq!(first.title, "Remove Pixel 8");
        assert_eq!(first.detail.as_deref(), Some("Paired on 2026-10-24"));
        assert!(first.destructive);
        assert!(!ids(&rows).contains(&"ludusPairedNone"));
        assert!(!ids(&rows).contains(&"ludus2"));
        assert_eq!(
            action(&state, &Change::Run { row: "ludus1" }),
            Some(LudusAction::Forget("b".into()))
        );
        assert_eq!(action(&state, &Change::Run { row: "ludus5" }), None);
        assert_eq!(action(&state, &Change::Run { row: "phone0" }), None);
    }

    #[test]
    fn a_phone_without_a_controller_says_so() {
        let state = LudusState {
            connected: Some("Pixel 8".into()),
            controller: false,
            ..LudusState::default()
        };
        for (language, text) in [
            (Language::English, "Connected to Pixel 8, no controller"),
            (Language::Turkish, "Pixel 8 bağlı, kumanda yok"),
        ] {
            let rows = rows(&on(), &state, None, true, language);
            let status = rows.iter().find(|r| r.id == "ludusStatus").unwrap();
            assert_eq!(status.control, Control::Info(Some(text.into())));
        }
    }

    #[test]
    fn the_rows_read_in_turkish_too() {
        let state = LudusState {
            connected: Some("Pixel 8".into()),
            controller: true,
            paired: vec![phone("a", "Pixel 8")],
            ..LudusState::default()
        };
        let rows = rows(&on(), &state, None, true, Language::Turkish);
        let status = rows.iter().find(|r| r.id == "ludusStatus").unwrap();
        assert_eq!(status.control, Control::Info(Some("Pixel 8 bağlı".into())));
        assert_eq!(status.title, "Uygulama");
        assert_eq!(
            rows.iter().find(|r| r.id == "ludusPaired").unwrap().title,
            "Eşleştirilmiş LudusCtrl Telefonları"
        );
        assert_eq!(
            rows.iter().find(|r| r.id == "ludus0").unwrap().title,
            "Pixel 8 Kaldır"
        );
    }

    #[test]
    fn the_row_ids_are_the_pages_own() {
        // The phone server's rows and these share one page: no id may be both.
        let phone = crate::screens::phone::REMOVE_ROWS;
        for id in REMOVE_ROWS {
            assert!(!phone.contains(&id), "{id}");
        }
        let shared = crate::screens::phone::PAIR_ROWS;
        for (a, b) in [
            (PAIR_ROWS.ask, shared.ask),
            (PAIR_ROWS.note, shared.note),
            (PAIR_ROWS.allow, shared.allow),
            (PAIR_ROWS.deny, shared.deny),
        ] {
            assert_ne!(a, b);
        }
    }
}
