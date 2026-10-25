//! [`Pipeline`] backed by the unlock queue and the launcher of `anyps5-core`.
//!
//! Commands become `UnlockRequest`s and `LaunchRequest`s; the core's events come back through
//! closures into a shared list that [`Pipeline::poll`] drains once per frame (each push also wakes
//! the UI). Diagnoses are turned into the launcher's language here, so the model only sees text.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyps5_core::compat::{self, Feature, Fit};
use anyps5_core::diagnosis::Diagnosis;
use anyps5_core::fonts;
use anyps5_core::hostcpu::{CpuConversion, HostCpu};
use anyps5_core::idle;
use anyps5_core::launch::{LaunchEvent, LaunchRequest, Launcher, RunHandle, WindowPrefs};
use anyps5_core::paths::{self, AppPaths, GameId};
use anyps5_core::settings::{self, EnvContext, Preferences};
use anyps5_core::toolkit::{
    self, GameToolkit, Toolkit, ToolkitChoice, adopt_folder, open_for_game,
};
use anyps5_core::unlock::{self, UnlockError, UnlockEvent, UnlockQueue, UnlockRequest};

use crate::app::model::ui_language;
use crate::app::pipeline::{Pipeline, PipelineCommand, PipelineContext, PipelineEvent, UnlockStep};
use crate::ui::i18n::{Arg, Key, Language};

type Shared = Arc<Mutex<Vec<PipelineEvent>>>;

pub struct CorePipeline {
    paths: AppPaths,
    toolkit: Option<Toolkit>,
    /// What `glibc::check` said when the toolkit was located.
    glibc: Option<anyps5_core::glibc::Shortfall>,
    /// What the releases games stay on look like on disk, by the pin as the preferences hold it.
    /// Cleared when a toolkit was installed, removed or chosen, so no frame reads the disk.
    pinned: HashMap<String, GameToolkit>,
    /// The releases games stay on, opened, by plain tag.
    opened: HashMap<String, Toolkit>,
    queue: Arc<UnlockQueue>,
    /// A game starts or runs: prepared games stand still (the idle gate's thread reads it).
    game_active: Arc<AtomicBool>,
    /// How long the steps of the last full unlock took here.
    step_seconds: Option<[f32; 5]>,
    /// Ends the idle gate's thread.
    gate_stop: Arc<AtomicBool>,
    runs: HashMap<GameId, RunHandle>,
    events: Shared,
    wake: Arc<dyn Fn() + Send + Sync>,
    language: Arc<Mutex<Language>>,
}

fn map_step(step: unlock::Step) -> UnlockStep {
    match step {
        unlock::Step::Unwrap => UnlockStep::Unwrap,
        unlock::Step::Relink => UnlockStep::Relink,
        unlock::Step::Copy => UnlockStep::Copy,
        unlock::Step::Assemble => UnlockStep::Bundle,
        unlock::Step::Finish => UnlockStep::Finish,
    }
}

/// The localised one-sentence reason (falls back to the English text for kinds without a key).
fn diagnosis_text(diagnosis: &Diagnosis, language: Language) -> String {
    let (name, args) = diagnosis.summary_key();
    let key = match name {
        "diagnosisMissingModule" => Key::DiagnosisMissingModule,
        "diagnosisUnsupported" => Key::DiagnosisUnsupported,
        "diagnosisGpuLost" => Key::DiagnosisGpuLost,
        "diagnosisOutOfMemory" => Key::DiagnosisOutOfMemory,
        "diagnosisEngineError" => Key::DiagnosisEngineError,
        "diagnosisCrashed" => Key::DiagnosisCrashed,
        _ => return diagnosis.summary(),
    };
    let args: Vec<Arg<'_>> = args.iter().map(|a| Arg::Str(a)).collect();
    language.format(key, &args)
}

