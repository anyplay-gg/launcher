//! Starting an unlocked game and watching it (PORT.md 6.10, `AppModel.play`): spawn `eboot.exe`
//! in its copy with the settings' environment, keep its output in the log, notice the first
//! window, place it, and report how the process ended and why.
//!
//! Interface: [`Launcher::start`] with a [`LaunchRequest`] returns a [`RunHandle`]; progress
//! arrives as [`LaunchEvent`]s through the sink. The game and its children live in a job object,
//! so [`RunHandle::kill`] ends all of them and "the game has ended" means the job is empty.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::diagnosis::{Diagnosis, ExitInfo};
use crate::error::EventSink;
use crate::paths::GameId;
use crate::unlock::EXECUTABLE;

/// Linux has no portable way to find another process's window (X11 and Wayland differ, and Wayland
/// offers none), so a game that has run this long counts as shown.
#[cfg(not(any(windows, target_os = "macos")))]
const LINUX_WINDOW_GRACE: Duration = Duration::from_secs(6);
/// The ring buffer of the game's latest output kept in memory.
pub const TAIL_BYTES: usize = 400 * 1024;
/// A log above this size is cut to its last [`KEEP_AFTER_TRUNCATE`] bytes at the next start.
const LOG_LIMIT: u64 = 50 * 1024 * 1024;
const KEEP_AFTER_TRUNCATE: u64 = 4 * 1024 * 1024;

/// Where the game's window goes once it shows (D9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowPrefs {
    pub fullscreen: bool,
    /// Percent of the monitor's work area, 30..=100. Ignored when fullscreen.
    pub size_percent: u32,
}

impl Default for WindowPrefs {
    fn default() -> Self {
        Self {
            fullscreen: false,
            size_percent: 75,
        }
    }
}

#[derive(Clone, Debug)]
pub struct LaunchRequest {
    pub game: GameId,
    pub title_id: String,
    /// The unlocked copy: working directory and home of `eboot.exe`.
    pub copy: PathBuf,
    /// Variables added to the launcher's own environment (`GameSettings::environment`).
    pub env: BTreeMap<String, String>,
    /// Folders to create before the start (screenshots, shader cache); errors are ignored.
    pub create_dirs: Vec<PathBuf>,
    pub log_file: PathBuf,
    pub window: WindowPrefs,
    /// How long to wait for the first window before reporting `forced: true` (240 s).
    pub window_deadline: Duration,
    /// How often to look for the window and for the end (400 ms).
    pub poll: Duration,
}

impl LaunchRequest {
    pub fn new(game: GameId, title_id: String, copy: PathBuf, log_file: PathBuf) -> Self {
        Self {
            game,
            title_id,
            copy,
            env: BTreeMap::new(),
            create_dirs: Vec::new(),
            log_file,
            window: WindowPrefs::default(),
            window_deadline: Duration::from_secs(240),
            poll: Duration::from_millis(400),
        }
    }
}

/// The process to start, without starting it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Added to the inherited environment.
    pub env: BTreeMap<String, String>,
}

pub fn build_command(request: &LaunchRequest) -> CommandSpec {
    let spec = CommandSpec {
        program: request.copy.join(EXECUTABLE),
        args: Vec::new(),
        cwd: request.copy.clone(),
        env: request.env.clone(),
    };
    #[cfg(target_os = "macos")]
    let spec = macos_command(spec, request.window);
    spec
}

#[cfg(target_os = "macos")]
fn macos_command(mut spec: CommandSpec, window: WindowPrefs) -> CommandSpec {
    let driver = spec.cwd.join("vulkan/icd.d/MoltenVK_icd.json");
    // Native test programs need no Vulkan runtime; converted games always carry this manifest.
    if driver.is_file() {
        spec.env
            .entry("VK_DRIVER_FILES".into())
            .or_insert_with(|| driver.to_string_lossy().into_owned());
        spec.env
            .entry("MVK_CONFIG_SYNCHRONOUS_QUEUE_SUBMITS".into())
            .or_insert_with(|| "0".into());
    }
    spec.env.insert(
        "ANYPS5_FULLSCREEN".into(),
        u8::from(window.fullscreen).to_string(),
    );
    spec.env.insert(
        "ANYPS5_WINDOW_SIZE".into(),
        window.size_percent.clamp(30, 100).to_string(),
    );
    spec
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("the game is not unlocked")]
    NotUnlocked,
    #[error("could not start the game: {0}")]
    Spawn(io::Error),
    #[error("could not create the job object: {0}")]
    Job(String),
    #[error("could not write the log: {0}")]
    Log(io::Error),
}

