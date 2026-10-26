//! LudusCtrl's browser client on the launcher's side (docs/DESIGN.md 18, "LudusCtrl browser
//! client"): a phone opens a page from this PC, and its browser passes on the controller connected
//! to it. No app, no pairing: the token in the link is the consent.
//!
//! The bridge itself (HTTPS, WebSocket, the token, the certificate) is the core's, a part of the
//! LudusCtrl server that feeds the same pad slot. The launcher meets it only through
//! [`BrowserBridge`], which every [`LudusReceiver`](crate::app::ludus::LudusReceiver) also is, so
//! the core plugs in at one place: `impl BrowserBridge for LudusServer` in `app::ludus`. Until
//! the core's bridge lands, that impl keeps the defaults below, and turning the option on says
//! that the browser client could not start.

use std::path::PathBuf;

use anyps5_core::paths::AppPaths;

/// The TCP port the page and its WebSocket are served on. A busy port falls back to a free one;
/// the link carries whichever is bound and the settings say so.
pub const LUDUS_WEB_PORT: u16 = 45991;

/// What the bridge is started with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowserConfig {
    /// Where the certificate, its key and the addresses it covers are kept.
    pub cert_file: PathBuf,
    /// [`LUDUS_WEB_PORT`].
    pub port: u16,
    /// The page's language, `"en"` or `"tr"` (the launcher's).
    pub language: &'static str,
}

impl BrowserConfig {
    pub fn new(paths: &AppPaths, language: &'static str) -> BrowserConfig {
        BrowserConfig {
            cert_file: cert_file_in(paths),
            port: LUDUS_WEB_PORT,
            language,
        }
    }
}

/// `<config>/LudusCtrlWeb.json`, next to `LudusCtrl.json`.
pub fn cert_file_in(paths: &AppPaths) -> PathBuf {
    paths.config.join("LudusCtrlWeb.json")
}

/// What the settings show of a running bridge.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrowserInfo {
    /// `https://<address>:<port>/#t=<token>`, one per LAN address, the most likely first. The
    /// first is the QR code's.
    pub urls: Vec<String>,
    /// The port really bound.
    pub port: u16,
    /// Whether that is [`LUDUS_WEB_PORT`].
    pub preferred_port: bool,
    /// The certificate was made again at this start (new addresses, expiry): every phone warns
    /// once more.
    pub reissued: bool,
}

/// The browser bridge as the launcher uses it. Every call returns at once. Who holds the pad
/// comes through the LudusCtrl server's status (`LudusVia::Browser`).
pub trait BrowserBridge {
    /// Starts serving the page with a new token. `Err` carries the reason for the log.
    fn start_browser(&self, config: BrowserConfig) -> Result<BrowserInfo, String> {
        let _ = config;
        Err("the browser client is not part of this build yet".into())
    }
    /// A new token: the browser that holds the pad is closed and the old link stops working. The
    /// certificate stays. `None` when the bridge is not running.
    fn new_browser_link(&self) -> Option<BrowserInfo> {
        None
    }
    /// Stops serving; a browser that holds the pad lets go of it.
    fn stop_browser(&self) {}
}

/// What the settings page and the top bar show of the browser client.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrowserState {
    /// The bridge runs and serves this link.
    pub info: Option<BrowserInfo>,
    /// The bridge could not start (the reason is in the log).
    pub failed: bool,
    /// A browser holds the pad: its address.
    pub peer: Option<String>,
}

/// The browser client runs while Phone as Controller, LudusCtrl and its browser option are on.
pub fn wanted(prefs: &anyps5_core::settings::Preferences) -> bool {
    crate::app::ludus::wanted(prefs) && prefs.phone_ludusctrl_browser
}

/// The page's language code for the launcher's language.
pub fn page_language(language: crate::ui::i18n::Language) -> &'static str {
    match language {
        crate::ui::i18n::Language::Turkish => "tr",
        crate::ui::i18n::Language::English => "en",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Plain;
    impl BrowserBridge for Plain {}

    #[test]
    fn the_stub_refuses_to_start_and_has_no_link() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let config = BrowserConfig::new(&paths, "en");
        assert_eq!(config.port, LUDUS_WEB_PORT);
        assert!(config.cert_file.ends_with("LudusCtrlWeb.json"));
        assert!(Plain.start_browser(config).is_err());
        assert!(Plain.new_browser_link().is_none());
    }

    #[test]
    fn the_option_needs_both_switches_above_it() {
        let mut prefs = anyps5_core::settings::Preferences::first_run();
        assert!(!prefs.phone_ludusctrl_browser, "off by default");
        prefs.phone_ludusctrl_browser = true;
        assert!(!wanted(&prefs));
        prefs.phone_remote = true;
        assert!(!wanted(&prefs));
        prefs.phone_ludusctrl = true;
        assert!(wanted(&prefs));
    }
}
