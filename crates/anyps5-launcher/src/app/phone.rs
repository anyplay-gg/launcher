//! Phone as Controller on the launcher's side (docs/DESIGN.md 18): starts and stops the phone
//! server with the setting, shows the library to the paired phone, runs what it asks, forwards
//! earned trophies and hands the phone's pad to the virtual controller while a game runs.
//!
//! The protocol, pairing and security rules are `anyps5_core::remote`; this module is the glue to
//! the model. Every frame [`LauncherApp::sync_phone`] drains what the server's threads reported
//! and works out what the phone should be told.

use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex, Weak,
        mpsc::{Receiver, Sender, channel},
    },
};

use anyps5_core::paths::GameId;
use anyps5_core::remote::{
    Layouts, MAX_DEVICES, Message, OutputPacket, PairedDevice, PairedDevices, PairingRequest,
    RemoteConfig, RemoteEvents, RemoteGame, RemoteInfo, RemoteServer, VirtualPad, VirtualPadStatus,
    pc_name, vpad,
};
use anyps5_core::trophies::{Grade, Trophy};
use base64::{Engine, engine::general_purpose::STANDARD};
use egui::Context;

use crate::app::LauncherApp;
use crate::app::model::{Effect, GameState, Model};
use crate::screens::phone::PhoneAction;
use crate::screens::phone_strings::PhoneText;
use crate::ui::home_button::{HomeButton, Route};
use crate::ui::i18n::Arg;
use crate::ui::icons::Icon;
use crate::ui::input::{Command, InputEvent};

/// What the settings page and the top bar show of the phone server.
#[derive(Clone, Debug)]
pub struct PhoneState {
    /// The server could not start.
    pub failed: bool,
    /// The phone that is welcomed, or the one that said hello and waits to be paired.
    pub connected: Option<String>,
    pub connecting: Option<String>,
    /// A phone asks to be paired and the player has not answered.
    pub pairing: Option<PairingRequest>,
    pub latency_ms: Option<f64>,
    /// Where the server listens, for typing into the app.
    pub info: Option<RemoteInfo>,
    pub paired: Vec<PairedDevice>,
    /// Whether the system can host the virtual controller; `None` until looked at.
    pub pad: Option<VirtualPadStatus>,
    /// Asked by the settings page, done by the next frame.
    pub commands: Vec<PhoneAction>,
    /// Which system the page speaks of (Windows has ViGEmBus, Linux has uinput); the gallery sets
    /// it to show both on one machine.
    pub windows: bool,
}

// `windows` follows the target: only on Windows is it true, which clippy cannot see from Linux.
#[allow(clippy::derivable_impls)]
impl Default for PhoneState {
    fn default() -> Self {
        Self {
            failed: false,
            connected: None,
            connecting: None,
            pairing: None,
            latency_ms: None,
            info: None,
            paired: Vec::new(),
            pad: None,
            commands: Vec::new(),
            windows: cfg!(windows),
        }
    }
}

/// What the server's threads tell the UI thread.
enum Ev {
    Pairing(PairingRequest),
    PairingEnded(u64),
    Connecting(Option<String>),
    Connected(String),
    Disconnected,
    Latency(f64),
    Request(Message),
}

/// The server's callbacks: enqueue and wake the UI, never block.
struct Events {
    tx: Mutex<Sender<Ev>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    home: Arc<HomeButton>,
}

impl Events {
    fn push(&self, event: Ev) {
        if let Ok(tx) = self.tx.lock() {
            let _ = tx.send(event);
        }
        (self.wake)();
    }
}

impl RemoteEvents for Events {
    fn pairing(&self, request: PairingRequest) {
        self.push(Ev::Pairing(request));
    }
    fn pairing_ended(&self, id: u64) {
        self.push(Ev::PairingEnded(id));
    }
    fn connecting(&self, name: Option<String>) {
        self.push(Ev::Connecting(name));
    }
    fn connected(&self, name: String) {
        self.push(Ev::Connected(name));
    }
    fn disconnected(&self) {
        self.push(Ev::Disconnected);
    }
    fn latency(&self, milliseconds: f64) {
        self.push(Ev::Latency(milliseconds));
    }
    fn request(&self, message: Message) {
        // The Home button of the phone switches between the game and the launcher right here:
        // a minimised launcher draws no frames to do it in. Without a game, or switched off, it
        // is the plain request below.
        if matches!(message, Message::Home) && self.home.press() != Route::Ignore {
            return;
        }
        self.push(Ev::Request(message));
    }
    fn pad_ready(&self, _ready: bool) {}
}

