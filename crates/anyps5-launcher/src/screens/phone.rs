//! The "Phone as Controller" page of the settings (docs/DESIGN.md 18): the switch, what the phone
//! server is doing, the pairing question, the paired phones, the LudusCtrl rows
//! ([`crate::screens::ludus`]) and the state of the virtual controller. Only rows: the page has no
//! drawing of its own.

use anyps5_core::remote::VirtualPadStatus;
use anyps5_core::settings::Preferences;

use crate::app::ludus::LudusState;
use crate::app::phone::PhoneState;
use crate::screens::phone_strings::PhoneText;
use crate::screens::rows::{Change, Row};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;

/// The ids of the "Remove" rows: one per paired phone, at most [`anyps5_core::remote::MAX_DEVICES`].
pub const REMOVE_ROWS: [&str; 8] = [
    "phone0", "phone1", "phone2", "phone3", "phone4", "phone5", "phone6", "phone7",
];

/// The official place to get the driver. The launcher opens it; it never downloads or installs.
pub const VIGEM_DOWNLOAD: &str = "https://github.com/nefarius/ViGEmBus/releases/latest";
/// The guide for the Linux side.
pub const UINPUT_GUIDE: &str =
    "https://github.com/anyplaynet/launcher/blob/main/docs/DESIGN.md#18-phone-as-controller";

/// What the host has to do after a change on this page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhoneAction {
    /// The answer to the pairing question.
    Respond(bool),
    /// Forget this paired phone (its device id).
    Forget(String),
    /// Open the driver's download page (Windows) or the guide (Linux).
    OpenDriverPage,
}

/// "2026-10-24T18:14:03Z" as the date part.
fn date_of(stamp: &str) -> &str {
    stamp.get(..10).unwrap_or(stamp)
}

pub fn rows(prefs: &Preferences, phone: &PhoneState, language: Language) -> Vec<Row> {
    let t = |text: PhoneText| text.text(language).to_owned();
    let mut rows = vec![
        Row::toggle("phone", t(PhoneText::Title), prefs.phone_remote)
            .detail(t(PhoneText::SwitchDetail))
            .icon(Icon::DEVICE_MOBILE),
    ];
    if !prefs.phone_remote {
        return rows;
    }
    if phone.failed {
        rows.push(Row::info("phoneFailed", t(PhoneText::StartFailed), None));
        return rows;
    }

    // Status.
    let status = match (&phone.connected, &phone.connecting) {
        (Some(name), _) => PhoneText::ConnectedTo.format(language, &[Arg::Str(name)]),
        (None, Some(name)) => PhoneText::ConnectingTo.format(language, &[Arg::Str(name)]),
        (None, None) => t(PhoneText::Waiting),
    };
    rows.push(
        Row::info("phoneStatus", t(PhoneText::Status), Some(status))
            .icon(Icon::WIFI_HIGH)
            .group_start(),
    );
    if let (Some(_), Some(milliseconds)) = (&phone.connected, phone.latency_ms) {
        let text = PhoneText::DelayValue.format(language, &[Arg::Int(milliseconds.round() as i64)]);
        rows.push(Row::info("phoneDelay", t(PhoneText::Delay), Some(text)));
    }

    // The pairing question: the code, a note, the two answers.
    if let Some(request) = &phone.pairing {
        rows.extend(pairing_rows(
            PAIR_ROWS,
            &request.device_name,
            &request.code,
            language,
        ));
    }

    // The address for typing it in.
    if let Some(info) = &phone.info
        && let Some(address) = info.addresses.first()
    {
        rows.push(
            Row::info(
                "phoneAddress",
                t(PhoneText::Address),
                Some(format!("{address}:{}", info.control_port)),
            )
            .group_start(),
        );
        rows.push(Row::info("phoneAddressNote", t(PhoneText::AddressNote), None).attached());
        if !info.preferred_port {
            rows.push(Row::info("phonePortBusy", t(PhoneText::PortBusy), None).attached());
        }
        if info.discovery_error.is_some() {
            rows.push(Row::info("phoneAnnounce", t(PhoneText::NotAnnounced), None).attached());
        }
    }

    // The paired phones.
    rows.push(Row::caption("phonePaired", t(PhoneText::Paired)).group_start());
    if phone.paired.is_empty() {
        rows.push(Row::info("phonePairedNone", t(PhoneText::PairedNone), None));
    }
    for (id, device) in REMOVE_ROWS.iter().zip(&phone.paired) {
        rows.push(
            Row::command(
                id,
                PhoneText::Remove.format(language, &[Arg::Str(&device.name)]),
            )
            .detail(PhoneText::PairedOn.format(language, &[Arg::Str(date_of(&device.added))]))
            .icon(Icon::X_CIRCLE)
            .destructive(),
        );
    }

    rows
}

