//! The "Phone as Controller" page of the settings (docs/DESIGN.md 18): the switch, what the phone
//! server is doing, the pairing question, the paired phones and the state of the virtual
//! controller. Only rows: the page has no drawing of its own.

use anyps5_core::remote::VirtualPadStatus;
use anyps5_core::settings::Preferences;

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
        rows.push(
            Row::info(
                "pairAsk",
                PhoneText::PairAsk.format(language, &[Arg::Str(&request.device_name)]),
                Some(request.code.clone()),
            )
            .icon(Icon::DEVICE_MOBILE)
            .group_start(),
        );
        rows.push(Row::info("pairNote", t(PhoneText::CodeNote), None).attached());
        rows.push(Row::command("pairAllow", t(PhoneText::PairAllow)).icon(Icon::CHECK_CIRCLE));
        rows.push(Row::command("pairDeny", t(PhoneText::PairDeny)).icon(Icon::X_CIRCLE));
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

    // The virtual controller.
    rows.extend(controller_rows(phone, language));
    rows
}

fn controller_rows(phone: &PhoneState, language: Language) -> Vec<Row> {
    let t = |text: PhoneText| text.text(language).to_owned();
    let Some(status) = &phone.pad else {
        return Vec::new();
    };
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
    let windows = phone.windows;
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
