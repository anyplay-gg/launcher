//! LudusCtrl on the launcher's side (docs/DESIGN.md 18, "LudusCtrl"): starts and stops the
//! LudusCtrl server with its option of Phone as Controller, asks the player about a phone that
//! wants to pair, keeps the list of paired phones and says which phone holds the pad.
//!
//! The server itself (protocol, pairing, keys, the pad slot) is the core's,
//! `anyps5_core::remote::ludusctrl` (`docs/ludusctrl-api.md`). This module meets it only through
//! [`LudusReceiver`] and a [`Start`] function, so the core plugs in at one place,
//! [`start_receiver`]. The shown state is the model's [`LudusState`], the same shape as the phone
//! server's [`PhoneState`](crate::app::phone::PhoneState). The virtual controller is the one the
//! phone server also drives ([`SharedPad`]), handed to the server as soon as it runs. The browser
//! client, an option of LudusCtrl, is a part of the same server ([`crate::app::ludus_web`]).

use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender, channel},
};

use anyps5_core::paths::AppPaths;
use anyps5_core::remote::{
    LudusConfig, LudusEvents, LudusServer, LudusStatus, LudusStore, LudusVia, MAX_DEVICES,
    PadTarget, PairingRequest, VirtualPadStatus, vpad,
};

use crate::app::ludus_web::{self, BrowserBridge, BrowserConfig, BrowserState};
use crate::app::model::Model;
use crate::app::pad::SharedPad;
use crate::screens::ludus::LudusAction;
use crate::screens::ludus_web_strings::BrowserText;
use crate::screens::phone_strings::PhoneText;
use crate::ui::i18n::Arg;
use crate::ui::icons::Icon;

/// A phone that asks to be paired: its name and the six-digit code it shows too. The core's
/// `PairingRequest` (`id`, `device_name`, `code` as "260 329").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LudusPairing {
    /// The server's handle for the answer.
    pub id: u64,
    pub phone_name: String,
    /// Six digits, with or without the space after the third.
    pub code: String,
}

/// A paired LudusCtrl phone (the core's `LudusPhone`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LudusPhone {
    /// The phone's id, for [`LudusReceiver::forget`].
    pub id: String,
    pub name: String,
    /// When it was paired, RFC 3339 (`2026-10-24T18:14:03Z`); the page shows the date.
    pub added: String,
}

/// What the server tells the launcher, from its own thread (the core's `LudusEvents`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LudusEvent {
    /// A phone asks to be paired; the player answers with [`LudusReceiver::respond`].
    Pairing(LudusPairing),
    /// The question is over: answered, 60 s without an answer, or the phone left.
    PairingEnded(u64),
    /// A phone holds the pad (`LudusStatus::Linked` with `LudusVia::App`). `controller`: a
    /// controller is connected to the phone; without one the pad is held at neutral.
    Linked { name: String, controller: bool },
    /// A browser client holds the pad (`LudusStatus::Linked` with `LudusVia::Browser`): the
    /// phone's address, and what it runs on when its browser said.
    Browser {
        peer: String,
        device: Option<String>,
    },
    /// Nobody feeds the pad (`LudusStatus::Waiting`).
    Waiting,
}

/// The server as the launcher uses it. Every call returns at once; the server does its work on
/// its own thread and reports through the events. Its browser client is the [`BrowserBridge`] part.
pub trait LudusReceiver: Send + Sync + BrowserBridge {
    /// The UDP port really bound, for typing into the app next to the PC's address.
    fn port(&self) -> u16;
    /// The paired phones, oldest first.
    fn paired(&self) -> Vec<LudusPhone>;
    /// The player's answer to [`LudusEvent::Pairing`].
    fn respond(&self, id: u64, allow: bool);
    /// Forget a paired phone; a phone that holds the pad lets go of it at once.
    fn forget(&self, phone_id: &str);
    /// The virtual controller the server writes to, or none (no driver).
    fn set_pad(&self, pad: Option<Arc<dyn PadTarget>>);
    /// Stop listening and unplug the pad.
    fn stop(&self);
}

/// The server's callback: queue and wake the UI, never block.
pub type Events = Arc<dyn Fn(LudusEvent) + Send + Sync>;
/// Starts the server with its files under the launcher's config folder.
pub type Start = fn(&AppPaths, Events) -> std::io::Result<Arc<dyn LudusReceiver>>;