#[derive(Debug)]
pub enum LaunchEvent {
    Spawned {
        game: GameId,
        pid: u32,
    },
    /// The first window is up (placed already), or the deadline passed with the game alive.
    WindowShown {
        game: GameId,
        forced: bool,
    },
    Exited {
        game: GameId,
        info: ExitInfo,
    },
    /// What the log and the exit status say about the end. `without_window`: the game ended
    /// before showing a window and nothing explains it ("the game did not start").
    Diagnosed {
        game: GameId,
        diagnosis: Option<Diagnosis>,
        without_window: bool,
    },
}

/// The line the launcher writes to a game's log before each start.
pub fn start_marker_line(now: &str, title_id: &str) -> String {
    format!("\n{} {now} {title_id}\n", Diagnosis::START_MARKER)
}

/// Opens the log for appending (creating folders and file), cuts a log above 50 MiB to its last
/// 4 MiB first, and writes the start marker.
pub fn open_log_for_start(path: &Path, title_id: &str, now: &str) -> io::Result<fs::File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::metadata(path).is_ok_and(|m| m.len() > LOG_LIMIT) {
        let tail = crate::diagnosis::read_tail(path, KEEP_AFTER_TRUNCATE)?;
        fs::write(path, tail)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(start_marker_line(now, title_id).as_bytes())?;
    file.flush()?;
    Ok(file)
}

fn now_utc() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .ok()
        .and_then(|t| t.format(&Rfc3339).ok())
        .unwrap_or_default()
}

/// The started process and what watches it.
struct Running {
    game: GameId,
    pid: u32,
    child: Mutex<Child>,
    #[cfg(windows)]
    job: Option<crate::win::Job>,
    killed: AtomicBool,
    /// Set when the `RunHandle` is dropped: the watcher ends without events.
    stop: AtomicBool,
    /// The game's window (0: none yet).
    window: AtomicIsize,
    tail: Mutex<VecDeque<u8>>,
}

impl Running {
    fn alive(&self) -> bool {
        #[cfg(windows)]
        if let Some(job) = &self.job {
            return job.active_processes() > 0;
        }
        let alive = matches!(self.child.lock().expect("child lock").try_wait(), Ok(None));
        #[cfg(target_os = "macos")]
        if !alive
            && !self.killed.load(Ordering::SeqCst)
            && let Ok(pid) = libc::pid_t::try_from(self.pid)
        {
            // SAFETY: signal zero only checks the process group. A LoadExec child can
            // outlive its parent; keep watching until the owned group has ended.
            return unsafe { libc::kill(-pid, 0) == 0 };
        }
        alive
    }

    #[cfg(windows)]
    fn pids(&self) -> Vec<u32> {
        match &self.job {
            Some(job) => job.process_ids(),
            None => vec![self.pid],
        }
    }

    #[cfg(not(windows))]
    fn pids(&self) -> Vec<u32> {
        vec![self.pid]
    }

    fn kill(&self) {
        self.killed.store(true, Ordering::SeqCst);
        #[cfg(windows)]
        if let Some(job) = &self.job {
            job.terminate(1);
            return;
        }
        // Elsewhere the game leads a process group of its own: end it with everything it started.
        crate::platform::kill_group(self.pid);
        let _ = self.child.lock().expect("child lock").kill();
    }
}

/// A started game. Dropping it stops the watching, not the game.
pub struct RunHandle {
    running: Arc<Running>,
}

impl RunHandle {
    pub fn game(&self) -> &GameId {
        &self.running.game
    }

    pub fn pid(&self) -> u32 {
        self.running.pid
    }

    /// The game's processes: the one started and, on Windows, what it started since. Share
    /// screen captures the window of these and no other.
    pub fn pids(&self) -> Vec<u32> {
        self.running.pids()
    }

    /// The game's window as an integer (a Windows `HWND`), once it has shown one.
    pub fn window(&self) -> Option<isize> {
        Some(self.running.window.load(Ordering::SeqCst)).filter(|&w| w != 0)
    }

    /// Ends the game and everything it started.
    pub fn kill(&self) {
        self.running.kill();
    }

    /// Whether the game (or a process it started) still runs.
    pub fn is_alive(&self) -> bool {
        self.running.alive()
    }

    /// Brings the game's window to the front, if it has shown one.
    pub fn activate(&self) {
        #[cfg(target_os = "macos")]
        crate::macos::activate(self.running.window.load(Ordering::SeqCst));
        #[cfg(windows)]
        {
            let window = self.running.window.load(Ordering::SeqCst);
            if window != 0 {
                crate::win::activate_window(window);
            }
        }
    }