/// The phone server and what is kept between frames. Lives in the app; the shown state is the
/// model's [`PhoneState`].
pub struct PhoneService {
    wake: Arc<dyn Fn() + Send + Sync>,
    home: Arc<HomeButton>,
    /// Previews and tests never open a port.
    simulated: bool,
    tx: Sender<Ev>,
    rx: Receiver<Ev>,
    server: Option<Arc<RemoteServer>>,
    layouts: Option<Layouts>,
    pad: Option<Arc<VirtualPad>>,
    pad_attached: bool,
    /// The last library told to the phone, as JSON: sent again only when it differs.
    told: Option<String>,
    last_library: f64,
    last_pad_check: f64,
    /// A launch waits for the running game to end.
    pending_launch: Option<GameId>,
}

/// How often the library is compared with what the phone knows.
const LIBRARY_EVERY: f64 = 0.4;
/// How often the driver is looked at while the settings are open.
const PAD_CHECK_EVERY: f64 = 2.0;

impl PhoneService {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>, simulated: bool, home: Arc<HomeButton>) -> Self {
        let (tx, rx) = channel();
        Self {
            wake,
            home,
            simulated,
            tx,
            rx,
            server: None,
            layouts: None,
            pad: None,
            pad_attached: false,
            told: None,
            last_library: f64::NEG_INFINITY,
            last_pad_check: f64::NEG_INFINITY,
            pending_launch: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.server.is_some()
    }

    fn start(&mut self, model: &mut Model) {
        let config = RemoteConfig::new(pc_name(), PairedDevices::file_in(&model.paths));
        let events = Arc::new(Events {
            tx: Mutex::new(self.tx.clone()),
            wake: self.wake.clone(),
            home: self.home.clone(),
        });
        match RemoteServer::start(config, events) {
            Ok(server) => {
                model.phone.failed = false;
                model.phone.info = Some(server.info().clone());
                model.phone.paired = server.paired();
                self.layouts = Some(Layouts::load(&Layouts::file_in(&model.paths)));
                self.server = Some(Arc::new(server));
                self.told = None;
            }
            Err(error) => {
                tracing::warn!("the phone server could not start: {error}");
                model.phone.failed = true;
            }
        }
        model.phone.pad = Some(vpad::status());
    }

    fn stop(&mut self, model: &mut Model) {
        self.detach_pad();
        if let Some(server) = self.server.take() {
            server.stop();
        }
        self.pad = None;
        self.layouts = None;
        self.told = None;
        self.pending_launch = None;
        let pad = model.phone.pad.take();
        model.phone = PhoneState {
            pad,
            ..PhoneState::default()
        };
    }

    fn attach_pad(&mut self) {
        let Some(server) = self.server.clone() else {
            return;
        };
        if self.pad_attached {
            return;
        }
        if self.pad.is_none() {
            let weak: Weak<RemoteServer> = Arc::downgrade(&server);
            let feedback: Arc<dyn Fn(OutputPacket) + Send + Sync> = Arc::new(move |packet| {
                if let Some(server) = weak.upgrade() {
                    server.send_feedback(&packet);
                }
            });
            match VirtualPad::open(Some(feedback)) {
                Ok(pad) => self.pad = Some(Arc::new(pad)),
                Err(error) => {
                    tracing::warn!("no virtual controller: {error}");
                    return;
                }
            }
        }
        if let Some(pad) = &self.pad {
            server.set_pad_sink(Some(pad.clone()));
            self.pad_attached = true;
        }
    }

