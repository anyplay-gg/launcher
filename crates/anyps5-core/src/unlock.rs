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
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

use serde::{Deserialize, Serialize};

use crate::error::{CancelToken, EventSink};
use crate::fsx::{self, LinkMode, Linker, TreeError};
use crate::paths::GameId;
use crate::platform::Os;
use crate::process::{ToolProcess, ToolSpec};
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
    /// Test hook: pretend the games folder's volume has this many bytes free.
    pub free_space_override: Option<u64>,
    /// Where to back up the saves of the copy this unlock replaces, just before it is replaced
    /// (see [`crate::saves`]); `None` makes no backup.
    pub backup_dir: Option<PathBuf>,
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

/// Lets another thread stop a running unlock: sets the flag and kills the relinker.
#[derive(Default)]
pub struct UnlockControl {
    cancel: CancelToken,
    process: Mutex<Option<ToolProcess>>,
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
    }

    fn register(&self, process: &ToolProcess) {
        *self.process.lock().expect("process lock") = Some(process.clone());
        // A cancel that arrived before the registration still takes effect.
        if self.cancel.is_cancelled() {
            process.kill();
        }
    }

    fn unregister(&self) {
        *self.process.lock().expect("process lock") = None;
    }

    fn check(&self) -> Result<(), UnlockError> {
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

/// Slack for the relinker's temporary files, the record and the volume's own bookkeeping.
const SPACE_HEADROOM: u64 = 256 * 1024 * 1024;
/// What one hard-linked file costs the volume in directory data.
const LINK_ENTRY_BYTES: u64 = 1024;

/// What an unlock writes to the games folder's volume at its peak.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SpaceNeed {
    /// Summed size of eboot.bin and the modules: unwrapped into the work folder, then relinked.
    binaries: u64,
    dump_bytes: u64,
    dump_files: u64,
    dump_copied: bool,
    libs_bytes: u64,
    libs_files: u64,
    libs_copied: bool,
}

/// The work folder holds the unwrapped and the relinked binaries (about twice their size); the
/// copy holds every file that is copied, and only directory data for the ones that are linked.
/// Both exist at the same time on the games folder's volume.
fn required_bytes(need: &SpaceNeed) -> u64 {
    let part = |bytes: u64, files: u64, copied: bool| {
        if copied {
            bytes
        } else {
            files * LINK_ENTRY_BYTES
        }
    };
    2 * need.binaries
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
    let free = request
        .free_space_override
        .or_else(|| fsx::free_space(root));
    let Some(free) = free else {
        return Ok(Some(plan));
    };
    let binaries: u64 = std::iter::once("eboot.bin")
        .chain(modules.iter().map(String::as_str))
        .map(|rel| {
            fs::metadata(
                request
                    .folder
                    .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)),
            )
            .map_or(0, |m| m.len())
        })
        .sum();
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
    let mut linker = Linker::new(request.link_mode);

    // 0. Enough room on the games folder's volume, before anything is written.
    let modules = modules_in(&request.folder).map_err(io_context("read the dump"))?;
    let dump_plan = preflight(request, &root, &modules, &reporter)?;

    // 1. The executable and every module as plain ELF files, in the dump's layout.
    reporter.progress(Step::Unwrap, 0.0);
    let source = work.join("source");
    fs::create_dir_all(&source).map_err(io_context("create work folder"))?;
    let mut inputs = vec!["eboot.bin".to_string()];
    inputs.extend(modules.iter().cloned());
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
    reporter.progress(Step::Relink, 0.0);
    let out = work.join("out");
    fs::create_dir_all(&out).map_err(io_context("create work folder"))?;
    stage_metadata(&request.folder, &source, &mut linker, control)?;
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
    reporter.progress(Step::Relink, 1.0);

    // 3. The game files, linked (no extra space on NTFS), with the relinked modules over them.
    control.check()?;
    fsx::remove_tree(&partial).map_err(io_context("remove old partial copy"))?;
    let app0 = partial.join("app0");
    fs::create_dir_all(&app0).map_err(io_context("create the copy"))?;
    fs::create_dir_all(partial.join("libs")).map_err(io_context("create the copy"))?;
    reporter.progress(Step::Copy, 0.0);
    match &dump_plan {
        Some(plan) => fsx::link_tree_planned(
            &request.folder,
            plan,
            &app0,
            &mut linker,
            control.cancel_token(),
            &mut |f| reporter.progress(Step::Copy, f),
        ),
        None => fsx::link_tree(
            &request.folder,
            &app0,
            &mut linker,
            &[STORE_NAME],
            control.cancel_token(),
            &mut |f| reporter.progress(Step::Copy, f),
        ),
    }
    .map_err(tree_error)?;
    let guest_modules = out.join("app0");
    if guest_modules.is_dir() {
        fsx::link_tree(
            &guest_modules,
            &app0,
            &mut linker,
            &[],
            control.cancel_token(),
            &mut |_| {},
        )
        .map_err(tree_error)?;
    }
    if let Some(note) = linker.take_note() {
        reporter.log(note);
    }

    // 4. The executable, the libraries and the record.
    control.check()?;
    reporter.progress(Step::Assemble, 0.0);
    fs::copy(out.join(EXECUTABLE), partial.join(EXECUTABLE))
        .map_err(io_context("copy the executable"))?;
    #[cfg(target_os = "macos")]
    assemble_macos(request, &partial, &reporter)?;
    fsx::link_tree(
        request.toolkit.libs(),
        &partial.join("libs"),
        &mut linker,
        &[],
        control.cancel_token(),
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
    };
    let json = serde_json::to_string_pretty(&record).map_err(|e| UnlockError::Io {
        context: "write the record".into(),
        source: io::Error::other(e),
    })?;
    fs::write(partial.join(RECORD_FILE), json).map_err(io_context("write the record"))?;
    reporter.progress(Step::Assemble, 1.0);

    // 5. The new copy takes the old one's place, keeping what the game wrote there.
    control.check()?;
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
    Ok(record)
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
    linker: &mut Linker,
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

