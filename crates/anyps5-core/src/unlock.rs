//! Unlocking a game: unwrap its executable and modules, relink them, and assemble the unlocked
//! copy the launcher starts (PORT.md 6.4 and 6.5, `Installer.swift`).
//!
//! Interface: [`UnlockQueue`] runs one unlock at a time and reports [`UnlockEvent`]s;
//! [`run_unlock`] is the same pipeline, synchronous, for tools and tests. The unlocked copy's
//! location and record are [`unlocked_copy`] and [`read_record`].
//!
//! Nothing here writes into the dump: its files are hard-linked (or copied) into `app0\` and the
//! converted modules are laid over by replacing, never by writing through a link (D4).

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{CancelToken, EventSink};
use crate::fsx::{self, LinkMode, Linker, TreeError, TreeOpts};
use crate::paths::GameId;
use crate::platform::Os;
use crate::process::{Priority, ToolProcess, ToolSpec};
use crate::saves::{Reason, SaveData, SavesError};
use crate::toolkit::{Toolkit, ToolkitError};
use crate::unwrap::{self, Refused};

/// The name of the hidden store folder inside the games folder.
pub const STORE_NAME: &str = ".AnyPS5";
/// The file inside an unlocked copy that says which dump and toolkit made it.
pub const RECORD_FILE: &str = "anyps5-unlock.json";
/// The name macOS launchers wrote; still read.
const LEGACY_RECORD_FILE: &str = "anyps5-install.json";
/// The relinked executable inside an unlocked copy.
pub const EXECUTABLE: &str = Os::current().executable();

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Unwrap,
    Relink,
    Copy,
    Assemble,
    Finish,
}

impl Step {
    pub const ALL: [Step; 5] = [
        Step::Unwrap,
        Step::Relink,
        Step::Copy,
        Step::Assemble,
        Step::Finish,
    ];

    /// The place of the step in [`StepSeconds`].
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// How many seconds each [`Step`] of one unlock took, by [`Step::index`]. The launcher keeps the
/// last ones to weigh the steps of its progress bar by what they cost on this PC.
pub type StepSeconds = [f32; 5];

/// What the launcher keeps in an unlocked copy.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct UnlockRecord {
    /// RFC 3339, UTC.
    pub date: String,
    /// The dump folder, as an OS path.
    pub source: String,
    #[serde(rename = "titleID")]
    pub title_id: String,
    pub toolkit: String,
    /// SHA-256 of the relinker and the libraries zip the copy was made with, when known.
    #[serde(
        default,
        rename = "relinkerSha256",
        skip_serializing_if = "Option::is_none"
    )]
    pub relinker_sha256: Option<String>,
    #[serde(
        default,
        rename = "libsSha256",
        skip_serializing_if = "Option::is_none"
    )]
    pub libs_sha256: Option<String>,
    /// A hash of everything the relinker's output depends on: its inputs, its arguments and the
    /// release. An unlock again that changes none of it keeps that output instead of relinking.
    #[serde(default, rename = "relinkKey", skip_serializing_if = "Option::is_none")]
    pub relink_key: Option<String>,
    /// The files the relinker made besides the executable, relative to `app0`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum UnlockError {
    #[error("cancelled")]
    Cancelled,
    #[error("no toolkit is installed")]
    ToolkitMissing,
    #[error("{file}: {source}")]
    Unwrap { file: String, source: Refused },
    #[error(
        "{file} is neither an ELF nor a decrypted SELF container (an encrypted dump cannot be used)"
    )]
    NotAnExecutable { file: String },
    #[error("relinker failed ({}): {last_line}", .code.map_or("ended".to_string(), |c| c.to_string()))]
    Relinker {
        code: Option<i32>,
        last_line: String,
    },
    #[error("relinker wrote no executable")]
    NoOutput,
    #[error("needs {} GB, {} GB free on {}{}", gb_up(*.needed), gb_down(*.free), .folder.display(), copy_note(*.copying))]
    NotEnoughSpace {
        needed: u64,
        free: u64,
        folder: PathBuf,
        /// The game files must be copied because the games folder cannot hard-link to the dump.
        copying: bool,
    },
    #[error("{context}: {source}")]
    Io { context: String, source: io::Error },
    #[error(transparent)]
    Toolkit(#[from] ToolkitError),
}

const GIB: u64 = 1024 * 1024 * 1024;

/// Gigabytes with one decimal for the space message: the need rounds up and the free space
/// rounds down, so the two numbers never read as equal when one is short.
fn gb_up(bytes: u64) -> String {
    format!("{:.1}", ((bytes as f64 / GIB as f64) * 10.0).ceil() / 10.0)
}

fn gb_down(bytes: u64) -> String {
    format!("{:.1}", ((bytes as f64 / GIB as f64) * 10.0).floor() / 10.0)
}

fn copy_note(copying: bool) -> &'static str {
    if copying {
        " (the games folder cannot hard-link to the dump, so the game files are copied)"
    } else {
        ""
    }
}

/// What went wrong, in the terms of what the player can do about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// No toolkit is installed.
    ToolkitMissing,
    /// The drive of the games folder is full.
    DiskFull,
    /// Another program holds a file (a virus scanner, a backup, a file manager).
    FileInUse,
    /// The dump cannot be used: encrypted, or not a PS5 executable.
    BadDump,
    /// The converter itself stopped.
    Converter,
    Other,
}

impl UnlockError {
    pub fn kind(&self) -> FailureKind {
        match self {
            UnlockError::ToolkitMissing => FailureKind::ToolkitMissing,
            UnlockError::Unwrap { .. } | UnlockError::NotAnExecutable { .. } => {
                FailureKind::BadDump
            }
            UnlockError::Relinker { .. } | UnlockError::NoOutput => FailureKind::Converter,
            UnlockError::Io { source, .. } => match source.raw_os_error() {
                // ERROR_DISK_FULL, ERROR_HANDLE_DISK_FULL, ENOSPC
                Some(112 | 39 | 28) => FailureKind::DiskFull,
                // ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION
                Some(5 | 32 | 33) => FailureKind::FileInUse,
                _ => match source.kind() {
                    io::ErrorKind::StorageFull => FailureKind::DiskFull,
                    io::ErrorKind::PermissionDenied => FailureKind::FileInUse,
                    _ => FailureKind::Other,
                },
            },
            UnlockError::NotEnoughSpace { .. } => FailureKind::DiskFull,
            UnlockError::Cancelled | UnlockError::Toolkit(_) => FailureKind::Other,
        }
    }
}

fn io_context(context: impl Into<String>) -> impl FnOnce(io::Error) -> UnlockError {
    let context = context.into();
    move |source| UnlockError::Io { context, source }
}

fn tree_error(error: TreeError) -> UnlockError {
    match error {
        TreeError::Cancelled => UnlockError::Cancelled,
        TreeError::Io { relative, source } => UnlockError::Io {
            context: format!("copy {relative}"),
            source,
        },
    }
}

#[derive(Debug)]
pub enum UnlockEvent {
    /// Put in the queue.
    Queued {
        game: GameId,
    },
    Started {
        game: GameId,
    },
    /// Coalesced: sent when the step changes or the fraction moved by 0.005 or reaches 1.
    Progress {
        game: GameId,
        step: Step,
        fraction: f64,
    },
    Log {
        game: GameId,
        line: String,
    },
    /// The unlock stopped at a safe point (`paused`) or goes on. It keeps what it did.
    Paused {
        game: GameId,
        paused: bool,
    },
    /// Sent before a successful `Finished`: how long each step took.
    Timings {
        game: GameId,
        seconds: StepSeconds,
    },
    Finished {
        game: GameId,
        result: Result<UnlockRecord, UnlockError>,
    },
}

/// Everything one unlock needs.
#[derive(Clone, Debug)]
pub struct UnlockRequest {
    pub game: GameId,
    /// The dump folder.
    pub folder: PathBuf,
    pub title_id: String,
    /// The games folder; the store is `<root>\.AnyPS5`.
    pub root: PathBuf,
    pub toolkit: Toolkit,
    /// D1: convert AMD-only instructions for an Intel host.
    pub to_intel: bool,
    /// Relinker patch file text for this title and build (PORT.md 6.9), if it has patches.
    pub patches_toml: Option<String>,
    /// Selected output resolution for a toolkit's Mach-O patch catalogue.
    pub resolution: Option<String>,
    /// Extra relinker flags (flag and value pairs) from the game's settings; only allowlisted
    /// ones are passed.
    pub relink_flags: Vec<String>,
    pub link_mode: LinkMode,
    /// Test hook: `Some(free)` replaces the games folder's real free space; `Some(None)`
    /// simulates a volume whose free space cannot be read; `None` asks the volume.
    pub free_space_override: Option<Option<u64>>,
    /// Where to back up the saves of the copy this unlock replaces, just before it is replaced
    /// (see [`crate::saves`]); `None` makes no backup.
    pub backup_dir: Option<PathBuf>,
    /// Nobody asked for this unlock yet: the launcher prepares the game while the PC is idle.
    /// It runs with the lowest priority and gives way to everything the player does.
    pub background: bool,
    /// Never keep the relinker's output of the copy this unlock replaces, relink again.
    pub fresh: bool,
}