    fn detach_pad(&mut self) {
        if !self.pad_attached {
            return;
        }
        self.pad_attached = false;
        if let Some(server) = &self.server {
            server.set_pad_sink(None);
        }
        if let Some(pad) = &self.pad {
            use anyps5_core::remote::PadSink;
            pad.ended();
        }
    }
}

/// Trophy grades as the phone names them.
fn grade_name(grade: Grade) -> &'static str {
    match grade {
        Grade::Platinum => "platinum",
        Grade::Gold => "gold",
        Grade::Silver => "silver",
        Grade::Bronze => "bronze",
    }
}

/// The trophy message for the phone.
pub fn trophy_message(trophy: &Trophy) -> Message {
    Message::Trophy {
        name: trophy.name.clone(),
        detail: trophy.detail.clone(),
        grade: grade_name(trophy.grade).to_owned(),
        icon: trophy.icon.as_deref().map(|png| STANDARD.encode(png)),
    }
}

/// How the phone words a game's state.
pub fn state_name(state: &GameState) -> &'static str {
    match state {
        GameState::Unreadable | GameState::Failed(_) => "unavailable",
        GameState::Locked => "locked",
        GameState::Queued | GameState::Installing { .. } | GameState::Preparing { .. } => {
            "installing"
        }
        GameState::Installed { outdated: true } => "update",
        GameState::Installed { outdated: false } => "installed",
        GameState::Starting => "starting",
        GameState::Running => "running",
    }
}

/// Changes whenever the icon or background of a game does (FNV-1a over path and change time, in
/// base 36, like the macOS launcher's tag).
pub fn art_tag(icon: Option<&Path>, background: Option<&Path>) -> String {
    let part = |path: Option<&Path>| match path {
        None => "-".to_owned(),
        Some(path) => {
            let seconds = std::fs::metadata(path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            format!("{}@{seconds}", path.display())
        }
    };
    let text = format!("{}|{}", part(icon), part(background));
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
    }
    to_base36(hash)
}

fn to_base36(mut value: u64) -> String {
    if value == 0 {
        return "0".into();
    }
    let mut digits = Vec::new();
    while value > 0 {
        digits.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(value % 36) as usize]);
        value /= 36;
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_default()
}

/// A picture as a small JPEG in base64 (the phone's list, not the screen): its longest side at
/// most `side` pixels.
pub fn jpeg_base64(path: &Path, side: u32) -> Option<String> {
    let image = image::open(path).ok()?;
    let small = image.thumbnail(side, side).to_rgb8();
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 72)
        .encode_image(&small)
        .ok()?;
    Some(STANDARD.encode(bytes))
}

pub const ICON_SIDE: u32 = 256;
pub const BACKGROUND_SIDE: u32 = 1280;

/// The library as the phone lists it.
pub fn library_message(model: &Model, layouts: Option<&Layouts>) -> Message {
    let games = model
        .games
        .iter()
        .map(|game| {
            let state = model.state_of(game);
            let trophies = model
                .trophies
                .get(&game.id)
                .map(|set| format!("{}/{}", set.unlocked_count(), set.trophies.len()));
            RemoteGame {
                id: game.id.as_str().to_owned(),
                name: game.name.clone(),
                state: state_name(&state).to_owned(),
                trophies,
                art: Some(art_tag(game.icon.as_deref(), game.background.as_deref())),
                layout: layouts
                    .and_then(|l| l.get(game.id.as_str()))
                    .map(str::to_owned),
            }
        })
        .collect();
    Message::Library {
        games,
        selected: model.selected_game().map(|g| g.id.as_str().to_owned()),
        running: model
            .running
            .as_ref()
            .or(model.starting.as_ref())
            .map(|id| id.as_str().to_owned()),
        language: model.language().code().to_owned(),
    }
}

/// The launcher command a phone button name stands for. `l1` and `r1` page through the games.
pub fn command_for(name: &str) -> Option<InputEvent> {
    Some(match name {
        "left" => InputEvent::Command(Command::Left),
        "right" => InputEvent::Command(Command::Right),
        "up" => InputEvent::Command(Command::Up),
        "down" => InputEvent::Command(Command::Down),
        "select" => InputEvent::Command(Command::Select),
        "back" => InputEvent::Command(Command::Back),
        "options" => InputEvent::Command(Command::Options),
        "l1" => InputEvent::Scroll(Command::Left),
        "r1" => InputEvent::Scroll(Command::Right),
        _ => return None,
    })
}