/// The core's server on every interface with the beacon on, as the app expects it. Its events
/// become [`LudusEvent`]s.
pub fn start_receiver(paths: &AppPaths, events: Events) -> std::io::Result<Arc<dyn LudusReceiver>> {
    let config = LudusConfig::new(LudusStore::file_in(paths));
    let server = LudusServer::start(config, Arc::new(Forward(events)))
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(Arc::new(server))
}

/// The core's events as the launcher's.
struct Forward(Events);

fn event_of(status: LudusStatus) -> LudusEvent {
    match status {
        LudusStatus::Linked {
            via: LudusVia::App { name },
            controller,
            ..
        } => LudusEvent::Linked { name, controller },
        // The core's bridge adds `device` (the User-Agent's coarse name); it goes here then.
        LudusStatus::Linked {
            via: LudusVia::Browser { peer, .. },
            ..
        } => LudusEvent::Browser {
            peer: peer.to_string(),
            device: None,
        },
        LudusStatus::Waiting => LudusEvent::Waiting,
    }
}

impl LudusEvents for Forward {
    fn pairing(&self, request: PairingRequest) {
        (self.0)(LudusEvent::Pairing(LudusPairing {
            id: request.id,
            phone_name: request.device_name,
            code: request.code,
        }));
    }
    fn pairing_ended(&self, id: u64) {
        (self.0)(LudusEvent::PairingEnded(id));
    }
    fn status(&self, status: LudusStatus) {
        (self.0)(event_of(status));
    }
}

/// The core's browser bridge is not built yet (T55 step 2, `docs/ludusctrl-web-api.md` on the
/// server's branch): the defaults say so. When it lands, these three forward to
/// `LudusServer::start_browser`, `new_browser_link` and `stop_browser`.
impl BrowserBridge for LudusServer {}

impl LudusReceiver for LudusServer {
    fn port(&self) -> u16 {
        LudusServer::port(self)
    }
    fn paired(&self) -> Vec<LudusPhone> {
        LudusServer::paired(self)
            .into_iter()
            .map(|p| LudusPhone {
                id: p.id,
                name: p.name,
                added: p.added,
            })
            .collect()
    }
    fn respond(&self, id: u64, allow: bool) {
        LudusServer::respond(self, id, allow);
    }
    fn forget(&self, phone_id: &str) {
        LudusServer::forget(self, phone_id);
    }
    fn set_pad(&self, pad: Option<Arc<dyn PadTarget>>) {
        LudusServer::set_pad(self, pad);
    }
    fn stop(&self) {
        LudusServer::stop(self);
    }
}

/// What the settings page and the top bar show of LudusCtrl.
#[derive(Clone, Debug, Default)]
pub struct LudusState {
    /// The server could not start.
    pub failed: bool,
    /// The phone that holds the pad.
    pub connected: Option<String>,
    /// Whether that phone has a controller connected to it.
    pub controller: bool,
    /// A phone asks to be paired and the player has not answered.
    pub pairing: Option<LudusPairing>,
    /// The UDP port the server listens on.
    pub port: Option<u16>,
    pub paired: Vec<LudusPhone>,
    /// Whether the system can host the virtual controller; `None` until looked at.
    pub pad: Option<VirtualPadStatus>,
    /// The browser client.
    pub browser: BrowserState,
    /// Asked by the settings page, done by the next frame.
    pub commands: Vec<LudusAction>,
}

/// How often the driver is looked at while the settings are open.
const PAD_CHECK_EVERY: f64 = 2.0;

/// The server and what is kept between frames. Lives in the app.
pub struct LudusService {
    start: Start,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// Previews never open a port.
    simulated: bool,
    tx: Sender<LudusEvent>,
    rx: Receiver<LudusEvent>,
    receiver: Option<Arc<dyn LudusReceiver>>,
    /// The one virtual controller, shared with the phone server.
    shared_pad: Arc<SharedPad>,
    /// The server has the controller (it may not: no driver yet).
    pad_given: bool,
    last_pad_check: f64,
}

/// LudusCtrl runs while Phone as Controller and its LudusCtrl option are both on.
pub fn wanted(prefs: &anyps5_core::settings::Preferences) -> bool {
    prefs.phone_remote && prefs.phone_ludusctrl
}

impl LudusService {
    pub fn new(
        wake: Arc<dyn Fn() + Send + Sync>,
        simulated: bool,
        shared_pad: Arc<SharedPad>,
    ) -> Self {
        Self::with_start(start_receiver, wake, simulated, shared_pad)
    }