impl UnlockRequest {
    pub fn new(folder: PathBuf, title_id: String, root: PathBuf, toolkit: Toolkit) -> Self {
        Self {
            game: GameId::from_path(&folder),
            folder,
            title_id,
            root,
            toolkit,
            to_intel: false,
            patches_toml: None,
            resolution: None,
            relink_flags: Vec::new(),
            link_mode: LinkMode::Auto,
            free_space_override: None,
            backup_dir: None,
            background: false,
            fresh: false,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Names and the record

fn games_dir(root: &Path) -> PathBuf {
    crate::paths::store_dir(root).join("Games")
}

fn work_dir(root: &Path) -> PathBuf {
    crate::paths::store_dir(root).join("Work")
}

/// The unlocked copy of a game inside the games folder's store.
pub fn unlocked_copy(title_id: &str, game: &GameId, root: &Path) -> PathBuf {
    crate::paths::unlocked_copy_dir(root, title_id, game)
}

/// The record of an unlocked copy; `None` when there is no relinked executable or no valid record.
pub fn read_record(copy: &Path) -> Option<UnlockRecord> {
    if !copy.join(EXECUTABLE).is_file() {
        return None;
    }
    [RECORD_FILE, LEGACY_RECORD_FILE].iter().find_map(|name| {
        let bytes = fs::read(copy.join(name)).ok()?;
        serde_json::from_slice(&bytes).ok()
    })
}

/// Deletes the work folders and half-made copies an interrupted unlock left, and settles a
/// `.previous` folder (the old copy of an unlock that was cut off in the swap): it is put back when
/// the copy is missing, otherwise what the game wrote there is carried over before it goes. Only
/// call while nothing is unlocking.
pub fn remove_leftovers(root: &Path) {
    let _ = fsx::remove_tree(&work_dir(root));
    if let Ok(entries) = fs::read_dir(games_dir(root)) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if file.ends_with(".partial") {
                let _ = fsx::remove_tree(&entry.path());
            } else if let Some(name) = file.strip_suffix(PREVIOUS_SUFFIX) {
                let copy = entry.path().with_file_name(name);
                let _ = settle_previous(&copy, &entry.path());
            }
        }
    }
}

/// Deletes a game's unlocked copy, saves and trophies included. The dump is untouched.
pub fn remove_unlocked(title_id: &str, game: &GameId, root: &Path) -> io::Result<()> {
    fsx::remove_tree_retry(&unlocked_copy(title_id, game, root))
}

// ---------------------------------------------------------------------------------------------
// Module discovery

/// Every `.prx` and `.sprx` of the dump outside `sce_sys`, at most three path components deep,
/// relative with `/` separators, sorted. Hidden files and folders are skipped.
pub fn modules_in(folder: &Path) -> io::Result<Vec<String>> {
    fn visit(dir: &Path, prefix: &str, depth: usize, out: &mut Vec<String>) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            if fsx::is_hidden(&entry) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                if prefix.is_empty() && name.eq_ignore_ascii_case("sce_sys") {
                    continue;
                }
                // Files below a third folder level are too deep to count.
                if depth < 2 {
                    visit(&entry.path(), &relative, depth + 1, out)?;
                }
            } else if file_type.is_file() {
                let lower = name.to_lowercase();
                // Outputs of an earlier conversion are host binaries, never PS5 inputs.
                // The original .prx is still staged and converted, replacing this old output.
                if lower.ends_with(".guest.prx") {
                    continue;
                }
                if lower.ends_with(".prx") || lower.ends_with(".sprx") {
                    out.push(relative);
                }
            }
        }
        Ok(())
    }
    let mut found = Vec::new();
    visit(folder, "", 0, &mut found)?;
    found.sort();
    Ok(found)
}

/// The folders besides `sce_module` whose modules the relinker converts too (Unity's
/// `Media/Modules`, `Media/Plugins`, `.prx` next to `eboot.bin`): `.` first. Only `.prx` files add
/// a folder; `fakelib` holds stand-ins for system libraries that AnyPS5 implements itself.
///
/// The relinker writes every converted module under its file name and fails when two modules share
/// one ("Conflicting guest output filename"). A dump can hold a second copy of its modules (the
/// Minecraft Dungeons II dump has `decrypted/prx` and `decrypted/sce_module`), so a folder is only
/// added when none of its file names was already taken by an earlier folder or by the default
/// locations (`sce_module`, `sce_modules`, `prx`).
pub fn module_directories(modules: &[String]) -> Vec<String> {
    const DEFAULT: [&str; 3] = ["sce_module", "sce_modules", "prx"];
    let split = |module: &str| -> (String, String) {
        match module.rsplit_once('/') {
            Some((dir, name)) => (dir.to_string(), name.to_lowercase()),
            None => (".".to_string(), module.to_lowercase()),
        }
    };
    let first_of = |dir: &str| dir.split('/').next().unwrap_or(".").to_lowercase();
    let mut taken: std::collections::HashSet<String> = modules
        .iter()
        .map(|m| split(m))
        .filter(|(dir, _)| DEFAULT.contains(&first_of(dir).as_str()))
        .map(|(_, name)| name)
        .collect();
    // Candidate folders in order of first appearance, with their file names.
    let mut candidates: Vec<(String, Vec<String>)> = Vec::new();
    for module in modules
        .iter()
        .filter(|m| m.to_lowercase().ends_with(".prx"))
    {
        let (dir, name) = split(module);
        let first = first_of(&dir);
        if DEFAULT.contains(&first.as_str()) || first == "fakelib" || dir == "." {
            continue;
        }
        match candidates.iter_mut().find(|(d, _)| *d == dir) {
            Some((_, names)) => names.push(name),
            None => candidates.push((dir, vec![name])),
        }
    }
    let mut directories = vec![".".to_string()];
    for (dir, names) in candidates {
        if names.iter().any(|n| taken.contains(n)) {
            continue;
        }
        taken.extend(names);
        directories.push(dir);
    }
    directories
}

// ---------------------------------------------------------------------------------------------
// Relinker progress

/// Turns the relinker's progress display (`Instruction scan | eboot.bin | 42%`) into one rising
/// fraction of the relink step.
#[derive(Debug, Default)]
pub struct RelinkProgress {
    reached: f64,
}

impl RelinkProgress {
    pub fn new() -> Self {
        Self::default()
    }

    /// The new fraction when `segment` moves the estimate forward.
    pub fn read(&mut self, segment: &str) -> Option<f64> {
        let fields: Vec<&str> = segment
            .split('|')
            .filter(|f| !f.is_empty())
            .map(|f| f.trim())
            .collect();
        if fields.len() < 3 {
            return None;
        }
        let (phase, file, value) = (fields[0], fields[1], fields[2]);
        let percent: f64 = value.split('%').next()?.trim().parse().ok()?;
        let done = (percent / 100.0).clamp(0.0, 1.0);
        let estimate = match (phase, file) {
            ("Instruction scan", "eboot.bin") => 0.2 * done,
            ("Instruction conversion", "eboot.bin") => 0.2 + 0.25 * done,
            ("Syscall scan", "eboot.bin") => 0.45 + 0.1 * done,
            ("PRX conversion", _) => match modules_converted(value) {
                Some((converted, total)) if total > 0.0 => 0.55 + 0.3 * converted / total,
                _ => 0.55,
            },
            ("Output image", _) => 0.85 + 0.1 * done,
            ("Write output", _) => 0.95 + 0.05 * done,
            _ => return None,
        };
        if estimate > self.reached {
            self.reached = estimate;
            Some(estimate)
        } else {
            None
        }
    }
}

/// `100% (3/7)` -> (3, 7).
fn modules_converted(value: &str) -> Option<(f64, f64)> {
    let open = value.find('(')?;
    let slash = value.find('/')?;
    let close = value.find(')')?;
    if !(open < slash && slash < close) {
        return None;
    }
    let converted = value[open + 1..slash].trim().parse().ok()?;
    let total = value[slash + 1..close].trim().parse().ok()?;
    Some((converted, total))
}

// ---------------------------------------------------------------------------------------------
// Control and the pipeline

/// Lets another thread steer a running unlock: stop it (sets the flag and kills the relinker),
/// pause it (freezes the relinker, holds the file work at the next file) and move it between
/// background and foreground.
pub struct UnlockControl {
    cancel: CancelToken,
    process: Mutex<Option<ToolProcess>>,
    paused: Mutex<Pause>,
    resumed: Condvar,
    background: AtomicBool,
}

/// Whether an unlock stands paused, since when, and how long it stood paused before.
#[derive(Default)]
struct Pause {
    since: Option<Instant>,
    before: Duration,
}

impl Default for UnlockControl {
    fn default() -> Self {
        Self {
            cancel: CancelToken::default(),
            process: Mutex::new(None),
            paused: Mutex::new(Pause::default()),
            resumed: Condvar::new(),
            background: AtomicBool::new(false),
        }
    }
}

impl UnlockControl {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Cancels the unlock and ends the relinker if it is running.
    pub fn cancel(&self) {
        self.cancel.cancel();
        if let Some(process) = self.process.lock().expect("process lock").as_ref() {
            process.kill();
        }
        // A paused unlock wakes up to see that it is over.
        self.resumed.notify_all();
    }

    /// Stops the unlock where it is: the relinker is frozen, the file work waits before its next
    /// file. Nothing is lost; [`resume`](Self::resume) goes on.
    pub fn pause(&self) {
        let mut paused = self.paused.lock().expect("pause lock");
        if paused.since.is_some() {
            return;
        }
        paused.since = Some(Instant::now());
        if let Some(process) = self.process.lock().expect("process lock").as_ref() {
            process.pause();
        }
    }

    pub fn resume(&self) {
        let mut paused = self.paused.lock().expect("pause lock");
        let Some(since) = paused.since.take() else {
            return;
        };
        paused.before += since.elapsed();
        if let Some(process) = self.process.lock().expect("process lock").as_ref() {
            process.resume();
        }
        drop(paused);
        self.resumed.notify_all();
    }

    pub fn is_paused(&self) -> bool {
        self.paused.lock().expect("pause lock").since.is_some()
    }

    /// How long the unlock has stood paused so far, a pause that is going on included.
    pub fn paused_for(&self) -> Duration {
        let paused = self.paused.lock().expect("pause lock");
        paused.before + paused.since.map_or(Duration::ZERO, |s| s.elapsed())
    }

    /// Background work takes the least the PC can spare; the player now waiting for it ends that.
    pub fn set_background(&self, background: bool) {
        self.background.store(background, Ordering::Relaxed);
        if let Some(process) = self.process.lock().expect("process lock").as_ref() {
            process.set_priority(Self::priority_of(background));
        }
    }

    pub fn is_background(&self) -> bool {
        self.background.load(Ordering::Relaxed)
    }

    fn priority_of(background: bool) -> Priority {
        if background {
            Priority::Idle
        } else {
            Priority::Low
        }
    }

    fn register(&self, process: &ToolProcess) {
        let paused = self.paused.lock().expect("pause lock");
        *self.process.lock().expect("process lock") = Some(process.clone());
        process.set_priority(Self::priority_of(self.is_background()));
        // A cancel or a pause that arrived before the registration still takes effect.
        if self.cancel.is_cancelled() {
            process.kill();
        } else if paused.since.is_some() {
            process.pause();
        }
    }

    fn unregister(&self) {
        *self.process.lock().expect("process lock") = None;
    }

