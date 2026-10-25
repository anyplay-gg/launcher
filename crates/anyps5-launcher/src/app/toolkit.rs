//! The toolkit work behind the Toolkit category of the settings: asking what the newest release
//! is, downloading one, adopting a folder, the release list for pinning, the release notes and
//! whether an update changes how games are converted. Everything runs on worker threads and
//! reports through one channel, so the UI thread never waits on the network or the disk.
//!
//! Simulated services (the demo and the tests) never touch the network or the disk.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use anyps5_core::events::CancelToken;
use anyps5_core::notes::ReleaseNotes;
use anyps5_core::paths::AppPaths;
use anyps5_core::releases::{self, Endpoints, Published};
use anyps5_core::toolkit::{self, InstalledRelease, Toolkit, ToolkitError};
use egui::Context;

use crate::screens::settings::UpdateState;
use crate::screens::whatsnew::SAMPLE_NOTES;

/// What a worker reports.
#[derive(Debug)]
pub enum ToolkitEvent {
    /// The update check ended: `Ok(Some(tag))` a newer (or first) release exists.
    Checked(Result<Option<String>, String>),
    /// The download ended: the installed version.
    Installed(Result<String, String>),
    /// A folder was adopted: the folder and the version.
    Adopted(Result<(std::path::PathBuf, String), String>),
    /// A release a game stays on was downloaded: the installed version. The global choice is
    /// not touched.
    PinInstalled(Result<String, String>),
    /// An unused release was removed: its tag.
    Removed(Result<String, String>),
}

/// The "What's new" notes of one release.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum NotesState {
    #[default]
    Idle,
    Loading,
    Ready(Box<Published>),
    Unavailable,
}

enum Inbox {
    Event(ToolkitEvent),
    Recent(Result<Vec<String>, String>),
    Notes {
        tag: String,
        result: Result<Option<Published>, String>,
    },
    Conversion {
        from: String,
        to: String,
        changed: bool,
    },
    Installed(Vec<InstalledRelease>),
}

pub struct ToolkitService {
    tx: Sender<Inbox>,
    rx: Receiver<Inbox>,
    ctx: Context,
    simulated: bool,
    checking: bool,
    installing: bool,
    /// What the settings show.
    pub state: UpdateState,
    /// The latest release found by the last check.
    pub latest: Option<String>,
    /// The last releases, newest first, as `v0.1.3`; empty until fetched.
    pub recent: Vec<String>,
    recent_asked: bool,
    /// The notes last asked for, and the release they belong to.
    pub notes: NotesState,
    pub notes_tag: Option<String>,
    /// Whether the releases between two versions changed the conversion, by (from, to).
    conversion: HashMap<(String, String), bool>,
    conversion_asked: HashSet<(String, String)>,
    /// The release a game stays on that is downloading now.
    pub pin_installing: Option<String>,
    /// The installed releases and their sizes; `None` until the first read.
    pub installed: Option<Vec<InstalledRelease>>,
    installed_stale: bool,
    installed_reading: bool,
}

