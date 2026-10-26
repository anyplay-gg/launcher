//! LudusCtrl on the launcher's side (docs/DESIGN.md 18, "LudusCtrl"): starts and stops the
//! LudusCtrl server with its option of Phone as Controller, asks the player about a phone that
//! wants to pair, keeps the list of paired phones and says which phone holds the pad.
//!
//! The server itself (protocol, pairing, keys, the pad slot) is the core's,
//! `anyps5_core::remote::ludusctrl` (`docs/ludusctrl-api.md`). This module meets it only through
//! [`LudusReceiver`] and a [`Start`] function, so the core plugs in at one place,
//! [`start_receiver`]. The shown state is the model's [`LudusState`], the same shape as the phone
//! server's [`PhoneState`](crate::app::phone::PhoneState).

use std::sync::{
    Arc, Mutex,
    mpsc::{Receiver, Sender, channel},
};

use anyps5_core::paths::AppPaths;
use anyps5_core::remote::{MAX_DEVICES, VirtualPadStatus, vpad};

use crate::app::model::Model;
use crate::screens::ludus::LudusAction;
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
    /// Nobody feeds the pad (`LudusStatus::Waiting`).
    Waiting,
}

/// The server as the launcher uses it. Every call returns at once; the server does its work on
/// its own thread and reports through the events.
pub trait LudusReceiver: Send + Sync {
    /// The UDP port really bound, for typing into the app next to the PC's address.
    fn port(&self) -> u16;
    /// The paired phones, oldest first.
    fn paired(&self) -> Vec<LudusPhone>;
    /// The player's answer to [`LudusEvent::Pairing`].
    fn respond(&self, id: u64, allow: bool);
    /// Forget a paired phone; a phone that holds the pad lets go of it at once.
    fn forget(&self, phone_id: &str);
    /// Stop listening and unplug the pad.
    fn stop(&self);
}

/// The server's callback: queue and wake the UI, never block.
pub type Events = Arc<dyn Fn(LudusEvent) + Send + Sync>;
/// Starts the server with its files under the launcher's config folder.
pub type Start = fn(&AppPaths, Events) -> std::io::Result<Arc<dyn LudusReceiver>>;

/// The core's server. Until `anyps5_core::remote::ludusctrl` is in this build the option says it
/// could not start. The binding: `LudusServer::start(LudusConfig::new(LudusStore::file_in(paths)),
/// events)`, its `status` events mapped to [`LudusEvent`], and the virtual pad handed to it with
/// `set_pad` at once (the LudusCtrl pad also drives the launcher), shared with the phone server's
/// pad so the system sees one controller.
pub fn start_receiver(
    _paths: &AppPaths,
    _events: Events,
) -> std::io::Result<Arc<dyn LudusReceiver>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "the LudusCtrl server is not part of this build",
    ))
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
    last_pad_check: f64,
}

/// LudusCtrl runs while Phone as Controller and its LudusCtrl option are both on.
pub fn wanted(prefs: &anyps5_core::settings::Preferences) -> bool {
    prefs.phone_remote && prefs.phone_ludusctrl
}

impl LudusService {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>, simulated: bool) -> Self {
        Self::with_start(start_receiver, wake, simulated)
    }

    pub fn with_start(start: Start, wake: Arc<dyn Fn() + Send + Sync>, simulated: bool) -> Self {
        let (tx, rx) = channel();
        Self {
            start,
            wake,
            simulated,
            tx,
            rx,
            receiver: None,
            last_pad_check: f64::NEG_INFINITY,
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
            receiver.stop();
        }
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
        // The driver can be installed while the page is open: look again now and then.
        if want
            && model.overlay == Some(crate::app::overlay::Overlay::Settings)
            && model.now - self.last_pad_check > PAD_CHECK_EVERY
        {
            self.last_pad_check = model.now;
            model.ludus.pad = Some(vpad::status());
        }
        for command in std::mem::take(&mut model.ludus.commands) {
            self.command(model, command);
        }
        while let Ok(event) = self.rx.try_recv() {
            self.event(model, event);
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
            // The host opens the page; nothing for the server.
            LudusAction::OpenProjectPage => {}
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
            }
            LudusEvent::Waiting => {
                model.ludus.connected = None;
                model.ludus.controller = false;
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
        events: Option<Events>,
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
        fn stop(&self) {
            calls().lock().unwrap().stopped += 1;
        }
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
        LudusService::with_start(fake_start, Arc::new(|| {}), false)
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
        let mut service = LudusService::with_start(start_receiver, Arc::new(|| {}), false);
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
    fn previews_never_start_the_server() {
        let _serial = serial();
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        on(&mut model);
        let mut service = LudusService::with_start(fake_start, Arc::new(|| {}), true);
        service.sync(&mut model);
        assert!(!service.is_running() && !model.ludus.failed);
    }
}
