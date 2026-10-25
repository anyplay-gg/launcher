//! Clips: the last seconds of the running game, saved as an MP4 file (docs/DESIGN.md 9.12).
//!
//! The share worker does the recording (network spec/clips.md); this module is the launcher's
//! side of it: the settings' values, the hotkey, the file names, the folder's listing and the
//! worker as a child process with a command and an event channel.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};

use serde::Deserialize;

/// The lengths the player can choose, in seconds; 0 is Off.
pub const LENGTHS: [u32; 5] = [0, 15, 30, 60, 120];

/// The hotkey until the player binds another.
pub const DEFAULT_HOTKEY: &str = "Alt+F12";

/// The length to record for a stored value: one of [`LENGTHS`], else Off.
pub fn length_or_off(seconds: u32) -> u32 {
    if LENGTHS.contains(&seconds) { seconds } else { 0 }
}

// --- the hotkey -------------------------------------------------------------------------------

/// A key with modifiers, stored as text like `Alt+F12` (modifiers in the order Ctrl, Alt, Shift,
/// then the key's name as egui spells it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl Hotkey {
    /// Reads `Ctrl+Alt+F9`; `None` for a key the launcher cannot register.
    pub fn parse(text: &str) -> Option<Hotkey> {
        let mut hotkey = Hotkey {
            ctrl: false,
            alt: false,
            shift: false,
            key: String::new(),
        };
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        let (key, modifiers) = parts.split_last()?;
        for modifier in modifiers {
            match *modifier {
                "Ctrl" => hotkey.ctrl = true,
                "Alt" => hotkey.alt = true,
                "Shift" => hotkey.shift = true,
                _ => return None,
            }
        }
        hotkey.key = (*key).to_owned();
        hotkey.vk()?;
        hotkey.allowed().then_some(hotkey)
    }

    /// Builds one from a key press: `None` when the key cannot be registered or would take over a
    /// key the player types with (a letter or a digit alone).
    pub fn from_press(key: &str, ctrl: bool, alt: bool, shift: bool) -> Option<Hotkey> {
        let hotkey = Hotkey {
            ctrl,
            alt,
            shift,
            key: key.to_owned(),
        };
        (hotkey.vk().is_some() && hotkey.allowed()).then_some(hotkey)
    }

    /// A function key may stand alone; everything else needs Ctrl or Alt, so that binding a
    /// letter cannot take it away from the game.
    fn allowed(&self) -> bool {
        self.ctrl || self.alt || self.is_function_key()
    }

    fn is_function_key(&self) -> bool {
        self.key
            .strip_prefix('F')
            .and_then(|n| n.parse::<u32>().ok())
            .is_some_and(|n| (1..=24).contains(&n))
    }

    /// The Windows virtual-key code of the key.
    pub fn vk(&self) -> Option<u32> {
        if let Some(n) = self
            .key
            .strip_prefix('F')
            .and_then(|n| n.parse::<u32>().ok())
        {
            return (1..=24).contains(&n).then_some(0x6F + n);
        }
        let mut chars = self.key.chars();
        if let (Some(c), None) = (chars.next(), chars.next())
            && (c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            return Some(c as u32);
        }
        Some(match self.key.as_str() {
            "Insert" => 0x2D,
            "Delete" => 0x2E,
            "Home" => 0x24,
            "End" => 0x23,
            "PageUp" => 0x21,
            "PageDown" => 0x22,
            _ => return None,
        })
    }

    /// The parts to show as key caps: `["Alt", "F12"]`.
    pub fn parts(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl".to_owned());
        }
        if self.alt {
            parts.push("Alt".to_owned());
        }
        if self.shift {
            parts.push("Shift".to_owned());
        }
        parts.push(self.key.clone());
        parts
    }
}

impl std::fmt::Display for Hotkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.parts().join("+"))
    }
}

/// The hotkey the preferences hold, else the default.
pub fn hotkey_or_default(stored: Option<&str>) -> Hotkey {
    stored
        .and_then(Hotkey::parse)
        .or_else(|| Hotkey::parse(DEFAULT_HOTKEY))
        .expect("the default hotkey is valid")
}