    /// Waits while the unlock is paused; returns at once when it is not, or when it is cancelled.
    pub fn hold(&self) {
        let mut paused = self.paused.lock().expect("pause lock");
        while paused.since.is_some() && !self.cancel.is_cancelled() {
            paused = self
                .resumed
                .wait_timeout(paused, Duration::from_millis(200))
                .expect("pause lock")
                .0;
        }
    }

    fn check(&self) -> Result<(), UnlockError> {
        self.hold();
        if self.cancel.is_cancelled() {
            Err(UnlockError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Sends coalesced progress and log events for one game.
struct Reporter<'a> {
    game: &'a GameId,
    sink: &'a dyn Fn(UnlockEvent),
    last: Option<(Step, f64)>,
}

impl Reporter<'_> {
    fn progress(&mut self, step: Step, fraction: f64) {
        let fraction = fraction.clamp(0.0, 1.0);
        let send = match self.last {
            Some((last_step, last)) => {
                last_step != step
                    || (fraction - last).abs() >= 0.005
                    || fraction >= 1.0 && last < 1.0
            }
            None => true,
        };
        if send {
            self.last = Some((step, fraction));
            (self.sink)(UnlockEvent::Progress {
                game: self.game.clone(),
                step,
                fraction,
            });
        }
    }

    fn log(&self, line: impl Into<String>) {
        (self.sink)(UnlockEvent::Log {
            game: self.game.clone(),
            line: line.into(),
        });
    }
}

/// Removes a folder when dropped, on every exit path.
struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fsx::remove_tree(&self.0);
    }
}

fn is_elf(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 4];
    fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok()
        && head == *b"\x7fELF"
}

/// An ELF as it is, a re-signed one from its `.esbak`, a SELF container unwrapped.
fn unwrap_one(original: &Path, target: &Path, reporter: &Reporter) -> Result<(), UnlockError> {
    let name = original
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let backup = {
        let mut p = original.as_os_str().to_os_string();
        p.push(".esbak");
        PathBuf::from(p)
    };
    if is_elf(original) {
        fs::copy(original, target).map_err(io_context(format!("copy {name}")))?;
    } else if is_elf(&backup) {
        fs::copy(&backup, target).map_err(io_context(format!("copy {name}.esbak")))?;
        reporter.log(format!("{name}: from .esbak"));
    } else if unwrap::is_container(original) {
        match unwrap::unwrap_file(original, target) {
            Ok(size) => reporter.log(format!("{name}: unwrapped, {size} bytes")),
            Err(source) => return Err(UnlockError::Unwrap { file: name, source }),
        }
    } else {
        return Err(UnlockError::NotAnExecutable { file: name });
    }
    Ok(())
}

/// Times the steps of one unlock without the time it stood paused.
struct Stopwatch<'a> {
    control: &'a UnlockControl,
    seconds: StepSeconds,
    current: Option<(Step, Instant, Duration)>,
}

impl<'a> Stopwatch<'a> {
    fn new(control: &'a UnlockControl) -> Self {
        Self {
            control,
            seconds: [0.0; 5],
            current: None,
        }
    }

    /// Ends the running step and starts `step`.
    fn enter(&mut self, step: Step) {
        self.stop();
        self.current = Some((step, Instant::now(), self.control.paused_for()));
    }

    fn stop(&mut self) {
        if let Some((step, since, paused_then)) = self.current.take() {
            let waited = self.control.paused_for().saturating_sub(paused_then);
            let spent = since.elapsed().saturating_sub(waited);
            self.seconds[step.index()] += spent.as_secs_f32();
        }
    }

    fn line(&self) -> String {
        let [unwrap, relink, copy, assemble, finish] = self.seconds;
        format!(
            "timing: unwrap {unwrap:.1}s, relink {relink:.1}s, layout {copy:.1}s, \
             assemble {assemble:.1}s, finish {finish:.1}s, total {:.1}s",
            self.seconds.iter().sum::<f32>()
        )
    }
}

/// Everything the relinker's output depends on, as a hash: the release, its arguments, the patch
/// text and what the inputs look like on disk (size and time; the files are not read, that
/// would cost seconds). `None` when it cannot be told, which never keeps anything.
fn relink_key(
    request: &UnlockRequest,
    inputs: &[String],
    args: &[std::ffi::OsString],
) -> Option<String> {
    use sha2::{Digest, Sha256};
    fn seen(path: &Path) -> String {
        match fs::metadata(path) {
            Ok(meta) => {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                format!("{}:{modified}", meta.len())
            }
            Err(_) => "-".to_string(),
        }
    }
    let mut hasher = Sha256::new();
    let mut part = |text: &str| {
        hasher.update((text.len() as u64).to_le_bytes());
        hasher.update(text.as_bytes());
    };
    part("relink-key-1");
    part(request.toolkit.version());
    match request.toolkit.digests() {
        Some(digests) => {
            part(&digests.relinker);
            part(&digests.libs);
        }
        // A folder the player adopted has no checksums: the relinker itself is hashed (a few MB).
        None => part(&match fs::read(request.toolkit.relinker()) {
            Ok(bytes) => crate::toolkit::hex(&Sha256::digest(bytes)),
            Err(_) => return None,
        }),
    }
    for arg in args {
        part(&arg.to_string_lossy());
    }
    part(request.patches_toml.as_deref().unwrap_or(""));
    for relative in inputs {
        let path = request
            .folder
            .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        let mut backup = path.clone().into_os_string();
        backup.push(".esbak");
        part(&format!(
            "{relative}|{}|{}",
            seen(&path),
            seen(Path::new(&backup))
        ));
    }
    // The icon and the title data go into the executable.
    for name in ["icon0.png", "param.json", "param.sfo"] {
        part(&format!(
            "sce_sys/{name}|{}",
            seen(&request.folder.join("sce_sys").join(name))
        ));
    }
    Some(crate::toolkit::hex(&hasher.finalize()))
}

/// The relinker's output a copy this unlock replaces still holds, when the key says it is the
/// one this unlock would make again.
struct Reusable {
    copy: PathBuf,
    generated: Vec<String>,
}

fn reusable_output(request: &UnlockRequest, copy: &Path, key: Option<&str>) -> Option<Reusable> {
    // The Mach-O patches change the executable after the relinker; its copy is not the output.
    if cfg!(target_os = "macos") || request.fresh {
        return None;
    }
    let key = key?;
    let record = read_record(copy)?;
    if record.relink_key.as_deref() != Some(key) {
        return None;
    }
    let intact = copy.join(EXECUTABLE).is_file()
        && record
            .generated
            .iter()
            .all(|relative| copy.join("app0").join(relative).is_file());
    intact.then(|| Reusable {
        copy: copy.to_path_buf(),
        generated: record.generated,
    })
}

/// Slack for the relinker's temporary files, the record and the volume's own bookkeeping.
const SPACE_HEADROOM: u64 = 256 * 1024 * 1024;
/// What one hard-linked file costs the volume in directory data.
const LINK_ENTRY_BYTES: u64 = 1024;

/// What an unlock writes to the games folder's volume at its peak.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SpaceNeed {
    /// Summed on-disk size of eboot.bin and the modules: unwrapped into the work folder, then
    /// relinked. An unwrapped binary can be larger than its container, so this is a floor.
    binaries: u64,
    /// Size of eboot.bin: the finished executable is copied into the partial copy while the
    /// work folder still holds it.
    executable: u64,
    dump_bytes: u64,
    dump_files: u64,
    dump_copied: bool,
    libs_bytes: u64,
    libs_files: u64,
    libs_copied: bool,
}

/// A lower-bound estimate, not an exact figure: the work folder holds the unwrapped and the
/// relinked binaries (about twice their on-disk size), the executable is copied once more into
/// the partial copy, and the copy holds every file that is copied and only directory data for
/// the ones that are linked. All of it exists at the same time on the games folder's volume.
/// The headroom covers the rest; a shortfall the estimate misses still ends in the late error.
fn required_bytes(need: &SpaceNeed) -> u64 {
    let part = |bytes: u64, files: u64, copied: bool| {
        if copied {
            bytes
        } else {
            files * LINK_ENTRY_BYTES
        }
    };
    2 * need.binaries
        + need.executable
        + SPACE_HEADROOM
        + part(need.dump_bytes, need.dump_files, need.dump_copied)
        + part(need.libs_bytes, need.libs_files, need.libs_copied)
}

/// Stops an unlock that cannot fit before it writes anything. Skipped (returns the plan only)
/// when the free space is unknown or the dump cannot be read; the later steps then report that.
fn preflight(
    request: &UnlockRequest,
    root: &Path,
    modules: &[String],
    reporter: &Reporter<'_>,
) -> Result<Option<fsx::TreePlan>, UnlockError> {
    let Ok(plan) = fsx::plan_tree(&request.folder, &[STORE_NAME]) else {
        return Ok(None);
    };
    let free = match request.free_space_override {
        Some(simulated) => simulated,
        None => fsx::free_space(root),
    };
    let Some(free) = free else {
        return Ok(Some(plan));
    };
    let size_of = |rel: &str| {
        fs::metadata(
            request
                .folder
                .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)),
        )
        .map_or(0, |m| m.len())
    };
    let executable = size_of("eboot.bin");
    let binaries: u64 = executable + modules.iter().map(|rel| size_of(rel)).sum::<u64>();
    let games = games_dir(root);
    let first_file = |plan: &fsx::TreePlan, base: &Path| {
        plan.entries
            .iter()
            .find(|e| !e.is_dir)
            .map(|e| base.join(&e.relative))
    };
    let probe = |plan: &fsx::TreePlan, base: &Path| -> Result<(), String> {
        if request.link_mode == LinkMode::ForceCopy {
            return Err("copy mode forced".to_string());
        }
        match first_file(plan, base) {
            Some(file) => fsx::link_probe(&file, &games).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    };
    let dump_probe = probe(&plan, &request.folder);
    let libs_plan = fsx::plan_tree(request.toolkit.libs(), &[]).unwrap_or_default();
    let libs_probe = probe(&libs_plan, request.toolkit.libs());
    let need = SpaceNeed {
        binaries,
        executable,
        dump_bytes: plan.bytes,
        dump_files: plan.files,
        dump_copied: dump_probe.is_err(),
        libs_bytes: libs_plan.bytes,
        libs_files: libs_plan.files,
        libs_copied: libs_probe.is_err(),
    };
    if let Err(reason) = &dump_probe {
        reporter.log(format!(
            "files copied, not linked: {reason}; {} GB of game files will be written",
            gb_up(plan.bytes)
        ));
    }
    let needed = required_bytes(&need);
    if free < needed {
        return Err(UnlockError::NotEnoughSpace {
            needed,
            free,
            folder: root.to_path_buf(),
            copying: need.dump_copied,
        });
    }
    Ok(Some(plan))
}