impl LauncherApp {
    /// Every frame: the server follows the setting, the phone hears of changes, its requests run.
    pub(super) fn sync_phone(&mut self, ctx: &Context) {
        let want = self.model.prefs.phone_remote && !self.phone.simulated;
        if want && !self.phone.is_running() && !self.model.phone.failed {
            self.phone.start(&mut self.model);
        } else if !want && (self.phone.is_running() || self.model.phone.failed) {
            self.phone.stop(&mut self.model);
        }
        let now = self.model.now;
        // The driver can be installed while the page is open: look again now and then.
        if want
            && self.model.overlay == Some(crate::app::overlay::Overlay::Settings)
            && now - self.phone.last_pad_check > PAD_CHECK_EVERY
        {
            self.phone.last_pad_check = now;
            self.model.phone.pad = Some(vpad::status());
        }
        let commands = std::mem::take(&mut self.model.phone.commands);
        for command in commands {
            self.phone_command(command);
        }
        while let Ok(event) = self.phone.rx.try_recv() {
            self.phone_event(ctx, event);
        }
        if self.phone.server.is_none() {
            self.model.phone_trophies.clear();
            return;
        }
        self.run_pending_launch();

        // The pad goes to the virtual controller from the start of a game to its end.
        // The launcher in front of the game (Home button) takes the pad back from the game.
        let playing =
            (self.model.running.is_some() || self.model.starting.is_some()) && !self.model.summoned;
        if playing && self.model.phone.connected.is_some() {
            self.phone.attach_pad();
        } else {
            self.phone.detach_pad();
        }

        let Some(server) = self.phone.server.clone() else {
            return;
        };
        for trophy in std::mem::take(&mut self.model.phone_trophies) {
            if self.model.phone.connected.is_some() {
                server.publish(trophy_message(&trophy));
            }
        }
        if now - self.phone.last_library >= LIBRARY_EVERY {
            self.phone.last_library = now;
            let message = library_message(&self.model, self.phone.layouts.as_ref());
            let json = message.to_json();
            if self.phone.told.as_deref() != Some(json.as_str()) {
                server.publish(message);
                self.phone.told = Some(json);
            }
        }
    }

    fn phone_command(&mut self, command: PhoneAction) {
        let Some(server) = self.phone.server.clone() else {
            return;
        };
        match command {
            PhoneAction::Respond(allow) => {
                if let Some(request) = self.model.phone.pairing.take() {
                    server.respond(request.id, allow);
                    if allow && self.model.phone.paired.len() >= MAX_DEVICES {
                        let text = PhoneText::ToastFull.text(self.model.language());
                        self.model.toast(text, Icon::WARNING);
                    }
                }
            }
            PhoneAction::Forget(id) => {
                let name = self
                    .model
                    .phone
                    .paired
                    .iter()
                    .find(|d| d.id == id)
                    .map(|d| d.name.clone())
                    .unwrap_or_default();
                server.forget(&id);
                // The hub removes it from the file in its own thread; the list follows the file.
                self.model.phone.paired.retain(|d| d.id != id);
                let text =
                    PhoneText::ToastRemoved.format(self.model.language(), &[Arg::Str(&name)]);
                self.model.toast(text, Icon::CHECK_CIRCLE);
            }
            PhoneAction::OpenDriverPage => {}
        }
    }

