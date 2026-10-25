//! Preparing games like a console installs them (docs/DESIGN.md 5.4): a game that appears in the
//! games folder is unlocked in the background, one at a time, while the PC is idle; the tile says
//! so quietly, and Play on it moves it to the front. The rest of the unlock progress lives here
//! too: the weights of the five steps on the one bar and the time left, estimated from measured
//! progress.
//!
//! The idle gate itself (no game, mains power, memory, load) is the pipeline's; the model decides
//! which game is next and never asks for more than one at a time.

use std::collections::{HashMap, HashSet, VecDeque};

use anyps5_core::paths::GameId;

use crate::app::model::{Effect, GameState, Model};
use crate::app::pipeline::{PipelineCommand, StepWeights, UnlockStep};

/// Seconds between two preparations, so the next one does not start the moment a toast shows.
const BETWEEN_GAMES: f64 = 6.0;
/// The window the speed of an unlock is read from.
const WINDOW: f64 = 40.0;
/// No time left is shown before this much work and this many seconds are behind it.
const MIN_OVERALL: f32 = 0.04;
const MIN_SECONDS: f64 = 8.0;

/// What the model keeps about preparing and about the time left.
#[derive(Debug, Default)]
pub struct Prep {
    /// Games the launcher unlocks on its own (queued, running or standing still).
    pub background: HashSet<GameId>,
    /// ... of which these stand still at the moment.
    pub paused: HashSet<GameId>,
    /// Games already tried with this toolkit in this session; a failed one is not tried again.
    tried: HashSet<(GameId, String)>,
    etas: HashMap<GameId, Eta>,
    /// What the steps weigh on this PC.
    pub weights: StepWeights,
    seconds: Option<[f32; 5]>,
    /// UI clock time before which no new preparation starts.
    after: f64,
}

/// The time left of one unlock: measured speed, read over a window and over the whole run, and
/// smoothed so the number does not jump. Time spent standing still does not count.
#[derive(Clone, Debug, Default)]
pub struct Eta {
    paused_total: f64,
    paused_since: Option<f64>,
    first: Option<(f64, f32)>,
    window: VecDeque<(f64, f32)>,
    /// (active time of the estimate, seconds left then)
    shown: Option<(f64, f32)>,
}

impl Eta {
    /// The clock of the unlock without the time it stood still.
    fn active(&self, now: f64) -> f64 {
        now - self.paused_total - self.paused_since.map_or(0.0, |since| now - since)
    }

    pub fn set_paused(&mut self, now: f64, paused: bool) {
        match (paused, self.paused_since) {
            (true, None) => self.paused_since = Some(now),
            (false, Some(since)) => {
                self.paused_total += now - since;
                self.paused_since = None;
            }
            _ => {}
        }
    }

    /// The bar stands at `overall` (0..=1) at UI time `now`.
    pub fn sample(&mut self, now: f64, overall: f32) {
        let t = self.active(now);
        let first = *self.first.get_or_insert((t, overall));
        self.window.push_back((t, overall));
        while self
            .window
            .front()
            .is_some_and(|&(then, _)| t - then > WINDOW)
        {
            self.window.pop_front();
        }
        if overall < MIN_OVERALL || t - first.0 < MIN_SECONDS {
            return;
        }
        let average = f64::from(overall - first.1) / (t - first.0);
        let recent = self.window.front().and_then(|&(then, at)| {
            (t - then >= 6.0 && overall > at).then(|| f64::from(overall - at) / (t - then))
        });
        let rate = match recent {
            Some(recent) => 0.5 * (recent + average),
            None => average,
        };
        if rate <= 1e-5 {
            return;
        }
        let left = (f64::from(1.0 - overall) / rate) as f32;
        let smoothed = match self.shown {
            Some((then, value)) => {
                let expected = (value - (t - then) as f32).max(0.0);
                let alpha = (1.0 - (-(t - then) / 8.0).exp()) as f32;
                expected + (left - expected) * alpha
            }
            None => left,
        };
        self.shown = Some((t, smoothed.max(0.0)));
    }

    /// Seconds left at UI time `now`, once there is enough to tell.
    pub fn left(&self, now: f64) -> Option<f32> {
        let (then, value) = self.shown?;
        Some((value - (self.active(now) - then) as f32).max(0.0))
    }
}

impl Model {
    /// Where the bar stands for a step and the fraction inside it, weighed by what the steps
    /// cost on this PC.
    pub fn overall(&self, step: UnlockStep, fraction: f32) -> f32 {
        self.prep.weights.overall(step, fraction)
    }