/// Runs the whole pipeline for one game. Emits `Started`, `Progress` and `Log` through `sink`;
/// the caller reports `Finished`. Cancelling through `control` ends it with `Cancelled`;
/// whatever it made (work folder, `.partial` copy) is removed on every exit path.
pub fn run_unlock(
    request: &UnlockRequest,
    control: &UnlockControl,
    sink: &dyn Fn(UnlockEvent),
) -> Result<UnlockRecord, UnlockError> {
    if !request.toolkit.relinker().is_file() {
        return Err(UnlockError::ToolkitMissing);
    }
    #[cfg(target_os = "macos")]
    {
        crate::platform::require_macho(request.toolkit.relinker())
            .map_err(io_context("macOS relinker"))?;
        crate::macos::require_rosetta_for(request.toolkit.relinker())
            .map_err(io_context("Rosetta 2"))?;
    }
    let root = std::path::absolute(&request.root).unwrap_or_else(|_| request.root.clone());
    let name = crate::paths::unlocked_copy_name(&request.title_id, &request.game);
    let copy = games_dir(&root).join(&name);
    let partial = games_dir(&root).join(format!("{name}.partial"));
    let previous = games_dir(&root).join(format!("{name}{PREVIOUS_SUFFIX}"));
    let work = work_dir(&root).join(unique_name());
    let _clean_work = RemoveOnDrop(work.clone());
    let _clean_partial = RemoveOnDrop(partial.clone());

    (sink)(UnlockEvent::Started {
        game: request.game.clone(),
    });
    let mut reporter = Reporter {
        game: &request.game,
        sink,
        last: None,
    };
    let linker = Linker::new(request.link_mode);
    let mut clock = Stopwatch::new(control);

    // 0. Enough room on the games folder's volume, before anything is written.
    let modules = modules_in(&request.folder).map_err(io_context("read the dump"))?;
    let dump_plan = preflight(request, &root, &modules, &reporter)?;

    // What the relinker will be asked, and so whether its last output still fits.
    let mut inputs = vec!["eboot.bin".to_string()];
    inputs.extend(modules.iter().cloned());
    let mut args: Vec<std::ffi::OsString> = Os::current()
        .relinker_args()
        .iter()
        .map(Into::into)
        .collect();
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        args.push("--to-rosetta".into());
    } else if request.to_intel {
        args.push("--to-intel".into());
    }
    for directory in module_directories(&modules) {
        args.push("--module-dir".into());
        args.push(directory.into());
    }
    if crate::recommended::relink_allowed(&request.relink_flags) {
        args.extend(
            Os::current()
                .relink_flags(&request.relink_flags)
                .into_iter()
                .map(std::ffi::OsString::from),
        );
    }
    let key = relink_key(request, &inputs, &args);
    let reuse = reusable_output(request, &copy, key.as_deref());

    let source = work.join("source");
    let out = work.join("out");
    let mut generated = Vec::new();
    if let Some(reuse) = &reuse {
        // Nothing the relinker reads has changed since it made the copy that is being replaced:
        // its output is laid into the new copy as it is.
        clock.enter(Step::Unwrap);
        reporter.log("relinker output kept: nothing it reads has changed");
        reporter.progress(Step::Unwrap, 1.0);
        clock.enter(Step::Relink);
        reporter.progress(Step::Relink, 1.0);
        generated.clone_from(&reuse.generated);
    } else {
        // 1. The executable and every module as plain ELF files, in the dump's layout.
        clock.enter(Step::Unwrap);
        reporter.progress(Step::Unwrap, 0.0);
        fs::create_dir_all(&source).map_err(io_context("create work folder"))?;
        for (index, relative) in inputs.iter().enumerate() {
            control.check()?;
            let original = request
                .folder
                .join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
            let target = source.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(io_context("create work folder"))?;
            }
            unwrap_one(&original, &target, &reporter)?;
            reporter.progress(Step::Unwrap, (index + 1) as f64 / inputs.len() as f64);
        }

        // 2. The relinker turns them into a native executable and guest modules.
        clock.enter(Step::Relink);
        reporter.progress(Step::Relink, 0.0);
        fs::create_dir_all(&out).map_err(io_context("create work folder"))?;
        stage_metadata(&request.folder, &source, &linker, control)?;
        if let Some(toml) = &request.patches_toml {
            let file = work.join("patches.toml");
            fs::write(&file, toml).map_err(io_context("write patches"))?;
            args.push("--patches".into());
            args.push(file.into());
        }
        args.push("eboot.bin".into());
        args.push(out.join(EXECUTABLE).into());
        run_relinker(request, control, &mut reporter, &source, args)?;
        if !out.join(EXECUTABLE).is_file() {
            return Err(UnlockError::NoOutput);
        }
        let guest_modules = out.join("app0");
        if guest_modules.is_dir() {
            let made = fsx::walk(&guest_modules, true, &[])
                .map_err(io_context("read the converted modules"))?;
            generated = made
                .iter()
                .filter(|e| !e.is_dir)
                .map(|e| e.relative.to_string_lossy().replace('\\', "/"))
                .collect();
        }
        reporter.progress(Step::Relink, 1.0);
    }

    // 3. The game files, linked (no extra space on NTFS), with the relinked modules over them.
    control.check()?;
    clock.enter(Step::Copy);
    fsx::remove_tree(&partial).map_err(io_context("remove old partial copy"))?;
    let app0 = partial.join("app0");
    fs::create_dir_all(&app0).map_err(io_context("create the copy"))?;
    fs::create_dir_all(partial.join("libs")).map_err(io_context("create the copy"))?;
    reporter.progress(Step::Copy, 0.0);
    let hold = || control.hold();
    let tree = TreeOpts {
        cancel: control.cancel_token(),
        hold: &hold,
        background: control.is_background(),
    };
    match &dump_plan {
        Some(plan) => {
            fsx::link_tree_planned(&request.folder, plan, &app0, &linker, &tree, &mut |f| {
                reporter.progress(Step::Copy, f)
            })
        }
        None => fsx::link_tree(
            &request.folder,
            &app0,
            &linker,
            &[STORE_NAME],
            &tree,
            &mut |f| reporter.progress(Step::Copy, f),
        ),
    }
    .map_err(tree_error)?;
    if let Some(reuse) = &reuse {
        for relative in &generated {
            let to = app0.join(relative);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(io_context("create the copy"))?;
            }
            lay_over(
                &linker,
                &reuse.copy.join("app0").join(relative),
                &to,
                control,
            )?;
        }
    } else if out.join("app0").is_dir() {
        fsx::link_tree(&out.join("app0"), &app0, &linker, &[], &tree, &mut |_| {})
            .map_err(tree_error)?;
    }
    if let Some(note) = linker.take_note() {
        reporter.log(note);
    }
    let (linked, copied) = linker.counts();
    reporter.log(format!(
        "layout: {linked} files linked, {copied} copied, {} threads",
        fsx::link_threads()
    ));

    // 4. The executable, the libraries and the record.
    control.check()?;
    clock.enter(Step::Assemble);
    reporter.progress(Step::Assemble, 0.0);
    let made = match &reuse {
        Some(reuse) => reuse.copy.join(EXECUTABLE),
        None => out.join(EXECUTABLE),
    };
    if cfg!(target_os = "macos") {
        // The Mach-O patches below change this file.
        fs::copy(&made, partial.join(EXECUTABLE)).map_err(io_context("copy the executable"))?;
    } else {
        // Nothing writes to the executable; sharing it saves copying hundreds of megabytes.
        linker
            .link_or_copy(&made, &partial.join(EXECUTABLE), control.cancel_token())
            .map_err(io_context("copy the executable"))?;
    }
    #[cfg(target_os = "macos")]
    assemble_macos(request, &partial, &reporter)?;
    fsx::link_tree(
        request.toolkit.libs(),
        &partial.join("libs"),
        &linker,
        &[],
        &tree,
        &mut |_| {},
    )
    .map_err(tree_error)?;
    for extra in request.toolkit.extra_runtime() {
        let target = partial
            .join("libs")
            .join(extra.file_name().unwrap_or_default());
        if !target.exists() {
            linker
                .link_or_copy(extra, &target, control.cancel_token())
                .map_err(io_context(format!("copy {}", extra.display())))?;
        }
    }
    for dll in Os::current().runtime_libs() {
        if !partial.join("libs").join(dll).is_file() {
            return Err(UnlockError::Io {
                context: format!("{dll} (MinGW runtime) not found in the toolkit"),
                source: io::Error::from(io::ErrorKind::NotFound),
            });
        }
    }
    reporter.progress(Step::Assemble, 0.6);
    let record = UnlockRecord {
        date: now_utc(),
        source: request.folder.to_string_lossy().into_owned(),
        title_id: request.title_id.clone(),
        toolkit: request.toolkit.version().to_string(),
        relinker_sha256: request.toolkit.digests().map(|d| d.relinker.clone()),
        libs_sha256: request.toolkit.digests().map(|d| d.libs.clone()),
        relink_key: key.filter(|_| !cfg!(target_os = "macos")),
        generated,
    };
    let json = serde_json::to_string_pretty(&record).map_err(|e| UnlockError::Io {
        context: "write the record".into(),
        source: io::Error::other(e),
    })?;
    fs::write(partial.join(RECORD_FILE), json).map_err(io_context("write the record"))?;
    reporter.progress(Step::Assemble, 1.0);

    // 5. The new copy takes the old one's place, keeping what the game wrote there.
    control.check()?;
    clock.enter(Step::Finish);
    reporter.progress(Step::Finish, 0.0);
    if copy.exists() {
        // The saves are carried over, so this is a second copy, not the only one. Failing to make
        // it is no reason to stop.
        if let Some(backups) = &request.backup_dir {
            match SaveData::new(copy.clone(), backups.clone()).backup(Reason::BeforeUnlock) {
                Ok(_) | Err(SavesError::NothingToSave) => {}
                Err(error) => reporter.log(format!("could not back up the saves: {error}")),
            }
        }
    }
    swap_in(&copy, &partial, &previous, &reporter)?;
    reporter.progress(Step::Finish, 1.0);
    clock.stop();
    reporter.log(clock.line());
    // A kept relinker output says nothing about what a relink costs here.
    if reuse.is_none() {
        (sink)(UnlockEvent::Timings {
            game: request.game.clone(),
            seconds: clock.seconds,
        });
    }
    Ok(record)
}