    fn phone_event(&mut self, ctx: &Context, event: Ev) {
        let language = self.model.language();
        match event {
            Ev::Pairing(request) => {
                let name = request.device_name.clone();
                self.model.phone.pairing = Some(request);
                let idle = self.model.onboarded()
                    && self.model.starting.is_none()
                    && self.model.running.is_none();
                if idle && self.model.overlay.is_none() {
                    self.model
                        .show_settings(Some(anyps5_core::links::SettingsPage::Phone));
                } else {
                    let text = PhoneText::ToastPair.format(language, &[Arg::Str(&name)]);
                    self.model.toast(text, Icon::DEVICE_MOBILE);
                }
            }
            Ev::PairingEnded(id) => {
                if self
                    .model
                    .phone
                    .pairing
                    .as_ref()
                    .is_some_and(|p| p.id == id)
                {
                    self.model.phone.pairing = None;
                }
                self.refresh_paired();
            }
            Ev::Connecting(name) => self.model.phone.connecting = name,
            Ev::Connected(name) => {
                let text = PhoneText::ToastConnected.format(language, &[Arg::Str(&name)]);
                self.model.phone.connected = Some(name);
                self.model.phone.connecting = None;
                self.model.phone.latency_ms = None;
                self.phone.told = None;
                self.refresh_paired();
                self.model.toast(text, Icon::DEVICE_MOBILE);
            }
            Ev::Disconnected => {
                self.model.phone.connected = None;
                self.model.phone.connecting = None;
                self.model.phone.latency_ms = None;
                self.phone.pending_launch = None;
            }
            Ev::Latency(milliseconds) => self.model.phone.latency_ms = Some(milliseconds),
            Ev::Request(message) => self.phone_request(ctx, message),
        }
    }

    fn refresh_paired(&mut self) {
        if let Some(server) = &self.phone.server {
            self.model.phone.paired = server.paired();
        }
    }

    /// The phone asked for something. Navigation does not reach a hidden launcher while a game
    /// runs, and nothing starts a second game.
    fn phone_request(&mut self, ctx: &Context, message: Message) {
        let playing =
            (self.model.running.is_some() || self.model.starting.is_some()) && !self.model.summoned;
        match message {
            Message::Command { name } if !playing => {
                if let Some(event) = command_for(&name) {
                    self.handle_event(ctx, event);
                }
            }
            Message::Select { id } if !playing && self.model.overlay.is_none() => {
                self.select_game(&id);
            }
            Message::Launch { id } => self.phone_launch(&id),
            Message::Close => {
                if let Some(id) = self.model.running.clone() {
                    self.model.stop(&id);
                } else if self.model.starting.is_some() {
                    self.model.cancel_start();
                }
                self.phone.pending_launch = None;
            }
            Message::Home => {
                self.model.dismiss_overlay();
                self.model.effects.push(Effect::RestoreWindow);
            }
            Message::WantArt { id } => self.send_art(&id),
            Message::SaveLayout { id, layout } => {
                if let Some(layouts) = &mut self.phone.layouts
                    && let Err(error) = layouts.set(&id, layout.as_deref())
                {
                    tracing::warn!("a pad layout was refused: {error}");
                }
                self.phone.told = None;
            }
            _ => {}
        }
    }

    fn select_game(&mut self, id: &str) -> bool {
        let Some(index) = self.model.games.iter().position(|g| g.id.as_str() == id) else {
            return false;
        };
        self.model.selected = index;
        self.model.measure_selected();
        true
    }

    /// Starts a game from the phone: the one that runs is closed first.
    fn phone_launch(&mut self, id: &str) {
        let Some(game) = self.model.games.iter().find(|g| g.id.as_str() == id) else {
            return;
        };
        let game = game.id.clone();
        if self.model.running.as_ref() == Some(&game) {
            self.select_game(id);
            self.model.primary_action();
            return;
        }
        if self.model.starting.is_some() {
            return;
        }
        self.model.dismiss_overlay();
        if let Some(running) = self.model.running.clone() {
            self.model.stop(&running);
            self.phone.pending_launch = Some(game);
        } else {
            self.select_game(id);
            self.model.primary_action();
        }
    }

    fn run_pending_launch(&mut self) {
        if self.phone.pending_launch.is_none()
            || self.model.running.is_some()
            || self.model.starting.is_some()
        {
            return;
        }
        if let Some(game) = self.phone.pending_launch.take() {
            self.select_game(game.as_str());
            self.model.primary_action();
        }
    }