    /// The pipeline measured how long the steps took: the next unlock weighs its bar by them.
    pub fn set_step_seconds(&mut self, seconds: Option<[f32; 5]>) {
        if self.prep.seconds == seconds {
            return;
        }
        self.prep.seconds = seconds;
        self.prep.weights = seconds
            .and_then(StepWeights::from_seconds)
            .unwrap_or_default();
    }

    /// Scenes only: a time left that needs no measuring.
    pub fn preview_eta(&mut self, id: &GameId, left: f32) {
        let eta = self.prep.etas.entry(id.clone()).or_default();
        eta.shown = Some((self.now, left));
    }

    /// Seconds an unlock still needs, when it can be told yet.
    pub fn time_left(&self, id: &GameId) -> Option<f32> {
        self.prep.etas.get(id)?.left(self.now)
    }

    /// The launcher unlocks this game on its own account (the player did not ask yet).
    pub fn is_preparing(&self, id: &GameId) -> bool {
        self.prep.background.contains(id)
    }

    /// The tile and the hero of a game being prepared.
    pub fn preparing_state(&self, id: &GameId) -> Option<GameState> {
        self.is_preparing(id).then(|| GameState::Preparing {
            progress: self.progress.get(id).copied(),
            paused: self.prep.paused.contains(id),
        })
    }

    /// The player wants a game that is being prepared: it goes first and shows its progress.
    pub fn promote(&mut self, id: &GameId) {
        if self.prep.background.remove(id) {
            self.prep.paused.remove(id);
            self.effects
                .push(Effect::Pipeline(PipelineCommand::Promote(id.clone())));
            self.sound(crate::ui::sound::Cue::Select);
        }
    }

    /// A scan found the library. Games that were not there before are prepared (those already
    /// there the first time the launcher looks are not: that could be a whole shelf).
    pub(crate) fn note_new_games(&mut self) {
        let present: std::collections::BTreeSet<String> = self
            .games
            .iter()
            .map(|g| g.id.as_str().to_owned())
            .collect();
        let Some(known) = self.prefs.known_games.clone() else {
            self.edit_prefs(|p| p.known_games = Some(present));
            return;
        };
        if present.is_subset(&known) {
            return;
        }
        let wanted: Vec<String> = self
            .games
            .iter()
            .filter(|g| !known.contains(g.id.as_str()))
            .filter(|g| matches!(self.state_of(g), GameState::Locked))
            .map(|g| g.id.as_str().to_owned())
            .collect();
        let prepare = self.prefs.prepare_games;
        self.edit_prefs(|p| {
            if let Some(known) = p.known_games.as_mut() {
                known.extend(present);
            }
            if prepare {
                p.prepare_pending.extend(wanted);
            }
        });
    }

    /// Forgets which games the launcher met (the games folder changed).
    pub(crate) fn forget_known_games(&mut self) {
        self.edit_prefs(|p| {
            p.known_games = None;
            p.prepare_pending.clear();
        });
    }

    /// Starts the next preparation when nothing else unlocks and nobody plays.
    pub(crate) fn prepare_tick(&mut self) {
        let free = self.prefs.prepare_games
            && self.library_loaded
            && self.prefs.games_folder.is_some()
            && self.toolkit.is_some()
            && self.starting.is_none()
            && self.running.is_none()
            && self.queue.is_empty()
            && self.progress.is_empty()
            && self.prep.background.is_empty()
            && self.now >= self.prep.after;
        if !free {
            return;
        }
        let Some(next) = self.next_to_prepare() else {
            // Nothing wants preparing: look again in a moment, not every frame.
            self.prep.after = self.now + 1.5;
            return;
        };
        let version = self
            .toolkit_of(&next)
            .version()
            .unwrap_or_default()
            .to_owned();
        self.prep.tried.insert((next.clone(), version));
        self.prep.background.insert(next.clone());
        self.effects
            .push(Effect::Pipeline(PipelineCommand::Prepare(next)));
    }

