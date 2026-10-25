//! The unlocked games location as the model sees it (docs/DESIGN.md 5.7): changing it, moving the
//! copies in the background, and what the settings row shows. The disk work is in
//! `anyps5_core::location` and `anyps5_core::relocate`; every call that touches the disk runs on a
//! worker (`app::services`), never on the UI thread.

use std::path::PathBuf;

use anyps5_core::events::{CancelToken, CoreEvent};
use anyps5_core::location::{self, Refusal};
use anyps5_core::paths;
use anyps5_core::relocate::{self, Left};

use crate::app::model::{Effect, Model};
use crate::screens::location_strings as words;
use crate::screens::settings::{LocationEnv, MoveEnv};
use crate::ui::icons::Icon;

/// How long a look at the location stays fresh while the settings are open.
const FRESH_SECONDS: f64 = 5.0;

/// A move that runs.
pub struct MoveJob {
    pub cancel: CancelToken,
    pub progress: Option<relocate::Progress>,
}

/// What the model knows about the location.
#[derive(Default)]
pub struct LocationState {
    /// The last look, from a worker.
    pub report: Option<location::Report>,
    asked_at: Option<f64>,
    /// The move that runs now.
    pub job: Option<MoveJob>,
    /// How many copies the last move left behind.
    pub left: usize,
    /// A move that was cut off by a restart was taken up again.
    resumed: bool,
}

/// Why the location cannot change now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Busy {
    Playing,
    Unlocking,
    Moving,
}

impl Model {
    /// The folder that holds the `.AnyPS5` store.
    pub fn store_root(&self) -> Option<PathBuf> {
        self.prefs.store_root()
    }

    pub fn location_busy(&self) -> Option<Busy> {
        if self.location.job.is_some() {
            Some(Busy::Moving)
        } else if self.running.is_some() || self.starting.is_some() {
            Some(Busy::Playing)
        } else if self.busy() {
            Some(Busy::Unlocking)
        } else {
            None
        }
    }

    /// Copies are being moved, or some still wait in the old place: unlocking and playing wait.
    pub fn moving_games(&self) -> bool {
        self.prefs.unlocked_moving_from.is_some()
    }

    /// Looks at the location again when the last look is old (the settings call this while the
    /// Library page is open).
    pub fn ask_location(&mut self) {
        let fresh = self
            .location
            .asked_at
            .is_some_and(|at| self.now - at < FRESH_SECONDS);
        if !fresh && self.prefs.games_folder.is_some() {
            self.location.asked_at = Some(self.now);
            self.effects.push(Effect::CheckLocation);
        }
    }

    fn ask_location_now(&mut self) {
        self.location.asked_at = None;
        self.ask_location();
    }

    /// The player picked a folder in the dialog.
    pub fn choose_unlocked_folder(&mut self, folder: PathBuf) {
        let language = self.language();
        if let Some(busy) = self.location_busy() {
            self.refuse_busy(busy);
            return;
        }
        let Some(games) = self.prefs.games_folder.clone() else {
            return;
        };
        // The check writes the store's first folder and one probe file, so a drive that is
        // slow to answer is a short wait, not a hang: it runs here only after the dialog.
        match location::adopt(&folder, &games) {
            Ok(kept) => self.change_location(kept),
            Err(why) => self.refuse(&why, language),
        }
    }

    /// The games folder changed: a location that is now the games folder itself, or inside it,
    /// goes back to the default (the dumps' folder is never written to). The copies stay where they
    /// were; nothing is moved or deleted.
    pub fn games_folder_changed(&mut self) {
        let (Some(unlocked), Some(games)) = (
            self.prefs.unlocked_folder.clone(),
            self.prefs.games_folder.clone(),
        ) else {
            return;
        };
        if location::check(&unlocked, &games) == Err(Refusal::InsideGamesFolder) {
            let text = words::refusal(&Refusal::InsideGamesFolder, self.language());
            self.toast(text, Icon::WARNING);
        } else if !location::is_games_folder(&unlocked, &games) {
            return;
        }
        self.edit_prefs(|p| {
            p.unlocked_folder = None;
            p.unlocked_moving_from = None;
        });
    }

    /// "Use Default Location".
    pub fn use_default_location(&mut self) {
        if let Some(busy) = self.location_busy() {
            self.refuse_busy(busy);
            return;
        }
        self.change_location(None);
    }

    fn refuse(&mut self, why: &Refusal, language: crate::ui::i18n::Language) {
        self.toast(words::refusal(why, language), Icon::WARNING);
    }

    fn refuse_busy(&mut self, busy: Busy) {
        let language = self.language();
        let text = match busy {
            Busy::Playing => words::busy_playing(language),
            Busy::Unlocking => words::busy_unlocking(language),
            Busy::Moving => words::busy_moving(language),
        };
        self.toast(text, Icon::WARNING);
    }

