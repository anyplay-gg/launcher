//! LudusCtrl's browser client on the "Phone as Controller" page (docs/DESIGN.md 18, "LudusCtrl
//! browser client"): its switch under the LudusCtrl option, who holds the pad, the QR code and the
//! link, the links of the other addresses, a new link, and the notes that explain what the phone
//! will say.

use anyps5_core::settings::Preferences;

use crate::app::ludus_web::{BrowserState, Certificate, LUDUS_WEB_PORT};
use crate::screens::ludus::LudusAction;
use crate::screens::ludus_web_strings::BrowserText;
use crate::screens::phone_strings::PhoneText;
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

/// The browser client's rows. `windows`: the firewall note applies.
pub fn rows(
    prefs: &Preferences,
    browser: &BrowserState,
    windows: bool,
    language: Language,
) -> Vec<Row> {
    let t = |text: BrowserText| text.text(language).to_owned();
    let mut rows = vec![
        Row::toggle(
            "ludusBrowser",
            t(BrowserText::Title),
            prefs.phone_ludusctrl_browser,
        )
        .detail(t(BrowserText::SwitchDetail))
        .icon(Icon::GLOBE)
        .group_start(),
    ];
    if !prefs.phone_ludusctrl_browser {
        return rows;
    }
    if browser.failed {
        rows.push(Row::info(
            "browserFailed",
            t(BrowserText::StartFailed),
            None,
        ));
        return rows;
    }
    let Some(info) = &browser.info else {
        return rows;
    };

    // Who holds the pad.
    let status = match (&browser.peer, &browser.device) {
        (Some(peer), Some(device)) => {
            BrowserText::ConnectedDevice.format(language, &[Arg::Str(device), Arg::Str(peer)])
        }
        (Some(peer), None) => BrowserText::ConnectedTo.format(language, &[Arg::Str(peer)]),
        (None, _) => t(BrowserText::Waiting),
    };
    rows.push(
        Row::info(
            "browserStatus",
            PhoneText::Status.text(language),
            Some(status),
        )
        .icon(Icon::WIFI_HIGH)
        .group_start(),
    );

    // The QR code, the link it encodes and the row that copies it.
    if let Some(url) = info.urls.first() {
        rows.push(
            Row::qr("browserQr", t(BrowserText::QrLabel), url.clone())
                .detail(t(BrowserText::QrHint))
                .group_start(),
        );
        rows.push(Row::link("browserLink", t(BrowserText::Link), url.clone()));
        rows.push(
            Row::command("browserCopy", t(BrowserText::Copy))
                .detail(t(BrowserText::CopyDetail))
                .icon(Icon::LINK),
        );
        rows.push(Row::info("browserCertNote", t(BrowserText::CertNote), None));
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

    // No address to put in a link: say what to do.
    if info.urls.is_empty() {
        rows.push(Row::info("browserNoAddress", t(BrowserText::NoAddress), None).warning());
    }

    // The other addresses of this PC, each a link to copy.
    let others = info.urls.iter().skip(1).zip(OTHER_ROWS);
    if info.urls.len() > 1 {
        rows.push(Row::caption("browserOther", t(BrowserText::Other)).group_start());
        rows.push(Row::info("browserOtherNote", t(BrowserText::OtherNote), None).attached());
    }
    for (url, id) in others {
        rows.push(
            Row::command(id, host_of(url))
                .detail(url.clone())
                .icon(Icon::LINK),
        );
    }

    // A new token, and what may stand in the way.
    rows.push(
        Row::command("browserNewLink", t(BrowserText::NewLink))
            .detail(t(BrowserText::NewLinkDetail))
            .icon(Icon::ARROWS_CLOCKWISE)
            .group_start(),
    );
    if windows {
        rows.push(Row::info("browserFirewall", t(BrowserText::Firewall), None));
    }
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

    fn on() -> Preferences {
        Preferences {
            phone_remote: true,
            phone_ludusctrl: true,
            phone_ludusctrl_browser: true,
            ..Preferences::first_run()
        }
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
    fn off_is_the_switch_alone_and_off_by_default() {
        let prefs = Preferences {
            phone_remote: true,
            phone_ludusctrl: true,
            ..Preferences::first_run()
        };
        let rows = rows(&prefs, &BrowserState::default(), true, Language::English);
        assert_eq!(ids(&rows), ["ludusBrowser"]);
        assert_eq!(rows[0].title, "Browser Client");
        assert_eq!(rows[0].control, Control::Toggle(false));
        assert!(rows[0].group_start);
    }

    #[test]
    fn a_failed_start_says_so_and_keeps_the_switch() {
        let state = BrowserState {
            failed: true,
            ..BrowserState::default()
        };
        let rows = rows(&on(), &state, true, Language::English);
        assert_eq!(ids(&rows), ["ludusBrowser", "browserFailed"]);
        assert_eq!(rows[0].control, Control::Toggle(true));
    }

    #[test]
    fn waiting_shows_the_code_the_link_and_the_notes() {
        let rows = rows(&on(), &running(&[LINK]), true, Language::English);
        assert_eq!(
            ids(&rows),
            [
                "ludusBrowser",
                "browserStatus",
                "browserQr",
                "browserLink",
                "browserCopy",
                "browserCertNote",
                "browserNewLink",
                "browserFirewall",
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
    fn the_firewall_note_is_for_windows() {
        let rows = rows(&on(), &running(&[LINK]), false, Language::English);
        assert!(!ids(&rows).contains(&"browserFirewall"));
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
            let rows = rows(&on(), &state, true, language);
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
            let rows = rows(&on(), &state, true, language);
            let status = rows.iter().find(|r| r.id == "browserStatus").unwrap();
            assert_eq!(status.control, Control::Info(Some(text.into())));
        }
    }

    #[test]
    fn a_pc_without_an_address_says_what_to_do() {
        let rows = rows(&on(), &running(&[]), true, Language::English);
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
        let rows = rows(&on(), &state, true, Language::English);
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
        let rows = rows(&on(), &state, true, Language::English);
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
        let rows = rows(&on(), &state, true, Language::Turkish);
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
        let rows = rows(&on(), &running(&[LINK]), true, Language::Turkish);
        let title = |id: &str| rows.iter().find(|r| r.id == id).unwrap().title.clone();
        assert_eq!(title("ludusBrowser"), "Tarayıcı İstemcisi");
        assert_eq!(title("browserStatus"), "Durum");
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
}