/// The one-sentence reason of a failed unlock, with the next step, in the player's language. The
/// technical message goes to the log.
fn failure_text(error: &UnlockError, language: Language) -> String {
    let sentence = crate::screens::prepare_strings::failure(error.kind(), language);
    // How much is missing is worth saying; the sentence then says what to do.
    if matches!(error, UnlockError::NotEnoughSpace { .. }) {
        return format!("{}. {sentence}", error.to_string().trim_end_matches('.'));
    }
    sentence.to_owned()
}

/// How often the idle gate looks at the machine.
const GATE_PERIOD: Duration = Duration::from_secs(2);

/// Holds the games the launcher prepares on its own while the PC is not idle (a game runs, the
/// battery is in use, memory is short, the processors are busy). Ends when the queue is gone or
/// `stop` is set.
fn watch_machine(queue: &Arc<UnlockQueue>, game_active: &Arc<AtomicBool>, stop: &Arc<AtomicBool>) {
    let (queue, game_active, stop) = (Arc::downgrade(queue), game_active.clone(), stop.clone());
    let spawned = std::thread::Builder::new()
        .name("idle-gate".into())
        .spawn(move || {
            let mut meter = idle::Meter::new();
            let mut gate = idle::Gate::new();
            loop {
                let Some(queue) = queue.upgrade() else {
                    return;
                };
                let reading = meter.sample();
                let hold = gate.update(
                    &reading,
                    game_active.load(Ordering::Relaxed),
                    queue.background_active(),
                );
                // Read again: a game may have started since the reading above.
                queue.hold_background(hold.is_some() || game_active.load(Ordering::Relaxed));
                drop(queue);
                // Short sleeps, so the launcher ends without waiting for the period.
                for _ in 0..20 {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(GATE_PERIOD / 20);
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!("the idle gate did not start: {error}");
    }
}

/// `{"seconds":[unwrap,relink,layout,assemble,finish]}` from the last full unlock here.
fn read_step_seconds(file: &Path) -> Option<[f32; 5]> {
    let text = std::fs::read_to_string(file).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let list = value.get("seconds")?.as_array()?;
    let mut seconds = [0.0f32; 5];
    if list.len() != seconds.len() {
        return None;
    }
    for (slot, item) in seconds.iter_mut().zip(list) {
        *slot = item.as_f64()? as f32;
    }
    Some(seconds)
}

fn write_step_seconds(file: &Path, seconds: [f32; 5]) {
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let text = serde_json::json!({ "seconds": seconds }).to_string();
    if let Err(error) = std::fs::write(file, text) {
        tracing::debug!("could not keep the unlock timings: {error}");
    }
}

impl Drop for CorePipeline {
    fn drop(&mut self) {
        self.gate_stop.store(true, Ordering::Relaxed);
    }
}

impl CorePipeline {
    /// `wake` repaints the UI (called after every event).
    pub fn new(paths: AppPaths, prefs: &Preferences, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let events: Shared = Arc::default();
        let language = Arc::new(Mutex::new(ui_language(prefs.language)));
        let sink = {
            let (events, wake, language) = (events.clone(), wake.clone(), language.clone());
            Arc::new(move |event: UnlockEvent| {
                let mapped = match event {
                    UnlockEvent::Started { game } => Some(PipelineEvent::Progress {
                        game,
                        step: UnlockStep::Unwrap,
                        fraction: 0.0,
                    }),
                    UnlockEvent::Progress {
                        game,
                        step,
                        fraction,
                    } => Some(PipelineEvent::Progress {
                        game,
                        step: map_step(step),
                        fraction: fraction as f32,
                    }),
                    UnlockEvent::Finished { game, result } => Some(match result {
                        Ok(record) => PipelineEvent::UnlockFinished {
                            game,
                            result: Ok(record.toolkit),
                        },
                        Err(UnlockError::Cancelled) => PipelineEvent::UnlockCancelled { game },
                        Err(error) => {
                            tracing::warn!("unlock of {} failed: {error}", game.as_str());
                            let language = *language.lock().expect("language lock");
                            PipelineEvent::UnlockFinished {
                                game,
                                result: Err(failure_text(&error, language)),
                            }
                        }
                    }),
                    UnlockEvent::Paused { game, paused } => {
                        Some(PipelineEvent::UnlockPaused { game, paused })
                    }
                    UnlockEvent::Timings { seconds, .. } => {
                        Some(PipelineEvent::StepSeconds(seconds))
                    }
                    _ => None,
                };
                if let Some(event) = mapped {
                    events.lock().expect("events lock").push(event);
                    wake();
                }
            })
        };
        let queue = Arc::new(UnlockQueue::new(sink));
        let game_active = Arc::new(AtomicBool::new(false));
        let gate_stop = Arc::new(AtomicBool::new(false));
        watch_machine(&queue, &game_active, &gate_stop);
        let mut pipeline = Self {
            toolkit: None,
            glibc: None,
            pinned: HashMap::new(),
            opened: HashMap::new(),
            queue,
            game_active,
            step_seconds: read_step_seconds(&paths.unlock_timings_file()),
            gate_stop,
            runs: HashMap::new(),
            events,
            wake,
            language,
            paths,
        };
        pipeline.locate_toolkit(prefs);
        pipeline
    }

    /// Looks for the toolkit again (after a download or a changed folder).
    pub fn locate_toolkit(&mut self, prefs: &Preferences) {
        self.pinned.clear();
        self.opened.clear();
        let pinned = prefs
            .toolkit_pinned
            .clone()
            .map(|tag| ToolkitChoice::Release { tag });
        self.toolkit = match &prefs.toolkit {
            Some(folder) => adopt_folder(folder)
                .ok()
                .or_else(|| Toolkit::locate(&self.paths, None)),
            // The engine channel never falls back to another release: no engine, no toolkit.
            None if prefs.toolkit_engine => anyps5_core::toolkit::open_engine(&self.paths),
            None => Toolkit::locate(&self.paths, pinned.as_ref()),
        };
        self.glibc = self.toolkit.as_ref().and_then(anyps5_core::glibc::check);
        if let Some(short) = self.glibc {
            tracing::warn!(
                "the toolkit needs glibc {} and this system has {}; games may not start",
                short.needed,
                short.system
            );
        }
        if let Some(toolkit) = &self.toolkit {
            match compat::fit(toolkit.version()) {
                Fit::Newer => tracing::info!(
                    "toolkit {} is newer than this launcher was tested with (v{}); using it as is",
                    toolkit.version(),
                    compat::TESTED_UP_TO
                ),
                Fit::Older => tracing::warn!(
                    "toolkit {} is older than v{}; features it lacks are turned off",
                    toolkit.version(),
                    compat::TESTED_UP_TO
                ),
                Fit::Tested | Fit::Local => {}
            }
        }
    }

    /// Whether the toolkit in use has `feature`; true when there is none (nothing to degrade).
    fn supports(&self, feature: Feature) -> bool {
        self.toolkit
            .as_ref()
            .is_none_or(|t| compat::supports(feature, t.version()))
    }

    /// The version of the toolkit a game uses, `Err` with the release it stays on when that is gone.
    fn version_for(&mut self, pin: Option<&str>) -> Result<Option<String>, String> {
        let global = self.toolkit.as_ref().map(|t| t.version().to_owned());
        match pin {
            None => Ok(global),
            Some(pin) => match self.pinned_toolkit(pin, global.as_deref()) {
                GameToolkit::Pinned { version, .. } => Ok(Some(version)),
                GameToolkit::Missing { tag } => Err(tag),
                GameToolkit::Global(version) => Ok(version),
            },
        }
    }

    fn push(&self, event: PipelineEvent) {
        self.events.lock().expect("events lock").push(event);
        (self.wake)();
    }

    /// The toolkit to unlock a game with: its own release, else the global one. A release that is
    /// gone is an error: the global toolkit never stands in for it.
    fn toolkit_for(&mut self, pin: Option<&str>) -> Result<Option<Toolkit>, toolkit::ToolkitError> {
        let Some(pin) = pin else {
            return Ok(self.toolkit.clone());
        };
        let tag = toolkit::pin_tag(pin)
            .ok_or_else(|| toolkit::ToolkitError::PinMissing(pin.to_owned()))?;
        if let Some(opened) = self.opened.get(&tag) {
            return Ok(Some(opened.clone()));
        }
        let opened = open_for_game(&self.paths, None, Some(&tag))?;
        if let Some(opened) = &opened {
            self.opened.insert(tag, opened.clone());
        }
        Ok(opened)
    }

    fn unlock(
        &mut self,
        game: &anyps5_core::library::Game,
        root: &Path,
        prefs: &Preferences,
        background: bool,
        fresh: bool,
    ) {
        let pin = prefs.game_settings(&game.id).toolkit;
        let toolkit = match self.toolkit_for(pin.as_deref()) {
            Ok(Some(toolkit)) => toolkit,
            Ok(None) => {
                // Nothing to unlock with: the model must not wait for it.
                self.push(PipelineEvent::UnlockCancelled {
                    game: game.id.clone(),
                });
                return;
            }
            Err(error) => {
                self.push(PipelineEvent::UnlockFinished {
                    game: game.id.clone(),
                    result: Err(error.to_string()),
                });
                return;
            }
        };
        let mut request = UnlockRequest::new(
            game.folder.clone(),
            game.title_id.clone(),
            root.to_path_buf(),
            toolkit,
        );
        let mode = match prefs.cpu_conversion {
            settings::CpuConversion::Auto => CpuConversion::Auto,
            settings::CpuConversion::None => CpuConversion::None,
            settings::CpuConversion::Intel => CpuConversion::Intel,
        };
        request.to_intel = HostCpu::detect().wants_intel_conversion(mode);
        request.relink_flags = prefs.game_settings(&game.id).relink;
        request.resolution = prefs.game_settings(&game.id).resolution;
        // Unlocking again replaces the copy; its saves are backed up first.
        request.backup_dir = Some(self.paths.game_backups_dir(&game.title_id));
        request.background = background;
        request.fresh = fresh;
        if background {
            // The gate looks only every few seconds; a game that runs now must not wait for it.
            self.queue
                .hold_background(self.game_active.load(Ordering::Relaxed));
        }
        if !self.queue.enqueue(request) {
            // Asked for by the player while the launcher prepares it: it goes first.
            if !background {
                self.queue.promote(&game.id);
            }
            tracing::debug!("{} is already queued", game.name);
        }
    }

    fn play(
        &mut self,
        game: &anyps5_core::library::Game,
        root: &Path,
        prefs: &Preferences,
        extra_env: Option<&std::collections::BTreeMap<String, String>>,
    ) {
        if self.runs.values().any(RunHandle::is_alive) {
            return;
        }
        let copy = paths::unlocked_copy_dir(root, &game.title_id, &game.id);
        let name = paths::unlocked_copy_name(&game.title_id, &game.id);
        let settings = prefs.game_settings(&game.id);
        // What the game was made for decides what it is told: its own release, else the global one.
        let version = match self.version_for(settings.toolkit.as_deref()) {
            Ok(version) => version,
            Err(tag) => {
                self.push(PipelineEvent::StartFailed {
                    game: game.id.clone(),
                    message: toolkit::ToolkitError::PinMissing(tag).to_string(),
                });
                return;
            }
        };
        // Without the input file the game keeps the engine's built-in keys.
        let input_file = self.paths.input_file(&name);
        let written = settings
            .write_input_file(
                &input_file,
                version.as_deref().and_then(compat::release_tag).as_deref(),
            )
            .is_ok();
        // F12 screenshots exist only in builds newer than v0.1.3; older toolkits ignore the folder.
        let screenshots = version
            .as_deref()
            .is_none_or(|v| compat::supports(Feature::Screenshots, v))
            .then(|| prefs.screenshots_dir(&self.paths));
        let shader_cache = self.paths.shader_cache_dir(&game.title_id);
        let launcher_language = match prefs.language {
            settings::Language::Turkish => 19,
            settings::Language::English => 1,
        };
        let fonts = fonts::resolve(
            &self.paths,
            &crate::fonts::BUNDLE,
            prefs.fonts_folder.as_deref(),
        );
        // The game starts with what is there; the Japanese, Chinese and Korean fonts follow in the
        // background and are used from the next start.
        if prefs.fonts_folder.is_none() && fonts.is_some() {
            let wake = self.wake.clone();
            fonts::download_cjk_in_background(
                &self.paths,
                crate::fonts::BUNDLE,
                true,
                Some(Arc::new(move || wake())),
            );
        }
        let env = settings.environment(&EnvContext {
            launcher_language,
            input_file: written.then_some(input_file),
            screenshots: screenshots.clone(),
            fonts,
            shader_cache: Some(shader_cache.clone()),
        });
        let mut request = LaunchRequest::new(
            game.id.clone(),
            game.title_id.clone(),
            copy,
            self.paths.log_file(&game.title_id),
        );
        request.env = env;
        if let Some(extra) = extra_env {
            request
                .env
                .extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        request.create_dirs = screenshots.into_iter().chain([shader_cache]).collect();
        request.window = WindowPrefs {
            fullscreen: settings.fullscreen,
            size_percent: settings.window_size_clamped(),
        };
        let language = self.language.clone();
        let (events, wake) = (self.events.clone(), self.wake.clone());
        let sink = Arc::new(move |event: LaunchEvent| {
            let mapped = match event {
                LaunchEvent::Spawned { game, .. } => Some(PipelineEvent::Spawned { game }),
                LaunchEvent::WindowShown { game, .. } => Some(PipelineEvent::WindowShown { game }),
                LaunchEvent::Exited { .. } => None,
                LaunchEvent::Diagnosed {
                    game, diagnosis, ..
                } => {
                    let language = *language.lock().expect("language lock");
                    Some(PipelineEvent::Exited {
                        game,
                        diagnosis: diagnosis.map(|d| diagnosis_text(&d, language)),
                    })
                }
            };
            if let Some(event) = mapped {
                events.lock().expect("events lock").push(event);
                wake();
            }
        });
        // The game gets the whole PC: what the launcher prepares stands still from now on.
        self.game_active.store(true, Ordering::Relaxed);
        self.queue.hold_background(true);
        match Launcher::start(request, sink) {
            Ok(handle) => {
                self.runs.insert(game.id.clone(), handle);
            }
            Err(error) => {
                self.game_active.store(false, Ordering::Relaxed);
                self.push(PipelineEvent::StartFailed {
                    game: game.id.clone(),
                    message: error.to_string(),
                });
            }
        }
    }
}

impl Pipeline for CorePipeline {
    fn toolkit_version(&self) -> Option<String> {
        self.toolkit.as_ref().map(|t| t.version().to_owned())
    }

    fn glibc_shortfall(&self) -> Option<anyps5_core::glibc::Shortfall> {
        self.glibc
    }

    fn submit(&mut self, command: PipelineCommand, cx: &PipelineContext<'_>) {
        *self.language.lock().expect("language lock") = ui_language(cx.prefs.language);
        let fresh = matches!(command, PipelineCommand::UnlockAgain(_));
        // Copies are moving to another place: nothing is unlocked or started until they are there.
        if cx.prefs.unlocked_moving_from.is_some() {
            let waits =
                crate::screens::location_strings::waits_for_move(ui_language(cx.prefs.language));
            match &command {
                PipelineCommand::Unlock(game)
                | PipelineCommand::Retry(game)
                | PipelineCommand::UnlockAgain(game) => {
                    self.push(PipelineEvent::UnlockFinished {
                        game: game.clone(),
                        result: Err(waits.to_owned()),
                    });
                    return;
                }
                PipelineCommand::Prepare(game) => {
                    self.push(PipelineEvent::UnlockCancelled { game: game.clone() });
                    return;
                }
                PipelineCommand::Play(game) => {
                    self.push(PipelineEvent::StartFailed {
                        game: game.clone(),
                        message: waits.to_owned(),
                    });
                    return;
                }
                _ => {}
            }
        }
        match command {
            PipelineCommand::Unlock(_)
            | PipelineCommand::Retry(_)
            | PipelineCommand::UnlockAgain(_) => {
                if self.toolkit.is_none() {
                    self.locate_toolkit(cx.prefs);
                }
                if let (Some(game), Some(root)) = (cx.game, cx.root) {
                    // Unlock again repairs a copy: it relinks and keeps nothing of the old one.
                    self.unlock(game, root, cx.prefs, false, fresh);
                }
            }
            PipelineCommand::Prepare(prepared) => {
                if self.toolkit.is_none() {
                    self.locate_toolkit(cx.prefs);
                }
                if let (Some(game), Some(root)) = (cx.game, cx.root) {
                    self.unlock(game, root, cx.prefs, true, false);
                } else {
                    self.push(PipelineEvent::UnlockCancelled { game: prepared });
                }
            }
            PipelineCommand::Promote(game) => {
                self.queue.promote(&game);
            }
            PipelineCommand::Cancel(game) => self.queue.cancel(&game),
            PipelineCommand::Play(game_id) => {
                if let (Some(game), Some(root)) = (cx.game, cx.root) {
                    self.play(game, root, cx.prefs, cx.extra_env);
                } else {
                    self.push(PipelineEvent::StartFailed {
                        game: game_id,
                        message: "game not found".to_owned(),
                    });
                }
            }
            PipelineCommand::Stop(game) => {
                if let Some(run) = self.runs.get(&game) {
                    run.kill();
                }
            }
            PipelineCommand::Activate(game) => {
                if let Some(run) = self.runs.get(&game) {
                    run.activate();
                }
            }
        }
    }

    fn game_target(&self, game: &GameId) -> Option<super::pipeline::GameTarget> {
        let run = self.runs.get(game)?;
        Some(super::pipeline::GameTarget {
            window: run.window().map(|w| w as i64),
            pids: run.pids(),
        })
    }

    fn poll(&mut self, _now: f64) -> Vec<PipelineEvent> {
        let events = std::mem::take(&mut *self.events.lock().expect("events lock"));
        for event in &events {
            match event {
                PipelineEvent::Exited { game, .. } => {
                    self.runs.remove(game);
                }
                PipelineEvent::StepSeconds(seconds) => {
                    self.step_seconds = Some(*seconds);
                    write_step_seconds(&self.paths.unlock_timings_file(), *seconds);
                }
                _ => {}
            }
        }
        if events.iter().any(|e| {
            matches!(
                e,
                PipelineEvent::Exited { .. } | PipelineEvent::StartFailed { .. }
            )
        }) {
            let active = self.runs.values().any(RunHandle::is_alive);
            self.game_active.store(active, Ordering::Relaxed);
        }
        events
    }

    fn step_seconds(&self) -> Option<[f32; 5]> {
        self.step_seconds
    }

    fn before_scan(&mut self, root: &Path) {
        if self.queue.is_idle() {
            unlock::remove_leftovers(root);
        }
    }

    fn reload_toolkit(&mut self, prefs: &Preferences) {
        self.locate_toolkit(prefs);
    }

    fn pinned_toolkit(&mut self, pin: &str, global: Option<&str>) -> GameToolkit {
        let paths = &self.paths;
        if let Some(found) = self.pinned.get(pin) {
            return found.clone();
        }
        let found = toolkit::game_toolkit(paths, global, Some(pin));
        // A release that is missing is looked for again every time: it may arrive any moment.
        if matches!(found, GameToolkit::Pinned { .. }) {
            self.pinned.insert(pin.to_owned(), found.clone());
        }
        found
    }

    fn patches_dir(&self) -> Option<std::path::PathBuf> {
        // Patches (and the resolutions they offer) need v0.1.3.
        self.toolkit
            .as_ref()
            .filter(|t| compat::supports(Feature::Patches, t.version()))
            .map(|t| t.patches().to_path_buf())
    }
}