    /// Stores the new location and moves the copies there. `None`: the games folder again.
    fn change_location(&mut self, new_folder: Option<PathBuf>) {
        let Some(old_root) = self.store_root() else {
            return;
        };
        if self.prefs.unlocked_folder == new_folder {
            return;
        }
        self.edit_prefs(|p| p.unlocked_folder = new_folder);
        let Some(new_root) = self.store_root() else {
            return;
        };
        if paths::GameId::from_path(&old_root) == paths::GameId::from_path(&new_root) {
            return;
        }
        self.edit_prefs(|p| p.unlocked_moving_from = Some(old_root));
        self.start_move();
        self.rescan();
        self.ask_location_now();
    }

    /// Starts moving what the old place still holds; the worker decides what that is.
    fn start_move(&mut self) {
        let (Some(from), Some(to)) = (
            self.prefs.unlocked_moving_from.clone(),
            self.store_root(),
        ) else {
            return;
        };
        if self.location.job.is_some() {
            return;
        }
        let running: Vec<String> = [&self.running, &self.starting]
            .into_iter()
            .flatten()
            .filter_map(|id| self.game(id))
            .map(|game| paths::unlocked_copy_name(&game.title_id, &game.id))
            .collect();
        let cancel = CancelToken::new();
        self.location.job = Some(MoveJob {
            cancel: cancel.clone(),
            progress: None,
        });
        self.effects.push(Effect::StartMove {
            from,
            to,
            running,
            cancel,
        });
    }

    /// "Continue Moving".
    pub fn continue_move(&mut self) {
        self.start_move();
    }

    /// After a restart that cut a move off: take it up again once, when the library is loaded.
    pub(crate) fn resume_move_tick(&mut self) {
        if !self.location.resumed
            && self.library_loaded
            && self.location.job.is_none()
            && self.prefs.unlocked_moving_from.is_some()
        {
            self.location.resumed = true;
            self.start_move();
        }
    }

    /// "Cancel Move": stops after the file it is on; the copy it was on stays whole in the old
    /// place, the copies already moved stay in the new one.
    pub fn cancel_move(&mut self) {
        if let Some(job) = &self.location.job {
            job.cancel.cancel();
        }
    }

    /// "Leave Games Where They Are": the copies still in the old place are no longer waited for.
    /// They stay on that drive, unused.
    pub fn leave_old_copies(&mut self) {
        if self.location.job.is_some() {
            return;
        }
        self.edit_prefs(|p| p.unlocked_moving_from = None);
        self.location.left = 0;
        self.rescan();
    }

    /// What the settings row shows.
    pub fn location_env(&self) -> LocationEnv {
        let moving = self.location.job.as_ref().map(|job| {
            job.progress.as_ref().map_or(
                MoveEnv {
                    done: 0,
                    total: 0,
                    permille: 0,
                },
                |p| MoveEnv {
                    done: p.done,
                    total: p.total,
                    permille: (p.done_bytes.saturating_mul(1000) / p.total_bytes.max(1)).min(1000)
                        as u16,
                },
            )
        });
        let left = match (&moving, &self.prefs.unlocked_moving_from) {
            (None, Some(from)) => Some((from.clone(), self.location.left)),
            _ => None,
        };
        LocationEnv {
            report: self.location.report.clone(),
            moving,
            left,
        }
    }

    /// The answers of the workers.
    pub fn on_location_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::LocationChecked(report) => self.location.report = Some(report),
            CoreEvent::MoveProgress(progress) => {
                if let Some(job) = &mut self.location.job {
                    job.progress = Some(progress);
                }
            }
            CoreEvent::MoveFinished(report) => {
                self.location.job = None;
                self.location.left = report.left.len();
                for name in &report.duplicates {
                    tracing::warn!("the old copy {name} could not be removed after the move");
                }
                let language = self.language();
                if report.complete() {
                    self.edit_prefs(|p| p.unlocked_moving_from = None);
                    if !report.moved.is_empty() {
                        self.toast(words::moved(language), Icon::CHECK_CIRCLE);
                    }
                } else if let Some((needed, free)) =
                    report.left.iter().find_map(|(_, why)| match why {
                        Left::NoSpace { needed, free } => Some((*needed, *free)),
                        _ => None,
                    })
                {
                    self.toast(words::no_space(needed, free, language), Icon::WARNING);
                } else {
                    self.toast(words::stopped(report.left.len(), language), Icon::WARNING);
                }
                for (name, why) in &report.left {
                    tracing::info!("{name} stays in the old place: {why:?}");
                }
                self.rescan();
                self.ask_location_now();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::paths::{AppPaths, GameId};
    use anyps5_core::settings::Preferences;
    use std::fs;

    struct Rig {
        _dir: tempfile::TempDir,
        games: PathBuf,
        fast: PathBuf,
        model: Model,
    }

    fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let (games, fast) = (dir.path().join("games"), dir.path().join("fast"));
        fs::create_dir_all(games.join("Stray")).unwrap();
        fs::create_dir_all(&fast).unwrap();
        let prefs = Preferences {
            onboarded: true,
            games_folder: Some(games.clone()),
            ..Preferences::default()
        };
        let model = Model::new(prefs, AppPaths::under(dir.path()), false);
        Rig {
            _dir: dir,
            games,
            fast,
            model,
        }
    }

