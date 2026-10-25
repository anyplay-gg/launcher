//! The launcher's own update: a background check at the start and then every few hours while the
//! launcher stays open, the download and its verification, and the swap when the launcher closes
//! (or "Restart Now"). The work is in `anyps5_core::selfupdate`; this module owns the threads, the
//! timing and the state the interface shows (docs/DESIGN.md 9.5).
//!
//! Simulated services (the demo, the previews and the tests) never touch the network or the disk.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use anyps5_core::error::CancelToken;
use anyps5_core::notes::ReleaseNotes;
use anyps5_core::paths::AppPaths;
use anyps5_core::selfupdate::{self, Failure, SelfUpdateError, Staged};
use egui::Context;

use crate::screens::settings::{LauncherUpdate, UpdateFailure};

/// How long a quiet launcher waits between two checks.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// After a check that could not reach GitHub: sooner, because the player may only have been
/// offline for a moment.
pub const RETRY_OFFLINE: Duration = Duration::from_secs(30 * 60);

/// What the worker reports.
#[derive(Debug)]
pub enum SelfUpdateEvent {
    /// A check began.
    Checking,
    /// The check found nothing newer.
    UpToDate,
    /// A newer release exists; its download begins.
    Found(String),
    /// The download of this version, in thousandths.
    Progress(String, u16),
    /// A verified update waits to be swapped in; the release's notes when they are known.
    Ready(Staged, Option<String>),
    /// A newer release exists but the launcher's folder cannot be written to.
    ReadOnly(String),
    /// The check or the download failed. The text is for the log only.
    Failed(Failure, String),
}

pub struct SelfUpdateService {
    tx: Sender<SelfUpdateEvent>,
    rx: Receiver<SelfUpdateEvent>,
    ctx: Context,
    simulated: bool,
    /// A worker runs (a check, or the download after it).
    busy: bool,
    /// The next check is not due before this.
    next_check: Option<Instant>,
    /// "Check Now": one check, even when the preference is off.
    force: bool,
    staged: Option<Staged>,
    /// The notes of the staged version (Markdown), when known.
    notes: Option<String>,
    /// "Restart Now" was pressed and a game runs: the restart waits until it ends.
    holding: bool,
    /// The swap at exit also starts the new exe.
    restart: bool,
    /// Where the new launcher should open: a deep link, set with the restart.
    resume: Option<String>,
    cancel: CancelToken,
    /// What the interface shows.
    pub state: LauncherUpdate,
}