    pub fn with_start(
        start: Start,
        wake: Arc<dyn Fn() + Send + Sync>,
        simulated: bool,
        shared_pad: Arc<SharedPad>,
    ) -> Self {
        let (tx, rx) = channel();
        Self {
            start,
            wake,
            simulated,
            tx,
            rx,
            receiver: None,
            shared_pad,
            pad_given: false,
            last_pad_check: f64::NEG_INFINITY,
        }
    }

    /// Hands the server the controller once there is one. The LudusCtrl pad also drives the
    /// launcher, so it is given at once, not only while a game runs.
    fn give_pad(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        if self.pad_given {
            return;
        }
        if let Some(pad) = self.shared_pad.get() {
            receiver.set_pad(Some(pad));
            self.pad_given = true;
        }
    }

    pub fn is_running(&self) -> bool {
        self.receiver.is_some()
    }

    fn start(&mut self, model: &mut Model) {
        let tx = Mutex::new(self.tx.clone());
        let wake = self.wake.clone();
        let events: Events = Arc::new(move |event| {
            if let Ok(tx) = tx.lock() {
                let _ = tx.send(event);
            }
            wake();
        });
        match (self.start)(&model.paths, events) {
            Ok(receiver) => {
                model.ludus.failed = false;
                model.ludus.port = Some(receiver.port());
                model.ludus.paired = receiver.paired();
                self.receiver = Some(receiver);
                self.pad_given = false;
                self.give_pad();
            }
            Err(error) => {
                tracing::warn!("the LudusCtrl server could not start: {error}");
                model.ludus.failed = true;
            }
        }
        model.ludus.pad = Some(vpad::status());
    }

    fn stop(&mut self, model: &mut Model) {
        if let Some(receiver) = self.receiver.take() {
            // The browser client is a part of the server and goes with it.
            if model.ludus.browser.info.is_some() {
                receiver.stop_browser();
            }
            receiver.stop();
        }
        self.pad_given = false;
        while self.rx.try_recv().is_ok() {}
        let pad = model.ludus.pad.take();
        model.ludus = LudusState {
            pad,
            ..LudusState::default()
        };
    }

    /// Every frame: the server follows the settings, the page's commands run, the server's
    /// events reach the model.
    pub fn sync(&mut self, model: &mut Model) {
        let want = wanted(&model.prefs) && !self.simulated;
        if want && !self.is_running() && !model.ludus.failed {
            self.start(model);
        } else if !want && (self.is_running() || model.ludus.failed) {
            self.stop(model);
        }
        self.sync_browser(model, want && ludus_web::wanted(&model.prefs));
        // The driver can be installed while the page is open: look again now and then.
        if want
            && model.overlay == Some(crate::app::overlay::Overlay::Settings)
            && model.now - self.last_pad_check > PAD_CHECK_EVERY
        {
            self.last_pad_check = model.now;
            model.ludus.pad = Some(vpad::status());
            // The driver may have been installed since the start.
            if model.ludus.pad == Some(VirtualPadStatus::Ready) {
                self.give_pad();
            }
        }
        for command in std::mem::take(&mut model.ludus.commands) {
            self.command(model, command);
        }
        while let Ok(event) = self.rx.try_recv() {
            self.event(model, event);
        }
    }

    /// The browser client follows its option, on the running server only. Stopping the server
    /// closes it too.
    fn sync_browser(&mut self, model: &mut Model, want: bool) {
        let Some(receiver) = self.receiver.clone() else {
            return;
        };
        let browser = &model.ludus.browser;
        if want && browser.info.is_none() && !browser.failed {
            let config = BrowserConfig::for_launcher(&model.paths, model.language());
            match receiver.start_browser(config) {
                Ok(info) => {
                    model.ludus.browser = BrowserState {
                        info: Some(info),
                        ..BrowserState::default()
                    };
                }
                Err(error) => {
                    tracing::warn!("the LudusCtrl browser client could not start: {error}");
                    model.ludus.browser.failed = true;
                }
            }
        } else if !want && (browser.info.is_some() || browser.failed) {
            if browser.info.is_some() {
                receiver.stop_browser();
            }
            model.ludus.browser = BrowserState::default();
        }
    }

