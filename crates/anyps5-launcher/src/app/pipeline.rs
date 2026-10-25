//! The seam between the UI and the unlock/launch pipeline of `anyps5-core`.
//!
//! The model never talks to the pipeline directly. It pushes [`PipelineCommand`]s (as
//! `Effect::Pipeline`) that the host hands to a [`Pipeline`], and it reads [`PipelineEvent`]s back
//! from [`Pipeline::poll`] once per frame. An adapter for the core pipeline implements the trait
//! (`submit` turns a command into an `UnlockRequest` / `LaunchRequest`, `poll` turns the core's
//! `UnlockEvent`s and `LaunchEvent`s into [`PipelineEvent`]s, texts already localised).
//!
//! Until that adapter lands the launcher runs with [`NoPipeline`]; [`SimPipeline`] simulates a
//! pipeline with timers for the demo mode and the tests.

use std::path::Path;

use anyps5_core::library::Game;
use anyps5_core::paths::GameId;
use anyps5_core::settings::Preferences;
use anyps5_core::toolkit::GameToolkit;

use crate::ui::i18n::Key;

/// The five steps of an unlock and their share of the single overall bar (DESIGN.md 5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnlockStep {
    Unwrap,
    Relink,
    Copy,
    Bundle,
    Finish,
}

impl UnlockStep {
    pub const ALL: [UnlockStep; 5] = [
        UnlockStep::Unwrap,
        UnlockStep::Relink,
        UnlockStep::Copy,
        UnlockStep::Bundle,
        UnlockStep::Finish,
    ];

    /// Where the step starts on the overall bar.
    pub const fn start(self) -> f32 {
        match self {
            UnlockStep::Unwrap => 0.0,
            UnlockStep::Relink => 0.12,
            UnlockStep::Copy => 0.62,
            UnlockStep::Bundle => 0.88,
            UnlockStep::Finish => 0.96,
        }
    }

    /// How much of the overall bar the step covers.
    pub const fn length(self) -> f32 {
        match self {
            UnlockStep::Unwrap => 0.12,
            UnlockStep::Relink => 0.50,
            UnlockStep::Copy => 0.26,
            UnlockStep::Bundle => 0.08,
            UnlockStep::Finish => 0.04,
        }
    }

    /// `start + length * fraction`.
    pub fn overall(self, fraction: f32) -> f32 {
        self.start() + self.length() * fraction.clamp(0.0, 1.0)
    }

    pub const fn title_key(self) -> Key {
        match self {
            UnlockStep::Unwrap => Key::StepUnwrap,
            UnlockStep::Relink => Key::StepRelink,
            UnlockStep::Copy => Key::StepCopy,
            UnlockStep::Bundle => Key::StepBundle,
            UnlockStep::Finish => Key::StepFinish,
        }
    }
}

/// What the steps of an unlock cost, as shares of the one bar. A PC that has unlocked before
/// weighs them by what they took there (the relink dominates on one, the file layout on another);
/// until then the shares are what a 300 MB executable with thousands of data files measured.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepWeights([f32; 5]);

impl StepWeights {
    /// No step weighs less than this: a step that took no time still shows.
    const FLOOR: f32 = 0.02;

    /// The shares before this PC has measured anything.
    pub const DEFAULT: StepWeights = StepWeights([0.04, 0.78, 0.12, 0.04, 0.02]);

    /// Shares from the seconds each step took; `None` when nothing was measured.
    pub fn from_seconds(seconds: [f32; 5]) -> Option<StepWeights> {
        let seconds = seconds.map(|s| if s.is_finite() { s.max(0.0) } else { 0.0 });
        let total: f32 = seconds.iter().sum();
        if total < 1.0 {
            return None;
        }
        let mut shares = seconds.map(|s| (s / total).max(Self::FLOOR));
        let sum: f32 = shares.iter().sum();
        shares.iter_mut().for_each(|s| *s /= sum);
        Some(StepWeights(shares))
    }

    pub fn share(&self, step: UnlockStep) -> f32 {
        self.0[step as usize]
    }