/// Puts `from` at `to`, replacing what is there (the dump's own file, linked earlier).
fn lay_over(
    linker: &Linker,
    from: &Path,
    to: &Path,
    control: &UnlockControl,
) -> Result<(), UnlockError> {
    fsx::replace_file(to).map_err(io_context(format!("replace {}", to.display())))?;
    linker
        .link_or_copy(from, to, control.cancel_token())
        .map_err(|source| match source.kind() {
            io::ErrorKind::Interrupted if control.is_cancelled() => UnlockError::Cancelled,
            _ => UnlockError::Io {
                context: format!("copy {}", from.display()),
                source,
            },
        })
}

fn unique_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!(
        "{:x}-{:x}-{}",
        nanos,
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(target_os = "macos")]
fn assemble_macos(
    request: &UnlockRequest,
    copy: &Path,
    reporter: &Reporter,
) -> Result<(), UnlockError> {
    use std::os::unix::fs::PermissionsExt;
    let eboot = copy.join(EXECUTABLE);
    fs::set_permissions(&eboot, fs::Permissions::from_mode(0o755))
        .map_err(io_context("make the game executable"))?;
    if let Some(patches) =
        crate::patches::PatchSet::load_macho(&request.title_id, request.toolkit.patches())
    {
        let states = patches
            .apply_macho(&eboot, request.resolution.as_deref())
            .map_err(|e| UnlockError::Io {
                context: "apply Mach-O patches".into(),
                source: io::Error::other(e),
            })?;
        for (name, state) in states {
            reporter.log(format!("{} patch {name}: {state}", request.title_id));
        }
    }
    let vulkan = request.toolkit.root().join("vulkan");
    for name in ["libvulkan.1.dylib", "libMoltenVK.dylib"] {
        fs::copy(vulkan.join(name), copy.join("libs").join(name))
            .map_err(io_context(format!("copy {name}")))?;
    }
    let mut icd: serde_json::Value = serde_json::from_slice(
        &fs::read(vulkan.join("MoltenVK_icd.json"))
            .map_err(io_context("read MoltenVK manifest"))?,
    )
    .map_err(|e| UnlockError::Io {
        context: "read MoltenVK manifest".into(),
        source: io::Error::other(e),
    })?;
    let driver = icd
        .get_mut("ICD")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| UnlockError::Io {
            context: "MoltenVK manifest has no ICD object".into(),
            source: io::Error::from(io::ErrorKind::InvalidData),
        })?;
    driver.insert("library_path".into(), "../../libs/libMoltenVK.dylib".into());
    let icd_dir = copy.join("vulkan/icd.d");
    fs::create_dir_all(&icd_dir).map_err(io_context("create Vulkan manifest folder"))?;
    fs::write(
        icd_dir.join("MoltenVK_icd.json"),
        serde_json::to_vec_pretty(&icd).map_err(|e| UnlockError::Io {
            context: "encode MoltenVK manifest".into(),
            source: io::Error::other(e),
        })?,
    )
    .map_err(io_context("write MoltenVK manifest"))?;
    Ok(())
}

fn now_utc() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .ok()
        .and_then(|t| t.format(&Rfc3339).ok())
        .unwrap_or_default()
}

/// A1/A5: the title's icon and metadata beside the staged executable, so the relinker can
/// embed the icon.
fn stage_metadata(
    dump: &Path,
    source: &Path,
    linker: &Linker,
    control: &UnlockControl,
) -> Result<(), UnlockError> {
    let target_dir = source.join("sce_sys");
    for name in ["icon0.png", "param.json", "param.sfo"] {
        let from = dump.join("sce_sys").join(name);
        if !from.is_file() {
            continue;
        }
        fs::create_dir_all(&target_dir).map_err(io_context("stage metadata"))?;
        linker
            .link_or_copy(&from, &target_dir.join(name), control.cancel_token())
            .map_err(io_context(format!("stage sce_sys/{name}")))?;
    }
    Ok(())
}

fn run_relinker(
    request: &UnlockRequest,
    control: &UnlockControl,
    reporter: &mut Reporter,
    cwd: &Path,
    args: Vec<std::ffi::OsString>,
) -> Result<(), UnlockError> {
    let spec = ToolSpec {
        program: request.toolkit.relinker().to_path_buf(),
        args,
        cwd: Some(cwd.to_path_buf()),
        env: Vec::new(),
        priority: if control.is_background() {
            Priority::Idle
        } else {
            Priority::Low
        },
    };
    let process = ToolProcess::spawn(&spec).map_err(io_context("start the relinker"))?;
    control.register(&process);
    let mut estimate = RelinkProgress::new();
    // Segments and lines both arrive on this thread; one cell keeps the reporter shared.
    let reporter_cell = std::cell::RefCell::new(reporter);
    let result = process.run(
        &mut |segment| {
            if let Some(fraction) = estimate.read(segment) {
                reporter_cell.borrow_mut().progress(Step::Relink, fraction);
            }
        },
        &mut |line| reporter_cell.borrow().log(line.to_string()),
    );
    control.unregister();
    let result = result.map_err(io_context("read the relinker output"))?;
    if control.is_cancelled() || result.killed {
        return Err(UnlockError::Cancelled);
    }
    if result.code != Some(0) {
        return Err(UnlockError::Relinker {
            code: result.code,
            last_line: result.lines.last().cloned().unwrap_or_default(),
        });
    }
    Ok(())
}

/// What the game wrote beside `app0` in the old copy (saves in `_sd`, trophies in `_trophy`, its
/// data and download folders) moves into the new one, so unlocking again keeps the progress.
/// Failing to move anything but `_sd` and `_trophy` is logged, not fatal, and the name is returned
/// in `failed` so the caller does not delete the old copy over it. A failing `_sd` or `_trophy`
/// names what had already moved, so the caller can put it back.
fn carry_over(old: &Path, new: &Path, reporter_log: &dyn LogLine) -> Result<Carried, CarryFailure> {
    carry_over_with(old, new, reporter_log, &fsx::rename_retry)
}

/// `carry_over` with the rename it uses, so a test can make one fail.
fn carry_over_with(
    old: &Path,
    new: &Path,
    reporter_log: &dyn LogLine,
    rename: &dyn Fn(&Path, &Path) -> io::Result<()>,
) -> Result<Carried, CarryFailure> {
    let mut moved = Vec::new();
    let mut failed = Vec::new();
    let Ok(entries) = fs::read_dir(old) else {
        return Ok(Carried { failed });
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let target = new.join(&name);
        if target.exists() {
            continue;
        }
        // A link belongs to the copy that made it.
        if fs::symlink_metadata(entry.path()).is_ok_and(|m| m.file_type().is_symlink()) {
            continue;
        }
        match rename(&entry.path(), &target) {
            Ok(()) => moved.push(name),
            Err(source) => {
                let lower = name.to_string_lossy().to_lowercase();
                if lower == "_sd" || lower == "_trophy" {
                    return Err(CarryFailure {
                        error: UnlockError::Io {
                            context: format!("keep {}", name.to_string_lossy()),
                            source,
                        },
                        moved,
                    });
                }
                reporter_log.log_line(&format!(
                    "could not keep {}: {source}",
                    name.to_string_lossy()
                ));
                failed.push(name);
            }
        }
    }
    Ok(Carried { failed })
}

/// What `carry_over` could not move without failing (not fatal).
#[derive(Debug)]
struct Carried {
    failed: Vec<OsString>,
}

/// Why the old copy in `previous` must stay after a carry-over, if it must: a name that could not
/// be moved, or saves or trophies that were skipped because the new copy already has its own
/// (the game ran there before the swap was settled). Deleting `previous` would destroy them.
fn must_keep(previous: &Path, carried: &Carried) -> Option<String> {
    let mut names: Vec<String> = carried
        .failed
        .iter()
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    if let Ok(entries) = fs::read_dir(previous) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_lowercase();
            if (lower == "_sd" || lower == "_trophy") && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    (!names.is_empty()).then(|| names.join(", "))
}

/// `carry_over` could not keep the saves or the trophies; `moved` is what it had already moved.
#[derive(Debug)]
struct CarryFailure {
    error: UnlockError,
    moved: Vec<OsString>,
}

/// The old copy is set aside under this suffix while the new one takes its place.
const PREVIOUS_SUFFIX: &str = ".previous";

/// A step of the swap that a test can make fail (see `inject_swap_fault`).
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapFault {
    /// Setting the old copy aside.
    SetAside,
    /// Moving the new copy into the old one's place.
    PutInPlace,
    /// Moving `_sd` from the old copy into the new one.
    KeepSaves,
}

thread_local! {
    static SWAP_FAULT: std::cell::Cell<Option<SwapFault>> = const { std::cell::Cell::new(None) };
}

/// Test hook: makes one step of the swap in `run_unlock` fail on this thread, as a handle held by
/// antivirus would. `None` clears it.
#[doc(hidden)]
pub fn inject_swap_fault(fault: Option<SwapFault>) {
    SWAP_FAULT.with(|f| f.set(fault));
}

fn swap_rename(step: SwapFault, from: &Path, to: &Path) -> io::Result<()> {
    let hit = SWAP_FAULT.with(|f| f.get()) == Some(step)
        && (step != SwapFault::KeepSaves
            || from
                .file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("_sd")));
    if hit {
        return Err(io::Error::other("injected failure"));
    }
    fsx::rename_retry(from, to)
}

