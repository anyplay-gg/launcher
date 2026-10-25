//! The app state machine (`AppModel.swift` of the original, minus everything that is I/O).
//!
//! [`Model`] holds what the user sees and chose: the preferences, the library, which game is
//! selected, where focus sits, which overlay is open, the onboarding step, the unlock queue and
//! progress, the running game and the toast. It has no egui types and does no I/O: whatever has to
//! happen outside (sounds, scanning, measuring, the folder dialog, the pipeline) is queued as an
//! [`Effect`] that the host executes after the frame's input was handled, and whatever happened
//! outside arrives through [`Model::on_core`] and [`Model::on_pipeline`].

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use anyps5_core::events::CoreEvent;
use anyps5_core::library::{ExecutableKind, Game};
use anyps5_core::online::Online;
use anyps5_core::paths::{AppPaths, GameId};
use anyps5_core::settings::{self, Preferences};
use anyps5_core::toolkit::GameToolkit;
use anyps5_core::trophies::{Trophy, TrophySet};

use crate::app::clock;
use crate::app::overlay::Overlay;
use crate::app::pipeline::{PipelineCommand, PipelineEvent, UnlockStep};
use crate::app::playtime::{Counted, Sessions};
use crate::screens::strings::Extra;
use crate::ui::focus::{HomeContext, HomeFocus, HomeIntent, PanelNav};
use crate::ui::i18n::{Arg, Key, Language};
use crate::ui::icons::Icon;
use crate::ui::input::Command;
use crate::ui::sound::Cue;
use crate::ui::theme::{Accent, Theme};

/// How long a toast stays up.
pub const TOAST_SECONDS: f64 = 3.2;
/// A rescan on window focus needs this many seconds since the last scan.
pub const FOCUS_RESCAN_GAP: f64 = 3.0;
/// The least time between two refreshes of the friends' gamer cards, in seconds.
pub const CARDS_REFRESH_GAP: f64 = 60.0;

// ---------------------------------------------------------------------------------------------
// Types shared with the screens
// ---------------------------------------------------------------------------------------------

/// What a game is doing, in priority order (DESIGN.md 5.1).
#[derive(Clone, Debug, PartialEq)]
pub enum GameState {
    Unreadable,
    Locked,
    Queued,
    Installing { step: UnlockStep, fraction: f32 },
    Failed(String),
    Installed { outdated: bool },
    Starting,
    Running,
}

/// The size of a game on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeState {
    Measuring,
    Known(u64),
}

/// A message at the top of the window.
#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub text: String,
    pub icon: Icon,
    /// UI-clock time at which it starts to leave.
    pub until: f64,
}

/// Where the first-run setup stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Onboarding {
    /// 0 language, 1 theme, 2 games folder.
    pub step: usize,
    choices: [Option<usize>; 3],
}

/// Something that has to happen outside the model.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Sound(Cue),
    Pipeline(PipelineCommand),
    /// Scan the games folder (and watch it).
    Rescan,
    /// Open the native folder dialog; the answer comes back through [`Model::choose_folder`].
    PickGamesFolder,
    MeasureSize(GameId),
    LoadTrophies {
        game: GameId,
        again: bool,
    },
    /// "Hide Launcher While Playing".
    MinimizeWindow,
    RestoreWindow,
    /// Show a game on Discord (`Some(name)`) or clear it (`None`).
    Presence(Option<String>),
    /// The toolkit changed on disk or in the preferences: the pipeline looks for it again.
    ReloadToolkit,
    /// A game ended well: back up its saves if the last backup is old (see `anyps5_core::saves`).
    BackUpSaves(GameId),
    /// Start the net service, or say hello again with a new cloud (`app::online`).
    StartOnline,
    /// Stop the net service.
    StopOnline,
    /// Relay only changed in the preferences: the net service hears of it (`app::online`).
    RelayOnlyChanged,
    /// The player came online: read what they earned in every unlocked game for the network.
    SyncTrophies,
}

/// What became of a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Routed {
    /// The model used it (or swallowed it).
    Done,
    /// An overlay is open: the overlay host decides.
    Overlay(Overlay),
}

// ---------------------------------------------------------------------------------------------
// Conversions between the core's setting types and the UI's
// ---------------------------------------------------------------------------------------------

pub fn ui_language(language: settings::Language) -> Language {
    match language {
        settings::Language::Turkish => Language::Turkish,
        settings::Language::English => Language::English,
    }
}

pub fn core_language(language: Language) -> settings::Language {
    match language {
        Language::Turkish => settings::Language::Turkish,
        Language::English => settings::Language::English,
    }
}

pub fn ui_theme(theme: settings::Theme) -> Theme {
    match theme {
        settings::Theme::Cosmic => Theme::Cosmic,
        settings::Theme::Aurora => Theme::Aurora,
        settings::Theme::Sunset => Theme::Sunset,
    }
}

pub fn core_theme(theme: Theme) -> settings::Theme {
    match theme {
        Theme::Cosmic => settings::Theme::Cosmic,
        Theme::Aurora => settings::Theme::Aurora,
        Theme::Sunset => settings::Theme::Sunset,
    }
}

pub fn ui_accent(accent: settings::Accent) -> Accent {
    match accent {
        settings::Accent::Blue => Accent::Blue,
        settings::Accent::Mint => Accent::Mint,
        settings::Accent::Coral => Accent::Coral,
        settings::Accent::Violet => Accent::Violet,
        settings::Accent::Gold => Accent::Gold,
        settings::Accent::White => Accent::White,
    }
}

pub fn core_accent(accent: Accent) -> settings::Accent {
    match accent {
        Accent::Blue => settings::Accent::Blue,
        Accent::Mint => settings::Accent::Mint,
        Accent::Coral => settings::Accent::Coral,
        Accent::Violet => settings::Accent::Violet,
        Accent::Gold => settings::Accent::Gold,
        Accent::White => settings::Accent::White,
    }
}

// ---------------------------------------------------------------------------------------------
// The model
// ---------------------------------------------------------------------------------------------

pub struct Model {
    pub prefs: Preferences,
    pub paths: AppPaths,
    persist: bool,
    prefs_dirty: bool,
    /// The UI clock, set by [`tick`](Self::tick).
    pub now: f64,

    pub games: Vec<Game>,
    pub scanning: bool,
    pub selected: usize,
    pub focus: HomeFocus,
    pub overlay: Option<Overlay>,
    /// Navigation state of the open panel (`panelCategory`, `panelRow`, `panelInRows`).
    pub panel: PanelNav,
    /// The focused tile of the library grid.
    pub library_index: usize,
    /// The focused row of the trophy list.
    pub trophy_index: usize,
    pub onboarding: Onboarding,
    /// The first scan of the games folder has finished.
    pub library_loaded: bool,
    /// The page of the settings a link asked for; the settings panel takes it when it opens.
    pub settings_page: Option<anyps5_core::links::SettingsPage>,