    /// Where the step starts on the overall bar.
    pub fn start(&self, step: UnlockStep) -> f32 {
        self.0[..step as usize].iter().sum()
    }

    /// `start + share * fraction`, 0..=1.
    pub fn overall(&self, step: UnlockStep, fraction: f32) -> f32 {
        (self.start(step) + self.share(step) * fraction.clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }
}

impl Default for StepWeights {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// What the UI asks the pipeline to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PipelineCommand {
    /// Queue an unlock of a locked game.
    Unlock(GameId),
    /// Queue an unlock again after a failure.
    Retry(GameId),
    /// Drop a queued unlock or stop the running one.
    Cancel(GameId),
    /// Start an unlocked game.
    Play(GameId),
    /// Kill the running game (also while it is still starting).
    Stop(GameId),
    /// Queue a fresh unlock of an already unlocked game (update, "Reinstall").
    UnlockAgain(GameId),
    /// Bring the running game's window to the front.
    Activate(GameId),
    /// Unlock a game on the launcher's own account, while the PC is idle: lowest priority, and
    /// it stands still whenever the player does something else.
    Prepare(GameId),
    /// The player now waits for a game that is being prepared: it goes first.
    Promote(GameId),
}

impl PipelineCommand {
    pub fn game(&self) -> &GameId {
        match self {
            PipelineCommand::Prepare(g) | PipelineCommand::Promote(g) => g,
            PipelineCommand::Unlock(g)
            | PipelineCommand::Retry(g)
            | PipelineCommand::Cancel(g)
            | PipelineCommand::Play(g)
            | PipelineCommand::Stop(g)
            | PipelineCommand::UnlockAgain(g)
            | PipelineCommand::Activate(g) => g,
        }
    }
}

/// What the pipeline reports back.
#[derive(Clone, Debug, PartialEq)]
pub enum PipelineEvent {
    /// A step is running (the first one also means "left the queue").
    Progress {
        game: GameId,
        step: UnlockStep,
        fraction: f32,
    },
    /// An unlock ended. `Ok` carries the toolkit version the copy was made with.
    UnlockFinished {
        game: GameId,
        result: Result<String, String>,
    },
    /// An unlock was cancelled (queued or running).
    UnlockCancelled { game: GameId },
    /// An unlock stands still (the player started a game, the PC went on battery) or goes on.
    UnlockPaused { game: GameId, paused: bool },
    /// How long each step of an unlock took here, for weighing the bar next time.
    StepSeconds([f32; 5]),
    /// The game process exists.
    Spawned { game: GameId },
    /// The game's first window showed.
    WindowShown { game: GameId },
    /// The process could not be started.
    StartFailed { game: GameId, message: String },
    /// The process ended. `diagnosis` is the localised one-sentence reason, when it ended on an error.
    Exited {
        game: GameId,
        diagnosis: Option<String>,
    },
}

/// What the pipeline needs to know to act on a command.
pub struct PipelineContext<'a> {
    /// The games folder.
    pub root: Option<&'a Path>,
    /// The game the command is about, when it is in the library.
    pub game: Option<&'a Game>,
    pub prefs: &'a Preferences,
    /// Variables a started game gets beyond its own settings (the network bridge's).
    pub extra_env: Option<&'a std::collections::BTreeMap<String, String>>,
}

/// The unlock/launch pipeline as the UI sees it.
/// The window and the processes of a running game.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameTarget {
    /// A Windows `HWND` as a number, once the game has shown a window.
    pub window: Option<i64>,
    pub pids: Vec<u32>,
}

