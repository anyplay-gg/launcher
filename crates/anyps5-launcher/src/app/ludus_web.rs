//! LudusCtrl's browser client on the launcher's side (docs/DESIGN.md 18, "LudusCtrl browser
//! client"): a phone opens a page from this PC, and its browser passes on the controller connected
//! to it. No app, no pairing: the token in the link is the consent. It has no switch of its own:
//! it runs whenever LudusCtrl does ([`crate::app::ludus::wanted`]).
//!
//! The bridge itself (HTTPS, WebSocket, the token, the certificate) is the core's, a part of the
//! LudusCtrl server that feeds the same pad slot (`docs/ludusctrl-web-api.md`). The launcher meets
//! it only through [`BrowserBridge`], which every
//! [`LudusReceiver`](crate::app::ludus::LudusReceiver) also is, so the core plugs in at one place:
//! `impl BrowserBridge for LudusServer` in `app::ludus`.

use anyps5_core::paths::AppPaths;

pub use anyps5_core::remote::{BrowserConfig, BrowserInfo, Certificate, LUDUS_WEB_PORT};

/// The launcher's: port 45991 on every interface, the certificate in
/// `<config>/LudusCtrlWeb.json`, next to `LudusCtrl.json`.
pub fn config_for_launcher(paths: &AppPaths) -> BrowserConfig {
    BrowserConfig::new(BrowserConfig::file_in(paths))
}

/// The browser bridge as the launcher uses it. Every call returns at once. Who holds the pad
/// comes through the LudusCtrl server's status (`LudusVia::Browser`).
pub trait BrowserBridge {
    /// Starts serving the page with a new token (restarts it when it runs). `Err` carries the
    /// reason for the log.
    fn start_browser(&self, config: BrowserConfig) -> Result<BrowserInfo, String>;
    /// A new token: the browser that holds the pad is closed and the old link stops working. The
    /// certificate stays. `None` when the bridge is not running.
    fn new_browser_link(&self) -> Option<BrowserInfo>;
    /// Stops serving; a browser that holds the pad lets go of it.
    fn stop_browser(&self);
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
    /// And what it runs on, from its User-Agent ("iPhone", "Android"), when it said.
    pub device: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launchers_config_serves_every_interface_on_the_port() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let config = config_for_launcher(&paths);
        assert_eq!(config.port, LUDUS_WEB_PORT);
        assert!(config.cert_file.ends_with("LudusCtrlWeb.json"));
        assert!(config.cert_file.starts_with(&paths.config));
        assert_eq!(config.bind, None, "every interface");
    }
}
