//! LudusCtrl's browser client on the "Phone as Controller" page (docs/DESIGN.md 18, "LudusCtrl
//! browser client"), in the LudusCtrl group ([`crate::screens::ludus::rows`]): the QR code and the
//! link, who holds the pad, the notes that explain what the phone will say, the links of the
//! other addresses and a new link. It has no switch: it runs with LudusCtrl.

use crate::app::ludus_web::{BrowserState, Certificate, LUDUS_WEB_PORT};
use crate::screens::ludus::LudusAction;
use crate::screens::ludus_web_strings::BrowserText;
use crate::screens::rows::{Change, Row};
use crate::ui::i18n::{Arg, Language};
use crate::ui::icons::Icon;

/// The ids of the rows that copy another address's link, one per address after the first.
pub const OTHER_ROWS: [&str; 6] = [
    "browserAddr0",
    "browserAddr1",
    "browserAddr2",
    "browserAddr3",
    "browserAddr4",
    "browserAddr5",
];

/// The address part of a link (`https://192.168.1.23:45991/#t=...` is `192.168.1.23`).
pub fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let host = rest.split(['/', '#']).next().unwrap_or(rest);
    if let Some(v6) = host.strip_prefix('[') {
        return v6.split(']').next().unwrap_or(v6);
    }
    host.rsplit_once(':').map_or(host, |(host, _)| host)
}

/// The QR code, the link it encodes and the row that copies it: the first rows under the LudusCtrl
/// switch. While the client starts they stand in place with an empty card, so nothing moves when
/// the link comes. Nothing when it could not start, a note when this PC has no address.
pub fn code_rows(browser: &BrowserState, language: Language) -> Vec<Row> {
    let t = |text: BrowserText| text.text(language).to_owned();
    if browser.failed {
        return Vec::new();
    }
    let url = match &browser.info {
        None => Some(String::new()),
        Some(info) => info.urls.first().cloned(),
    };
    let Some(url) = url else {
        return vec![Row::info("browserNoAddress", t(BrowserText::NoAddress), None).warning()];
    };
    vec![
        Row::qr("browserQr", t(BrowserText::QrLabel), url.clone()).detail(t(BrowserText::QrHint)),
        Row::link("browserLink", t(BrowserText::Link), url),
        Row::command("browserCopy", t(BrowserText::Copy))
            .detail(t(BrowserText::CopyDetail))
            .icon(Icon::LINK),
    ]
}

/// The browser client's status row, under the app's in the LudusCtrl group: who holds the pad,
/// or the note that it could not start.
pub fn status_row(browser: &BrowserState, language: Language) -> Row {
    let t = |text: BrowserText| text.text(language).to_owned();
    if browser.failed {
        return Row::info("browserFailed", t(BrowserText::StartFailed), None);
    }
    let status = match (&browser.peer, &browser.device) {
        (Some(peer), Some(device)) => {
            BrowserText::ConnectedDevice.format(language, &[Arg::Str(device), Arg::Str(peer)])
        }
        (Some(peer), None) => BrowserText::ConnectedTo.format(language, &[Arg::Str(peer)]),
        (None, _) => t(BrowserText::Waiting),
    };
    Row::info("browserStatus", t(BrowserText::Status), Some(status)).icon(Icon::GLOBE)
}

/// The browser client's notes under the status rows: what the phone will say about the
/// certificate, a busy port, the links of the other addresses and New Link.
pub fn note_rows(browser: &BrowserState, language: Language) -> Vec<Row> {
    let t = |text: BrowserText| text.text(language).to_owned();
    let mut rows = Vec::new();
    if browser.failed {
        return rows;
    }
    let urls = browser
        .info
        .as_ref()
        .map_or(&[][..], |info| info.urls.as_slice());

    // The certificate and the port, once there is a link (or while it comes).
    if browser.info.is_none() || !urls.is_empty() {
        rows.push(Row::info("browserCertNote", t(BrowserText::CertNote), None).step());
    }
    if let Some(info) = browser.info.as_ref().filter(|info| !info.urls.is_empty()) {
        if info.certificate == Certificate::Reissued {
            rows.push(
                Row::info("browserReissued", t(BrowserText::Reissued), None)
                    .attached()
                    .warning(),
            );
        }
        if !info.preferred_port {
            let text = BrowserText::PortBusy.format(
                language,
                &[
                    Arg::Int(i64::from(LUDUS_WEB_PORT)),
                    Arg::Int(i64::from(info.port)),
                ],
            );
            rows.push(Row::info("browserPortBusy", text, None).attached());
        }
    }

    // The other addresses of this PC, each a link to copy.
    if urls.len() > 1 {
        rows.push(Row::caption("browserOther", t(BrowserText::Other)).group_start());
        rows.push(Row::info("browserOtherNote", t(BrowserText::OtherNote), None).attached());
    }
    for (url, id) in urls.iter().skip(1).zip(OTHER_ROWS) {
        rows.push(
            Row::command(id, host_of(url))
                .detail(url.clone())
                .icon(Icon::LINK),
        );
    }

    // A new token.
    rows.push(
        Row::command("browserNewLink", t(BrowserText::NewLink))
            .detail(t(BrowserText::NewLinkDetail))
            .icon(Icon::ARROWS_CLOCKWISE)
            .group_start(),
    );
    rows.push(Row::info("browserShareNote", t(BrowserText::ShareNote), None).attached());
    rows
}