pub trait Pipeline {
    /// The toolkit version unlocked copies are compared against; `None` when this PC has no toolkit.
    fn toolkit_version(&self) -> Option<String>;
    /// How long the steps of the last full unlock took on this PC.
    fn step_seconds(&self) -> Option<[f32; 5]> {
        None
    }
    /// The glibc the toolkit needs against the system's, when the system's is older (Linux).
    fn glibc_shortfall(&self) -> Option<anyps5_core::glibc::Shortfall> {
        None
    }
    /// Runs a command. Never blocks.
    fn submit(&mut self, command: PipelineCommand, cx: &PipelineContext<'_>);
    /// Events since the last call. `now` is the UI clock in seconds.
    fn poll(&mut self, now: f64) -> Vec<PipelineEvent>;
    /// Called before a rescan while nothing unlocks: clean up half-made copies.
    fn before_scan(&mut self, _root: &Path) {}
    /// A toolkit was downloaded or adopted, or the folder preference changed: look again.
    fn reload_toolkit(&mut self, _prefs: &Preferences) {}
    /// The toolkit's per-title patch files (the resolutions of the game settings come from them).
    fn patches_dir(&self) -> Option<std::path::PathBuf> {
        None
    }
    /// Where a running game shows its picture and which processes are its (share screen).
    fn game_target(&self, _game: &GameId) -> Option<GameTarget> {
        None
    }
    /// What a game that stays on the release `pin` finds on this PC: that release, or nothing.
    /// Never the global toolkit instead. `global` is the version in use, for the games that follow it.
    fn pinned_toolkit(&mut self, pin: &str, _global: Option<&str>) -> GameToolkit {
        GameToolkit::Missing {
            tag: pin.trim_start_matches('v').to_owned(),
        }
    }
}

/// No pipeline: no toolkit, nothing starts. What the launcher runs with until the core's pipeline
/// is wired in.
#[derive(Default)]
pub struct NoPipeline {
    events: Vec<PipelineEvent>,
}

impl Pipeline for NoPipeline {
    fn toolkit_version(&self) -> Option<String> {
        None
    }

    fn submit(&mut self, command: PipelineCommand, _cx: &PipelineContext<'_>) {
        if let PipelineCommand::Play(game) = command {
            self.events.push(PipelineEvent::StartFailed {
                game,
                message: "no toolkit".to_owned(),
            });
        }
    }

    fn poll(&mut self, _now: f64) -> Vec<PipelineEvent> {
        std::mem::take(&mut self.events)
    }
}

/// A pipeline that only simulates: an unlock walks through the five steps in about 5 s, a start
/// shows its window after 2.5 s and the game runs until stopped. Used by `--demo` and the tests.
pub struct SimPipeline {
    version: String,
    queue: Vec<GameId>,
    current: Option<(GameId, f64)>,
    run: Option<(GameId, f64, bool)>,
    events: Vec<PipelineEvent>,
    /// Seconds an unlock takes in total.
    pub unlock_seconds: f64,
    /// Seconds from the start until the game's window shows.
    pub window_seconds: f64,
    /// Make every unlock fail with this message.
    pub fail_with: Option<String>,
    /// The releases the simulation has beside its own (a game can stay on one of them).
    pub installed: Vec<String>,
    /// A glibc shortfall to show (the gallery has no Linux toolkit to read).
    pub glibc: Option<anyps5_core::glibc::Shortfall>,
}

impl SimPipeline {
    pub fn new(version: &str) -> Self {
        Self {
            version: version.to_owned(),
            queue: Vec::new(),
            current: None,
            run: None,
            events: Vec::new(),
            unlock_seconds: 5.0,
            window_seconds: 2.5,
            fail_with: None,
            installed: Vec::new(),
            glibc: None,
        }
    }
}

impl Pipeline for SimPipeline {
    fn toolkit_version(&self) -> Option<String> {
        Some(self.version.clone())
    }

    fn glibc_shortfall(&self) -> Option<anyps5_core::glibc::Shortfall> {
        self.glibc
    }

    fn pinned_toolkit(&mut self, pin: &str, _global: Option<&str>) -> GameToolkit {
        let tag = pin.trim_start_matches('v').to_owned();
        if self.installed.contains(&tag) {
            GameToolkit::Pinned {
                version: format!("{tag}-sim"),
                tag,
            }
        } else {
            GameToolkit::Missing { tag }
        }
    }