    fn started(model: &Model) -> Option<(PathBuf, PathBuf)> {
        model.effects.iter().find_map(|e| match e {
            Effect::StartMove { from, to, .. } => Some((from.clone(), to.clone())),
            _ => None,
        })
    }

    #[test]
    fn a_new_folder_is_stored_and_the_copies_start_to_move() {
        let mut r = rig();
        r.model.choose_unlocked_folder(r.fast.clone());
        assert_eq!(r.model.prefs.unlocked_folder.as_deref(), Some(r.fast.as_path()));
        assert_eq!(r.model.store_root().as_deref(), Some(r.fast.as_path()));
        assert_eq!(
            r.model.prefs.unlocked_moving_from.as_deref(),
            Some(r.games.as_path())
        );
        assert_eq!(started(&r.model), Some((r.games.clone(), r.fast.clone())));
        assert!(r.model.location.job.is_some());
        assert!(r.fast.join(".AnyPS5").is_dir());
        assert!(!r.games.join(".AnyPS5").exists(), "nothing written next to the dumps");
    }

    #[test]
    fn a_folder_inside_the_games_folder_is_refused_and_nothing_changes() {
        let mut r = rig();
        r.model.choose_unlocked_folder(r.games.join("Stray"));
        assert_eq!(r.model.prefs.unlocked_folder, None);
        assert_eq!(r.model.prefs.unlocked_moving_from, None);
        assert!(started(&r.model).is_none());
        assert!(r.model.toast_visible() || r.model.toast.is_some());
        assert!(fs::read_dir(r.games.join("Stray")).unwrap().next().is_none());
    }

    #[test]
    fn a_running_game_or_a_running_move_blocks_the_change() {
        let mut r = rig();
        r.model.running = Some(GameId::from_key("c:/x"));
        r.model.choose_unlocked_folder(r.fast.clone());
        assert_eq!(r.model.prefs.unlocked_folder, None);
        r.model.running = None;
        r.model.choose_unlocked_folder(r.fast.clone());
        assert!(r.model.location.job.is_some());
        r.model.use_default_location();
        assert!(
            r.model.prefs.unlocked_folder.is_some(),
            "no change while the move runs"
        );
    }

    #[test]
    fn a_finished_move_forgets_the_old_place_and_a_stopped_one_keeps_it() {
        use anyps5_core::relocate::{Left, Report};
        let mut r = rig();
        r.model.choose_unlocked_folder(r.fast.clone());
        let stopped = Report {
            moved: vec!["A-1".into()],
            left: vec![("B-2".into(), Left::Cancelled)],
            cancelled: true,
            ..Report::default()
        };
        r.model.on_location_event(CoreEvent::MoveFinished(stopped));
        assert!(r.model.location.job.is_none());
        assert!(r.model.moving_games(), "copies still wait in the old place");
        let env = r.model.location_env();
        assert_eq!(env.left, Some((r.games.clone(), 1)));

        r.model.continue_move();
        assert!(r.model.location.job.is_some());
        r.model
            .on_location_event(CoreEvent::MoveFinished(Report::default()));
        assert!(!r.model.moving_games());
        assert_eq!(r.model.location_env().left, None);
    }

    #[test]
    fn leaving_the_copies_stops_the_waiting() {
        use anyps5_core::relocate::{Left, Report};
        let mut r = rig();
        r.model.choose_unlocked_folder(r.fast.clone());
        r.model.on_location_event(CoreEvent::MoveFinished(Report {
            left: vec![("B-2".into(), Left::Cancelled)],
            cancelled: true,
            ..Report::default()
        }));
        r.model.leave_old_copies();
        assert!(!r.model.moving_games());
        assert_eq!(r.model.prefs.unlocked_folder.as_deref(), Some(r.fast.as_path()));
    }

    #[test]
    fn a_games_folder_that_contains_the_location_returns_it_to_the_default() {
        let mut r = rig();
        r.model.choose_unlocked_folder(r.fast.clone());
        r.model.prefs.unlocked_moving_from = None;
        // The player now picks the folder that holds both as the games folder's child: use the
        // location's own parent as the new games folder, which contains `fast`.
        let parent = r.fast.parent().unwrap().to_path_buf();
        r.model.prefs.games_folder = Some(parent);
        r.model.games_folder_changed();
        assert_eq!(r.model.prefs.unlocked_folder, None);
    }
}