/// The ids of one pairing question's rows: the question, its note, the two answers.
#[derive(Clone, Copy, Debug)]
pub struct PairRows {
    pub ask: &'static str,
    pub note: &'static str,
    pub allow: &'static str,
    pub deny: &'static str,
}

/// The phone server's question.
pub const PAIR_ROWS: PairRows = PairRows {
    ask: "pairAsk",
    note: "pairNote",
    allow: "pairAllow",
    deny: "pairDeny",
};

/// A code as the player compares it: six digits in two groups of three (`260 329`), the way the
/// LudusCtrl app shows them; any other code as it is.
pub fn code_text(code: &str) -> String {
    let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit()) {
        format!("{} {}", &digits[..3], &digits[3..])
    } else {
        code.to_owned()
    }
}

/// A phone asks to be paired: "Pair Pixel 8?" with the code as its value, the note to compare the
/// codes, then Pair Phone and Deny. The phone server and LudusCtrl ask the same way.
pub fn pairing_rows(ids: PairRows, name: &str, code: &str, language: Language) -> Vec<Row> {
    let t = |text: PhoneText| text.text(language).to_owned();
    vec![
        Row::info(
            ids.ask,
            PhoneText::PairAsk.format(language, &[Arg::Str(name)]),
            Some(code_text(code)),
        )
        .icon(Icon::DEVICE_MOBILE)
        .group_start(),
        Row::info(ids.note, t(PhoneText::CodeNote), None).attached(),
        Row::command(ids.allow, t(PhoneText::PairAllow)).icon(Icon::CHECK_CIRCLE),
        Row::command(ids.deny, t(PhoneText::PairDeny)).icon(Icon::X_CIRCLE),
    ]
}

/// The whole page: the phone server, its LudusCtrl option (only while the switch is on), then the
/// virtual controller both of them use, shown once.
pub fn page(
    prefs: &Preferences,
    phone: &PhoneState,
    ludus: &LudusState,
    language: Language,
) -> Vec<Row> {
    let mut rows = rows(prefs, phone, language);
    if !prefs.phone_remote {
        return rows;
    }
    // The PC's address with LudusCtrl's own port, for typing into its app.
    let address = phone
        .info
        .as_ref()
        .and_then(|info| info.addresses.first())
        .zip(ludus.port)
        .map(|(address, port)| format!("{address}:{port}"));
    rows.extend(crate::screens::ludus::rows(prefs, ludus, address, language));
    // The browser client is an option of LudusCtrl: under its rows, while it runs.
    if prefs.phone_ludusctrl && !ludus.failed {
        rows.extend(crate::screens::ludus_web::rows(
            prefs,
            &ludus.browser,
            phone.windows,
            language,
        ));
    }
    let phone_on = !phone.failed;
    let ludus_on = prefs.phone_ludusctrl && !ludus.failed;
    let pad = phone
        .pad
        .as_ref()
        .filter(|_| phone_on)
        .or(ludus.pad.as_ref().filter(|_| ludus_on));
    if let Some(status) = pad {
        rows.extend(controller_rows(status, phone.windows, language));
    }
    rows
}

