//! The I/O behind the model's effects: library scan and watcher, size measuring, trophy loading and
//! the native folder dialog. Everything runs on worker threads and answers through one channel of
//! [`CoreEvent`]s (plus one for the dialog), so the UI thread never waits on the disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use anyps5_core::events::{CancelToken, CoreEvent, Notifier};
use anyps5_core::fsx::LinkMode;
use anyps5_core::library::{self, Game, LibraryWatcher};
use anyps5_core::{location, relocate};
use anyps5_core::paths::{self, AppPaths, GameId};
use anyps5_core::saves::{AFTER_PLAYING_EVERY, Reason, SaveData};
use anyps5_core::settings::{ConsoleLanguage, Preferences};
use anyps5_core::trophies::{Trophies, TrophyWatcher};
use egui::Context;

/// The pause between two games of a trophy sync.
const TROPHY_SYNC_PAUSE: std::time::Duration = std::time::Duration::from_millis(250);

pub struct Services {
    tx: Sender<CoreEvent>,
    rx: Receiver<CoreEvent>,
    notify: Notifier,
    watcher: Option<LibraryWatcher>,
    watched: Option<PathBuf>,
    busy: Arc<AtomicBool>,
    folder: Option<Receiver<Option<PathBuf>>>,
    /// Polls the running game's trophy records (the banner and the live list).
    trophy_watch: Option<(GameId, TrophyWatcher)>,
}

impl Services {
    pub fn new(ctx: &Context) -> Self {
        let (tx, rx) = channel();
        let repaint = ctx.clone();
        Self {
            tx,
            rx,
            notify: Arc::new(move || repaint.request_repaint()),
            watcher: None,
            watched: None,
            busy: Arc::new(AtomicBool::new(false)),
            folder: None,
            trophy_watch: None,
        }
    }

    /// Events the workers produced since the last call.
    pub fn drain(&mut self) -> Vec<CoreEvent> {
        self.rx.try_iter().collect()
    }

    /// Tells the watcher whether an unlock is queued or running (it stays silent then).
    pub fn set_busy(&self, busy: bool) {
        self.busy.store(busy, Ordering::Relaxed);
    }

    /// Scans `root` on a worker thread and watches it.
    pub fn rescan(&mut self, root: &Path, store_root: &Path) {
        if self.watched.as_deref() != Some(root) {
            self.watcher = match LibraryWatcher::start(
                root.to_path_buf(),
                self.tx.clone(),
                self.notify.clone(),
                self.busy.clone(),
            ) {
                Ok(watcher) => Some(watcher),
                Err(error) => {
                    tracing::warn!("{error}");
                    None
                }
            };
            self.watched = Some(root.to_path_buf());
        }
        if let Some(watcher) = &self.watcher {
            watcher.mark_scanned();
        }
        let (tx, notify, root) = (self.tx.clone(), self.notify.clone(), root.to_path_buf());
        let store_root = store_root.to_path_buf();
        std::thread::Builder::new()
            .name("scan".into())
            .spawn(move || {
                let games = library::scan_with_store(&root, &store_root);
                let records: HashMap<GameId, String> = games
                    .iter()
                    .filter_map(|g| Some((g.id.clone(), g.unlocked_with.clone()?)))
                    .collect();
                let _ = tx.send(CoreEvent::LibraryScanned {
                    root,
                    games,
                    records,
                });
                notify();
            })
            .ok();
    }

    /// Looks at the unlocked games location on a worker (a share that is down can take seconds to
    /// answer, and the UI thread never waits on the disk).
    pub fn check_location(&self, games: &Path, unlocked: Option<&Path>) {
        let (tx, notify) = (self.tx.clone(), self.notify.clone());
        let (games, unlocked) = (games.to_path_buf(), unlocked.map(Path::to_path_buf));
        std::thread::Builder::new()
            .name("location".into())
            .spawn(move || {
                if let Some(report) = location::report(Some(&games), unlocked.as_deref()) {
                    let _ = tx.send(CoreEvent::LocationChecked(report));
                    notify();
                }
            })
            .ok();
    }

    /// Moves the unlocked copies of the store `from` into the store `to`, in the background.
    /// `running` names the copies a game runs from; they stay. `token` cancels.
    pub fn start_move(&self, from: PathBuf, to: PathBuf, running: Vec<String>, token: CancelToken) {
        let (tx, notify) = (self.tx.clone(), self.notify.clone());
        std::thread::Builder::new()
            .name("move".into())
            .spawn(move || {
                let progress_tx = tx.clone();
                let progress_notify = notify.clone();
                // The player is watching a bar: do not flood the channel with every file.
                let mut last = std::time::Instant::now() - std::time::Duration::from_secs(1);
                let report = relocate::move_all(
                    &from,
                    &to,
                    LinkMode::Auto,
                    &|name| running.iter().any(|r| r == name),
                    &token,
                    &mut |progress| {
                        if last.elapsed() >= std::time::Duration::from_millis(100) {
                            last = std::time::Instant::now();
                            let _ = progress_tx.send(CoreEvent::MoveProgress(progress));
                            progress_notify();
                        }
                    },
                );
                let _ = tx.send(CoreEvent::MoveFinished(report));
                notify();
            })
            .ok();
    }

    /// Measures the size on disk of a game (a game folder can hold tens of gigabytes in many files).
    pub fn measure(&self, game: &Game) {
        let (tx, notify) = (self.tx.clone(), self.notify.clone());
        let (id, folder) = (game.id.clone(), game.folder.clone());
        std::thread::Builder::new()
            .name("size".into())
            .spawn(move || {
                let bytes = library::folder_size(&folder, &CancelToken::new());
                let _ = tx.send(CoreEvent::SizeMeasured { game: id, bytes });
                notify();
            })
            .ok();
    }

