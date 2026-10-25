//! The launcher's own update: one background check per start, the download and its verification,
//! and the swap when the launcher closes (or "Restart Now"). The work is in
//! `anyps5_core::selfupdate`; this module owns the thread and the state the settings show.
//!
//! Simulated services (the demo, the previews and the tests) never touch the network or the disk.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use anyps5_core::error::CancelToken;
use anyps5_core::paths::AppPaths;
use anyps5_core::selfupdate::{self, SelfUpdateError, Staged};
use egui::Context;

use crate::screens::settings::LauncherUpdate;

/// What the worker reports.
#[derive(Debug)]
pub enum SelfUpdateEvent {
    /// A verified update waits to be swapped in.
    Ready(Staged),
    /// A newer release exists but the launcher's folder cannot be written to.
    ReadOnly(String),
    /// A newer release exists but this launcher cannot replace itself (a Linux `.tar.gz`).
    Manual(String),
    /// The check or the download failed; nothing to do for the player.
    Failed(String),
}

pub struct SelfUpdateService {
    tx: Sender<SelfUpdateEvent>,
    rx: Receiver<SelfUpdateEvent>,
    ctx: Context,
    simulated: bool,
    started: bool,
    staged: Option<Staged>,
    /// "Restart Now" was pressed: the swap at exit also starts the new exe.
    restart: bool,
    /// What the settings show.
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
            started: false,
            staged: None,
            restart: false,
            state: LauncherUpdate::None,
        }
    }

    /// Checks for a newer launcher in the background, once per start. A verified update from an
    /// earlier run is reported at once, without waiting for the network.
    pub fn start(&mut self, paths: &AppPaths) {
        if self.started || self.simulated {
            return;
        }
        self.started = true;
        // The Windows exe, or the Linux AppImage that is running; `None` is a launcher that cannot
        // replace itself (the Linux `.tar.gz`), which is only told that a newer version exists.
        let install = selfupdate::install_path();
        let (tx, ctx, paths) = (self.tx.clone(), self.ctx.clone(), paths.clone());
        spawn("launcher update", move || {
            let current = env!("CARGO_PKG_VERSION");
            let send = |event| {
                let _ = tx.send(event);
                ctx.request_repaint();
            };
            let Some(exe) = install else {
                match selfupdate::check(current) {
                    Ok(Some(release)) => {
                        send(SelfUpdateEvent::Manual(release.version().to_owned()));
                    }
                    Ok(None) => {}
                    Err(error) => send(SelfUpdateEvent::Failed(error.to_string())),
                }
                return;
            };
            if let Some(staged) = selfupdate::pending(&paths, current)
                && selfupdate::can_install(&exe)
            {
                send(SelfUpdateEvent::Ready(staged));
            }
            let release = match selfupdate::check(current) {
                Ok(Some(release)) => release,
                Ok(None) => return,
                Err(error) => return send(SelfUpdateEvent::Failed(error.to_string())),
            };
            let staged =
                selfupdate::stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new());
            send(match staged {
                Ok(staged) => SelfUpdateEvent::Ready(staged),
                Err(SelfUpdateError::NotWritable(folder)) => {
                    tracing::warn!("launcher update: cannot write to {}", folder.display());
                    SelfUpdateEvent::ReadOnly(release.version().to_owned())
                }
                Err(error) => SelfUpdateEvent::Failed(error.to_string()),
            });
        });
    }

    /// Finished work; also updates [`state`](Self::state).
    pub fn poll(&mut self) -> Vec<SelfUpdateEvent> {
        let events: Vec<SelfUpdateEvent> = self.rx.try_iter().collect();
        for event in &events {
            match event {
                SelfUpdateEvent::Ready(staged) => {
                    // A newer download replaces an older one.
                    let newer = self
                        .staged
                        .as_ref()
                        .is_none_or(|have| selfupdate::is_newer(&staged.version, &have.version));
                    if newer {
                        self.state = LauncherUpdate::Ready(staged.version.clone());
                        self.staged = Some(staged.clone());
                    }
                }
                SelfUpdateEvent::ReadOnly(version) => {
                    if self.staged.is_none() {
                        self.state = LauncherUpdate::ReadOnly(version.clone());
                    }
                }
                SelfUpdateEvent::Manual(version) => {
                    if self.staged.is_none() {
                        self.state = LauncherUpdate::Manual(version.clone());
                    }
                }
                SelfUpdateEvent::Failed(error) => tracing::warn!("launcher update: {error}"),
            }
        }
        events
    }

    /// "Restart Now": the swap and the new exe happen in [`finish`](Self::finish), after the
    /// launcher has saved its preferences. False when nothing is staged.
    pub fn request_restart(&mut self) -> bool {
        self.restart = self.staged.is_some();
        self.restart
    }

    /// At exit: swaps the staged update in, and starts it when a restart was asked for. Does
    /// nothing without a staged update. A failure is logged and returned for the caller to log.
    pub fn finish(&mut self) -> Result<(), SelfUpdateError> {
        let Some(staged) = self.staged.take() else {
            return Ok(());
        };
        self.state = LauncherUpdate::None;
        let exe: PathBuf = selfupdate::install_path().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "this launcher cannot replace itself",
            )
        })?;
        if std::mem::take(&mut self.restart) {
            // The new launcher waits for this one to end before it takes the single instance.
            let mut args: Vec<std::ffi::OsString> = std::env::args_os()
                .skip(1)
                .filter(|a| {
                    !a.to_string_lossy()
                        .starts_with(crate::instance::AFTER_UPDATE)
                })
                .collect();
            args.push(format!("{}{}", crate::instance::AFTER_UPDATE, std::process::id()).into());
            staged.restart(&exe, args)
        } else {
            staged.swap_in(&exe)
        }
    }
}

fn spawn(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.into()).spawn(work) {
        tracing::warn!("cannot start the {name} thread: {error}");
    }
}