    /// Unlock progress of the game that is being unlocked.
    pub progress: HashMap<GameId, (UnlockStep, f32)>,
    /// Games waiting for their unlock.
    pub queue: Vec<GameId>,
    pub failures: HashMap<GameId, String>,
    /// The game from the press of Play until its first window shows.
    pub starting: Option<GameId>,
    pub running: Option<GameId>,
    pub sizes: HashMap<GameId, SizeState>,
    pub trophies: HashMap<GameId, TrophySet>,
    /// Why each game stopped the last time it was started, when it stopped on an error.
    pub diagnoses: HashMap<GameId, String>,
    pub toast: Option<Toast>,
    /// The toolkit version on this PC (`None`: no toolkit, nothing can be unlocked).
    pub toolkit: Option<String>,
    /// The toolkit of each game that stays on a release of its own, as the pipeline found it.
    /// A game that is not in here follows [`toolkit`](Self::toolkit).
    pub game_toolkits: HashMap<GameId, GameToolkit>,
    /// The game whose toolkit is missing and who was just asked to start or unlock: the question
    /// (`Overlay::ToolkitMissing`) is about this game.
    pub toolkit_question: Option<GameId>,
    /// The toolkit's patch files, for the resolutions of the game settings.
    pub patches_dir: Option<std::path::PathBuf>,
    /// Trophies earned by the running game, waiting for the banner. The banner drains it.
    pub trophy_banners: Vec<(GameId, Trophy)>,
    /// Work for the host; drained after every input pass.
    pub effects: Vec<Effect>,
    /// What the launcher knows of the network. Commands queue here; the host hands them to the
    /// net service and its answers back (`app::online`).
    pub online: Online,
    /// A session that ended and whose "How did it run?" has not been answered ([`Model::run_question`]).
    pub run_prompt: Option<crate::app::run_question::RunPrompt>,

    /// The clock of the running game's playtime.
    sessions: Sessions,
    /// The game whose saves a backup or restore is working on: it is not started meanwhile.
    pub saves_busy: Option<GameId>,
    trophies_read: HashSet<GameId>,
    last_scan: f64,
    cards_asked: f64,
    started_without_window: bool,
    toast_counter: u64,
}

impl Model {
    /// `persist` is off for the previews and tests, which must not touch the user's settings.
    pub fn new(prefs: Preferences, paths: AppPaths, persist: bool) -> Self {
        Self {
            prefs,
            paths,
            persist,
            prefs_dirty: false,
            now: 0.0,
            games: Vec::new(),
            scanning: false,
            selected: 0,
            focus: HomeFocus::Tiles,
            overlay: None,
            panel: PanelNav::single(0),
            library_index: 0,
            trophy_index: 0,
            onboarding: Onboarding::default(),
            library_loaded: false,
            settings_page: None,
            progress: HashMap::new(),
            queue: Vec::new(),
            failures: HashMap::new(),
            starting: None,
            running: None,
            sizes: HashMap::new(),
            trophies: HashMap::new(),
            diagnoses: HashMap::new(),
            toast: None,
            toolkit: None,
            game_toolkits: HashMap::new(),
            toolkit_question: None,
            patches_dir: None,
            trophy_banners: Vec::new(),
            effects: Vec::new(),
            online: Online::default(),
            run_prompt: None,
            sessions: Sessions::default(),
            saves_busy: None,
            trophies_read: HashSet::new(),
            last_scan: f64::NEG_INFINITY,
            cards_asked: f64::NEG_INFINITY,
            started_without_window: false,
            toast_counter: 0,
        }
    }

    // --- preferences -------------------------------------------------------------------------

    /// Changes the preferences; they are saved at the end of the frame when something changed.
    pub fn edit_prefs(&mut self, change: impl FnOnce(&mut Preferences)) {
        let before = self.prefs.clone();
        change(&mut self.prefs);
        if self.prefs != before {
            self.prefs_dirty = true;
        }
    }

    /// Marks the preferences as changed after a panel edited them in place (they are saved at the
    /// end of the frame).
    pub fn mark_prefs_dirty(&mut self) {
        self.prefs_dirty = true;
    }

    /// True once after a change that has to be written (and only when persisting).
    pub fn take_prefs_dirty(&mut self) -> bool {
        std::mem::take(&mut self.prefs_dirty) && self.persist
    }

    pub fn language(&self) -> Language {
        ui_language(self.prefs.language)
    }

    pub fn theme(&self) -> Theme {
        ui_theme(self.prefs.theme)
    }

    /// The chosen accent, or the theme's own.
    pub fn accent(&self) -> Accent {
        self.prefs
            .accent
            .map(ui_accent)
            .unwrap_or_else(|| self.theme().default_accent())
    }

    pub fn onboarded(&self) -> bool {
        self.prefs.onboarded
    }