/// Puts the new copy in the old one's place without ever destroying the old one first: the old
/// copy is renamed to `previous`, the new one takes its name, the old one's saves and trophies
/// move across, and only then does `previous` go. Every failure on the way puts the old copy
/// back as it was.
fn swap_in(
    copy: &Path,
    partial: &Path,
    previous: &Path,
    log: &dyn LogLine,
) -> Result<(), UnlockError> {
    // A cut-off earlier swap comes first: nothing may be lost to this one.
    settle_previous(copy, previous)?;
    if !copy.exists() {
        return swap_rename(SwapFault::PutInPlace, partial, copy)
            .map_err(io_context("put the copy in place"));
    }
    swap_rename(SwapFault::SetAside, copy, previous)
        .map_err(io_context("set the old copy aside"))?;
    if let Err(source) = swap_rename(SwapFault::PutInPlace, partial, copy) {
        if let Err(back) = fsx::rename_retry(previous, copy) {
            log.log_line(&format!("could not put the old copy back: {back}"));
        }
        return Err(UnlockError::Io {
            context: "put the copy in place".into(),
            source,
        });
    }
    let keep = |from: &Path, to: &Path| swap_rename(SwapFault::KeepSaves, from, to);
    let carried = match carry_over_with(previous, copy, log, &keep) {
        Ok(carried) => carried,
        Err(CarryFailure { error, moved }) => {
            undo_swap(copy, previous, &moved, log);
            return Err(error);
        }
    };
    // The new copy is in place. `previous` goes only when it holds nothing the new copy lacks;
    // otherwise it stays and the next `remove_leftovers` or unlock tries again.
    if let Some(names) = must_keep(previous, &carried) {
        log.log_line(&format!(
            "kept the old copy as {}: {names} could not be moved yet",
            previous.display()
        ));
        return Ok(());
    }
    if let Err(error) = fsx::remove_tree_retry(previous) {
        log.log_line(&format!("could not remove the old copy: {error}"));
    }
    Ok(())
}

/// Takes back a swap whose carry-over failed: what moved goes back into `previous`, the new copy
/// goes, `previous` becomes the copy again. If anything cannot go back, the new copy stays (it
/// holds that piece) and the next `remove_leftovers` or unlock finishes the merge.
fn undo_swap(copy: &Path, previous: &Path, moved: &[OsString], log: &dyn LogLine) {
    let mut all_back = true;
    for name in moved {
        if let Err(error) = fsx::rename_retry(&copy.join(name), &previous.join(name)) {
            all_back = false;
            log.log_line(&format!(
                "could not put {} back: {error}",
                name.to_string_lossy()
            ));
        }
    }
    if !all_back {
        return;
    }
    match fsx::remove_tree_retry(copy) {
        Ok(()) => {
            if let Err(error) = fsx::rename_retry(previous, copy) {
                log.log_line(&format!("could not put the old copy back: {error}"));
            }
        }
        Err(error) => log.log_line(&format!("could not remove the new copy: {error}")),
    }
}

/// Settles what a cut-off swap left in `previous`: it becomes the copy when there is none, else
/// whatever the game wrote there moves into the copy first and `previous` goes only after that.
fn settle_previous(copy: &Path, previous: &Path) -> Result<(), UnlockError> {
    if fs::symlink_metadata(previous).is_err() {
        return Ok(());
    }
    if !copy.exists() {
        return fsx::rename_retry(previous, copy).map_err(io_context("restore the old copy"));
    }
    let carried = carry_over(previous, copy, &()).map_err(|failure| failure.error)?;
    if let Some(names) = must_keep(previous, &carried) {
        return Err(UnlockError::Io {
            context: "merge the old copy".into(),
            source: io::Error::other(format!("{names} stays in {}", previous.display())),
        });
    }
    fsx::remove_tree_retry(previous).map_err(io_context("remove the old copy"))
}

/// Where `carry_over` reports what it could not keep.
trait LogLine {
    fn log_line(&self, line: &str);
}

impl LogLine for Reporter<'_> {
    fn log_line(&self, line: &str) {
        self.log(line);
    }
}

impl LogLine for () {
    fn log_line(&self, _line: &str) {}
}

// ---------------------------------------------------------------------------------------------
// The queue

struct Running {
    game: GameId,
    control: Arc<UnlockControl>,
    background: bool,
}

struct QueueState {
    /// Waiting games, in the order they start within their kind (asked-for first).
    queue: VecDeque<UnlockRequest>,
    running: Vec<Running>,
    shutdown: bool,
    /// The PC is busy: background unlocks stand still until it is not.
    hold_background: bool,
}

struct Shared {
    state: Mutex<QueueState>,
    sink: EventSink<UnlockEvent>,
    threads: Mutex<Vec<thread::JoinHandle<()>>>,
}

/// Unlocks games, one at a time and in the order they were asked for.
///
/// A game the player asked for runs before any game the launcher only prepares on its own
/// (`UnlockRequest::background`). Prepared games wait for an idle PC ([`hold_background`]),
/// stand still while an asked-for game unlocks, and keep what they did: the relinker is frozen,
/// not killed. [`promote`] turns a prepared game into an asked-for one.
///
/// [`hold_background`]: UnlockQueue::hold_background
/// [`promote`]: UnlockQueue::promote
pub struct UnlockQueue {
    shared: Arc<Shared>,
}

impl UnlockQueue {
    pub fn new(sink: EventSink<UnlockEvent>) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(QueueState {
                    queue: VecDeque::new(),
                    running: Vec::new(),
                    shutdown: false,
                    hold_background: false,
                }),
                sink,
                threads: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Queues a game. Returns false when it is already queued or running. The event sink runs
    /// on the calling thread here and on the worker thread otherwise; it must not call back into
    /// the queue.
    pub fn enqueue(&self, request: UnlockRequest) -> bool {
        let mut state = self.shared.state.lock().expect("queue lock");
        let busy = state.queue.iter().any(|r| r.game == request.game)
            || state.running.iter().any(|r| r.game == request.game);
        if busy || state.shutdown {
            return false;
        }
        (self.shared.sink)(UnlockEvent::Queued {
            game: request.game.clone(),
        });
        if request.background {
            state.queue.push_back(request);
        } else {
            // Behind the games the player asked for before, in front of the prepared ones.
            let at = state
                .queue
                .iter()
                .position(|r| r.background)
                .unwrap_or(state.queue.len());
            state.queue.insert(at, request);
        }
        pump(&self.shared, &mut state);
        true
    }

    /// The player now waits for a game the launcher was only preparing: it goes to the front,
    /// takes the machine it needs and shows its progress. False when the game is not queued or
    /// running, or was asked for already.
    pub fn promote(&self, game: &GameId) -> bool {
        let mut state = self.shared.state.lock().expect("queue lock");
        let mut changed = false;
        if let Some(index) = state.queue.iter().position(|r| r.game == *game)
            && let Some(mut request) = state.queue.remove(index)
        {
            changed = request.background;
            request.background = false;
            state.queue.push_front(request);
        } else if let Some(running) = state.running.iter_mut().find(|r| r.game == *game) {
            changed = running.background;
            running.background = false;
            running.control.set_background(false);
        }
        pump(&self.shared, &mut state);
        changed
    }

    /// Prepared games wait while the PC is busy (a game runs, the battery is in use, memory is
    /// short) and go on when it is not. Games the player asked for ignore this.
    pub fn hold_background(&self, hold: bool) {
        let mut state = self.shared.state.lock().expect("queue lock");
        if state.hold_background != hold {
            state.hold_background = hold;
            pump(&self.shared, &mut state);
        }
    }

    /// Removes a queued game (it finishes with `Cancelled`) or stops the running one.
    pub fn cancel(&self, game: &GameId) {
        let mut state = self.shared.state.lock().expect("queue lock");
        if let Some(index) = state.queue.iter().position(|r| r.game == *game) {
            state.queue.remove(index);
            drop(state);
            (self.shared.sink)(UnlockEvent::Finished {
                game: game.clone(),
                result: Err(UnlockError::Cancelled),
            });
        } else if let Some(running) = state.running.iter().find(|r| r.game == *game) {
            running.control.cancel();
        }
    }

    /// Nothing queued and nothing running.
    /// A game the launcher prepares on its own is running (or stands still half done).
    pub fn background_active(&self) -> bool {
        let state = self.shared.state.lock().expect("queue lock");
        state.running.iter().any(|r| r.background)
    }

    pub fn is_idle(&self) -> bool {
        let state = self.shared.state.lock().expect("queue lock");
        state.queue.is_empty() && state.running.is_empty()
    }

    /// Position of a queued game (0 = next); `None` when it is not waiting.
    pub fn position(&self, game: &GameId) -> Option<usize> {
        let state = self.shared.state.lock().expect("queue lock");
        state.queue.iter().position(|r| r.game == *game)
    }

    /// Whether a game's unlock stands still at the moment.
    pub fn is_paused(&self, game: &GameId) -> bool {
        let state = self.shared.state.lock().expect("queue lock");
        state
            .running
            .iter()
            .any(|r| r.game == *game && r.control.is_paused())
    }
}

impl Drop for UnlockQueue {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().expect("queue lock");
            state.shutdown = true;
            state.queue.clear();
            for running in &state.running {
                running.control.cancel();
            }
        }
        let threads = std::mem::take(&mut *self.shared.threads.lock().expect("threads lock"));
        for handle in threads {
            let _ = handle.join();
        }
    }
}