impl ToolkitService {
    pub fn new(ctx: &Context, simulated: bool) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            ctx: ctx.clone(),
            simulated,
            checking: false,
            installing: false,
            state: UpdateState::Unknown,
            latest: None,
            recent: Vec::new(),
            recent_asked: false,
            notes: NotesState::Idle,
            notes_tag: None,
            conversion: HashMap::new(),
            conversion_asked: HashSet::new(),
            pin_installing: None,
            installed: None,
            installed_stale: true,
            installed_reading: false,
        }
    }

    /// The folders on disk changed: read the installed releases again when they are next wanted.
    pub fn invalidate_installed(&mut self) {
        self.installed_stale = true;
    }

    /// Reads the installed releases with their sizes, in the background, when that is out of date.
    pub fn load_installed(&mut self, paths: &AppPaths) {
        if !self.installed_stale || self.installed_reading {
            return;
        }
        self.installed_stale = false;
        self.installed_reading = true;
        let (tx, ctx, paths) = (self.tx.clone(), self.ctx.clone(), paths.clone());
        let simulated = self.simulated;
        spawn("toolkit sizes", move || {
            let list = if simulated {
                Vec::new()
            } else {
                toolkit::installed_with_sizes(&paths, &CancelToken::new())
            };
            let _ = tx.send(Inbox::Installed(list));
            ctx.request_repaint();
        });
    }

    /// Deletes an installed release nothing uses (`in_use` are the tags that must stay).
    pub fn remove(&mut self, paths: &AppPaths, tag: &str, in_use: Vec<String>) {
        let (tx, ctx, paths, tag) = (
            self.tx.clone(),
            self.ctx.clone(),
            paths.clone(),
            tag.to_owned(),
        );
        let simulated = self.simulated;
        spawn("toolkit remove", move || {
            let result = if simulated {
                Err("the demo does not remove".to_owned())
            } else {
                toolkit::remove_release(&paths, &tag, &in_use)
                    .map(|()| tag)
                    .map_err(describe)
            };
            let _ = tx.send(Inbox::Event(ToolkitEvent::Removed(result)));
            ctx.request_repaint();
        });
    }

    pub fn installing(&self) -> bool {
        self.installing
    }

    /// Asks for the newest release. Ignored while a check or a download runs.
    pub fn check(&mut self, paths: &AppPaths, repository: &str) {
        if self.checking || self.installing {
            return;
        }
        self.checking = true;
        self.state = UpdateState::Checking;
        let (tx, ctx, paths, repo) = self.worker(paths, repository);
        let simulated = self.simulated;
        spawn("toolkit check", move || {
            let result = if simulated {
                std::thread::sleep(Duration::from_millis(700));
                Ok(None)
            } else {
                check_latest(&paths, &repo)
            };
            let _ = tx.send(Inbox::Event(ToolkitEvent::Checked(result)));
            ctx.request_repaint();
        });
    }

    /// Downloads and installs a release: `tag`, or the latest. False when a download already runs.
    /// `keep` are the releases games stay on: they survive the clean-up of old ones.
    pub fn install(
        &mut self,
        paths: &AppPaths,
        repository: &str,
        tag: Option<&str>,
        keep: Vec<String>,
    ) -> bool {
        if self.installing {
            return false;
        }
        self.installing = true;
        self.state = UpdateState::Downloading;
        let (tx, ctx, paths, repo) = self.worker(paths, repository);
        let (simulated, tag) = (self.simulated, tag.map(str::to_owned));
        spawn("toolkit install", move || {
            let result = if simulated {
                std::thread::sleep(Duration::from_millis(900));
                Err("the demo does not download".to_owned())
            } else {
                install_release(&paths, &repo, tag.as_deref(), &keep)
            };
            let _ = tx.send(Inbox::Event(ToolkitEvent::Installed(result)));
            ctx.request_repaint();
        });
        true
    }

    /// Downloads the release a game stays on, again. Only the configured repository is asked, and
    /// the release is checked like any other; the global choice stays as it is. False when a
    /// download already runs.
    pub fn install_pinned(
        &mut self,
        paths: &AppPaths,
        repository: &str,
        tag: &str,
        keep: Vec<String>,
    ) -> bool {
        if self.installing {
            return false;
        }
        self.installing = true;
        self.pin_installing = Some(tag.to_owned());
        let (tx, ctx, paths, repo) = self.worker(paths, repository);
        let (simulated, tag) = (self.simulated, tag.to_owned());
        spawn("toolkit install", move || {
            let result = if simulated {
                std::thread::sleep(Duration::from_millis(900));
                Err("the demo does not download".to_owned())
            } else {
                install_release(&paths, &repo, Some(&tag), &keep)
            };
            let _ = tx.send(Inbox::Event(ToolkitEvent::PinInstalled(result)));
            ctx.request_repaint();
        });
        true
    }

    /// Checks a toolkit folder the player picked (it hashes the files, so not on the UI thread).
    pub fn adopt(&mut self, folder: std::path::PathBuf) {
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        let simulated = self.simulated;
        spawn("toolkit adopt", move || {
            let result = if simulated {
                Err("the demo does not adopt folders".to_owned())
            } else {
                toolkit::adopt_folder(&folder)
                    .map(|t| (folder.clone(), t.version().to_owned()))
                    .map_err(describe)
            };
            let _ = tx.send(Inbox::Event(ToolkitEvent::Adopted(result)));
            ctx.request_repaint();
        });
    }

    /// Fetches the list of the last releases once (for the pinned channel).
    pub fn load_recent(&mut self, paths: &AppPaths, repository: &str) {
        if self.recent_asked {
            return;
        }
        self.recent_asked = true;
        let (tx, ctx, paths, repo) = self.worker(paths, repository);
        let simulated = self.simulated;
        spawn("toolkit releases", move || {
            let result = if simulated {
                Ok(["v0.1.3", "v0.1.2", "v0.1.1", "v0.1.0"]
                    .map(str::to_owned)
                    .to_vec())
            } else {
                releases::recent(
                    &paths,
                    &Endpoints::github(&repo),
                    &repo,
                    Duration::from_secs(6 * 3600),
                )
                .map(|list| list.iter().map(|r| r.tag.clone()).collect())
                .map_err(describe)
            };
            let _ = tx.send(Inbox::Recent(result));
            ctx.request_repaint();
        });
    }

    /// Fetches the notes of release `tag`. The panel shows [`NotesState::Loading`] meanwhile.
    pub fn load_notes(&mut self, paths: &AppPaths, repository: &str, tag: &str) {
        if self.notes_tag.as_deref() == Some(tag)
            && matches!(self.notes, NotesState::Loading | NotesState::Ready(_))
        {
            return;
        }
        self.notes = NotesState::Loading;
        self.notes_tag = Some(tag.to_owned());
        let (tx, ctx, paths, repo) = self.worker(paths, repository);
        let (simulated, tag) = (self.simulated, tag.to_owned());
        spawn("toolkit notes", move || {
            let result = if simulated {
                std::thread::sleep(Duration::from_millis(300));
                Ok(Some(Published {
                    tag: tag.trim_start_matches('v').to_owned(),
                    date: "2026-10-20".to_owned(),
                    notes: ReleaseNotes::parse(SAMPLE_NOTES),
                }))
            } else {
                releases::published_notes(&paths, &Endpoints::github(&repo), &repo, &tag)
                    .map_err(describe)
            };
            let _ = tx.send(Inbox::Notes { tag, result });
            ctx.request_repaint();
        });
    }

    /// Whether the releases after `from` up to `to` changed how games are converted: `None` until
    /// known (or when the notes could not be had).
    pub fn conversion_changed(&self, from: &str, to: &str) -> Option<bool> {
        self.conversion
            .get(&(from.to_owned(), to.to_owned()))
            .copied()
    }

    /// Looks it up once per pair, in the background.
    pub fn ask_conversion(&mut self, paths: &AppPaths, repository: &str, from: &str, to: &str) {
        let key = (from.to_owned(), to.to_owned());
        if !self.conversion_asked.insert(key.clone()) {
            return;
        }
        let (tx, ctx, paths, repo) = self.worker(paths, repository);
        let simulated = self.simulated;
        spawn("toolkit conversion", move || {
            let changed = if simulated {
                Some(true)
            } else {
                releases::conversion_changed(
                    &paths,
                    &Endpoints::github(&repo),
                    &repo,
                    &key.0,
                    &key.1,
                )
                .inspect_err(|e| tracing::debug!("conversion lookup: {e}"))
                .ok()
            };
            if let Some(changed) = changed {
                let _ = tx.send(Inbox::Conversion {
                    from: key.0,
                    to: key.1,
                    changed,
                });
                ctx.request_repaint();
            }
        });
    }

    fn worker(
        &self,
        paths: &AppPaths,
        repository: &str,
    ) -> (Sender<Inbox>, Context, AppPaths, String) {
        (
            self.tx.clone(),
            self.ctx.clone(),
            paths.clone(),
            repository.to_owned(),
        )
    }

    /// Finished work that the host reacts to; also updates the service's own state.
    pub fn poll(&mut self) -> Vec<ToolkitEvent> {
        let mut events = Vec::new();
        for inbox in self.rx.try_iter().collect::<Vec<_>>() {
            match inbox {
                Inbox::Event(event) => {
                    match &event {
                        ToolkitEvent::Checked(result) => {
                            self.checking = false;
                            self.state = match result {
                                Ok(Some(tag)) => {
                                    self.latest = Some(tag.clone());
                                    UpdateState::Available(tag.clone())
                                }
                                Ok(None) => UpdateState::UpToDate,
                                Err(_) => UpdateState::Failed,
                            };
                        }
                        ToolkitEvent::Adopted(_) => {}
                        ToolkitEvent::PinInstalled(_) => {
                            self.installing = false;
                            self.pin_installing = None;
                            self.installed_stale = true;
                        }
                        ToolkitEvent::Removed(_) => self.installed_stale = true,
                        ToolkitEvent::Installed(result) => {
                            self.installed_stale = true;
                            self.installing = false;
                            self.state = if result.is_ok() {
                                UpdateState::UpToDate
                            } else {
                                UpdateState::Failed
                            };
                        }
                    }
                    events.push(event);
                }
                Inbox::Recent(Ok(list)) => self.recent = list,
                Inbox::Recent(Err(error)) => {
                    tracing::debug!("release list: {error}");
                    self.recent_asked = false;
                }
                Inbox::Notes { tag, result } => {
                    if self.notes_tag.as_deref() == Some(tag.as_str()) {
                        self.notes = match result {
                            Ok(Some(published)) => NotesState::Ready(Box::new(published)),
                            Ok(None) => NotesState::Unavailable,
                            Err(error) => {
                                tracing::debug!("release notes: {error}");
                                NotesState::Unavailable
                            }
                        };
                    }
                }
                Inbox::Conversion { from, to, changed } => {
                    self.conversion.insert((from, to), changed);
                }
                Inbox::Installed(list) => {
                    self.installed_reading = false;
                    self.installed = Some(list);
                }
            }
        }
        events
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!("cannot start the {name} thread: {error}");
    }
}