    /// Sends the pictures of a game on a thread of their own: decoding a background takes a while.
    fn send_art(&mut self, id: &str) {
        let Some(game) = self.model.games.iter().find(|g| g.id.as_str() == id) else {
            return;
        };
        let (icon, background) = (game.icon.clone(), game.background.clone());
        let id = id.to_owned();
        let Some(server) = self.phone.server.clone() else {
            return;
        };
        let work = move || {
            let message = Message::Art {
                id,
                icon: icon.as_deref().and_then(|p| jpeg_base64(p, ICON_SIDE)),
                background: background
                    .as_deref()
                    .and_then(|p| jpeg_base64(p, BACKGROUND_SIDE)),
            };
            server.publish(message);
        };
        if let Err(error) = std::thread::Builder::new()
            .name("phone-art".into())
            .spawn(work)
        {
            tracing::warn!("cannot start the art thread: {error}");
        }
    }
}

/// Games the phone could be told about, for tests.
#[allow(dead_code)]
pub fn count_by_state(model: &Model) -> HashMap<&'static str, usize> {
    let mut counts = HashMap::new();
    for game in &model.games {
        *counts.entry(state_name(&model.state_of(game))).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::library::{ExecutableKind, Game};
    use anyps5_core::paths::AppPaths;
    use anyps5_core::settings::Preferences;
    use anyps5_core::trophies::{Trophy, TrophySet};

    fn game(name: &str, unlocked: bool) -> Game {
        Game {
            id: GameId::from_key(format!("d:/games/{name}")),
            folder: format!("d:/games/{name}").into(),
            title_id: "PPSA00001".into(),
            name: name.into(),
            names: Default::default(),
            version: "01.000.000".into(),
            content_id: None,
            icon: None,
            background: None,
            music: None,
            executable: ExecutableKind::Elf,
            size: None,
            unlocked_with: unlocked.then(|| "0.1.3".into()),
        }
    }

    fn model(dir: &tempfile::TempDir) -> Model {
        let mut model = Model::new(Preferences::first_run(), AppPaths::under(dir.path()), false);
        model.games = vec![game("Hollow Orchard", true), game("Lantern Run", false)];
        model
    }

    fn trophy(grade: Grade, unlocked: bool) -> Trophy {
        Trophy {
            service_label: 0,
            number: 1,
            grade,
            hidden: false,
            name: "First Light".into(),
            detail: "Reach the lantern.".into(),
            icon: Some(vec![1, 2, 3]),
            unlocked: unlocked.then_some(1),
            progress: None,
        }
    }

    #[test]
    fn game_states_have_the_phones_words() {
        assert_eq!(state_name(&GameState::Locked), "locked");
        assert_eq!(
            state_name(&GameState::Installed { outdated: false }),
            "installed"
        );
        assert_eq!(
            state_name(&GameState::Installed { outdated: true }),
            "update"
        );
        assert_eq!(state_name(&GameState::Running), "running");
        assert_eq!(state_name(&GameState::Starting), "starting");
        assert_eq!(state_name(&GameState::Unreadable), "unavailable");
        assert_eq!(state_name(&GameState::Failed("x".into())), "unavailable");
        assert_eq!(state_name(&GameState::Queued), "installing");
    }

    #[test]
    fn the_library_lists_games_states_and_the_selection() {
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        model.selected = 1;
        model.trophies.insert(
            model.games[0].id.clone(),
            TrophySet {
                title: "t".into(),
                icon: None,
                trophies: vec![trophy(Grade::Gold, true), trophy(Grade::Bronze, false)],
            },
        );
        let Message::Library {
            games,
            selected,
            running,
            language,
        } = library_message(&model, None)
        else {
            panic!("a library");
        };
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].name, "Hollow Orchard");
        assert_eq!(games[0].state, "installed");
        assert_eq!(games[0].trophies.as_deref(), Some("1/2"));
        assert_eq!(games[1].state, "locked");
        assert_eq!(games[1].trophies, None);
        assert_eq!(selected.as_deref(), Some(games[1].id.as_str()));
        assert_eq!(running, None);
        assert!(language == "en" || language == "tr");
        assert!(games[0].art.is_some(), "the phone always gets an art tag");
    }

    #[test]
    fn a_running_game_is_named_to_the_phone() {
        let dir = tempfile::tempdir().unwrap();
        let mut model = model(&dir);
        model.running = Some(model.games[0].id.clone());
        let Message::Library { running, games, .. } = library_message(&model, None) else {
            panic!("a library");
        };
        assert_eq!(running.as_deref(), Some(games[0].id.as_str()));
        assert_eq!(games[0].state, "running");
    }

    #[test]
    fn the_layout_of_a_game_goes_with_the_library() {
        let dir = tempfile::tempdir().unwrap();
        let model = model(&dir);
        let mut layouts = Layouts::load(&dir.path().join("l.json"));
        layouts
            .set(model.games[0].id.as_str(), Some(r#"{"a":1}"#))
            .unwrap();
        let Message::Library { games, .. } = library_message(&model, Some(&layouts)) else {
            panic!("a library");
        };
        assert_eq!(games[0].layout.as_deref(), Some(r#"{"a":1}"#));
        assert_eq!(games[1].layout, None);
    }

    #[test]
    fn a_trophy_reaches_the_phone_with_grade_and_picture() {
        let Message::Trophy {
            name,
            detail,
            grade,
            icon,
        } = trophy_message(&trophy(Grade::Platinum, true))
        else {
            panic!("a trophy");
        };
        assert_eq!(
            (name.as_str(), detail.as_str()),
            ("First Light", "Reach the lantern.")
        );
        assert_eq!(grade, "platinum");
        assert_eq!(icon.as_deref(), Some("AQID"));
        for (grade, word) in [
            (Grade::Gold, "gold"),
            (Grade::Silver, "silver"),
            (Grade::Bronze, "bronze"),
        ] {
            assert_eq!(grade_name(grade), word);
        }
    }

    #[test]
    fn phone_buttons_are_launcher_commands() {
        for (name, event) in [
            ("left", InputEvent::Command(Command::Left)),
            ("select", InputEvent::Command(Command::Select)),
            ("back", InputEvent::Command(Command::Back)),
            ("options", InputEvent::Command(Command::Options)),
            ("l1", InputEvent::Scroll(Command::Left)),
            ("r1", InputEvent::Scroll(Command::Right)),
        ] {
            assert_eq!(command_for(name), Some(event), "{name}");
        }
        assert_eq!(command_for("triangle"), None);
        assert_eq!(command_for(""), None);
    }

    #[test]
    fn the_art_tag_changes_with_the_picture() {
        let dir = tempfile::tempdir().unwrap();
        let icon = dir.path().join("icon0.png");
        assert_eq!(art_tag(None, None), art_tag(None, None));
        std::fs::write(&icon, b"a").unwrap();
        let first = art_tag(Some(&icon), None);
        assert_ne!(first, art_tag(None, None));
        assert_eq!(first, art_tag(Some(&icon), None));
        assert_ne!(first, art_tag(Some(&icon), Some(&icon)));
        assert!(first.bytes().all(|b| b.is_ascii_alphanumeric()));
    }

    #[test]
    fn pictures_go_to_the_phone_as_small_jpegs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pic0.png");
        image::RgbaImage::from_pixel(1920, 1080, image::Rgba([40, 90, 200, 255]))
            .save(&path)
            .unwrap();
        let text = jpeg_base64(&path, BACKGROUND_SIDE).expect("a picture");
        let bytes = STANDARD.decode(text).unwrap();
        assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "a JPEG");
        let small = image::load_from_memory(&bytes).unwrap();
        assert_eq!((small.width(), small.height()), (1280, 720));
        assert!(jpeg_base64(&dir.path().join("none.png"), 256).is_none());
    }

    #[test]
    fn base36_is_the_macs_tag_alphabet() {
        assert_eq!(to_base36(0), "0");
        assert_eq!(to_base36(35), "z");
        assert_eq!(to_base36(36), "10");
    }

    #[test]
    fn the_state_starts_quiet() {
        let state = PhoneState::default();
        assert!(state.connected.is_none() && state.pairing.is_none() && !state.failed);
        assert!(state.paired.is_empty() && state.commands.is_empty());
    }
}