/// Decides what runs and what stands still. Called with the state locked after every change.
///
/// One game the player asked for runs at a time; a prepared game runs only when none is asked
/// for, the PC is idle and no other prepared game is under way. Prepared games under way stand
/// still whenever one of those is not so.
fn pump(shared: &Arc<Shared>, state: &mut QueueState) {
    if state.shutdown {
        return;
    }
    let asked_running = state.running.iter().any(|r| !r.background);
    let asked_waiting = state.queue.iter().any(|r| !r.background);
    let make_way = asked_running || asked_waiting;
    let stand_still = make_way || state.hold_background;
    // A prepared game stands still for the PC or for an asked-for one; an asked-for game waits
    // only for another asked-for game that started before it (a promoted game, for instance).
    let mut asked_before = false;
    for running in &state.running {
        let still = if running.background {
            stand_still
        } else {
            std::mem::replace(&mut asked_before, true)
        };
        if still && !running.control.is_paused() {
            running.control.pause();
            (shared.sink)(UnlockEvent::Paused {
                game: running.game.clone(),
                paused: true,
            });
        } else if !still && running.control.is_paused() {
            running.control.resume();
            (shared.sink)(UnlockEvent::Paused {
                game: running.game.clone(),
                paused: false,
            });
        }
    }
    let next = if !asked_running && asked_waiting {
        state.queue.iter().position(|r| !r.background)
    } else if !make_way && !state.hold_background && state.running.is_empty() {
        // Only the front entry can be a prepared game here: the asked-for ones are first.
        (!state.queue.is_empty()).then_some(0)
    } else {
        None
    };
    let Some(request) = next.and_then(|index| state.queue.remove(index)) else {
        return;
    };
    let control = Arc::new(UnlockControl::new());
    control.set_background(request.background);
    state.running.push(Running {
        game: request.game.clone(),
        control: control.clone(),
        background: request.background,
    });
    let worker = shared.clone();
    let spawned = thread::Builder::new()
        .name("unlock".into())
        .spawn(move || run_job(&worker, &request, &control));
    let mut threads = shared.threads.lock().expect("threads lock");
    threads.retain(|handle| !handle.is_finished());
    match spawned {
        Ok(handle) => threads.push(handle),
        // No thread, no unlock: the game must not stay "running" forever.
        Err(_) => {
            drop(threads);
            if let Some(failed) = state.running.pop() {
                (shared.sink)(UnlockEvent::Finished {
                    game: failed.game,
                    result: Err(UnlockError::Io {
                        context: "start the unlock".into(),
                        source: io::Error::other("no thread"),
                    }),
                });
            }
        }
    }
}