fn describe(error: ToolkitError) -> String {
    error.to_string()
}

fn check_latest(paths: &AppPaths, repository: &str) -> Result<Option<String>, String> {
    let newest = releases::newest_tag(paths, &Endpoints::github(repository), repository)
        .map_err(describe)?;
    let newer = match Toolkit::locate(paths, None) {
        Some(installed) => match installed.tag() {
            Some(tag) if !installed.is_adopted() => toolkit::is_newer(&newest, tag),
            _ => false,
        },
        None => true,
    };
    Ok(newer.then_some(newest))
}

fn install_release(
    paths: &AppPaths,
    repository: &str,
    tag: Option<&str>,
    keep: &[String],
) -> Result<String, String> {
    let endpoints = Endpoints::github(repository);
    let info = match tag {
        None => toolkit::check_latest(repository).map_err(describe)?,
        Some(tag) => releases::by_tag(paths, &endpoints, repository, tag)
            .map_err(|e| format!("release {tag}: {e}"))?,
    };
    if releases::libs_digest_disagrees(paths, &endpoints, &info) {
        tracing::warn!(
            "the Pages release file and the API disagree about the libraries; using the API"
        );
    }
    let cancel = CancelToken::new();
    let installed =
        toolkit::download_and_install_keeping(&info, paths, None, keep, &mut |_, _| {}, &cancel)
            .map_err(describe)?;
    Ok(installed.version().to_owned())
}