/// A global hotkey: pressing it anywhere, also in a game that has the keyboard, sends `()`.
/// Windows only; elsewhere [`HotkeyListener::start`] answers that it cannot.
pub struct HotkeyListener {
    pub presses: Receiver<()>,
    #[cfg(windows)]
    thread_id: u32,
}

#[cfg(windows)]
impl HotkeyListener {
    pub fn start(hotkey: &Hotkey, wake: impl Fn() + Send + 'static) -> Result<HotkeyListener, String> {
        use windows::Win32::System::Threading::GetCurrentThreadId;
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey,
            UnregisterHotKey,
        };
        use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

        let vk = hotkey.vk().ok_or("this key cannot be a hotkey")?;
        let mut modifiers = MOD_NOREPEAT;
        for (on, flag) in [
            (hotkey.ctrl, MOD_CONTROL),
            (hotkey.alt, MOD_ALT),
            (hotkey.shift, MOD_SHIFT),
        ] {
            if on {
                modifiers = HOT_KEY_MODIFIERS(modifiers.0 | flag.0);
            }
        }
        let (press_tx, presses) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("clips-hotkey".into())
            .spawn(move || {
                // SAFETY: plain Win32 calls on this thread, which owns the registration and
                // its message queue; `msg` outlives the calls that fill it.
                unsafe {
                    if let Err(error) = RegisterHotKey(None, 1, modifiers, vk) {
                        let _ = ready_tx.send(Err(error.to_string()));
                        return;
                    }
                    let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                        if msg.message == WM_HOTKEY {
                            if press_tx.send(()).is_err() {
                                break;
                            }
                            wake();
                        }
                    }
                    let _ = UnregisterHotKey(None, 1);
                }
            })
            .map_err(|e| e.to_string())?;
        let thread_id = ready_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|e| e.to_string())??;
        Ok(HotkeyListener { presses, thread_id })
    }
}

#[cfg(windows)]
impl Drop for HotkeyListener {
    fn drop(&mut self) {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        // SAFETY: posts a quit message to the listener's own thread.
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

#[cfg(not(windows))]
impl HotkeyListener {
    pub fn start(_hotkey: &Hotkey, _wake: impl Fn() + Send + 'static) -> Result<HotkeyListener, String> {
        Err("a global hotkey is not built for this system".into())
    }
}

// --- files ------------------------------------------------------------------------------------

/// A name for a file that is safe on Windows and Linux: no separators or reserved characters.
fn safe_name(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_matches('.').trim().to_owned();
    if cleaned.is_empty() {
        "Game".to_owned()
    } else {
        cleaned.chars().take(60).collect()
    }
}

/// Where a clip of `title` taken at `stamp` (`2026-10-25 18-04-31`) goes: beside the screenshots.
/// An existing file is never overwritten; a second clip in the same second gets a number.
pub fn clip_path(folder: &Path, title: &str, stamp: &str) -> PathBuf {
    let base = format!("{} {stamp}", safe_name(title));
    let mut path = folder.join(format!("{base}.mp4"));
    let mut n = 2;
    while path.exists() {
        path = folder.join(format!("{base} ({n}).mp4"));
        n += 1;
    }
    path
}

/// The clips in `folder` (every `.mp4`), newest first, with their size.
pub fn list_clips(folder: &Path) -> Vec<(PathBuf, u64)> {
    let mut found: Vec<(PathBuf, u64, std::time::SystemTime)> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("mp4"))
        })
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            meta.is_file()
                .then(|| (e.path(), meta.len(), meta.modified().unwrap_or(std::time::UNIX_EPOCH)))
        })
        .collect();
    found.sort_by(|a, b| b.2.cmp(&a.2));
    found.into_iter().map(|(p, n, _)| (p, n)).collect()
}

// --- the worker -------------------------------------------------------------------------------

/// What the worker reports, one JSON object per line (network spec/clips.md).
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum WorkerEvent {
    Ready {
        width: u32,
        height: u32,
        fps: u32,
        kbps: u32,
    },
    Saved {
        path: String,
        seconds: f64,
        bytes: u64,
    },
    SaveFailed {
        reason: String,
    },
    Ended {
        reason: String,
    },
}