    fn command(&mut self, model: &mut Model, command: LudusAction) {
        let Some(receiver) = self.receiver.clone() else {
            return;
        };
        match command {
            LudusAction::Respond(allow) => {
                if let Some(request) = model.ludus.pairing.take() {
                    receiver.respond(request.id, allow);
                    // The server turns an Allow past eight phones into a refusal; say why.
                    if allow && model.ludus.paired.len() >= MAX_DEVICES {
                        let text = PhoneText::ToastFull.text(model.language());
                        model.toast(text, Icon::WARNING);
                    }
                }
            }
            LudusAction::Forget(id) => {
                let name = model
                    .ludus
                    .paired
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.name.clone())
                    .unwrap_or_default();
                receiver.forget(&id);
                model.ludus.paired.retain(|p| p.id != id);
                let text = PhoneText::ToastRemoved.format(model.language(), &[Arg::Str(&name)]);
                model.toast(text, Icon::CHECK_CIRCLE);
            }
            LudusAction::NewLink => {
                if let Some(info) = receiver.new_browser_link() {
                    model.ludus.browser.info = Some(info);
                    model.ludus.browser.peer = None;
                    model.ludus.browser.device = None;
                }
            }
            // The host opens the page or copies the link; nothing for the server.
            LudusAction::OpenProjectPage | LudusAction::CopyLink(_) => {}
        }
    }

    fn event(&mut self, model: &mut Model, event: LudusEvent) {
        let language = model.language();
        match event {
            LudusEvent::Pairing(request) => {
                let name = request.phone_name.clone();
                model.ludus.pairing = Some(request);
                let idle = model.onboarded() && model.starting.is_none() && model.running.is_none();
                if idle && model.overlay.is_none() {
                    model.show_settings(Some(anyps5_core::links::SettingsPage::Phone));
                } else {
                    let text = PhoneText::ToastPair.format(language, &[Arg::Str(&name)]);
                    model.toast(text, Icon::DEVICE_MOBILE);
                }
            }
            LudusEvent::PairingEnded(id) => {
                if model.ludus.pairing.as_ref().is_some_and(|p| p.id == id) {
                    model.ludus.pairing = None;
                }
                self.refresh_paired(model);
            }
            LudusEvent::Linked { name, controller } => {
                if model.ludus.connected.as_deref() != Some(name.as_str()) {
                    let text = PhoneText::ToastConnected.format(language, &[Arg::Str(&name)]);
                    model.toast(text, Icon::GAME_CONTROLLER);
                    self.refresh_paired(model);
                }
                model.ludus.connected = Some(name);
                model.ludus.controller = controller;
                model.ludus.browser.peer = None;
                model.ludus.browser.device = None;
            }
            LudusEvent::Browser { peer, device } => {
                if model.ludus.browser.peer.as_deref() != Some(peer.as_str()) {
                    let text = BrowserText::ToastConnected.text(language);
                    model.toast(text, Icon::GAME_CONTROLLER);
                }
                model.ludus.connected = None;
                model.ludus.controller = false;
                model.ludus.browser.peer = Some(peer);
                model.ludus.browser.device = device;
            }
            LudusEvent::Waiting => {
                model.ludus.connected = None;
                model.ludus.controller = false;
                model.ludus.browser.peer = None;
                model.ludus.browser.device = None;
            }
        }
    }

    fn refresh_paired(&self, model: &mut Model) {
        if let Some(receiver) = &self.receiver {
            model.ludus.paired = receiver.paired();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::settings::Preferences;
    use std::sync::OnceLock;

    /// What the fake server was asked, shared with the test (the start function is a plain fn).
    #[derive(Default)]
    struct Calls {
        responses: Vec<(u64, bool)>,
        forgotten: Vec<String>,
        stopped: usize,
        pads: Vec<Option<Arc<dyn PadTarget>>>,
        events: Option<Events>,
        browser_starts: Vec<BrowserConfig>,
        browser_stops: usize,
        browser_fails: bool,
        new_links: usize,
    }

    fn calls() -> &'static Mutex<Calls> {
        static CALLS: OnceLock<Mutex<Calls>> = OnceLock::new();
        CALLS.get_or_init(Default::default)
    }

    /// The tests share the fake's record, so they run one at a time.
    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        let guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        *calls().lock().unwrap() = Calls::default();
        guard
    }

    struct Fake;

    fn browser_info(token: &str) -> crate::app::ludus_web::BrowserInfo {
        crate::app::ludus_web::BrowserInfo {
            urls: vec![format!("https://192.168.1.23:45991/#t={token}")],
            port: ludus_web::LUDUS_WEB_PORT,
            preferred_port: true,
            certificate: ludus_web::Certificate::Kept,
            fingerprint: String::new(),
        }
    }

    impl BrowserBridge for Fake {
        fn start_browser(
            &self,
            config: BrowserConfig,
        ) -> Result<crate::app::ludus_web::BrowserInfo, String> {
            let mut calls = calls().lock().unwrap();
            calls.browser_starts.push(config);
            if calls.browser_fails {
                Err("port in use".into())
            } else {
                Ok(browser_info("first"))
            }
        }
        fn new_browser_link(&self) -> Option<crate::app::ludus_web::BrowserInfo> {
            calls().lock().unwrap().new_links += 1;
            Some(browser_info("second"))
        }
        fn stop_browser(&self) {
            calls().lock().unwrap().browser_stops += 1;
        }
    }

    impl LudusReceiver for Fake {
        fn port(&self) -> u16 {
            45989
        }
        fn paired(&self) -> Vec<LudusPhone> {
            let forgotten = calls().lock().unwrap().forgotten.clone();
            [("a", "Pixel 8"), ("b", "Galaxy S23")]
                .into_iter()
                .filter(|(id, _)| !forgotten.iter().any(|f| f == id))
                .map(|(id, name)| LudusPhone {
                    id: id.into(),
                    name: name.into(),
                    added: "2026-10-24T18:14:03Z".into(),
                })
                .collect()
        }
        fn respond(&self, id: u64, allow: bool) {
            calls().lock().unwrap().responses.push((id, allow));
        }
        fn forget(&self, phone_id: &str) {
            calls().lock().unwrap().forgotten.push(phone_id.into());
        }
        fn set_pad(&self, pad: Option<Arc<dyn PadTarget>>) {
            calls().lock().unwrap().pads.push(pad);
        }
        fn stop(&self) {
            calls().lock().unwrap().stopped += 1;
        }
    }

    fn failing_start(_: &AppPaths, _: Events) -> std::io::Result<Arc<dyn LudusReceiver>> {
        Err(std::io::Error::new(std::io::ErrorKind::AddrInUse, "busy"))
    }

    fn inert_pad() -> Arc<SharedPad> {
        SharedPad::with_open(Box::new(|_| Ok(anyps5_core::remote::VirtualPad::inert())))
    }

    fn fake_start(_paths: &AppPaths, events: Events) -> std::io::Result<Arc<dyn LudusReceiver>> {
        calls().lock().unwrap().events = Some(events);
        Ok(Arc::new(Fake))
    }

    fn emit(event: LudusEvent) {
        let events = calls().lock().unwrap().events.clone().expect("started");
        events(event);
    }

    fn model(dir: &tempfile::TempDir) -> Model {
        let prefs = Preferences {
            onboarded: true,
            ..Preferences::first_run()
        };
        Model::new(prefs, AppPaths::under(dir.path()), false)
    }

    /// Phone as Controller and its LudusCtrl option on.
    fn on(model: &mut Model) {
        model.prefs.phone_remote = true;
        model.prefs.phone_ludusctrl = true;
    }

    fn service() -> LudusService {
        LudusService::with_start(fake_start, Arc::new(|| {}), false, inert_pad())
    }

    #[test]
    fn the_server_runs_only_with_both_switches() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        let mut service = service();
        model.prefs.phone_ludusctrl = true;
        service.sync(&mut model);
        assert!(!service.is_running(), "an option of Phone as Controller");
        model.prefs.phone_remote = true;
        service.sync(&mut model);
        assert!(service.is_running());
        assert_eq!(model.ludus.port, Some(45989));
        assert_eq!(model.ludus.paired.len(), 2);
        assert!(
            model.ludus.pad.is_some(),
            "the driver is looked at on start"
        );
        model.prefs.phone_remote = false;
        service.sync(&mut model);
        assert!(!service.is_running());
        assert_eq!(calls().lock().unwrap().stopped, 1);
        assert!(model.ludus.paired.is_empty() && model.ludus.port.is_none());
    }

    #[test]
    fn a_server_that_cannot_start_says_so_once() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        let mut service =
            LudusService::with_start(failing_start, Arc::new(|| {}), false, inert_pad());
        service.sync(&mut model);
        assert!(model.ludus.failed && !service.is_running());
        service.sync(&mut model);
        assert!(model.ludus.failed, "no retry every frame");
        model.prefs.phone_ludusctrl = false;
        service.sync(&mut model);
        assert!(!model.ludus.failed, "turning it off clears the note");
    }

    #[test]
    fn a_pairing_question_opens_the_page_and_is_answered() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        let mut service = service();
        service.sync(&mut model);
        emit(LudusEvent::Pairing(LudusPairing {
            id: 9,
            phone_name: "Pixel 8".into(),
            code: "260 329".into(),
        }));
        service.sync(&mut model);
        assert_eq!(model.ludus.pairing.as_ref().map(|p| p.id), Some(9));
        assert_eq!(
            model.overlay,
            Some(crate::app::overlay::Overlay::Settings),
            "an idle launcher opens the page"
        );
        model.ludus.commands.push(LudusAction::Respond(true));
        service.sync(&mut model);
        assert_eq!(calls().lock().unwrap().responses, [(9, true)]);
        assert!(model.ludus.pairing.is_none());
    }

    #[test]
    fn the_question_ends_by_itself() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        let mut service = service();
        service.sync(&mut model);
        emit(LudusEvent::Pairing(LudusPairing {
            id: 3,
            phone_name: "Pixel 8".into(),
            code: "000 042".into(),
        }));
        emit(LudusEvent::PairingEnded(4));
        service.sync(&mut model);
        assert!(model.ludus.pairing.is_some(), "another question's end");
        emit(LudusEvent::PairingEnded(3));
        service.sync(&mut model);
        assert!(model.ludus.pairing.is_none());
    }

    #[test]
    fn the_phone_that_holds_the_pad_is_named_and_removing_forgets_it() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        let mut service = service();
        service.sync(&mut model);
        emit(LudusEvent::Linked {
            name: "Pixel 8".into(),
            controller: true,
        });
        service.sync(&mut model);
        assert_eq!(model.ludus.connected.as_deref(), Some("Pixel 8"));
        assert!(model.ludus.controller);
        model.ludus.commands.push(LudusAction::Forget("a".into()));
        service.sync(&mut model);
        assert_eq!(calls().lock().unwrap().forgotten, ["a"]);
        assert_eq!(model.ludus.paired.len(), 1);
        assert_eq!(model.ludus.paired[0].name, "Galaxy S23");
        emit(LudusEvent::Waiting);
        service.sync(&mut model);
        assert!(model.ludus.connected.is_none() && !model.ludus.controller);
    }

    #[test]
    fn the_server_gets_the_one_shared_controller_at_once() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        let shared = inert_pad();
        let mut service =
            LudusService::with_start(fake_start, Arc::new(|| {}), false, shared.clone());
        service.sync(&mut model);
        service.sync(&mut model);
        let pads = std::mem::take(&mut calls().lock().unwrap().pads);
        assert_eq!(pads.len(), 1, "given once, with the start");
        let given = pads[0].clone().expect("a pad");
        // The phone server asks the same SharedPad: one controller for both.
        let phone_side: Arc<dyn PadTarget> = shared.get().unwrap();
        assert!(std::ptr::addr_eq(
            Arc::as_ptr(&given),
            Arc::as_ptr(&phone_side)
        ));
    }

    #[test]
    fn a_driver_installed_later_reaches_the_server() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        let missing = SharedPad::with_open(Box::new(|_| {
            Err(anyps5_core::remote::VirtualPadError::Unsupported)
        }));
        let mut service = LudusService::with_start(fake_start, Arc::new(|| {}), false, missing);
        service.sync(&mut model);
        assert!(service.is_running(), "the server runs without a pad");
        assert!(calls().lock().unwrap().pads.is_empty());
    }

    #[test]
    fn the_core_status_becomes_the_launchers_event() {
        assert_eq!(
            event_of(LudusStatus::Linked {
                via: LudusVia::App {
                    name: "Pixel 8".into()
                },
                layout: anyps5_core::remote::PadLayout::Xbox,
                controller: true,
                pad_live: true,
            }),
            LudusEvent::Linked {
                name: "Pixel 8".into(),
                controller: true
            }
        );
        assert_eq!(event_of(LudusStatus::Waiting), LudusEvent::Waiting);
        assert_eq!(
            event_of(LudusStatus::Linked {
                via: LudusVia::Browser {
                    peer: "192.168.1.40".parse().unwrap()
                },
                layout: anyps5_core::remote::PadLayout::Unknown,
                controller: false,
                pad_live: true,
            }),
            LudusEvent::Browser {
                peer: "192.168.1.40".into(),
                device: None,
            }
        );
    }

    #[test]
    fn the_browser_client_runs_only_with_its_option_on_the_running_server() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        let mut service = service();
        model.prefs.phone_ludusctrl_browser = true;
        service.sync(&mut model);
        assert!(model.ludus.browser.info.is_none(), "LudusCtrl is off");
        on(&mut model);
        service.sync(&mut model);
        let info = model.ludus.browser.info.clone().expect("running");
        assert!(info.urls[0].ends_with("#t=first"));
        service.sync(&mut model);
        {
            let calls = calls().lock().unwrap();
            assert_eq!(calls.browser_starts.len(), 1, "started once");
            assert!(
                calls.browser_starts[0]
                    .cert_file
                    .ends_with("LudusCtrlWeb.json")
            );
            assert_eq!(calls.browser_starts[0].language, "en");
        }
        model.prefs.phone_ludusctrl_browser = false;
        service.sync(&mut model);
        assert!(model.ludus.browser.info.is_none());
        assert_eq!(calls().lock().unwrap().browser_stops, 1);
        assert!(service.is_running(), "LudusCtrl itself stays");
    }

    #[test]
    fn a_browser_client_that_cannot_start_says_so_once() {
        let _serial = serial();
        calls().lock().unwrap().browser_fails = true;
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        model.prefs.phone_ludusctrl_browser = true;
        let mut service = service();
        service.sync(&mut model);
        service.sync(&mut model);
        assert!(model.ludus.browser.failed && model.ludus.browser.info.is_none());
        assert_eq!(calls().lock().unwrap().browser_starts.len(), 1, "no retry");
        model.prefs.phone_ludusctrl_browser = false;
        service.sync(&mut model);
        assert!(
            !model.ludus.browser.failed,
            "turning it off clears the note"
        );
        assert_eq!(calls().lock().unwrap().browser_stops, 0, "nothing ran");
    }

    #[test]
    fn a_browser_that_holds_the_pad_is_named_and_a_new_link_drops_it() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        model.prefs.phone_ludusctrl_browser = true;
        let mut service = service();
        service.sync(&mut model);
        emit(LudusEvent::Browser {
            peer: "192.168.1.40".into(),
            device: Some("iPhone".into()),
        });
        service.sync(&mut model);
        assert_eq!(model.ludus.browser.peer.as_deref(), Some("192.168.1.40"));
        assert_eq!(model.ludus.browser.device.as_deref(), Some("iPhone"));
        assert!(model.ludus.connected.is_none(), "one holder at a time");
        assert!(model.toast_visible());
        model.ludus.commands.push(LudusAction::NewLink);
        service.sync(&mut model);
        assert_eq!(calls().lock().unwrap().new_links, 1);
        assert!(model.ludus.browser.peer.is_none());
        let info = model.ludus.browser.info.as_ref().unwrap();
        assert!(info.urls[0].ends_with("#t=second"));
        emit(LudusEvent::Browser {
            peer: "192.168.1.40".into(),
            device: Some("iPhone".into()),
        });
        emit(LudusEvent::Linked {
            name: "Pixel 8".into(),
            controller: true,
        });
        service.sync(&mut model);
        assert!(model.ludus.browser.peer.is_none(), "the app took the pad");
        assert!(model.ludus.browser.device.is_none());
        assert_eq!(model.ludus.connected.as_deref(), Some("Pixel 8"));
    }

    #[test]
    fn stopping_ludusctrl_closes_the_browser_client_too() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        model.prefs.phone_ludusctrl_browser = true;
        let mut service = service();
        service.sync(&mut model);
        assert!(model.ludus.browser.info.is_some());
        model.prefs.phone_ludusctrl = false;
        service.sync(&mut model);
        assert!(!service.is_running());
        assert_eq!(calls().lock().unwrap().browser_stops, 1);
        assert_eq!(model.ludus.browser, BrowserState::default());
    }

    #[test]
    fn previews_never_start_the_server() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        let mut service = LudusService::with_start(fake_start, Arc::new(|| {}), true, inert_pad());
        service.sync(&mut model);
        assert!(!service.is_running() && !model.ludus.failed);
    }
}