struct QueueState {
    queue: VecDeque<UnlockRequest>,
    running: Option<(GameId, Arc<UnlockControl>)>,
    shutdown: bool,
}

struct Shared {
    state: Mutex<QueueState>,
    wake: Condvar,
    sink: EventSink<UnlockEvent>,
}

/// Unlocks one game at a time, in the order they were queued.
pub struct UnlockQueue {
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl UnlockQueue {
    pub fn new(sink: EventSink<UnlockEvent>) -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(QueueState {
                    queue: VecDeque::new(),
                    running: None,
                    shutdown: false,
                }),
                wake: Condvar::new(),
                sink,
            }),
            worker: Mutex::new(None),
        }
    }

    /// Queues a game. Returns false when it is already queued or running. The event sink runs
    /// on the calling thread here and on the worker thread otherwise; it must not call back into
    /// the queue.
    pub fn enqueue(&self, request: UnlockRequest) -> bool {
        {
            let mut state = self.shared.state.lock().expect("queue lock");
            let busy = state.queue.iter().any(|r| r.game == request.game)
                || state
                    .running
                    .as_ref()
                    .is_some_and(|(g, _)| *g == request.game);
            if busy || state.shutdown {
                return false;
            }
            (self.shared.sink)(UnlockEvent::Queued {
                game: request.game.clone(),
            });
            state.queue.push_back(request);
        }
        self.shared.wake.notify_all();
        let mut worker = self.worker.lock().expect("worker lock");
        if worker.is_none() {
            let shared = self.shared.clone();
            *worker = thread::Builder::new()
                .name("unlock".into())
                .spawn(move || worker_loop(shared))
                .ok();
        }
        true
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
        } else if let Some((running, control)) = &state.running
            && running == game
        {
            control.cancel();
        }
    }

    /// Nothing queued and nothing running.
    pub fn is_idle(&self) -> bool {
        let state = self.shared.state.lock().expect("queue lock");
        state.queue.is_empty() && state.running.is_none()
    }

    /// Position of a queued game (0 = next); `None` when it is not waiting.
    pub fn position(&self, game: &GameId) -> Option<usize> {
        let state = self.shared.state.lock().expect("queue lock");
        state.queue.iter().position(|r| r.game == *game)
    }
}

impl Drop for UnlockQueue {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().expect("queue lock");
            state.shutdown = true;
            state.queue.clear();
            if let Some((_, control)) = &state.running {
                control.cancel();
            }
        }
        self.shared.wake.notify_all();
        if let Some(handle) = self.worker.lock().expect("worker lock").take() {
            let _ = handle.join();
        }
    }
}

fn worker_loop(shared: Arc<Shared>) {
    loop {
        let (request, control) = {
            let mut state = shared.state.lock().expect("queue lock");
            loop {
                if state.shutdown {
                    return;
                }
                if let Some(request) = state.queue.pop_front() {
                    let control = Arc::new(UnlockControl::new());
                    state.running = Some((request.game.clone(), control.clone()));
                    break (request, control);
                }
                state = shared.wake.wait(state).expect("queue lock");
            }
        };
        let sink = shared.sink.clone();
        let result = run_unlock(&request, &control, &*sink);
        // Clear `running` before the final event, so a listener that enqueues again succeeds.
        shared.state.lock().expect("queue lock").running = None;
        (shared.sink)(UnlockEvent::Finished {
            game: request.game.clone(),
            result,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_bytes_counts_work_twice_and_copies_only_what_is_copied() {
        let linked = SpaceNeed {
            binaries: 100,
            dump_bytes: 10_000_000,
            dump_files: 50,
            libs_bytes: 5_000,
            libs_files: 2,
            ..SpaceNeed::default()
        };
        assert_eq!(
            required_bytes(&linked),
            200 + SPACE_HEADROOM + 50 * LINK_ENTRY_BYTES + 2 * LINK_ENTRY_BYTES
        );
        let copied = SpaceNeed {
            dump_copied: true,
            libs_copied: true,
            ..linked
        };
        assert_eq!(required_bytes(&copied), 200 + SPACE_HEADROOM + 10_005_000);
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