impl WorkerEvent {
    pub fn parse(line: &str) -> Option<WorkerEvent> {
        serde_json::from_str(line).ok()
    }
}

/// The command line of the worker for a game: `--clips SECONDS`, the window the launcher found
/// and the game's processes.
pub fn worker_args(seconds: u32, window: Option<i64>, pids: &[u32], title: &str) -> Vec<String> {
    let mut args = vec!["--clips".to_owned(), seconds.to_string()];
    if let Some(window) = window {
        args.push("--window".into());
        args.push(window.to_string());
    }
    for pid in pids {
        args.push("--pid".into());
        args.push(pid.to_string());
    }
    if !title.is_empty() {
        args.push("--title".into());
        args.push(title.to_owned());
    }
    args
}

/// The worker running for one game. Dropping it ends the worker.
pub struct Recorder {
    child: Child,
    input: Option<ChildStdin>,
    pub events: Receiver<WorkerEvent>,
}

impl Recorder {
    pub fn start(
        worker: &Path,
        seconds: u32,
        window: Option<i64>,
        pids: &[u32],
        title: &str,
        wake: impl Fn() + Send + 'static,
    ) -> std::io::Result<Recorder> {
        let mut command = Command::new(worker);
        command
            .args(worker_args(seconds, window, pids, title))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn()?;
        let input = child.stdin.take();
        let stdout = child.stdout.take().ok_or(std::io::ErrorKind::BrokenPipe)?;
        let (tx, events) = mpsc::channel();
        std::thread::Builder::new()
            .name("clips-events".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(event) = WorkerEvent::parse(&line) {
                        if tx.send(event).is_err() {
                            break;
                        }
                        wake();
                    }
                }
            })?;
        Ok(Recorder {
            child,
            input,
            events,
        })
    }

    /// Asks for the kept seconds to be written to `path`.
    pub fn save(&mut self, path: &Path) -> std::io::Result<()> {
        let line = serde_json::json!({"do": "save", "path": path.to_string_lossy()});
        let input = self.input.as_mut().ok_or(std::io::ErrorKind::BrokenPipe)?;
        writeln!(input, "{line}")?;
        input.flush()
    }

    /// The worker is still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        // Closing the input is a quit; a worker that stays is killed, nothing is left to save.
        if let Some(mut input) = self.input.take() {
            let _ = writeln!(input, r#"{{"do":"quit"}}"#);
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        if self.alive() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_hotkey_is_alt_f12_and_round_trips() {
        let key = hotkey_or_default(None);
        assert_eq!(key.to_string(), "Alt+F12");
        assert_eq!(key.vk(), Some(0x7B));
        assert_eq!(Hotkey::parse("Ctrl+Shift+K").unwrap().to_string(), "Ctrl+Shift+K");
        assert_eq!(Hotkey::parse("Ctrl+Alt+Home").unwrap().vk(), Some(0x24));
    }

    #[test]
    fn a_stored_hotkey_that_makes_no_sense_falls_back() {
        for bad in ["", "K", "Alt+", "Alt+Hyper", "Meta+F1", "F25", "Shift+K", "5"] {
            assert!(Hotkey::parse(bad).is_none(), "{bad}");
            assert_eq!(hotkey_or_default(Some(bad)).to_string(), "Alt+F12", "{bad}");
        }
        assert!(Hotkey::parse("F9").is_some(), "a function key may stand alone");
    }

    #[test]
    fn a_letter_alone_cannot_become_the_hotkey() {
        assert!(Hotkey::from_press("K", false, false, false).is_none());
        assert!(Hotkey::from_press("K", false, true, false).is_some());
        assert!(Hotkey::from_press("F8", false, false, false).is_some());
        assert!(Hotkey::from_press("Enter", true, false, false).is_none());
    }

    #[test]
    fn only_the_offered_lengths_count() {
        assert_eq!(length_or_off(30), 30);
        assert_eq!(length_or_off(45), 0);
        assert_eq!(length_or_off(0), 0);
    }

    #[test]
    fn file_names_are_safe_and_never_overwrite() {
        let folder = tempfile::tempdir().unwrap();
        let first = clip_path(folder.path(), "Bad: <Name>/ of ?", "2026-10-25 18-04-31");
        assert_eq!(
            first.file_name().unwrap().to_string_lossy(),
            "Bad Name of 2026-10-25 18-04-31.mp4"
        );
        std::fs::write(&first, b"x").unwrap();
        let second = clip_path(folder.path(), "Bad: <Name>/ of ?", "2026-10-25 18-04-31");
        assert!(second.to_string_lossy().ends_with("18-04-31 (2).mp4"));
        let nameless = clip_path(folder.path(), "...", "t");
        assert_eq!(nameless.file_name().unwrap().to_string_lossy(), "Game t.mp4");
    }

    #[test]
    fn the_listing_shows_mp4_files_newest_first() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("shot.png"), b"p").unwrap();
        std::fs::write(folder.path().join("old.mp4"), b"12").unwrap();
        let old = std::fs::File::options()
            .write(true)
            .open(folder.path().join("old.mp4"))
            .unwrap();
        old.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000))
            .unwrap();
        std::fs::write(folder.path().join("new.MP4"), b"123").unwrap();
        let clips = list_clips(folder.path());
        assert_eq!(clips.len(), 2);
        assert!(clips[0].0.ends_with("new.MP4"));
        assert_eq!(clips[0].1, 3);
        assert!(list_clips(&folder.path().join("missing")).is_empty());
    }

    #[test]
    fn the_worker_events_parse_and_garbage_is_ignored() {
        assert_eq!(
            WorkerEvent::parse(r#"{"event":"saved","path":"a.mp4","seconds":15.5,"bytes":9}"#),
            Some(WorkerEvent::Saved {
                path: "a.mp4".into(),
                seconds: 15.5,
                bytes: 9
            })
        );
        assert_eq!(
            WorkerEvent::parse(r#"{"event":"ended","reason":"no_encoder"}"#),
            Some(WorkerEvent::Ended {
                reason: "no_encoder".into()
            })
        );
        assert!(WorkerEvent::parse("not json").is_none());
        assert!(WorkerEvent::parse(r#"{"event":"future"}"#).is_none());
    }

    #[test]
    fn the_worker_gets_the_window_and_the_game_s_processes() {
        let args = worker_args(30, Some(77), &[10, 11], "My Game");
        assert_eq!(
            args,
            ["--clips", "30", "--window", "77", "--pid", "10", "--pid", "11", "--title", "My Game"]
        );
        assert_eq!(worker_args(15, None, &[], ""), ["--clips", "15"]);
    }

    /// The real worker (`ANYPLAY_SHARE` names it) started the way the launcher starts it, for a
    /// process with no window: it answers with `ended`, never hangs, and exits.
    #[test]
    #[ignore = "needs the share worker program (ANYPLAY_SHARE)"]
    fn the_real_worker_answers_over_the_pipes() {
        let worker = std::path::PathBuf::from(std::env::var("ANYPLAY_SHARE").expect("ANYPLAY_SHARE"));
        // Process 4 is the system: it has no window the worker may take.
        let mut recorder = Recorder::start(&worker, 15, None, &[4], "", || {}).expect("starts");
        let event = recorder
            .events
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("an answer");
        assert!(
            matches!(&event, WorkerEvent::Ended { reason } if reason == "no_window" || reason == "no_encoder"),
            "{event:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(!recorder.alive());
    }

    /// The hotkey is taken while the listener lives, a second one for the same keys is refused
    /// (the launcher then says another program has it), and the keys are free again after.
    #[cfg(windows)]
    #[test]
    fn a_hotkey_can_be_taken_once_and_is_given_back() {
        let key = Hotkey::parse("Ctrl+Alt+Shift+F23").unwrap();
        let first = HotkeyListener::start(&key, || {}).expect("the keys are free");
        assert!(HotkeyListener::start(&key, || {}).is_err());
        drop(first);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(HotkeyListener::start(&key, || {}).is_ok());
    }
}