    /// The first game that wants preparing: the new ones in the order of the library, then the
    /// ones a newer toolkit left behind.
    fn next_to_prepare(&self) -> Option<GameId> {
        let fit = |g: &&anyps5_core::library::Game| {
            let toolkit = self.toolkit_of(&g.id);
            !toolkit.is_missing()
                && toolkit.version().is_some_and(|version| {
                    !self
                        .prep
                        .tried
                        .contains(&(g.id.clone(), version.to_owned()))
                })
                && !self.failures.contains_key(&g.id)
                && !self.is_active(&g.id)
        };
        let pending = self
            .games
            .iter()
            .filter(|g| self.prefs.prepare_pending.contains(g.id.as_str()))
            .filter(|g| matches!(self.state_of(g), GameState::Locked))
            .find(fit);
        let outdated = || {
            self.games
                .iter()
                .filter(|g| matches!(self.state_of(g), GameState::Installed { outdated: true }))
                .find(fit)
        };
        pending.or_else(outdated).map(|g| g.id.clone())
    }

    /// A scan after an unlock: a game may have appeared while the folder watcher stood quiet.
    pub(crate) fn rescan_after_unlock(&mut self) {
        if self.prefs.prepare_games && !self.busy() {
            self.rescan();
        }
    }

    /// Progress of an unlock: feeds the time left.
    pub(crate) fn prep_progress(&mut self, id: &GameId, step: UnlockStep, fraction: f32) {
        let overall = self.overall(step, fraction);
        let now = self.now;
        self.prep
            .etas
            .entry(id.clone())
            .or_default()
            .sample(now, overall);
    }

    pub(crate) fn prep_paused(&mut self, id: &GameId, paused: bool) {
        let now = self.now;
        if paused {
            self.prep.paused.insert(id.clone());
        } else {
            self.prep.paused.remove(id);
        }
        if let Some(eta) = self.prep.etas.get_mut(id) {
            eta.set_paused(now, paused);
        }
    }

    /// An unlock ended (done, failed or cancelled). Returns whether the launcher had been
    /// preparing the game on its own.
    pub(crate) fn prep_ended(&mut self, id: &GameId) -> bool {
        self.prep.etas.remove(id);
        self.prep.paused.remove(id);
        let was = self.prep.background.remove(id);
        let key = id.as_str().to_owned();
        if self.prefs.prepare_pending.contains(&key) {
            self.edit_prefs(|p| {
                p.prepare_pending.remove(&key);
            });
        }
        self.prep.after = self.now + BETWEEN_GAMES;
        was
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_left_follows_a_steady_unlock() {
        let mut eta = Eta::default();
        // 1 % per second.
        for second in 0..=30 {
            eta.sample(f64::from(second), second as f32 / 100.0);
        }
        let left = eta.left(30.0).unwrap();
        assert!((left - 70.0).abs() < 6.0, "{left}");
        // It counts down between two events.
        assert!(eta.left(40.0).unwrap() < left - 9.0);
    }

    #[test]
    fn nothing_is_said_before_there_is_enough_to_tell() {
        let mut eta = Eta::default();
        eta.sample(0.0, 0.0);
        eta.sample(3.0, 0.02);
        assert_eq!(eta.left(3.0), None, "too early");
        eta.sample(9.0, 0.03);
        assert_eq!(eta.left(9.0), None, "too little done");
    }

    #[test]
    fn a_stall_says_nothing_and_a_pause_does_not_count() {
        let mut eta = Eta::default();
        for second in 0..=20 {
            eta.sample(f64::from(second), second as f32 / 100.0);
        }
        let before = eta.left(20.0).unwrap();
        // Paused for five minutes: the estimate does not shrink and then jump.
        eta.set_paused(20.0, true);
        eta.set_paused(320.0, false);
        let after = eta.left(320.0).unwrap();
        assert!((before - after).abs() < 1.0, "{before} {after}");
    }

    #[test]
    fn the_weights_follow_what_the_steps_took() {
        let weights = StepWeights::from_seconds([2.0, 100.0, 40.0, 1.0, 0.0]).unwrap();
        let total: f32 = UnlockStep::ALL.iter().map(|s| weights.share(*s)).sum();
        assert!((total - 1.0).abs() < 1e-5);
        assert!(weights.share(UnlockStep::Relink) > 0.6);
        assert!(
            weights.share(UnlockStep::Finish) >= 0.015,
            "never invisible"
        );
        assert!((weights.overall(UnlockStep::Finish, 1.0) - 1.0).abs() < 1e-5);
        assert_eq!(StepWeights::from_seconds([0.0; 5]), None);
        let mut last = 0.0;
        for step in UnlockStep::ALL {
            let now = weights.overall(step, 0.0);
            assert!(now >= last);
            last = now;
        }
    }
}
