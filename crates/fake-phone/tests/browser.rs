//! LudusCtrl's browser bridge against `fake-phone browser`, over loopback: the link the settings
//! show, the TLS handshake, the token, input into the one pad, and the neutral packet at the end.

use std::{
    net::Ipv4Addr,
    process::Command,
    sync::{Arc, Mutex},
};

use anyps5_core::remote::{
    BrowserConfig, LudusConfig, LudusEvents, LudusServer, LudusStatus, PadTarget, PairingRequest,
    vpad::PadState,
};

struct Quiet;

impl LudusEvents for Quiet {
    fn pairing(&self, _: PairingRequest) {}
    fn pairing_ended(&self, _: u64) {}
    fn status(&self, _: LudusStatus) {}
}

#[derive(Default)]
struct Pad(Mutex<Vec<PadState>>);

impl PadTarget for Pad {
    fn write(&self, state: &PadState) -> bool {
        self.0.lock().unwrap().push(*state);
        true
    }
    fn release(&self) {}
}

#[test]
fn the_fake_browser_plays_through_the_link() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = LudusConfig::new(dir.path().join("LudusCtrl.json"));
    config.port = 0;
    config.bind = Some(Ipv4Addr::LOCALHOST.into());
    config.beacon = None;
    let server = LudusServer::start(config, Arc::new(Quiet)).unwrap();
    let pad = Arc::new(Pad::default());
    server.set_pad(Some(pad.clone()));
    let mut browser = BrowserConfig::new(dir.path().join("LudusCtrlWeb.json"));
    browser.port = 0;
    browser.bind = Some(Ipv4Addr::LOCALHOST.into());
    let info = server.start_browser(browser).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_fake-phone"))
        .args([
            "browser",
            "--url",
            &info.urls[0],
            "--press",
            "cross+r1",
            "--seconds",
            "1",
        ])
        .output()
        .expect("the fake phone runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    for line in ["< ready", "< padkind:x360", "< ack:0:1:1", "< pong:"] {
        assert!(stdout.contains(line), "{line} missing in:\n{stdout}");
    }
    let writes = pad.0.lock().unwrap();
    assert!(writes.iter().any(|s| s.buttons == 0x1000 | 0x0200));
    assert_eq!(
        writes.last(),
        Some(&PadState::default()),
        "released at the end"
    );
}