    /// Backs up a game's saves after it ended, unless the last backup is under an hour old. Quiet:
    /// a failure is only logged, and nothing comes back.
    pub fn back_up_saves(&self, game: &Game, root: &Path, paths: &AppPaths) {
        let copy = paths::unlocked_copy_dir(root, &game.title_id, &game.id);
        let saves = SaveData::for_title(paths, &game.title_id, copy);
        std::thread::Builder::new()
            .name("saves".into())
            .spawn(move || {
                match saves.backup_unless_recent(Reason::AfterPlaying, AFTER_PLAYING_EVERY) {
                    Ok(_) => {}
                    Err(error) => tracing::warn!("cannot back up the saves: {error}"),
                }
            })
            .ok();
    }

    /// Reads the trophies of a game in the language it runs in.
    pub fn load_trophies(&self, game: &Game, root: Option<&Path>, prefs: &Preferences) {
        let (tx, notify) = (self.tx.clone(), self.notify.clone());
        let game = game.clone();
        let number = prefs
            .game_settings(&game.id)
            .language
            .unwrap_or(match prefs.language {
                anyps5_core::settings::Language::Turkish => 19,
                anyps5_core::settings::Language::English => 1,
            });
        let languages: Vec<String> = ConsoleLanguage::tag(number)
            .map(|tag| vec![tag.to_owned()])
            .unwrap_or_default();
        let copy = root.map(|root| paths::unlocked_copy_dir(root, &game.title_id, &game.id));
        std::thread::Builder::new()
            .name("trophies".into())
            .spawn(move || {
                let copy = copy.filter(|c| c.is_dir());
                let set = Trophies::load(&game, copy.as_deref(), &languages);
                let _ = tx.send(CoreEvent::Trophies {
                    game: game.id.clone(),
                    set,
                });
                notify();
            })
            .ok();
    }

    /// Reads what the player earned in each unlocked game, one game after the other, so the
    /// network learns of trophies earned before the player signed in. The pause between games
    /// keeps the uploads far below the cloud's limit per account.
    pub fn sync_trophies(&self, games: &[Game], root: &Path) {
        let (tx, notify) = (self.tx.clone(), self.notify.clone());
        let copies: Vec<(Game, PathBuf)> = games
            .iter()
            .map(|g| {
                (
                    g.clone(),
                    paths::unlocked_copy_dir(root, &g.title_id, &g.id),
                )
            })
            .collect();
        std::thread::Builder::new()
            .name("trophy sync".into())
            .spawn(move || {
                for (game, copy) in copies {
                    if !copy.is_dir() {
                        continue;
                    }
                    let earned = Trophies::earned_records(&game, &copy);
                    if earned.is_empty() {
                        continue;
                    }
                    let event = CoreEvent::TrophyRecords {
                        title_id: game.title_id,
                        earned,
                    };
                    if tx.send(event).is_err() {
                        return;
                    }
                    notify();
                    std::thread::sleep(TROPHY_SYNC_PAUSE);
                }
            })
            .ok();
    }

    /// Starts watching the trophy records of a running game (one game at a time).
    pub fn watch_trophies(&mut self, game: &Game, root: Option<&Path>, prefs: &Preferences) {
        if self
            .trophy_watch
            .as_ref()
            .is_some_and(|(id, _)| *id == game.id)
        {
            return;
        }
        self.stop_trophy_watch();
        let Some(root) = root else { return };
        let copy = paths::unlocked_copy_dir(root, &game.title_id, &game.id);
        let number = prefs
            .game_settings(&game.id)
            .language
            .unwrap_or(match prefs.language {
                anyps5_core::settings::Language::Turkish => 19,
                anyps5_core::settings::Language::English => 1,
            });
        let languages: Vec<String> = ConsoleLanguage::tag(number)
            .map(|tag| vec![tag.to_owned()])
            .unwrap_or_default();
        let watcher = TrophyWatcher::start(
            game.clone(),
            copy,
            languages,
            self.tx.clone(),
            self.notify.clone(),
        );
        self.trophy_watch = Some((game.id.clone(), watcher));
    }

    /// Stops the trophy watch; the thread is joined off the UI thread.
    pub fn stop_trophy_watch(&mut self) {
        if let Some((_, watcher)) = self.trophy_watch.take() {
            std::thread::spawn(move || drop(watcher));
        }
    }

    /// Opens the native folder dialog on its own thread. False when one is already open.
    pub fn pick_folder(&mut self, title: &str, start: Option<&Path>, ctx: &Context) -> bool {
        if self.folder.is_some() {
            return false;
        }
        let (tx, rx) = channel();
        self.folder = Some(rx);
        let (title, start, ctx) = (title.to_owned(), start.map(Path::to_path_buf), ctx.clone());
        std::thread::Builder::new()
            .name("folder dialog".into())
            .spawn(move || {
                let mut dialog = rfd::FileDialog::new().set_title(&title);
                if let Some(start) = start.filter(|p| p.is_dir()) {
                    dialog = dialog.set_directory(start);
                }
                let _ = tx.send(dialog.pick_folder());
                ctx.request_repaint();
            })
            .ok();
        true
    }

    /// A native dialog is open: the launcher ignores input meanwhile.
    pub fn dialog_open(&self) -> bool {
        self.folder.is_some()
    }

    /// `Some(answer)` once the dialog closed (`None` inside: cancelled).
    pub fn poll_folder(&mut self) -> Option<Option<PathBuf>> {
        let rx = self.folder.as_ref()?;
        match rx.try_recv() {
            Ok(answer) => {
                self.folder = None;
                Some(answer)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.folder = None;
                Some(None)
            }
        }
    }
}