fn controller_rows(status: &VirtualPadStatus, windows: bool, language: Language) -> Vec<Row> {
    let t = |text: PhoneText| text.text(language).to_owned();
    let value = match status {
        VirtualPadStatus::Ready => PhoneText::ControllerReady,
        VirtualPadStatus::DriverMissing => PhoneText::ControllerMissing,
        VirtualPadStatus::NoAccess(_) => PhoneText::ControllerNoAccess,
        VirtualPadStatus::Unsupported => PhoneText::ControllerUnsupported,
    };
    let mut rows = vec![
        Row::info("padStatus", t(PhoneText::Controller), Some(t(value)))
            .icon(Icon::PLUG)
            .group_start(),
    ];
    match status {
        VirtualPadStatus::DriverMissing | VirtualPadStatus::NoAccess(_) if windows => {
            rows.push(Row::info("padNote", t(PhoneText::ViGemNote), None).attached());
            rows.push(Row::command("padOpen", t(PhoneText::ViGemOpen)).icon(Icon::DOWNLOAD_SIMPLE));
        }
        VirtualPadStatus::DriverMissing | VirtualPadStatus::NoAccess(_) => {
            rows.push(Row::info("padNote", t(PhoneText::UinputNote), None).attached());
            rows.push(Row::info("padRule", t(PhoneText::UinputRule), None).attached());
            rows.push(Row::command("padOpen", t(PhoneText::UinputOpen)).icon(Icon::FILE_TEXT));
        }
        _ => {}
    }
    rows
}

/// What a run of one of this page's rows asks of the host.
pub fn action(phone: &PhoneState, change: &Change) -> Option<PhoneAction> {
    let Change::Run { row } = change else {
        return None;
    };
    match *row {
        "pairAllow" => Some(PhoneAction::Respond(true)),
        "pairDeny" => Some(PhoneAction::Respond(false)),
        "padOpen" => Some(PhoneAction::OpenDriverPage),
        other => {
            let at = REMOVE_ROWS.iter().position(|id| *id == other)?;
            phone
                .paired
                .get(at)
                .map(|device| PhoneAction::Forget(device.id.clone()))
        }
    }
}