impl SelfUpdateService {
    pub fn new(ctx: &Context, simulated: bool) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            ctx: ctx.clone(),
            simulated,
            busy: false,
            next_check: None,
            force: false,
            staged: None,
            notes: None,
            holding: false,
            restart: false,
            resume: None,
            cancel: CancelToken::new(),
            state: LauncherUpdate::None,
        }
    }

    /// A service that only shows `state` (the previews and the gallery).
    pub fn showing(ctx: &Context, state: LauncherUpdate, notes: Option<&str>) -> Self {
        let mut service = Self::new(ctx, true);
        if let LauncherUpdate::Ready(version) | LauncherUpdate::Holding(version) = &state {
            service.staged = Some(Staged {
                version: version.clone(),
                path: PathBuf::new(),
                sha256: None,
            });
            service.holding = matches!(state, LauncherUpdate::Holding(_));
        }
        service.notes = notes.map(str::to_owned);
        service.state = state;
        service
    }

    /// Whether a check is due at `now`, given the interval since the last one.
    fn due(&self, now: Instant) -> bool {
        !self.busy && self.next_check.is_none_or(|at| now >= at)
    }

    /// Called every frame: starts the check when it is due (the first time at once, then every
    /// [`CHECK_EVERY`]) and keeps the window waking up for the next one. Does nothing when the
    /// *Check for Updates* preference is off.
    pub fn tick(&mut self, paths: &AppPaths, enabled: bool) {
        // The update is the Windows portable exe; a Linux build is replaced from the releases page.
        if self.simulated || !cfg!(windows) || !(enabled || self.force) {
            return;
        }
        let now = Instant::now();
        if self.due(now) {
            self.force = false;
            self.start(paths);
            self.next_check = Some(now + CHECK_EVERY);
        } else if let Some(at) = self.next_check
            && !self.busy
        {
            self.ctx
                .request_repaint_after(at.saturating_duration_since(now));
        }
    }

    fn start(&mut self, paths: &AppPaths) {
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        self.busy = true;
        let (tx, ctx, paths) = (self.tx.clone(), self.ctx.clone(), paths.clone());
        let (have, cancel) = (self.staged.clone(), self.cancel.clone());
        spawn("launcher update", move || {
            let send = |event| {
                let _ = tx.send(event);
                ctx.request_repaint();
            };
            work(&paths, &exe, have, &cancel, &send);
        });
    }

    /// "Check Now": a check at the next frame, whatever the preference says and however recent
    /// the last one was.
    pub fn check_now(&mut self) {
        self.force = true;
        self.next_check = None;
    }

    /// Finished work; also updates [`state`](Self::state).
    pub fn poll(&mut self) -> Vec<SelfUpdateEvent> {
        let events: Vec<SelfUpdateEvent> = self.rx.try_iter().collect();
        for event in &events {
            self.apply(event);
        }
        events
    }

    fn apply(&mut self, event: &SelfUpdateEvent) {
        // Something staged outranks everything a later check says.
        let idle = self.staged.is_none();
        match event {
            SelfUpdateEvent::Checking => {
                if idle && !matches!(self.state, LauncherUpdate::ReadOnly(_)) {
                    self.state = LauncherUpdate::Checking;
                }
            }
            SelfUpdateEvent::UpToDate => {
                self.busy = false;
                if idle {
                    self.state = LauncherUpdate::UpToDate;
                }
            }
            SelfUpdateEvent::Found(version) => {
                if idle {
                    self.state = LauncherUpdate::Available(version.clone());
                }
            }
            SelfUpdateEvent::Progress(version, permille) => {
                if idle {
                    self.state = LauncherUpdate::Downloading {
                        version: version.clone(),
                        permille: *permille,
                    };
                }
            }
            SelfUpdateEvent::Ready(staged, notes) => {
                // A newer download replaces an older one; the same version only brings its notes.
                let newer = self
                    .staged
                    .as_ref()
                    .is_none_or(|have| selfupdate::is_newer(&staged.version, &have.version));
                if newer {
                    self.state = if self.holding {
                        LauncherUpdate::Holding(staged.version.clone())
                    } else {
                        LauncherUpdate::Ready(staged.version.clone())
                    };
                    self.staged = Some(staged.clone());
                    self.notes = None;
                }
                if notes.is_some()
                    && self
                        .staged
                        .as_ref()
                        .is_some_and(|have| have.version == staged.version)
                {
                    self.notes.clone_from(notes);
                }
            }
            SelfUpdateEvent::ReadOnly(version) => {
                self.busy = false;
                if idle {
                    self.state = LauncherUpdate::ReadOnly(version.clone());
                }
            }
            SelfUpdateEvent::Failed(failure, error) => {
                self.busy = false;
                tracing::warn!("launcher update: {error}");
                if *failure == Failure::Offline {
                    // Soon again: the player may only have been offline for a moment.
                    self.next_check = Some(Instant::now() + RETRY_OFFLINE);
                }
                if idle {
                    self.state = LauncherUpdate::Failed(match failure {
                        Failure::Offline => UpdateFailure::Offline,
                        Failure::Refused => UpdateFailure::Refused,
                        Failure::Other => UpdateFailure::Other,
                    });
                }
            }
        }
        // A worker that ends with a staged update is done as well.
        if matches!(event, SelfUpdateEvent::Ready(..)) {
            self.busy = false;
        }
    }

    /// The release notes of the staged version, read as the "What's New" panel shows them.
    pub fn notes(&self) -> Option<ReleaseNotes> {
        let notes = ReleaseNotes::parse_launcher(self.notes.as_deref()?);
        (!notes.changes.is_empty()).then_some(notes)
    }

    /// The version waiting to be installed.
    pub fn staged_version(&self) -> Option<&str> {
        self.staged.as_ref().map(|s| s.version.as_str())
    }

    /// "Restart Now". With nothing staged: `Nothing`. While a game runs the restart is held and
    /// the game is left alone; otherwise the caller closes the window and [`finish`](Self::finish)
    /// swaps and starts the new exe once the preferences are saved. `resume` is where the new
    /// launcher should open.
    pub fn request_restart(&mut self, game_running: bool, resume: Option<String>) -> Restart {
        let Some(staged) = &self.staged else {
            return Restart::Nothing;
        };
        self.resume = resume;
        if game_running {
            self.holding = true;
            self.state = LauncherUpdate::Holding(staged.version.clone());
            Restart::Held
        } else {
            self.restart = true;
            Restart::Now
        }
    }

    /// "Cancel" while the restart waits for a game: the update installs when the launcher closes.
    pub fn cancel_hold(&mut self) {
        if self.holding {
            self.holding = false;
            self.restart = false;
            if let Some(staged) = &self.staged {
                self.state = LauncherUpdate::Ready(staged.version.clone());
            }
        }
    }

    /// Called every frame: true once a held restart may go ahead because no game runs any more.
    pub fn hold_released(&mut self, game_running: bool) -> bool {
        if self.holding && !game_running {
            self.holding = false;
            self.restart = true;
            return true;
        }
        false
    }

    /// At exit: swaps the staged update in, and starts it when a restart was asked for. Does
    /// nothing without a staged update. A failure is logged and returned for the caller to log.
    pub fn finish(&mut self) -> Result<(), SelfUpdateError> {
        // A download that is still running is not wanted any more.
        self.cancel.cancel();
        let Some(staged) = self.staged.take() else {
            return Ok(());
        };
        self.state = LauncherUpdate::None;
        let exe: PathBuf = std::env::current_exe()?;
        if std::mem::take(&mut self.restart) {
            // The new launcher waits for this one to end before it takes the single instance.
            let mut args: Vec<std::ffi::OsString> = std::env::args_os()
                .skip(1)
                .filter(|a| a.to_string_lossy().starts_with('-'))
                .filter(|a| {
                    !a.to_string_lossy()
                        .starts_with(crate::instance::AFTER_UPDATE)
                })
                .collect();
            args.extend(self.resume.take().map(std::ffi::OsString::from));
            args.push(format!("{}{}", crate::instance::AFTER_UPDATE, std::process::id()).into());
            staged.restart(&exe, args)
        } else {
            staged.swap_in(&exe)
        }
    }
}