    /// The last 400 KiB the game wrote, lossily decoded.
    pub fn tail(&self) -> String {
        let tail = self.running.tail.lock().expect("tail lock");
        let bytes: Vec<u8> = tail.iter().copied().collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl Drop for RunHandle {
    fn drop(&mut self) {
        self.running.stop.store(true, Ordering::SeqCst);
    }
}

pub struct Launcher;

impl Launcher {
    /// Starts the game. Writes the start marker, creates the folders, spawns the process and two
    /// threads (`run-pump`, `run-watch`); everything after the spawn is reported as events.
    pub fn start(
        request: LaunchRequest,
        sink: EventSink<LaunchEvent>,
    ) -> Result<RunHandle, LaunchError> {
        let spec = build_command(&request);
        if !spec.program.is_file() {
            return Err(LaunchError::NotUnlocked);
        }
        #[cfg(target_os = "macos")]
        {
            crate::platform::require_macho(&spec.program).map_err(LaunchError::Spawn)?;
            crate::macos::require_rosetta_for(&spec.program).map_err(LaunchError::Spawn)?;
        }
        let mut log = open_log_for_start(&request.log_file, &request.title_id, &now_utc())
            .map_err(LaunchError::Log)?;
        for dir in &request.create_dirs {
            let _ = fs::create_dir_all(dir);
        }

        let (mut reader, writer) = io::pipe().map_err(LaunchError::Spawn)?;
        let mut command = Command::new(&spec.program);
        command
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .envs(&spec.env)
            .stdin(Stdio::null())
            .stdout(writer.try_clone().map_err(LaunchError::Spawn)?)
            .stderr(writer);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
            command.creation_flags(0x0800_0000 | 0x0000_0200);
        }
        crate::platform::isolate_group(&mut command);
        let child = command.spawn().map_err(LaunchError::Spawn)?;
        // The command holds the write end; the pipe only ends when it is gone.
        drop(command);
        let pid = child.id();

        #[cfg(windows)]
        let job = {
            use std::os::windows::io::AsRawHandle;
            let job = crate::win::Job::new(false).map_err(|e| LaunchError::Job(e.to_string()))?;
            match job.assign(child.as_raw_handle()) {
                Ok(()) => Some(job),
                Err(error) => {
                    // The process may already be gone; fall back to watching the child itself.
                    tracing::warn!("could not assign the game to a job: {error}");
                    None
                }
            }
        };
        #[cfg(windows)]
        crate::win::allow_foreground(pid);

        let running = Arc::new(Running {
            game: request.game.clone(),
            pid,
            child: Mutex::new(child),
            #[cfg(windows)]
            job,
            killed: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            window: AtomicIsize::new(0),
            tail: Mutex::new(VecDeque::new()),
        });

        // The pump: every byte to the log, the latest ones to memory.
        let (pumped_tx, pumped_rx) = mpsc::channel::<()>();
        {
            let running = running.clone();
            thread::Builder::new()
                .name("run-pump".into())
                .spawn(move || {
                    let mut buffer = [0u8; 8192];
                    loop {
                        let n = match reader.read(&mut buffer) {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                            Err(_) => break,
                        };
                        let _ = log.write_all(&buffer[..n]);
                        let _ = log.flush();
                        let mut tail = running.tail.lock().expect("tail lock");
                        tail.extend(&buffer[..n]);
                        if tail.len() > TAIL_BYTES {
                            let excess = tail.len() - TAIL_BYTES;
                            tail.drain(..excess);
                        }
                    }
                    let _ = pumped_tx.send(());
                })
                .map_err(LaunchError::Spawn)?;
        }

        (sink)(LaunchEvent::Spawned {
            game: request.game.clone(),
            pid,
        });

        // The watcher: first window, end of the game, diagnosis.
        {
            let running = running.clone();
            thread::Builder::new()
                .name("run-watch".into())
                .spawn(move || watch(running, request, sink, pumped_rx))
                .map_err(LaunchError::Spawn)?;
        }
        Ok(RunHandle { running })
    }
}

fn watch(
    running: Arc<Running>,
    request: LaunchRequest,
    sink: EventSink<LaunchEvent>,
    pumped: mpsc::Receiver<()>,
) {
    let started = Instant::now();
    let mut had_window = false;
    let mut reported = false;
    loop {
        if running.stop.load(Ordering::SeqCst) {
            return;
        }
        if !running.alive() {
            break;
        }
        if !reported {
            #[cfg(windows)]
            let found = crate::win::find_window(&running.pids());
            #[cfg(target_os = "macos")]
            let found = crate::macos::find_window(running.pid);
            #[cfg(not(any(windows, target_os = "macos")))]
            let found: Option<isize> = (started.elapsed() >= LINUX_WINDOW_GRACE).then_some(0);
            if let Some(window) = found {
                running.window.store(window, Ordering::SeqCst);
                had_window = true;
                reported = true;
                #[cfg(windows)]
                if !request.window.fullscreen {
                    crate::win::place_window(window, request.window.size_percent);
                }
                (sink)(LaunchEvent::WindowShown {
                    game: request.game.clone(),
                    forced: false,
                });
                #[cfg(windows)]
                if request.window.fullscreen {
                    // D9: the engine toggles fullscreen on F11; give it a second to settle.
                    if wait_alive(&running, Duration::from_secs(1)) {
                        crate::win::send_f11(window);
                    }
                }
            } else if started.elapsed() >= request.window_deadline {
                reported = true;
                (sink)(LaunchEvent::WindowShown {
                    game: request.game.clone(),
                    forced: true,
                });
            }
        }
        thread::sleep(request.poll);
    }

    // The game has ended. Let the pump drain so the log is complete before anyone reads it.
    let _ = pumped.recv_timeout(Duration::from_secs(2));
    let killed = running.killed.load(Ordering::SeqCst);
    let code = if killed {
        None
    } else {
        running
            .child
            .lock()
            .expect("child lock")
            .try_wait()
            .ok()
            .flatten()
            .and_then(|status| status.code())
            .map(|c| c as u32)
    };
    let info = ExitInfo { code, had_window };
    (sink)(LaunchEvent::Exited {
        game: request.game.clone(),
        info: info.clone(),
    });
    let session = {
        let tail = running.tail.lock().expect("tail lock");
        let bytes: Vec<u8> = tail.iter().copied().collect();
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let diagnosis = Diagnosis::read(&session, Some(&info));
    let without_window = !had_window && !killed && diagnosis.is_none();
    (sink)(LaunchEvent::Diagnosed {
        game: request.game.clone(),
        diagnosis,
        without_window,
    });
}

/// Sleeps up to `total`, returns whether the game is still alive afterwards.
#[cfg(windows)]
fn wait_alive(running: &Running, total: Duration) -> bool {
    let end = Instant::now() + total;
    while Instant::now() < end {
        if !running.alive() || running.stop.load(Ordering::SeqCst) {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
    running.alive()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_eboot_in_its_copy_with_the_settings_environment() {
        let mut request = LaunchRequest::new(
            GameId::from_path(Path::new("c:/games/x")),
            "PPSA00001".into(),
            PathBuf::from("c:/games/.AnyPS5/Games/PPSA00001-abc"),
            PathBuf::from("c:/logs/PPSA00001.log"),
        );
        request
            .env
            .insert("ANYPS5_PRESENT_MODE".into(), "fifo".into());
        let spec = build_command(&request);
        assert_eq!(spec.program, request.copy.join(EXECUTABLE));
        assert_eq!(spec.cwd, request.copy);
        assert!(spec.args.is_empty());
        assert_eq!(spec.env["ANYPS5_PRESENT_MODE"], "fifo");
        assert_eq!(
            request.window,
            WindowPrefs {
                fullscreen: false,
                size_percent: 75
            }
        );
        assert_eq!(request.window_deadline, Duration::from_secs(240));
    }

    #[test]
    fn an_executable_missing_is_not_unlocked() {
        let dir = tempfile::tempdir().unwrap();
        let request = LaunchRequest::new(
            GameId::from_path(Path::new("c:/g")),
            "T".into(),
            dir.path().to_path_buf(),
            dir.path().join("T.log"),
        );
        let result = Launcher::start(request, Arc::new(|_| {}));
        assert!(matches!(result, Err(LaunchError::NotUnlocked)));
        assert!(
            !dir.path().join("T.log").exists(),
            "nothing is written before the checks"
        );
    }

    #[test]
    fn the_start_marker_is_appended_and_a_huge_log_is_cut() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("logs/A.log");
        drop(open_log_for_start(&log, "PPSA1", "2026-10-22T09:30:00Z").unwrap());
        drop(open_log_for_start(&log, "PPSA1", "2026-10-22T09:31:00Z").unwrap());
        let text = fs::read_to_string(&log).unwrap();
        assert_eq!(
            text,
            "\n[anyps5-launcher] start 2026-10-22T09:30:00Z PPSA1\n\n[anyps5-launcher] start 2026-10-22T09:31:00Z PPSA1\n"
        );
        assert_eq!(
            Diagnosis::last_session(&text),
            " 2026-10-22T09:31:00Z PPSA1\n"
        );

        let mut big = vec![b'x'; 51 * 1024 * 1024];
        big.extend(b"THE-END");
        fs::write(&log, big).unwrap();
        drop(open_log_for_start(&log, "PPSA1", "t").unwrap());
        let len = fs::metadata(&log).unwrap().len();
        assert!(len < 5 * 1024 * 1024, "{len}");
        assert!(
            fs::read_to_string(&log)
                .unwrap()
                .contains("THE-END\n[anyps5-launcher] start t PPSA1")
        );
    }
}