    fn text(&self, key: Key) -> &'static str {
        self.language().text(key)
    }

    // --- clock and toast ---------------------------------------------------------------------

    /// Call first thing in every frame.
    pub fn tick(&mut self, now: f64) {
        self.now = now;
        self.checkpoint(Instant::now());
    }

    // --- playtime ----------------------------------------------------------------------------

    /// A running session adds what it has counted once a minute, so what a crash or a closed
    /// launcher cuts short is at most that.
    fn checkpoint(&mut self, at: Instant) {
        if self.sessions.checkpoint_due(at) {
            let counted = self.sessions.take(at);
            self.bank(counted);
        }
    }

    /// The game's window showed: a session starts and is counted.
    fn begin_session(&mut self, game: &GameId, at: Instant) {
        let left = self.sessions.begin(game.clone(), at);
        self.bank(left);
        self.edit_prefs(|p| p.count_session(game));
    }

    /// The game exited, cleanly or not: what the session still has is added.
    fn end_session(&mut self, game: &GameId, at: Instant) {
        let counted = self.sessions.end(game, at);
        self.bank(counted);
    }

    fn bank(&mut self, counted: Option<Counted>) {
        if let Some((game, seconds)) = counted.filter(|(_, seconds)| *seconds > 0) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
            self.edit_prefs(|p| {
                p.add_playtime(&game, seconds);
                p.add_playtime_day(&game, seconds, now);
            });
            self.sync_playtime();
        }
    }

    /// Sends the network the minutes per title and day that grew (nothing while signed out).
    pub fn sync_playtime(&mut self) {
        let entries = anyps5_core::online::playtime_entries(&self.prefs.playtime_days, |id| {
            self.games
                .iter()
                .find(|g| g.id.as_str() == id)
                .map(|g| g.title_id.clone())
        });
        self.online.upload_playtime(entries);
    }

    /// Shows a toast; a warning also plays the error cue.
    pub fn toast(&mut self, text: impl Into<String>, icon: Icon) {
        if icon == Icon::WARNING {
            self.sound(Cue::Error);
        }
        self.toast_counter += 1;
        self.toast = Some(Toast {
            id: self.toast_counter,
            text: text.into(),
            icon,
            until: self.now + TOAST_SECONDS,
        });
    }

    /// The toast is on screen (it is kept after that so it can animate out).
    pub fn toast_visible(&self) -> bool {
        self.toast.as_ref().is_some_and(|t| self.now < t.until)
    }

    // --- selection and states ----------------------------------------------------------------

    pub fn selected_game(&self) -> Option<&Game> {
        self.games.get(self.selected)
    }

    pub fn game(&self, id: &GameId) -> Option<&Game> {
        self.games.iter().find(|g| &g.id == id)
    }

    /// The state of a game, first match wins (DESIGN.md 5.1).
    pub fn state_of(&self, game: &Game) -> GameState {
        let id = &game.id;
        if self.starting.as_ref() == Some(id) {
            return GameState::Starting;
        }
        if self.running.as_ref() == Some(id) {
            return GameState::Running;
        }
        if let Some(&(step, fraction)) = self.progress.get(id) {
            return GameState::Installing { step, fraction };
        }
        if self.queue.contains(id) {
            return GameState::Queued;
        }
        if let Some(message) = self.failures.get(id) {
            return GameState::Failed(message.clone());
        }
        if let Some(record) = &game.unlocked_with {
            // The toolkit the game would be unlocked with now: its own, or the global one. A game
            // whose release is missing is not called outdated; starting it asks what to do.
            let outdated = self.toolkit_of(id).version().is_some_and(|t| t != record);
            return GameState::Installed { outdated };
        }
        if game.executable == ExecutableKind::Unreadable {
            return GameState::Unreadable;
        }
        GameState::Locked
    }

    /// The toolkit a game is unlocked and started with (docs/DESIGN.md 3.8).
    pub fn toolkit_of(&self, id: &GameId) -> GameToolkit {
        self.game_toolkits
            .get(id)
            .cloned()
            .unwrap_or_else(|| GameToolkit::Global(self.toolkit.clone()))
    }

    /// The patch files of the toolkit a game uses (the resolutions its settings offer).
    pub fn patches_dir_for(&self, id: &GameId) -> Option<std::path::PathBuf> {
        match self.game_toolkits.get(id) {
            Some(GameToolkit::Pinned { tag, version }) => {
                anyps5_core::compat::supports(anyps5_core::compat::Feature::Patches, version)
                    .then(|| self.paths.toolkit_dir().join(tag).join("patches"))
            }
            Some(GameToolkit::Missing { .. }) => None,
            _ => self.patches_dir.clone(),
        }
    }

    /// Asks what to do about a game whose release is gone, instead of using another one.
    /// True when the game can go on.
    fn toolkit_ready(&mut self, id: &GameId) -> bool {
        if !self.toolkit_of(id).is_missing() {
            return true;
        }
        self.toolkit_question = Some(id.clone());
        self.open(Overlay::ToolkitMissing);
        false
    }

    pub fn has_trophies(&self) -> bool {
        self.selected_game()
            .is_some_and(|g| self.trophies.contains_key(&g.id))
    }

    /// Buttons in the hero's action row: primary, trophies (if the game has any), "...".
    pub fn action_count(&self) -> usize {
        if self.has_trophies() { 3 } else { 2 }
    }

    fn home_context(&self) -> HomeContext {
        HomeContext {
            games: self.games.len(),
            selected: self.selected,
            has_trophies: self.has_trophies(),
            asking: self.run_question().is_some(),
        }
    }

    /// Something unlocks or waits to: the folder watcher stays quiet then.
    pub fn busy(&self) -> bool {
        !self.queue.is_empty() || !self.progress.is_empty()
    }

    fn sound(&mut self, cue: Cue) {
        self.effects.push(Effect::Sound(cue));
    }

    // --- library -----------------------------------------------------------------------------

    pub fn rescan(&mut self) {
        if self.prefs.games_folder.is_none() {
            return;
        }
        self.last_scan = self.now;
        self.scanning = true;
        self.effects.push(Effect::Rescan);
    }

    /// The folder dialog was answered. `None`: cancelled.
    pub fn choose_folder(&mut self, folder: Option<std::path::PathBuf>) {
        let Some(folder) = folder else { return };
        self.edit_prefs(|p| p.games_folder = Some(folder));
        self.selected = 0;
        if !self.onboarded() && self.onboarding.step == 2 {
            self.onboarding.set_choice(2, 1);
        }
        self.rescan();
    }

    pub fn request_folder_dialog(&mut self) {
        self.effects.push(Effect::PickGamesFolder);
    }

    /// Reads the selected game's trophies once and starts measuring its size once.
    pub fn measure_selected(&mut self) {
        let Some(id) = self.selected_game().map(|g| g.id.clone()) else {
            return;
        };
        if self.trophies_read.insert(id.clone()) {
            self.effects.push(Effect::LoadTrophies {
                game: id.clone(),
                again: false,
            });
        }
        if !self.sizes.contains_key(&id) {
            self.sizes.insert(id.clone(), SizeState::Measuring);
            self.effects.push(Effect::MeasureSize(id));
        }
        self.request_selected_runs();
    }

    /// Reloads the trophies of a game (it may have earned some).
    pub fn reload_trophies(&mut self, id: &GameId) {
        self.trophies_read.insert(id.clone());
        self.effects.push(Effect::LoadTrophies {
            game: id.clone(),
            again: true,
        });
    }

    /// The window got the focus back: rescan when the last scan is old, reload the trophies.
    pub fn window_focused(&mut self) {
        if !self.onboarded() {
            return;
        }
        if self.now - self.last_scan > FOCUS_RESCAN_GAP {
            self.rescan();
        }
        if let Some(id) = self.selected_game().map(|g| g.id.clone()) {
            self.reload_trophies(&id);
        }
        if self.online.is_online() && self.now - self.cards_asked > CARDS_REFRESH_GAP {
            self.cards_asked = self.now;
            self.online.request_cards(true);
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.games.is_empty() {
            return;
        }
        let next =
            (self.selected as isize + delta).clamp(0, self.games.len() as isize - 1) as usize;
        if next == self.selected {
            return;
        }
        self.selected = next;
        self.sound(Cue::Move);
        self.measure_selected();
    }

    // --- events from outside -----------------------------------------------------------------

    pub fn on_core(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::LibraryScanned { root, games, .. } => {
                if self.prefs.games_folder.as_deref() != Some(root.as_path()) {
                    return;
                }
                let current = self.selected_game().map(|g| g.id.clone());
                self.games = games;
                self.scanning = false;
                self.library_loaded = true;
                self.selected = current
                    .and_then(|id| self.games.iter().position(|g| g.id == id))
                    .unwrap_or_else(|| self.selected.min(self.games.len().saturating_sub(1)));
                self.library_index = self.library_index.min(self.games.len().saturating_sub(1));
                self.measure_selected();
            }
            CoreEvent::LibraryChanged if !self.busy() => {
                self.rescan();
            }
            CoreEvent::SizeMeasured { game, bytes } => {
                self.sizes.insert(game, SizeState::Known(bytes));
            }
            CoreEvent::Trophies { game, set } => match set {
                Some(set) => {
                    if let Some(title_id) = self.game(&game).map(|g| g.title_id.clone()) {
                        self.online.upload_trophies(&title_id, &set.earned());
                    }
                    self.trophies.insert(game, set);
                }
                None => {
                    self.trophies.remove(&game);
                }
            },
            CoreEvent::TrophyUnlocked { game, trophy } => {
                self.trophy_banners.push((game, trophy));
            }
            CoreEvent::TrophyRecords { title_id, earned } => {
                self.online.upload_trophies(&title_id, &earned);
            }
            _ => {}
        }
    }

    pub fn on_pipeline(&mut self, event: PipelineEvent) {
        match event {
            PipelineEvent::Progress {
                game,
                step,
                fraction,
            } => {
                self.queue.retain(|q| q != &game);
                // The relinker reports thousands of times; the bar moves in visible steps.
                let moved = match self.progress.get(&game) {
                    Some(&(s, f)) => s != step || (f - fraction).abs() >= 0.005 || fraction >= 1.0,
                    None => true,
                };
                if moved {
                    self.progress.insert(game, (step, fraction));
                }
            }
            PipelineEvent::UnlockFinished { game, result } => {
                self.progress.remove(&game);
                self.queue.retain(|q| q != &game);
                let name = self.game(&game).map(|g| g.name.clone()).unwrap_or_default();
                match result {
                    Ok(toolkit) => {
                        self.failures.remove(&game);
                        if let Some(g) = self.games.iter_mut().find(|g| g.id == game) {
                            g.unlocked_with = Some(toolkit);
                        }
                        self.sound(Cue::Unlocked);
                        let text = format!("{name} · {}", self.text(Key::Ready));
                        self.toast(text, Icon::LOCK_OPEN);
                    }
                    Err(message) => {
                        self.failures.insert(game, message);
                        let text = format!("{}: {name}", self.text(Key::UnlockFailed));
                        self.toast(text, Icon::WARNING);
                    }
                }
            }
            PipelineEvent::UnlockCancelled { game } => {
                self.progress.remove(&game);
                self.queue.retain(|q| q != &game);
            }
            PipelineEvent::Spawned { game } => {
                if self.starting.as_ref() == Some(&game) {
                    self.running = Some(game);
                } else {
                    // Cancelled while it was spawning.
                    self.effects
                        .push(Effect::Pipeline(PipelineCommand::Stop(game)));
                }
            }
            PipelineEvent::WindowShown { game } => {
                if self.starting.as_ref() == Some(&game) {
                    self.starting = None;
                    self.begin_session(&game, Instant::now());
                    if self.prefs.discord_presence
                        && let Some(name) = self.game(&game).map(|g| g.name.clone())
                    {
                        self.effects.push(Effect::Presence(Some(name)));
                    }
                    if self.prefs.hide_while_playing {
                        self.effects.push(Effect::MinimizeWindow);
                    }
                }
            }
            PipelineEvent::StartFailed { game, message } => {
                if self.starting.as_ref() == Some(&game) {
                    self.starting = None;
                }
                let text = format!("{}: {message}", self.text(Key::StartFailed));
                self.toast(text, Icon::WARNING);
            }
            PipelineEvent::Exited { game, diagnosis } => {
                if self.running.as_ref() != Some(&game) && self.starting.as_ref() != Some(&game) {
                    return;
                }
                self.running = None;
                let had_window = self.sessions.running() == Some(&game);
                self.end_session(&game, Instant::now());
                if had_window && diagnosis.is_none() {
                    self.effects.push(Effect::BackUpSaves(game.clone()));
                }
                self.effects.push(Effect::Presence(None));
                self.reload_trophies(&game);
                if self.starting.as_ref() == Some(&game) {
                    self.starting = None;
                    self.started_without_window = true;
                }
                let without_window = std::mem::take(&mut self.started_without_window);
                self.effects.push(Effect::RestoreWindow);
                match diagnosis {
                    Some(summary) => {
                        let name = self.game(&game).map(|g| g.name.clone()).unwrap_or_default();
                        self.toast(format!("{name}: {summary}"), Icon::WARNING);
                        self.diagnoses.insert(game, summary);
                    }
                    None if without_window => {
                        let text = self.text(Key::StartFailed);
                        self.toast(text, Icon::WARNING);
                    }
                    None => {}
                }
            }
        }
    }

    // --- actions -----------------------------------------------------------------------------

    /// The primary action of the selected game by its state: play / unlock / retry / cancel /
    /// bring the window forward.
    pub fn primary_action(&mut self) {
        let Some(game) = self.selected_game().cloned() else {
            return;
        };
        match self.state_of(&game) {
            GameState::Installed { .. } => self.play(&game.id),
            GameState::Locked => self.unlock(&game.id),
            GameState::Failed(_) => self.retry(&game.id),
            GameState::Installing { .. } | GameState::Queued => self.cancel_unlock(&game.id),
            GameState::Running => self
                .effects
                .push(Effect::Pipeline(PipelineCommand::Activate(game.id))),
            GameState::Starting => {}
            GameState::Unreadable => {
                let text = self.text(Key::UnreadableDump);
                self.toast(text, Icon::X_CIRCLE);
            }
        }
    }

    /// The game is starting or running: its files are in use.
    pub fn is_active(&self, id: &GameId) -> bool {
        self.starting.as_ref() == Some(id) || self.running.as_ref() == Some(id)
    }

    pub fn unlock(&mut self, id: &GameId) {
        self.queue_unlock(id, PipelineCommand::Unlock(id.clone()));
    }

    pub fn retry(&mut self, id: &GameId) {
        self.failures.remove(id);
        self.queue_unlock(id, PipelineCommand::Retry(id.clone()));
    }

    /// Unlocks an already unlocked game again (update, "Reinstall").
    pub fn unlock_again(&mut self, id: &GameId) {
        self.queue_unlock(id, PipelineCommand::UnlockAgain(id.clone()));
    }

    fn queue_unlock(&mut self, id: &GameId, command: PipelineCommand) {
        if self.prefs.games_folder.is_none() {
            return;
        }
        if !self.toolkit_ready(id) {
            return;
        }
        if self.toolkit_of(id).version().is_none() {
            let text = self.text(Key::NoToolkit);
            self.toast(text, Icon::WARNING);
            return;
        }
        self.failures.remove(id);
        if self.queue.contains(id) || self.progress.contains_key(id) {
            return;
        }
        self.queue.push(id.clone());
        self.sound(Cue::Select);
        self.effects.push(Effect::Pipeline(command));
    }

    pub fn cancel_unlock(&mut self, id: &GameId) {
        if let Some(index) = self.queue.iter().position(|q| q == id) {
            self.queue.remove(index);
            self.effects
                .push(Effect::Pipeline(PipelineCommand::Cancel(id.clone())));
            return;
        }
        if self.progress.contains_key(id) {
            self.effects
                .push(Effect::Pipeline(PipelineCommand::Cancel(id.clone())));
        }
        self.sound(Cue::Back);
    }

    pub fn play(&mut self, id: &GameId) {
        if self.prefs.games_folder.is_none() || self.starting.is_some() || self.running.is_some() {
            return;
        }
        if self.saves_busy.as_ref() == Some(id) {
            let text = Extra::SavesBusy.text(self.language()).to_owned();
            self.toast(text, Icon::WARNING);
            return;
        }
        if !self.toolkit_ready(id) {
            return;
        }
        self.diagnoses.remove(id);
        self.started_without_window = false;
        let stamp = clock::rfc3339_utc(clock::unix_seconds());
        self.edit_prefs(|p| {
            p.last_played.insert(id.as_str().to_owned(), stamp);
        });
        self.sound(Cue::Select);
        self.starting = Some(id.clone());
        self.effects
            .push(Effect::Pipeline(PipelineCommand::Play(id.clone())));
    }

    /// Esc on the splash: the game is stopped before it ever shows.
    pub fn cancel_start(&mut self) {
        let Some(id) = self.starting.take() else {
            return;
        };
        self.sound(Cue::Back);
        self.effects
            .push(Effect::Pipeline(PipelineCommand::Stop(id)));
    }

    /// Stops the running game (also while it starts).
    pub fn stop(&mut self, id: &GameId) {
        self.effects
            .push(Effect::Pipeline(PipelineCommand::Stop(id.clone())));
    }

    // --- overlays ----------------------------------------------------------------------------

    pub fn open(&mut self, overlay: Overlay) {
        self.switch_overlay(overlay);
        self.sound(Cue::Select);
    }

    /// Opens or replaces the overlay without a sound: the panel modules play their own cues.
    pub fn switch_overlay(&mut self, overlay: Overlay) {
        self.panel = match overlay {
            Overlay::Settings => PanelNav::settings(overlay.categories()),
            // The controls panel opens on its first key, below the hint; advanced below its header.
            Overlay::Controls | Overlay::Advanced => PanelNav::single(1),
            _ => PanelNav::single(0),
        };
        if overlay == Overlay::Trophies {
            self.trophy_index = 0;
        }
        if overlay == Overlay::Library {
            self.library_index = self.selected;
        }
        self.overlay = Some(overlay);
    }

    /// Closes the overlay without a sound (the panel played its own cue).
    pub fn dismiss_overlay(&mut self) {
        self.overlay = None;
    }

    pub fn close_overlay(&mut self) {
        self.overlay = None;
        self.sound(Cue::Back);
    }

    // --- home input --------------------------------------------------------------------------

    /// One command from keyboard or gamepad.
    pub fn handle(&mut self, command: Command) -> Routed {
        if !self.onboarded() {
            self.onboarding_command(command);
            return Routed::Done;
        }
        if self.starting.is_some() {
            if command == Command::Back {
                self.cancel_start();
            }
            return Routed::Done;
        }
        if let Some(overlay) = self.overlay {
            return Routed::Overlay(overlay);
        }
        // The question line may have gone (answered, signed out) while the focus was on it.
        if matches!(self.focus, HomeFocus::Ask(_)) && self.run_question().is_none() {
            self.focus = HomeFocus::Actions(0);
        }
        let step = self.focus.navigate(command, self.home_context());
        self.focus = step.focus;
        let moved = step.selected != self.selected;
        self.selected = step.selected;
        if let Some(cue) = step.sound {
            self.sound(cue);
        }
        if moved {
            self.measure_selected();
        }
        if let Some(intent) = step.intent {
            self.apply_intent(intent);
        }
        Routed::Done
    }

    /// What a select on the home screen leads to (also used by mouse clicks).
    pub fn apply_intent(&mut self, intent: HomeIntent) {
        match intent {
            HomeIntent::PrimaryAction => self.primary_action(),
            HomeIntent::AnswerRun(i) => self.answer_run(i),
            HomeIntent::OpenTrophies => self.open(Overlay::Trophies),
            HomeIntent::OpenGameSettings => self.open(Overlay::GameSettings),
            HomeIntent::OpenLibrary => self.open(Overlay::Library),
            HomeIntent::OpenSettings => self.open(Overlay::Settings),
            // The online panel opens on the friends list once signed in, else on sign-in.
            HomeIntent::OpenFriends | HomeIntent::OpenOnline => self.open(Overlay::Online),
            HomeIntent::ChooseFolder => self.request_folder_dialog(),
        }
    }

    /// A press on a button of the action row or the top bar: focus moves there, the action runs.
    pub fn click(&mut self, focus: HomeFocus) {
        self.focus = focus;
        let intent = match focus {
            HomeFocus::Top(index) => HomeFocus::top_intent(index),
            HomeFocus::Actions(0) => HomeIntent::PrimaryAction,
            HomeFocus::Actions(1) if self.has_trophies() => HomeIntent::OpenTrophies,
            HomeFocus::Actions(_) => HomeIntent::OpenGameSettings,
            HomeFocus::Tiles => HomeIntent::PrimaryAction,
            HomeFocus::Ask(i) => HomeIntent::AnswerRun(i),
        };
        self.apply_intent(intent);
    }

    /// A click on a tile selects it and focuses the row; a double click also runs its primary action.
    pub fn click_tile(&mut self, index: usize, double: bool) {
        if index >= self.games.len() {
            return;
        }
        if double {
            self.selected = index;
            self.focus = HomeFocus::Tiles;
            self.measure_selected();
            self.primary_action();
            return;
        }
        if self.selected != index {
            self.sound(Cue::Move);
        }
        self.selected = index;
        self.focus = HomeFocus::Tiles;
        self.measure_selected();
    }

    // --- onboarding --------------------------------------------------------------------------

    /// The focused choice of the current step.
    pub fn onboarding_choice(&self) -> usize {
        let step = self.onboarding.step;
        self.onboarding.choices[step.min(2)].unwrap_or_else(|| match step {
            0 => Language::ALL
                .iter()
                .position(|l| *l == self.language())
                .unwrap_or(0),
            1 => self.theme().index() as usize,
            _ => usize::from(self.prefs.games_folder.is_some()),
        })
    }

    fn onboarding_count(&self) -> usize {
        match self.onboarding.step {
            0 => Language::ALL.len(),
            1 => Theme::ALL.len(),
            _ => {
                if self.prefs.games_folder.is_some() {
                    2
                } else {
                    1
                }
            }
        }
    }

    pub fn set_onboarding_choice(&mut self, choice: usize) {
        let step = self.onboarding.step;
        self.onboarding.set_choice(step, choice);
    }

    pub fn onboarding_command(&mut self, command: Command) {
        let choice = self.onboarding_choice();
        match command {
            Command::Left if choice > 0 => {
                self.set_onboarding_choice(choice - 1);
                self.preview_choice();
                self.sound(Cue::Move);
            }
            Command::Right if choice + 1 < self.onboarding_count() => {
                self.set_onboarding_choice(choice + 1);
                self.preview_choice();
                self.sound(Cue::Move);
            }
            Command::Select => self.onboarding_confirm(),
            Command::Back => self.onboarding_back(),
            _ => {}
        }
    }

    /// The language and theme steps show their choice at once.
    fn preview_choice(&mut self) {
        let choice = self.onboarding_choice();
        match self.onboarding.step {
            0 => {
                let language = core_language(Language::ALL[choice.min(1)]);
                self.edit_prefs(|p| p.language = language);
            }
            1 => {
                let theme = core_theme(Theme::from_index(choice as u32));
                self.edit_prefs(|p| p.theme = theme);
            }
            _ => {}
        }
    }

    pub fn onboarding_confirm(&mut self) {
        let choice = self.onboarding_choice();
        match self.onboarding.step {
            0 | 1 => {
                self.preview_choice();
                self.onboarding_advance();
            }
            _ => {
                if choice == 1 && self.prefs.games_folder.is_some() {
                    self.onboarding_finish();
                } else {
                    self.request_folder_dialog();
                }
            }
        }
    }

    fn onboarding_advance(&mut self) {
        self.sound(Cue::Select);
        self.onboarding.step = (self.onboarding.step + 1).min(2);
    }

    pub fn onboarding_back(&mut self) {
        if self.onboarding.step == 0 {
            return;
        }
        self.sound(Cue::Back);
        self.onboarding.step -= 1;
    }

    pub fn onboarding_finish(&mut self) {
        self.sound(Cue::Unlocked);
        self.edit_prefs(|p| p.onboarded = true);
        self.focus = HomeFocus::Tiles;
        if self.games.is_empty() {
            self.rescan();
        }
    }

    /// A tap on a language card selects and confirms it.
    pub fn tap_language(&mut self, index: usize) {
        self.set_onboarding_choice(index);
        self.onboarding_confirm();
    }

    /// A tap on a theme card selects it (no advance).
    pub fn tap_theme(&mut self, index: usize) {
        self.set_onboarding_choice(index);
        self.preview_choice();
        self.sound(Cue::Move);
    }

    /// Text of the "N games found" line.
    pub fn games_found_text(&self) -> String {
        self.language()
            .format(Key::GamesFound, &[Arg::Int(self.games.len() as i64)])
    }
}