/// What a run of one of the browser client's rows asks of the host.
pub fn action(browser: &BrowserState, change: &Change) -> Option<LudusAction> {
    let Change::Run { row } = change else {
        return None;
    };
    let urls = &browser.info.as_ref()?.urls;
    match *row {
        "browserCopy" => urls.first().cloned().map(LudusAction::CopyLink),
        "browserNewLink" => Some(LudusAction::NewLink),
        other => {
            let at = OTHER_ROWS.iter().position(|id| *id == other)?;
            urls.get(at + 1).cloned().map(LudusAction::CopyLink)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ludus_web::BrowserInfo;
    use crate::screens::rows::Control;

    const LINK: &str = "https://192.168.178.20:45991/#t=q8Zk3VtW0bHn5sLc2pRj1A";

    /// The code, the status row, then the notes, as the LudusCtrl group lays them out.
    fn rows(browser: &BrowserState, language: Language) -> Vec<Row> {
        let mut rows = code_rows(browser, language);
        rows.push(status_row(browser, language));
        rows.extend(note_rows(browser, language));
        rows
    }

    fn running(urls: &[&str]) -> BrowserState {
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

    #[test]
    fn while_it_starts_the_code_holds_its_place() {
        let rows = rows(&BrowserState::default(), Language::English);
        assert_eq!(
            ids(&rows),
            [
                "browserQr",
                "browserLink",
                "browserCopy",
                "browserStatus",
                "browserCertNote",
                "browserNewLink",
                "browserShareNote",
            ]
        );
        assert_eq!(rows[0].control, Control::Qr(String::new()), "an empty card");
        assert_eq!(
            action(
                &BrowserState::default(),
                &Change::Run { row: "browserCopy" }
            ),
            None
        );
    }

    #[test]
    fn a_failed_start_is_one_note_in_place_of_the_status() {
        let state = BrowserState {
            failed: true,
            ..BrowserState::default()
        };
        let rows = rows(&state, Language::English);
        assert_eq!(ids(&rows), ["browserFailed"]);
        assert_eq!(
            rows[0].title,
            "The browser client could not start. The log has the reason."
        );
    }

    #[test]
    fn waiting_shows_the_code_the_link_and_the_notes() {
        let rows = rows(&running(&[LINK]), Language::English);
        assert_eq!(
            ids(&rows),
            [
                "browserQr",
                "browserLink",
                "browserCopy",
                "browserStatus",
                "browserCertNote",
                "browserNewLink",
                "browserShareNote",
            ]
        );
        let at = |id: &str| rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!(
            at("browserStatus").control,
            Control::Info(Some("Waiting for a browser".into()))
        );
        assert_eq!(at("browserQr").control, Control::Qr(LINK.into()));
        assert_eq!(at("browserLink").control, Control::Link(LINK.into()));
        assert_eq!(at("browserCopy").title, "Copy Link");
        assert_eq!(at("browserNewLink").title, "New Link");
        assert_eq!(
            action(&running(&[LINK]), &Change::Run { row: "browserCopy" }),
            Some(LudusAction::CopyLink(LINK.into()))
        );
        assert_eq!(
            action(
                &running(&[LINK]),
                &Change::Run {
                    row: "browserNewLink"
                }
            ),
            Some(LudusAction::NewLink)
        );
    }

    #[test]
    fn a_browser_that_holds_the_pad_is_named_by_its_address() {
        let mut state = running(&[LINK]);
        state.peer = Some("192.168.178.41".into());
        for (language, text) in [
            (
                Language::English,
                "Connected to a browser on 192.168.178.41",
            ),
            (
                Language::Turkish,
                "192.168.178.41 adresindeki tarayıcı bağlı",
            ),
        ] {
            let rows = rows(&state, language);
            let status = rows.iter().find(|r| r.id == "browserStatus").unwrap();
            assert_eq!(status.control, Control::Info(Some(text.into())));
        }
    }

    #[test]
    fn what_the_browser_runs_on_comes_first_when_it_said() {
        let mut state = running(&[LINK]);
        state.peer = Some("192.168.178.41".into());
        state.device = Some("iPhone".into());
        for (language, text) in [
            (
                Language::English,
                "Connected to a browser on iPhone (192.168.178.41)",
            ),
            (
                Language::Turkish,
                "iPhone üzerindeki tarayıcı bağlı (192.168.178.41)",
            ),
        ] {
            let rows = rows(&state, language);
            let status = rows.iter().find(|r| r.id == "browserStatus").unwrap();
            assert_eq!(status.control, Control::Info(Some(text.into())));
        }
    }

    #[test]
    fn a_pc_without_an_address_says_what_to_do() {
        let rows = rows(&running(&[]), Language::English);
        let list = ids(&rows);
        assert!(list.contains(&"browserNoAddress"));
        assert!(!list.contains(&"browserQr") && !list.contains(&"browserCopy"));
        assert!(
            list.contains(&"browserNewLink"),
            "it looks at the addresses again"
        );
    }

    #[test]
    fn a_first_certificate_needs_no_extra_note() {
        let mut state = running(&[LINK]);
        if let Some(info) = &mut state.info {
            info.certificate = Certificate::New;
        }
        let rows = rows(&state, Language::English);
        assert!(!ids(&rows).contains(&"browserReissued"));
        assert!(ids(&rows).contains(&"browserCertNote"));
    }

    #[test]
    fn a_new_certificate_and_a_busy_port_are_said() {
        let mut state = running(&[LINK]);
        if let Some(info) = &mut state.info {
            info.certificate = Certificate::Reissued;
            info.preferred_port = false;
            info.port = 51234;
        }
        let rows = rows(&state, Language::English);
        let reissued = rows.iter().find(|r| r.id == "browserReissued").unwrap();
        assert!(reissued.warning && reissued.attached);
        let busy = rows.iter().find(|r| r.id == "browserPortBusy").unwrap();
        assert!(busy.title.contains("45991") && busy.title.contains("51234"));
    }

    #[test]
    fn other_addresses_copy_their_own_link() {
        let second = "https://10.0.0.7:45991/#t=q8Zk3VtW0bHn5sLc2pRj1A";
        let third = "https://[fe80::1]:45991/#t=q8Zk3VtW0bHn5sLc2pRj1A";
        let state = running(&[LINK, second, third]);
        let rows = rows(&state, Language::Turkish);
        let caption = rows.iter().find(|r| r.id == "browserOther").unwrap();
        assert_eq!(caption.title, "Diğer Adresler");
        let first = rows.iter().find(|r| r.id == "browserAddr0").unwrap();
        assert_eq!(first.title, "10.0.0.7");
        assert_eq!(first.detail.as_deref(), Some(second));
        let v6 = rows.iter().find(|r| r.id == "browserAddr1").unwrap();
        assert_eq!(v6.title, "fe80::1");
        assert_eq!(
            action(
                &state,
                &Change::Run {
                    row: "browserAddr1"
                }
            ),
            Some(LudusAction::CopyLink(third.into()))
        );
        assert_eq!(
            action(
                &state,
                &Change::Run {
                    row: "browserAddr2"
                }
            ),
            None
        );
        assert_eq!(
            action(
                &BrowserState::default(),
                &Change::Run { row: "browserCopy" }
            ),
            None,
            "nothing to copy before the bridge runs"
        );
    }

    #[test]
    fn the_rows_read_in_turkish() {
        let rows = rows(&running(&[LINK]), Language::Turkish);
        let title = |id: &str| rows.iter().find(|r| r.id == id).unwrap().title.clone();
        assert_eq!(title("browserStatus"), "Tarayıcı");
        assert_eq!(title("browserCopy"), "Bağlantıyı Kopyala");
        assert_eq!(title("browserNewLink"), "Yeni Bağlantı");
    }

    #[test]
    fn the_longest_link_fits_the_card_in_whole_pixels() {
        // A long IPv6 address and the highest port: still at least three physical pixels a
        // module with the four-module quiet zone, at the smallest scale (0.68).
        let link = "https://[fe80::1234:5678:9abc:def0]:65535/#t=q8Zk3VtW0bHn5sLc2pRj1A";
        let code = qrcodegen::QrCode::encode_text(link, qrcodegen::QrCodeEcc::Medium).unwrap();
        let total = (code.size() as usize + 2 * crate::ui::theme::size::QR_QUIET) as f32;
        let pixels = crate::ui::theme::size::QR_CARD * 0.68;
        assert!((pixels / total).floor() >= 3.0, "{} modules", code.size());
    }

    #[test]
    fn hosts_come_out_of_links() {
        assert_eq!(host_of(LINK), "192.168.178.20");
        assert_eq!(host_of("https://[fe80::1]:45991/#t=x"), "fe80::1");
        assert_eq!(host_of("https://localhost/#t=x"), "localhost");
    }

    #[test]
    fn the_phone_steps_and_the_shared_screen_are_said() {
        let rows = rows(&running(&[LINK]), Language::English);
        let at = |id: &str| rows.iter().find(|r| r.id == id).unwrap();
        let cert = at("browserCertNote");
        assert!(cert.step, "the one thing to do on the phone: detail ink");
        for words in ["Show Details", "visit this website", "Advanced", "Proceed"] {
            assert!(cert.title.contains(words), "{words}");
        }
        let share = at("browserShareNote");
        assert!(share.attached && share.title.contains("New Link"));
        let list = ids(&rows);
        let pos = |id: &str| list.iter().position(|r| *r == id).unwrap();
        assert_eq!(pos("browserShareNote"), pos("browserNewLink") + 1);
        let turkish = rows_tr();
        assert!(
            turkish
                .iter()
                .find(|r| r.id == "browserShareNote")
                .unwrap()
                .title
                .contains("Yeni Bağlantı")
        );
    }

    fn rows_tr() -> Vec<Row> {
        rows(&running(&[LINK]), Language::Turkish)
    }
}