/// What "Restart Now" came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Restart {
    /// Nothing is staged.
    Nothing,
    /// The window closes now and the swap follows.
    Now,
    /// A game runs: the restart waits for it.
    Held,
}

/// The worker: an update from an earlier run is offered at once, then the check, the download and
/// the verification.
fn work(
    paths: &AppPaths,
    exe: &std::path::Path,
    have: Option<Staged>,
    cancel: &CancelToken,
    send: &dyn Fn(SelfUpdateEvent),
) {
    let current = env!("CARGO_PKG_VERSION");
    send(SelfUpdateEvent::Checking);
    let mut have = have;
    if have.is_none()
        && let Some(staged) = selfupdate::pending(paths, current)
        && selfupdate::can_install(exe)
    {
        send(SelfUpdateEvent::Ready(staged.clone(), None));
        have = Some(staged);
    }
    let release = match selfupdate::check_cached(paths, current) {
        Ok(Some(release)) => release,
        Ok(None) => return send(SelfUpdateEvent::UpToDate),
        Err(error) => return send(SelfUpdateEvent::Failed(error.failure(), error.to_string())),
    };
    let notes = release.notes().map(str::to_owned);
    // Already downloaded: only the notes are new.
    if let Some(staged) = have.filter(|s| s.version == release.version()) {
        return send(SelfUpdateEvent::Ready(staged, notes));
    }
    let version = release.version().to_owned();
    send(SelfUpdateEvent::Found(version.clone()));
    let mut last = 0u16;
    let mut progress = |share: f64| {
        let permille = (share.clamp(0.0, 1.0) * 1000.0) as u16;
        // About one event per percent.
        if permille >= last.saturating_add(10) || (permille == 1000 && last != 1000) {
            last = permille;
            send(SelfUpdateEvent::Progress(version.clone(), permille));
        }
    };
    send(
        match selfupdate::stage(&release, exe, paths, &mut progress, cancel) {
            Ok(staged) => SelfUpdateEvent::Ready(staged, notes),
            Err(SelfUpdateError::NotWritable(folder)) => {
                tracing::warn!("launcher update: cannot write to {}", folder.display());
                SelfUpdateEvent::ReadOnly(release.version().to_owned())
            }
            Err(error) => SelfUpdateEvent::Failed(error.failure(), error.to_string()),
        },
    );
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!("cannot start the {name} thread: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service() -> SelfUpdateService {
        SelfUpdateService::new(&Context::default(), false)
    }

    fn staged(version: &str) -> Staged {
        Staged {
            version: version.into(),
            path: PathBuf::from("x"),
            sha256: None,
        }
    }

    fn feed(s: &mut SelfUpdateService, event: SelfUpdateEvent) {
        s.apply(&event);
    }

    #[test]
    fn a_check_is_due_at_the_start_and_then_every_six_hours() {
        let mut s = service();
        let now = Instant::now();
        assert!(s.due(now));
        s.next_check = Some(now + CHECK_EVERY);
        assert!(!s.due(now + CHECK_EVERY - Duration::from_secs(1)));
        assert!(s.due(now + CHECK_EVERY));
        // Never two at once.
        s.busy = true;
        assert!(!s.due(now + CHECK_EVERY * 2));
    }

    #[test]
    fn the_states_follow_the_events() {
        let mut s = service();
        feed(&mut s, SelfUpdateEvent::Checking);
        assert_eq!(s.state, LauncherUpdate::Checking);
        feed(&mut s, SelfUpdateEvent::Found("0.3.0".into()));
        assert_eq!(s.state, LauncherUpdate::Available("0.3.0".into()));
        feed(&mut s, SelfUpdateEvent::Progress("0.3.0".into(), 420));
        assert_eq!(
            s.state,
            LauncherUpdate::Downloading {
                version: "0.3.0".into(),
                permille: 420
            }
        );
        feed(&mut s, SelfUpdateEvent::Ready(staged("0.3.0"), None));
        assert_eq!(s.state, LauncherUpdate::Ready("0.3.0".into()));
        assert!(!s.busy);
        // A later check that finds nothing new does not take the staged update away.
        feed(&mut s, SelfUpdateEvent::Checking);
        feed(&mut s, SelfUpdateEvent::UpToDate);
        assert_eq!(s.state, LauncherUpdate::Ready("0.3.0".into()));
    }

    #[test]
    fn a_failure_is_quiet_and_a_retry_comes_sooner_when_offline() {
        let mut s = service();
        s.busy = true;
        feed(
            &mut s,
            SelfUpdateEvent::Failed(Failure::Offline, "no network".into()),
        );
        assert_eq!(s.state, LauncherUpdate::Failed(UpdateFailure::Offline));
        assert!(!s.busy);
        let wait = s
            .next_check
            .unwrap()
            .saturating_duration_since(Instant::now());
        assert!(wait <= RETRY_OFFLINE && wait > RETRY_OFFLINE - Duration::from_secs(5));
        feed(
            &mut s,
            SelfUpdateEvent::Failed(Failure::Refused, "bad signature".into()),
        );
        assert_eq!(s.state, LauncherUpdate::Failed(UpdateFailure::Refused));
    }

    #[test]
    fn the_same_version_again_only_brings_its_notes() {
        let mut s = service();
        feed(&mut s, SelfUpdateEvent::Ready(staged("0.3.0"), None));
        assert!(s.notes().is_none());
        feed(
            &mut s,
            SelfUpdateEvent::Ready(staged("0.3.0"), Some("### New\n- A thing".into())),
        );
        assert_eq!(s.state, LauncherUpdate::Ready("0.3.0".into()));
        assert_eq!(s.notes().map(|n| n.changes.len()), Some(1));
        // A newer download replaces the staged one and drops its notes.
        feed(&mut s, SelfUpdateEvent::Ready(staged("0.3.1"), None));
        assert_eq!(s.staged_version(), Some("0.3.1"));
        assert!(s.notes().is_none());
    }

    #[test]
    fn restart_now_never_interrupts_a_running_game() {
        let mut s = service();
        assert_eq!(s.request_restart(false, None), Restart::Nothing);
        feed(&mut s, SelfUpdateEvent::Ready(staged("0.3.0"), None));
        // A game runs: held, the state says so, and nothing is released while it runs.
        assert_eq!(s.request_restart(true, None), Restart::Held);
        assert_eq!(s.state, LauncherUpdate::Holding("0.3.0".into()));
        assert!(!s.hold_released(true));
        // The game ended: the restart goes ahead exactly once.
        assert!(s.hold_released(false));
        assert!(s.restart);
        assert!(!s.hold_released(false));
    }

    #[test]
    fn cancelling_the_hold_leaves_the_update_for_the_next_close() {
        let mut s = service();
        feed(&mut s, SelfUpdateEvent::Ready(staged("0.3.0"), None));
        s.request_restart(true, None);
        s.cancel_hold();
        assert_eq!(s.state, LauncherUpdate::Ready("0.3.0".into()));
        assert!(!s.hold_released(false));
        assert!(!s.restart);
        // With no game, the restart is immediate.
        assert_eq!(
            s.request_restart(false, Some("anyps5://settings/about".into())),
            Restart::Now
        );
    }
}