impl Onboarding {
    fn set_choice(&mut self, step: usize, choice: usize) {
        self.choices[step.min(2)] = Some(choice);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::app::pipeline::{Pipeline, PipelineContext, SimPipeline};

    fn game(name: &str, unlocked: Option<&str>, executable: ExecutableKind) -> Game {
        let folder = PathBuf::from(format!("C:/games/{name}"));
        Game {
            id: GameId::from_path(&folder),
            folder,
            title_id: format!("PPSA{:05}", name.len()),
            name: name.to_owned(),
            names: Default::default(),
            version: "01.000.000".to_owned(),
            content_id: None,
            icon: None,
            background: None,
            music: None,
            executable,
            size: None,
            unlocked_with: unlocked.map(str::to_owned),
        }
    }

    fn model_with(games: Vec<Game>) -> Model {
        let prefs = Preferences {
            onboarded: true,
            games_folder: Some(PathBuf::from("C:/games")),
            ..Preferences::default()
        };
        let mut model = Model::new(
            prefs,
            AppPaths::under(std::path::Path::new("C:/none")),
            false,
        );
        model.toolkit = Some("v1".to_owned());
        model.games = games;
        model
    }

    fn sounds(model: &mut Model) -> Vec<Cue> {
        model
            .effects
            .iter()
            .filter_map(|e| match e {
                Effect::Sound(c) => Some(*c),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn state_priority_is_starting_running_installing_queued_failed_installed() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g.clone()]);
        assert_eq!(m.state_of(&g), GameState::Installed { outdated: false });
        m.toolkit = Some("v2".into());
        assert_eq!(m.state_of(&g), GameState::Installed { outdated: true });
        m.failures.insert(id.clone(), "boom".into());
        assert_eq!(m.state_of(&g), GameState::Failed("boom".into()));
        m.queue.push(id.clone());
        assert_eq!(m.state_of(&g), GameState::Queued);
        m.progress.insert(id.clone(), (UnlockStep::Copy, 0.5));
        assert!(matches!(m.state_of(&g), GameState::Installing { .. }));
        m.running = Some(id.clone());
        assert_eq!(m.state_of(&g), GameState::Running);
        m.starting = Some(id);
        assert_eq!(m.state_of(&g), GameState::Starting);
    }

    #[test]
    fn a_game_on_its_own_toolkit_is_judged_by_that_one() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g.clone()]);
        // The global toolkit moved on; the game stays on the release its copy was made with.
        m.toolkit = Some("v2".into());
        assert_eq!(m.state_of(&g), GameState::Installed { outdated: true });
        m.game_toolkits.insert(
            id.clone(),
            GameToolkit::Pinned {
                tag: "1".into(),
                version: "v1".into(),
            },
        );
        assert_eq!(m.state_of(&g), GameState::Installed { outdated: false });
        assert_eq!(m.toolkit_of(&id).version(), Some("v1"));
        // A release that is gone: no judgement, and the global one does not stand in.
        m.game_toolkits
            .insert(id.clone(), GameToolkit::Missing { tag: "1".into() });
        assert_eq!(m.state_of(&g), GameState::Installed { outdated: false });
        assert_eq!(m.toolkit_of(&id).version(), None);
    }

    #[test]
    fn a_missing_release_asks_instead_of_starting_or_unlocking() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let locked = game("b", None, ExecutableKind::Elf);
        let (id, locked_id) = (g.id.clone(), locked.id.clone());
        let mut m = model_with(vec![g, locked]);
        for who in [&id, &locked_id] {
            m.game_toolkits.insert(
                who.clone(),
                GameToolkit::Missing {
                    tag: "0.1.2".into(),
                },
            );
        }
        m.play(&id);
        assert_eq!(m.overlay, Some(Overlay::ToolkitMissing));
        assert_eq!(m.toolkit_question.as_ref(), Some(&id));
        assert!(m.starting.is_none(), "nothing started");
        assert!(
            !m.effects
                .iter()
                .any(|e| matches!(e, Effect::Pipeline(PipelineCommand::Play(_))))
        );
        m.overlay = None;
        m.unlock(&locked_id);
        assert_eq!(m.overlay, Some(Overlay::ToolkitMissing));
        assert_eq!(m.toolkit_question.as_ref(), Some(&locked_id));
        assert!(m.queue.is_empty());
        assert!(
            !m.effects
                .iter()
                .any(|e| matches!(e, Effect::Pipeline(PipelineCommand::Unlock(_))))
        );
        // The same game, following the global setting again, goes on.
        m.overlay = None;
        m.game_toolkits.clear();
        m.play(&id);
        assert!(m.starting.is_some());
        assert_eq!(m.overlay, None);
    }

    #[test]
    fn a_pinned_game_unlocks_without_a_global_toolkit() {
        let locked = game("b", None, ExecutableKind::Elf);
        let id = locked.id.clone();
        let mut m = model_with(vec![locked]);
        m.toolkit = None;
        m.unlock(&id);
        assert!(
            m.queue.is_empty(),
            "no toolkit at all: nothing to unlock with"
        );
        m.game_toolkits.insert(
            id.clone(),
            GameToolkit::Pinned {
                tag: "0.1.2".into(),
                version: "0.1.2-abc".into(),
            },
        );
        m.unlock(&id);
        assert_eq!(m.queue, vec![id]);
    }

    #[test]
    fn locked_and_unreadable() {
        let a = game("a", None, ExecutableKind::Container);
        let b = game("b", None, ExecutableKind::Unreadable);
        let m = model_with(vec![a.clone(), b.clone()]);
        assert_eq!(m.state_of(&a), GameState::Locked);
        assert_eq!(m.state_of(&b), GameState::Unreadable);
    }

    #[test]
    fn tiles_navigation_clamps_and_measures() {
        let mut m = model_with(vec![
            game("a", None, ExecutableKind::Elf),
            game("bb", None, ExecutableKind::Elf),
        ]);
        m.handle(Command::Left);
        assert_eq!(m.selected, 0);
        assert!(sounds(&mut m).is_empty());
        m.handle(Command::Right);
        assert_eq!(m.selected, 1);
        assert_eq!(sounds(&mut m), vec![Cue::Move]);
        assert!(
            m.effects
                .iter()
                .any(|e| matches!(e, Effect::MeasureSize(_)))
        );
        m.handle(Command::Right);
        assert_eq!(m.selected, 1);
        m.handle(Command::Down);
        assert_eq!(m.focus, HomeFocus::Actions(0));
        m.handle(Command::Up);
        m.handle(Command::Up);
        assert_eq!(m.focus, HomeFocus::SETTINGS);
        m.handle(Command::Back);
        assert_eq!(m.focus, HomeFocus::Tiles);
    }

    #[test]
    fn select_without_games_opens_the_folder_dialog() {
        let mut m = model_with(Vec::new());
        m.handle(Command::Select);
        assert!(m.effects.contains(&Effect::PickGamesFolder));
    }

    #[test]
    fn unlock_needs_a_toolkit_and_queues_once() {
        let g = game("a", None, ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g]);
        m.toolkit = None;
        m.handle(Command::Select);
        assert!(m.toast.as_ref().unwrap().text.contains("conversion tools"));
        assert!(m.queue.is_empty());

        m.toolkit = Some("v1".into());
        m.effects.clear();
        m.handle(Command::Select);
        m.handle(Command::Select); // now queued: the second select cancels
        assert!(m.queue.is_empty());
        assert!(
            m.effects
                .contains(&Effect::Pipeline(PipelineCommand::Unlock(id.clone())))
        );
        assert!(
            m.effects
                .contains(&Effect::Pipeline(PipelineCommand::Cancel(id)))
        );
    }

    #[test]
    fn unlock_flow_with_the_simulated_pipeline() {
        let g = game("Lantern", None, ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g.clone()]);
        let mut sim = SimPipeline::new("v1");
        sim.unlock_seconds = 1.0;
        m.handle(Command::Select);
        assert_eq!(m.state_of(&g), GameState::Queued);
        let prefs = m.prefs.clone();
        for effect in std::mem::take(&mut m.effects) {
            if let Effect::Pipeline(c) = effect {
                sim.submit(
                    c,
                    &PipelineContext {
                        root: None,
                        game: None,
                        prefs: &prefs,
                        extra_env: None,
                    },
                );
            }
        }
        let mut saw_installing = false;
        for i in 0..=15 {
            for e in sim.poll(f64::from(i) * 0.1) {
                m.on_pipeline(e);
            }
            saw_installing |= matches!(
                m.state_of(&m.games[0].clone()),
                GameState::Installing { .. }
            );
        }
        assert!(saw_installing);
        let g = m.games[0].clone();
        assert_eq!(m.state_of(&g), GameState::Installed { outdated: false });
        assert!(m.toast.as_ref().unwrap().text.starts_with("Lantern · "));
        assert!(sounds(&mut m).contains(&Cue::Unlocked));
        assert!(!m.failures.contains_key(&id));
    }

    #[test]
    fn a_failed_unlock_shows_retry_and_a_toast() {
        let g = game("Lantern", None, ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g.clone()]);
        m.on_pipeline(PipelineEvent::UnlockFinished {
            game: id.clone(),
            result: Err("relinker exited with 3".into()),
        });
        assert_eq!(
            m.state_of(&g),
            GameState::Failed("relinker exited with 3".into())
        );
        assert!(m.toast.as_ref().unwrap().text.ends_with(": Lantern"));
        m.effects.clear();
        m.handle(Command::Select);
        assert!(
            m.effects
                .contains(&Effect::Pipeline(PipelineCommand::Retry(id)))
        );
    }

    #[test]
    fn progress_is_coalesced() {
        let g = game("a", None, ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g]);
        let p = |fraction| PipelineEvent::Progress {
            game: id.clone(),
            step: UnlockStep::Relink,
            fraction,
        };
        m.on_pipeline(p(0.10));
        m.on_pipeline(p(0.102));
        assert_eq!(m.progress[&id].1, 0.10);
        m.on_pipeline(p(0.11));
        assert_eq!(m.progress[&id].1, 0.11);
    }

    #[test]
    fn a_running_game_shows_on_discord_until_it_ends() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g.clone()]);
        m.handle(Command::Select);
        m.on_pipeline(PipelineEvent::Spawned { game: id.clone() });
        m.effects.clear();
        m.on_pipeline(PipelineEvent::WindowShown { game: id.clone() });
        assert!(m.effects.contains(&Effect::Presence(Some(g.name.clone()))));
        m.effects.clear();
        m.on_pipeline(PipelineEvent::Exited {
            game: id,
            diagnosis: None,
        });
        assert!(m.effects.contains(&Effect::Presence(None)));
    }

    #[test]
    fn discord_stays_quiet_when_turned_off() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g]);
        m.prefs.discord_presence = false;
        m.handle(Command::Select);
        m.on_pipeline(PipelineEvent::Spawned { game: id.clone() });
        m.on_pipeline(PipelineEvent::WindowShown { game: id });
        assert!(
            !m.effects
                .iter()
                .any(|e| matches!(e, Effect::Presence(Some(_))))
        );
    }

    #[test]
    fn play_starts_runs_and_ends() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g.clone()]);
        m.handle(Command::Select);
        assert_eq!(m.state_of(&g), GameState::Starting);
        assert!(m.prefs.last_played.contains_key(id.as_str()));
        // While the splash is up everything but back is swallowed.
        assert_eq!(m.handle(Command::Right), Routed::Done);
        m.on_pipeline(PipelineEvent::Spawned { game: id.clone() });
        m.on_pipeline(PipelineEvent::WindowShown { game: id.clone() });
        assert_eq!(m.state_of(&g), GameState::Running);
        m.on_pipeline(PipelineEvent::Exited {
            game: id.clone(),
            diagnosis: Some("The game crashed (0xC0000005).".into()),
        });
        assert_eq!(m.state_of(&g), GameState::Installed { outdated: false });
        assert!(m.diagnoses.contains_key(&id));
        assert!(m.effects.contains(&Effect::RestoreWindow));
    }

    fn playtime_of(m: &Model, id: &GameId) -> (u64, u32) {
        let time = m.prefs.playtime(id);
        (time.seconds, time.sessions)
    }

    #[test]
    fn playtime_counts_from_the_window_and_adds_up_over_sessions() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g]);
        let t0 = Instant::now();
        assert_eq!(playtime_of(&m, &id), (0, 0));
        // Pressing Play and the splash count for nothing: the clock starts at the window.
        m.play(&id);
        m.on_pipeline(PipelineEvent::Spawned { game: id.clone() });
        assert_eq!(playtime_of(&m, &id), (0, 0));
        m.begin_session(&id, t0);
        assert_eq!(playtime_of(&m, &id), (0, 1));
        m.end_session(&id, t0 + Duration::from_secs(600));
        assert_eq!(playtime_of(&m, &id), (600, 1));
        m.begin_session(&id, t0 + Duration::from_secs(5000));
        m.end_session(&id, t0 + Duration::from_secs(5090));
        assert_eq!(playtime_of(&m, &id), (690, 2));
    }

    #[test]
    fn a_launcher_that_closes_mid_game_keeps_what_the_checkpoints_saw() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g]);
        let t0 = Instant::now();
        m.begin_session(&id, t0);
        m.checkpoint(t0 + Duration::from_secs(30));
        assert_eq!(playtime_of(&m, &id), (0, 1), "not due yet");
        m.checkpoint(t0 + Duration::from_secs(61));
        m.checkpoint(t0 + Duration::from_secs(90));
        m.checkpoint(t0 + Duration::from_secs(125));
        // The launcher is gone here: the model is dropped without an exit.
        assert_eq!(playtime_of(&m, &id), (125, 1));
        assert!(m.prefs_dirty);
    }

    #[test]
    fn a_crash_still_counts_the_time_before_it() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g]);
        m.handle(Command::Select);
        m.on_pipeline(PipelineEvent::Spawned { game: id.clone() });
        m.on_pipeline(PipelineEvent::WindowShown { game: id.clone() });
        assert_eq!(playtime_of(&m, &id), (0, 1));
        // Back-dated: the session began a quarter of an hour ago.
        m.sessions
            .begin(id.clone(), Instant::now() - Duration::from_secs(900));
        m.on_pipeline(PipelineEvent::Exited {
            game: id.clone(),
            diagnosis: Some("The game crashed (0xC0000005).".into()),
        });
        let (seconds, sessions) = playtime_of(&m, &id);
        assert!((900..=905).contains(&seconds), "{seconds}");
        assert_eq!(sessions, 1);
        assert!(m.sessions.running().is_none());
    }

    #[test]
    fn a_clean_exit_asks_for_a_backup_but_a_crash_or_no_window_does_not() {
        let backs_up = |m: &Model| {
            m.effects
                .iter()
                .any(|e| matches!(e, Effect::BackUpSaves(_)))
        };
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        for (window, diagnosis, expected) in [
            (true, None, true),
            (true, Some("The game crashed.".to_owned()), false),
            (false, None, false),
        ] {
            let mut m = model_with(vec![g.clone()]);
            m.play(&id);
            m.on_pipeline(PipelineEvent::Spawned { game: id.clone() });
            if window {
                m.on_pipeline(PipelineEvent::WindowShown { game: id.clone() });
            }
            m.effects.clear();
            m.on_pipeline(PipelineEvent::Exited {
                game: id.clone(),
                diagnosis,
            });
            assert_eq!(backs_up(&m), expected, "window {window}");
        }
    }

    #[test]
    fn a_game_that_never_showed_a_window_has_no_playtime() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g]);
        m.play(&id);
        m.on_pipeline(PipelineEvent::Spawned { game: id.clone() });
        m.on_pipeline(PipelineEvent::Exited {
            game: id.clone(),
            diagnosis: None,
        });
        assert_eq!(playtime_of(&m, &id), (0, 0));
        assert!(!m.prefs.playtime(&id).is_played());
    }

    #[test]
    fn back_on_the_splash_stops_the_start() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g]);
        m.play(&id);
        m.effects.clear();
        m.handle(Command::Back);
        assert!(m.starting.is_none());
        assert!(
            m.effects
                .contains(&Effect::Pipeline(PipelineCommand::Stop(id)))
        );
        assert!(m.effects.contains(&Effect::Sound(Cue::Back)));
    }

    #[test]
    fn a_game_that_dies_before_its_window_says_it_did_not_start() {
        let g = game("a", Some("v1"), ExecutableKind::Elf);
        let id = g.id.clone();
        let mut m = model_with(vec![g]);
        m.play(&id);
        m.on_pipeline(PipelineEvent::Spawned { game: id.clone() });
        m.on_pipeline(PipelineEvent::Exited {
            game: id,
            diagnosis: None,
        });
        assert!(m.starting.is_none() && m.running.is_none());
        assert_eq!(m.toast.as_ref().unwrap().text, "The game could not start");
    }

    #[test]
    fn overlay_commands_are_routed() {
        let mut m = model_with(vec![game("a", None, ExecutableKind::Elf)]);
        m.handle(Command::Options);
        assert_eq!(m.overlay, Some(Overlay::GameSettings));
        assert_eq!(
            m.handle(Command::Left),
            Routed::Overlay(Overlay::GameSettings)
        );
        m.close_overlay();
        assert_eq!(m.overlay, None);
    }

    #[test]
    fn scan_keeps_the_selected_game() {
        let a = game("a", None, ExecutableKind::Elf);
        let b = game("b", None, ExecutableKind::Elf);
        let c = game("c", None, ExecutableKind::Elf);
        let mut m = model_with(vec![a.clone(), b.clone()]);
        m.selected = 1;
        m.on_core(CoreEvent::LibraryScanned {
            root: PathBuf::from("C:/games"),
            games: vec![c, a, b],
            records: Default::default(),
        });
        assert_eq!(m.selected_game().unwrap().name, "b");
        // A scan of another folder is ignored.
        m.on_core(CoreEvent::LibraryScanned {
            root: PathBuf::from("D:/other"),
            games: vec![],
            records: Default::default(),
        });
        assert_eq!(m.games.len(), 3);
    }

    #[test]
    fn onboarding_walks_three_steps_and_finishes() {
        let prefs = Preferences {
            language: settings::Language::English,
            ..Preferences::default()
        };
        let mut m = Model::new(
            prefs,
            AppPaths::under(std::path::Path::new("C:/none")),
            false,
        );
        assert!(!m.onboarded());
        // Step 0: moving right previews Turkish at once (Turkish is the first card, English the second).
        assert_eq!(m.onboarding_choice(), 1);
        m.handle(Command::Left);
        assert_eq!(m.language(), Language::Turkish);
        m.handle(Command::Select);
        assert_eq!(m.onboarding.step, 1);
        // Step 1: right previews Aurora, select advances.
        m.handle(Command::Right);
        assert_eq!(m.theme(), Theme::Aurora);
        m.handle(Command::Select);
        assert_eq!(m.onboarding.step, 2);
        // Step 2: select opens the dialog; picking a folder moves focus to Start.
        m.effects.clear();
        m.handle(Command::Select);
        assert!(m.effects.contains(&Effect::PickGamesFolder));
        m.choose_folder(Some(PathBuf::from("C:/games")));
        assert_eq!(m.onboarding_choice(), 1);
        assert!(m.effects.contains(&Effect::Rescan));
        m.handle(Command::Select);
        assert!(m.onboarded());
        // Back from step 2 returns.
        let mut m2 = Model::new(
            Preferences::default(),
            AppPaths::under(std::path::Path::new("C:/none")),
            false,
        );
        m2.handle(Command::Select);
        m2.handle(Command::Back);
        assert_eq!(m2.onboarding.step, 0);
    }

    #[test]
    fn preferences_are_only_dirty_after_a_real_change() {
        let mut m = Model::new(
            Preferences::default(),
            AppPaths::under(std::path::Path::new("C:/none")),
            true,
        );
        m.edit_prefs(|p| p.sounds = true);
        assert!(!m.take_prefs_dirty());
        m.edit_prefs(|p| p.sounds = false);
        assert!(m.take_prefs_dirty());
        assert!(!m.take_prefs_dirty());
    }

    #[test]
    fn toast_expires_but_stays_for_its_exit_animation() {
        let mut m = model_with(Vec::new());
        m.tick(10.0);
        m.toast("hi", Icon::CHECK);
        assert!(m.toast_visible());
        m.tick(13.3);
        assert!(!m.toast_visible());
        assert!(m.toast.is_some());
    }
}