/// The page the "Open ..." row leads to on this system.
pub fn driver_page() -> &'static str {
    if cfg!(windows) {
        VIGEM_DOWNLOAD
    } else {
        UINPUT_GUIDE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screens::rows::Control;
    use anyps5_core::remote::{PairedDevice, PairingRequest, RemoteInfo};

    fn on() -> Preferences {
        Preferences {
            phone_remote: true,
            ..Preferences::first_run()
        }
    }

    fn ids(rows: &[Row]) -> Vec<&'static str> {
        rows.iter().map(|r| r.id).collect()
    }

    fn device(id: &str, name: &str) -> PairedDevice {
        PairedDevice {
            added: "2026-10-24T18:14:03Z".into(),
            id: id.into(),
            key: "k".into(),
            name: name.into(),
        }
    }

    #[test]
    fn off_is_the_switch_alone() {
        let rows = rows(
            &Preferences::first_run(),
            &PhoneState::default(),
            Language::English,
        );
        assert_eq!(ids(&rows), ["phone"]);
        assert!(
            rows[0].detail.as_deref().unwrap().contains("home network"),
            "the switch says who can connect"
        );
    }

    #[test]
    fn a_failed_start_says_so_and_stops() {
        let state = PhoneState {
            failed: true,
            ..PhoneState::default()
        };
        let rows = rows(&on(), &state, Language::English);
        assert_eq!(ids(&rows), ["phone", "phoneFailed"]);
    }

    #[test]
    fn a_pairing_question_shows_the_code_and_two_answers() {
        let state = PhoneState {
            pairing: Some(PairingRequest {
                id: 7,
                device_name: "Pixel 8".into(),
                code: "4821".into(),
            }),
            pad: Some(VirtualPadStatus::Ready),
            ..PhoneState::default()
        };
        let rows = rows(&on(), &state, Language::English);
        let ask = rows.iter().find(|r| r.id == "pairAsk").unwrap();
        assert_eq!(ask.title, "Pair Pixel 8?");
        assert_eq!(ask.control, Control::Info(Some("4821".into())));
        let list = ids(&rows);
        let at = |id: &str| list.iter().position(|r| *r == id).unwrap();
        assert!(at("pairAsk") < at("pairNote") && at("pairNote") < at("pairAllow"));
        assert!(at("pairAllow") < at("pairDeny"));
        assert_eq!(
            action(&state, &Change::Run { row: "pairAllow" }),
            Some(PhoneAction::Respond(true))
        );
        assert_eq!(
            action(&state, &Change::Run { row: "pairDeny" }),
            Some(PhoneAction::Respond(false))
        );
    }

    #[test]
    fn every_paired_phone_has_a_remove_row() {
        let state = PhoneState {
            connected: Some("Pixel 8".into()),
            latency_ms: Some(3.6),
            paired: vec![device("a", "Pixel 8"), device("b", "Galaxy")],
            ..PhoneState::default()
        };
        let rows = rows(&on(), &state, Language::English);
        let first = rows.iter().find(|r| r.id == "phone0").unwrap();
        assert_eq!(first.title, "Remove Pixel 8");
        assert_eq!(first.detail.as_deref(), Some("Paired on 2026-10-24"));
        assert!(first.destructive);
        assert!(rows.iter().any(|r| r.id == "phone1"));
        assert!(!rows.iter().any(|r| r.id == "phone2"));
        let delay = rows.iter().find(|r| r.id == "phoneDelay").unwrap();
        assert_eq!(delay.control, Control::Info(Some("4 ms".into())));
        assert_eq!(
            action(&state, &Change::Run { row: "phone1" }),
            Some(PhoneAction::Forget("b".into()))
        );
        assert_eq!(action(&state, &Change::Run { row: "phone5" }), None);
        assert_eq!(action(&state, &Change::Run { row: "rescan" }), None);
    }

    #[test]
    fn no_phone_yet_says_how_to_start() {
        let rows = rows(&on(), &PhoneState::default(), Language::English);
        assert!(ids(&rows).contains(&"phonePairedNone"));
        assert!(!ids(&rows).contains(&"phone0"));
    }

    #[test]
    fn the_address_comes_with_its_troubles() {
        let state = PhoneState {
            info: Some(RemoteInfo {
                addresses: vec!["192.168.178.20".parse().unwrap()],
                control_port: 47701,
                udp_port: 47702,
                preferred_port: false,
                discovery_error: Some("no adapter".into()),
            }),
            ..PhoneState::default()
        };
        let rows = rows(&on(), &state, Language::English);
        let address = rows.iter().find(|r| r.id == "phoneAddress").unwrap();
        assert_eq!(
            address.control,
            Control::Info(Some("192.168.178.20:47701".into()))
        );
        assert!(ids(&rows).contains(&"phonePortBusy"));
        assert!(ids(&rows).contains(&"phoneAnnounce"));
    }

    #[test]
    fn the_driver_is_explained_where_it_is_missing() {
        let missing = |windows| PhoneState {
            windows,
            pad: Some(VirtualPadStatus::DriverMissing),
            ..PhoneState::default()
        };
        let quiet = LudusState::default();
        let rows = |prefs: &Preferences, state: &PhoneState, language| {
            page(prefs, state, &quiet, language)
        };
        let win = rows(&on(), &missing(true), Language::English);
        let note = win.iter().find(|r| r.id == "padNote").unwrap();
        assert!(note.title.contains("ViGEmBus") && note.title.contains("never installs"));
        assert_eq!(
            win.iter().find(|r| r.id == "padOpen").unwrap().title,
            "Open the ViGEmBus Download Page"
        );
        let linux = rows(&on(), &missing(false), Language::English);
        assert!(
            linux
                .iter()
                .find(|r| r.id == "padNote")
                .unwrap()
                .title
                .contains("uinput")
        );
        assert!(linux.iter().any(|r| r.id == "padRule"));
        let ready = rows(
            &on(),
            &PhoneState {
                pad: Some(VirtualPadStatus::Ready),
                ..PhoneState::default()
            },
            Language::English,
        );
        assert!(!ids(&ready).contains(&"padOpen"));
    }

    #[test]
    fn six_digit_codes_read_in_two_groups() {
        assert_eq!(code_text("260329"), "260 329");
        assert_eq!(code_text("000 042"), "000 042");
        assert_eq!(
            code_text("4821"),
            "4821",
            "the phone server's four digits stay"
        );
        assert_eq!(code_text("12a456"), "12a456");
    }

    #[test]
    fn ludusctrl_is_an_option_of_the_phone_switch() {
        let ludus = LudusState {
            pad: Some(VirtualPadStatus::DriverMissing),
            port: Some(45989),
            ..LudusState::default()
        };
        let quiet = PhoneState {
            windows: true,
            ..PhoneState::default()
        };
        let off = Preferences {
            phone_ludusctrl: true,
            ..Preferences::first_run()
        };
        assert_eq!(
            ids(&page(&off, &quiet, &ludus, Language::English)),
            ["phone"],
            "the option waits for the phone switch"
        );

        // The phone server could not start; LudusCtrl's own server still runs and has the pad.
        let both = Preferences {
            phone_remote: true,
            ..off.clone()
        };
        let failed = PhoneState {
            failed: true,
            ..quiet.clone()
        };
        let list = page(&both, &failed, &ludus, Language::English);
        assert_eq!(list.iter().filter(|r| r.id == "padStatus").count(), 1);
        assert!(list.iter().any(|r| r.id == "padOpen"));

        // Both run: one controller section, after both groups; LudusCtrl's address is the PC's.
        let phone = PhoneState {
            pad: Some(VirtualPadStatus::Ready),
            info: Some(RemoteInfo {
                addresses: vec!["192.168.178.20".parse().unwrap()],
                control_port: 47700,
                udp_port: 47702,
                preferred_port: true,
                discovery_error: None,
            }),
            ..quiet.clone()
        };
        let list = page(&both, &phone, &ludus, Language::English);
        assert_eq!(list.iter().filter(|r| r.id == "padStatus").count(), 1);
        let at = |id: &str| list.iter().position(|r| r.id == id).unwrap();
        assert!(at("phone") < at("ludusctrl") && at("ludusctrl") < at("padStatus"));
        assert_eq!(
            list[at("ludusAddress")].control,
            Control::Info(Some("192.168.178.20:45989".into()))
        );

        // The option off: its switch, and the phone server's controller rows.
        let phone_only = Preferences {
            phone_ludusctrl: false,
            ..both.clone()
        };
        let list = page(&phone_only, &phone, &ludus, Language::English);
        let at = |id: &str| list.iter().position(|r| r.id == id).unwrap();
        assert_eq!(list[at("ludusctrl") + 1].id, "padStatus");
    }

    #[test]
    fn the_page_reads_in_turkish_too() {
        let state = PhoneState {
            connected: Some("Pixel 8".into()),
            ..PhoneState::default()
        };
        let rows = rows(&on(), &state, Language::Turkish);
        let status = rows.iter().find(|r| r.id == "phoneStatus").unwrap();
        assert_eq!(status.control, Control::Info(Some("Pixel 8 bağlı".into())));
    }

    #[test]
    fn the_driver_page_is_a_fixed_address() {
        assert!(driver_page().starts_with("https://github.com/"));
    }
}