    fn submit(&mut self, command: PipelineCommand, _cx: &PipelineContext<'_>) {
        match command {
            PipelineCommand::Unlock(g)
            | PipelineCommand::Retry(g)
            | PipelineCommand::UnlockAgain(g)
            | PipelineCommand::Prepare(g) => {
                self.queue.push(g);
            }
            PipelineCommand::Cancel(g) => {
                if self.current.as_ref().is_some_and(|(c, _)| *c == g) {
                    self.current = None;
                }
                self.queue.retain(|q| *q != g);
                self.events.push(PipelineEvent::UnlockCancelled { game: g });
            }
            PipelineCommand::Play(g) => {
                self.events.push(PipelineEvent::Spawned { game: g.clone() });
                self.run = Some((g, f64::NAN, false));
            }
            PipelineCommand::Stop(g) => {
                self.run = None;
                self.events.push(PipelineEvent::Exited {
                    game: g,
                    diagnosis: None,
                });
            }
            PipelineCommand::Activate(_) | PipelineCommand::Promote(_) => {}
        }
    }

    fn poll(&mut self, now: f64) -> Vec<PipelineEvent> {
        if self.current.is_none() && !self.queue.is_empty() {
            let game = self.queue.remove(0);
            self.events.push(PipelineEvent::Progress {
                game: game.clone(),
                step: UnlockStep::Unwrap,
                fraction: 0.0,
            });
            self.current = Some((game, now));
        }
        if let Some((game, since)) = self.current.clone() {
            let t = ((now - since) / self.unlock_seconds).clamp(0.0, 1.0) as f32;
            if t >= 1.0 {
                self.current = None;
                let result = match &self.fail_with {
                    Some(message) => Err(message.clone()),
                    None => Ok(self.version.clone()),
                };
                self.events
                    .push(PipelineEvent::UnlockFinished { game, result });
            } else {
                // Map the overall time to a step and the fraction inside it.
                let step = UnlockStep::ALL
                    .into_iter()
                    .rev()
                    .find(|s| t >= s.start())
                    .unwrap_or(UnlockStep::Unwrap);
                let fraction = ((t - step.start()) / step.length()).clamp(0.0, 1.0);
                self.events.push(PipelineEvent::Progress {
                    game,
                    step,
                    fraction,
                });
            }
        }
        if let Some((game, since, shown)) = self.run.as_mut() {
            if since.is_nan() {
                *since = now;
            }
            if !*shown && now - *since >= self.window_seconds {
                *shown = true;
                self.events
                    .push(PipelineEvent::WindowShown { game: game.clone() });
            }
        }
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_cover_the_whole_bar() {
        let mut at = 0.0;
        for step in UnlockStep::ALL {
            assert!((step.start() - at).abs() < 1e-6, "{step:?}");
            at += step.length();
        }
        assert!((at - 1.0).abs() < 1e-6);
        assert!((UnlockStep::Relink.overall(0.5) - 0.37).abs() < 1e-6);
    }

    #[test]
    fn sim_walks_through_the_steps_and_finishes() {
        let prefs = Preferences::default();
        let cx = PipelineContext {
            root: None,
            game: None,
            prefs: &prefs,
            extra_env: None,
        };
        let id = GameId::from_key("c:/games/a");
        let mut sim = SimPipeline::new("v1");
        sim.unlock_seconds = 1.0;
        sim.submit(PipelineCommand::Unlock(id.clone()), &cx);
        let mut seen = Vec::new();
        for i in 0..=20 {
            seen.extend(sim.poll(f64::from(i) * 0.1));
        }
        assert!(matches!(
            seen.first(),
            Some(PipelineEvent::Progress {
                step: UnlockStep::Unwrap,
                ..
            })
        ));
        assert!(matches!(
            seen.last(),
            Some(PipelineEvent::UnlockFinished { result: Ok(v), .. }) if v == "v1"
        ));
        assert!(seen.iter().any(|e| matches!(
            e,
            PipelineEvent::Progress {
                step: UnlockStep::Copy,
                ..
            }
        )));
    }
}