fn run_job(shared: &Arc<Shared>, request: &UnlockRequest, control: &UnlockControl) {
    let sink = shared.sink.clone();
    let result = run_unlock(request, control, &*sink);
    // Leave `running` before the final event, so a listener that enqueues again succeeds.
    {
        let mut state = shared.state.lock().expect("queue lock");
        state.running.retain(|r| r.game != request.game);
    }
    (shared.sink)(UnlockEvent::Finished {
        game: request.game.clone(),
        result,
    });
    let mut state = shared.state.lock().expect("queue lock");
    pump(shared, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_bytes_counts_work_twice_and_copies_only_what_is_copied() {
        let linked = SpaceNeed {
            binaries: 100,
            executable: 40,
            dump_bytes: 10_000_000,
            dump_files: 50,
            libs_bytes: 5_000,
            libs_files: 2,
            ..SpaceNeed::default()
        };
        assert_eq!(
            required_bytes(&linked),
            240 + SPACE_HEADROOM + 50 * LINK_ENTRY_BYTES + 2 * LINK_ENTRY_BYTES
        );
        let copied = SpaceNeed {
            dump_copied: true,
            libs_copied: true,
            ..linked
        };
        assert_eq!(required_bytes(&copied), 240 + SPACE_HEADROOM + 10_005_000);
    }

    #[test]
    fn not_enough_space_says_needed_and_free() {
        let error = UnlockError::NotEnoughSpace {
            needed: 5 * GIB + GIB / 2 + 1,
            free: GIB + GIB / 2 - 1,
            folder: PathBuf::from("D:/Games"),
            copying: false,
        };
        assert_eq!(error.to_string(), "needs 5.6 GB, 1.4 GB free on D:/Games");
        let copying = UnlockError::NotEnoughSpace {
            needed: 2 * GIB,
            free: GIB,
            folder: PathBuf::from("E:/Games"),
            copying: true,
        };
        let text = copying.to_string();
        assert!(text.starts_with("needs 2.0 GB, 1.0 GB free on E:/Games"));
        assert!(text.contains("cannot hard-link"));
    }

    #[test]
    fn copy_names_follow_the_arm_formula() {
        // macOS formula, kept for reference of the hash.
        let mac = GameId::from_path(Path::new("/Users/x/Games/Stray"));
        assert_eq!(
            crate::paths::fnv1a64(mac.as_str()),
            crate::paths::fnv1a64("/users/x/games/stray")
        );
        // The path of PORT.md 6.4 for Windows.
        let id = GameId::from_path(Path::new("C:\\Games\\PPSA01234 - Stray"));
        assert_eq!(
            crate::paths::unlocked_copy_name("PPSA01234", &id),
            "PPSA01234-d356b9"
        );
        assert_eq!(
            crate::paths::fnv1a64("/Users/x/Games/Stray") & 0xFF_FFFF,
            0x6d6bf
        );
        assert_eq!(
            format!("{:x}", crate::paths::fnv1a64("/Users/x/Games/Stray")),
            "f8de232e6e06d6bf"
        );
        assert_eq!(crate::paths::fnv1a64("c:/games") & 0xFF_FFFF, 0x602bcc);
    }

    #[test]
    fn copy_names_drop_symbols_and_do_not_pad() {
        let id = GameId::from_path(Path::new("c:/g"));
        let name = crate::paths::unlocked_copy_name("PP-SA 0.1", &id);
        assert!(name.starts_with("PPSA01-"));
        // Find an id whose hash tail is short: brute force a few.
        let short = (0..200_000)
            .map(|i| GameId::from_path(Path::new(&format!("c:/games/{i}"))))
            .find(|g| crate::paths::fnv1a64(g.as_str()) & 0xFF_FFFF < 0x10_0000)
            .expect("one in sixteen");
        let tail = crate::paths::unlocked_copy_name("X", &short);
        assert!(tail.trim_start_matches("X-").len() < 6, "{tail}");
    }

    #[test]
    fn module_discovery_rules() {
        let dir = tempfile::tempdir().unwrap();
        let touch = |rel: &str| {
            let p = dir.path().join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
        };
        touch("eboot.bin");
        touch("a.prx");
        touch("sce_module/b.prx");
        touch("sce_module/c.SPRX");
        touch("Media/Plugins/d.prx");
        touch("Media/Modules/e.sprx");
        touch("deep/er/est/f.prx"); // four components: too deep
        touch("deep/er/g.prx"); // three components: fine
        touch("sce_sys/h.prx"); // skipped
        touch(".hidden/i.prx"); // skipped
        touch("UPPER.PRX");
        touch("fakelib/libSceFoo.prx");
        let modules = modules_in(dir.path()).unwrap();
        assert_eq!(
            modules,
            [
                "Media/Modules/e.sprx",
                "Media/Plugins/d.prx",
                "UPPER.PRX",
                "a.prx",
                "deep/er/g.prx",
                "fakelib/libSceFoo.prx",
                "sce_module/b.prx",
                "sce_module/c.SPRX",
            ]
        );
        assert_eq!(
            module_directories(&modules),
            [".", "Media/Plugins", "deep/er"]
        );
    }

    #[test]
    fn module_directories_cases() {
        let list =
            |m: &[&str]| module_directories(&m.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(list(&["x.sprx", "Media/Modules/y.sprx"]), ["."]);
        assert_eq!(
            list(&["Media/Plugins/a.prx", "Media/Plugins/b.prx"]),
            [".", "Media/Plugins"]
        );
        assert_eq!(
            list(&[
                "sce_module/x.prx",
                "prx/x.prx",
                "fakelib/x.prx",
                "SCE_MODULES/y.prx"
            ]),
            ["."]
        );
        assert_eq!(list(&["fakelib/deep/x.prx"]), ["."]);
        assert_eq!(list(&["a.PRX"]), ["."]);
        // A second copy of the default folders is not added again.
        assert_eq!(
            list(&[
                "decrypted/prx/a.prx",
                "decrypted/sce_module/libc.prx",
                "prx/a.prx",
                "sce_module/libc.prx"
            ]),
            ["."]
        );
        // Two plugin folders with the same names: the first one wins.
        assert_eq!(
            list(&["Media/A/x.prx", "Media/B/x.prx", "Media/C/y.prx"]),
            [".", "Media/A", "Media/C"]
        );
    }

    #[test]
    fn relink_progress_rows() {
        let mut p = RelinkProgress::new();
        let near = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-9;
        assert!(near(
            p.read("Instruction scan       | eboot.bin  |  42%"),
            0.084
        ));
        assert_eq!(
            p.read("Instruction scan       | eboot.bin  |  10%"),
            None,
            "monotone"
        );
        assert!(near(
            p.read("Instruction conversion | eboot.bin  | 100%"),
            0.45
        ));
        assert!(near(
            p.read("Syscall scan           | eboot.bin  |  50%"),
            0.5
        ));
        assert!(near(
            p.read("PRX conversion         | libc.prx   | 100% (3/7)"),
            0.55 + 0.3 * 3.0 / 7.0
        ));
        assert_eq!(
            p.read("PRX conversion         | libc.prx   | 100% (2/7)"),
            None
        );
        assert!(near(
            p.read("Output image           | eboot.bin  |  50%"),
            0.9
        ));
        assert!(near(
            p.read("Write output           | eboot.bin  | 100%"),
            1.0
        ));
        assert_eq!(p.read("Write output           | eboot.bin  | 100%"), None);
    }

    #[test]
    fn relink_progress_ignores_what_it_cannot_read() {
        let mut p = RelinkProgress::new();
        assert_eq!(p.read("Executable             | eboot.bin  | 100%"), None);
        assert_eq!(p.read("Import analysis"), None);
        assert_eq!(p.read("Converting libc.prx"), None);
        assert_eq!(p.read("a | b"), None);
        assert_eq!(p.read("Instruction scan | other.prx | 50%"), None);
        assert_eq!(p.read("Instruction scan | eboot.bin | abc"), None);
        // PRX conversion without a count starts the range.
        assert!((p.read("PRX conversion | libc.prx | 10%").unwrap() - 0.55).abs() < 1e-9);
        // Percent above 100 is clamped.
        assert!((p.read("Output image | eboot.bin | 900%").unwrap() - 0.95).abs() < 1e-9);
    }

    #[test]
    fn record_json_has_sorted_keys() {
        let record = UnlockRecord {
            date: "2026-10-22T09:30:00Z".into(),
            source: "C:\\Games\\X".into(),
            title_id: "PPSA00001".into(),
            toolkit: "0.1.3-abc".into(),
            relinker_sha256: None,
            libs_sha256: None,
            relink_key: None,
            generated: Vec::new(),
        };
        let json = serde_json::to_string_pretty(&record).unwrap();
        assert_eq!(
            json,
            "{\n  \"date\": \"2026-10-22T09:30:00Z\",\n  \"source\": \"C:\\\\Games\\\\X\",\n  \"titleID\": \"PPSA00001\",\n  \"toolkit\": \"0.1.3-abc\"\n}"
        );
    }

    #[test]
    fn carry_over_moves_what_the_game_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        let new = dir.path().join("new");
        for p in [
            "_sd/slot1",
            "_trophy/1_0.txt",
            "app0/a",
            "libs/b",
            "data/c",
            "eboot.exe",
        ] {
            let f = old.join(p);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, p).unwrap();
        }
        fs::create_dir_all(new.join("app0")).unwrap();
        fs::write(new.join("app0/new"), "n").unwrap();
        fs::write(new.join("eboot.exe"), "NEW").unwrap();
        carry_over(&old, &new, &()).unwrap();
        assert_eq!(
            fs::read_to_string(new.join("_sd/slot1")).unwrap(),
            "_sd/slot1"
        );
        assert!(new.join("_trophy/1_0.txt").is_file());
        assert!(new.join("data/c").is_file());
        // What the new copy has stays its own.
        assert_eq!(fs::read_to_string(new.join("eboot.exe")).unwrap(), "NEW");
        assert!(new.join("app0/new").is_file());
        assert!(!new.join("app0/a").exists());
        // `libs` exists only in the old copy and is not a link: it moves too, harmlessly.
        assert!(old.join("app0/a").is_file());
    }

    #[test]
    fn read_record_needs_the_executable_and_accepts_the_legacy_name() {
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path();
        let json = r#"{"date":"2026-10-22T09:30:00Z","source":"s","titleID":"T","toolkit":"v"}"#;
        fs::write(copy.join(LEGACY_RECORD_FILE), json).unwrap();
        assert!(read_record(copy).is_none());
        fs::write(copy.join(EXECUTABLE), b"x").unwrap();
        assert_eq!(read_record(copy).unwrap().title_id, "T");
        fs::write(copy.join(RECORD_FILE), "not json").unwrap();
        // A broken record under the new name falls back to the legacy file.
        assert_eq!(read_record(copy).unwrap().title_id, "T");
        fs::remove_file(copy.join(LEGACY_RECORD_FILE)).unwrap();
        assert!(read_record(copy).is_none());
    }

    #[test]
    fn leftovers_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(work_dir(root).join("abc")).unwrap();
        fs::create_dir_all(games_dir(root).join("X-1.partial")).unwrap();
        fs::create_dir_all(games_dir(root).join("X-1")).unwrap();
        remove_leftovers(root);
        assert!(!work_dir(root).exists());
        assert!(!games_dir(root).join("X-1.partial").exists());
        assert!(games_dir(root).join("X-1").exists());
    }

    #[test]
    fn carry_over_returns_what_it_moved_and_a_failing_sd_stays_recoverable() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        let new = dir.path().join("new");
        for p in ["_sd/slot1", "_trophy/1_0.txt", "data/c", "eboot.exe"] {
            let f = old.join(p);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, p).unwrap();
        }
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join("eboot.exe"), "NEW").unwrap();
        carry_over(&old, &new, &()).unwrap();
        let mut moved: Vec<OsString> = ["_sd", "_trophy", "data"]
            .into_iter()
            .filter(|n| new.join(n).exists() && !old.join(n).exists())
            .map(OsString::from)
            .collect();
        moved.sort();
        assert_eq!(moved, ["_sd", "_trophy", "data"].map(OsString::from));

        // Move everything back, then fail on `_sd`: the failure names what already moved.
        for name in &moved {
            fs::rename(new.join(name), old.join(name)).unwrap();
        }
        let failing = |from: &Path, to: &Path| {
            if from.file_name().is_some_and(|n| n == "_sd") {
                Err(io::Error::other("held"))
            } else {
                fs::rename(from, to)
            }
        };
        let failure = carry_over_with(&old, &new, &(), &failing).unwrap_err();
        assert!(matches!(&failure.error, UnlockError::Io { context, .. } if context == "keep _sd"));
        assert!(old.join("_sd/slot1").is_file(), "the saves never left");
        for name in &failure.moved {
            fs::rename(new.join(name), old.join(name)).unwrap();
        }
        for p in ["_sd/slot1", "_trophy/1_0.txt", "data/c"] {
            assert!(old.join(p).is_file(), "{p} is back where it was");
        }
        assert!(!new.join("_trophy").exists() && !new.join("data").exists());
    }

    /// An old copy with progress and a new copy beside it, as `swap_in` finds them.
    fn swap_fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let games = games_dir(root);
        let copy = games.join("X-1");
        let partial = games.join("X-1.partial");
        let previous = games.join(format!("X-1{PREVIOUS_SUFFIX}"));
        for (dir, tag) in [(&copy, "OLD"), (&partial, "NEW")] {
            fs::create_dir_all(dir.join("app0")).unwrap();
            fs::write(dir.join(EXECUTABLE), tag).unwrap();
        }
        fs::create_dir_all(copy.join("_sd")).unwrap();
        fs::write(copy.join("_sd/slot0.sav"), "SAVE").unwrap();
        fs::create_dir_all(copy.join("_trophy")).unwrap();
        fs::write(copy.join("_trophy/1_0.txt"), "T").unwrap();
        fs::create_dir_all(copy.join("data")).unwrap();
        fs::write(copy.join("data/c"), "C").unwrap();
        (copy, partial, previous)
    }

    fn assert_old_copy_whole(copy: &Path) {
        assert_eq!(fs::read_to_string(copy.join(EXECUTABLE)).unwrap(), "OLD");
        assert_eq!(
            fs::read_to_string(copy.join("_sd/slot0.sav")).unwrap(),
            "SAVE"
        );
        assert!(copy.join("_trophy/1_0.txt").is_file());
        assert!(copy.join("data/c").is_file());
    }

    struct ClearFault;
    impl Drop for ClearFault {
        fn drop(&mut self) {
            inject_swap_fault(None);
        }
    }

    #[test]
    fn the_swap_keeps_saves_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let (copy, partial, previous) = swap_fixture(dir.path());
        swap_in(&copy, &partial, &previous, &()).unwrap();
        assert_eq!(fs::read_to_string(copy.join(EXECUTABLE)).unwrap(), "NEW");
        assert_eq!(
            fs::read_to_string(copy.join("_sd/slot0.sav")).unwrap(),
            "SAVE"
        );
        assert!(copy.join("_trophy/1_0.txt").is_file());
        assert!(!partial.exists() && !previous.exists());
    }

    #[test]
    fn every_failing_swap_step_leaves_the_old_copy_whole() {
        for fault in [
            SwapFault::SetAside,
            SwapFault::PutInPlace,
            SwapFault::KeepSaves,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let (copy, partial, previous) = swap_fixture(dir.path());
            let _clear = ClearFault;
            inject_swap_fault(Some(fault));
            assert!(
                swap_in(&copy, &partial, &previous, &()).is_err(),
                "{fault:?}"
            );
            assert_old_copy_whole(&copy);
            assert!(!previous.exists(), "{fault:?}: nothing set aside is left");
        }
    }

    #[test]
    fn leftovers_put_the_previous_copy_back_or_carry_its_saves_over() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Cut off after the old copy was set aside: no copy, the previous one comes back.
        let (copy, partial, previous) = swap_fixture(root);
        fs::remove_dir_all(&partial).unwrap();
        fs::rename(&copy, &previous).unwrap();
        remove_leftovers(root);
        assert_old_copy_whole(&copy);
        assert!(!previous.exists());

        // Cut off while the saves moved: the new copy has some, the previous one the rest.
        let (copy, partial, previous) = swap_fixture(root);
        fs::remove_dir_all(&copy).unwrap();
        fs::rename(&partial, &copy).unwrap();
        fs::create_dir_all(previous.join("_sd")).unwrap();
        fs::write(previous.join("_sd/slot0.sav"), "SAVE").unwrap();
        fs::create_dir_all(copy.join("_trophy")).unwrap();
        remove_leftovers(root);
        assert_eq!(fs::read_to_string(copy.join(EXECUTABLE)).unwrap(), "NEW");
        assert_eq!(
            fs::read_to_string(copy.join("_sd/slot0.sav")).unwrap(),
            "SAVE"
        );
        assert!(!previous.exists());
    }

    #[test]
    fn settling_keeps_the_previous_copy_when_the_new_one_already_has_its_own_saves() {
        let dir = tempfile::tempdir().unwrap();
        let (copy, partial, previous) = swap_fixture(dir.path());
        // A cut-off swap: the new copy is in place, the game ran there and made its own `_sd`.
        fs::remove_dir_all(&copy).unwrap();
        fs::rename(&partial, &copy).unwrap();
        fs::create_dir_all(copy.join("_sd")).unwrap();
        fs::write(copy.join("_sd/slot9.sav"), "FRESH").unwrap();
        fs::create_dir_all(previous.join("_sd")).unwrap();
        fs::write(previous.join("_sd/slot0.sav"), "SAVE").unwrap();
        fs::create_dir_all(previous.join("data")).unwrap();
        fs::write(previous.join("data/c"), "C").unwrap();

        assert!(settle_previous(&copy, &previous).is_err());
        assert_eq!(
            fs::read_to_string(previous.join("_sd/slot0.sav")).unwrap(),
            "SAVE",
            "the old saves are not deleted unmerged"
        );
        assert!(copy.join("data/c").is_file(), "what could move did move");
        assert!(copy.join("_sd/slot9.sav").is_file());
        // remove_leftovers swallows the error and leaves the saves for the next try.
        remove_leftovers(dir.path());
        assert!(previous.join("_sd/slot0.sav").is_file());
    }

    #[test]
    fn a_name_that_could_not_move_keeps_the_previous_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (copy, partial, previous) = swap_fixture(dir.path());
        fs::rename(&copy, &previous).unwrap();
        fs::rename(&partial, &copy).unwrap();
        let keep = |from: &Path, to: &Path| {
            if from.file_name().is_some_and(|n| n == "data") {
                Err(io::Error::other("held"))
            } else {
                fs::rename(from, to)
            }
        };
        let carried = carry_over_with(&previous, &copy, &(), &keep).unwrap();
        assert_eq!(carried.failed, [OsString::from("data")]);
        assert_eq!(must_keep(&previous, &carried).as_deref(), Some("data"));
        assert!(previous.join("data/c").is_file());
        // Once nothing is in the way, the next settle merges and deletes.
        settle_previous(&copy, &previous).unwrap();
        assert!(copy.join("data/c").is_file() && !previous.exists());
    }
}
