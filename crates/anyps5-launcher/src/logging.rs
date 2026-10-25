//! The launcher's own log: `Logs\launcher.log`, kept in release builds too (they have no console),
//! and a panic hook that writes the panic to the same file. `panic = "abort"` ends the process
//! right after the hook, so the hook is the only place a panic can be recorded.
//!
//! The file is capped: at about 1 MiB it moves to `launcher.log.1` (replacing the older one), so
//! the two together stay under about 2 MiB.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyps5_core::paths::AppPaths;

/// Size at which `launcher.log` moves to `launcher.log.1`.
const CAP: u64 = 1024 * 1024;

/// Where the panic hook writes; set once the log file could be opened.
static PANIC_FILE: OnceLock<PathBuf> = OnceLock::new();

/// Sets up the log and the panic hook. Call once, first thing in `main`.
pub fn init() {
    let paths = AppPaths::from_env()
        .unwrap_or_else(|| AppPaths::under(&std::env::temp_dir().join("anyps5-launcher")));
    let writer = fs::create_dir_all(paths.logs_dir())
        .ok()
        .and_then(|()| SizeCappedFile::open(paths.launcher_log(), paths.launcher_log_old(), CAP));
    match writer {
        Some(file) => {
            let _ = PANIC_FILE.set(paths.launcher_log());
            let _ = tracing_subscriber::fmt()
                .with_writer(move || file.clone())
                .with_ansi(false)
                .try_init();
        }
        None => {
            let _ = tracing_subscriber::fmt().try_init();
        }
    }
    install_panic_hook();
    tracing::info!(
        "launcher {} on {} {}, pid {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::process::id()
    );
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(path) = PANIC_FILE.get() {
            let text = panic_text(
                std::thread::current().name().unwrap_or("unnamed"),
                info,
                &std::backtrace::Backtrace::force_capture().to_string(),
            );
            // A fresh append handle, not the log writer: the panic may have happened while that
            // writer was locked, and waiting for it would hang the dying process.
            if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = file.write_all(text.as_bytes());
                let _ = file.flush();
                let _ = file.sync_data();
            }
        }
        previous(info);
    }));
}

/// `PANIC thread=<name> at <file>:<line>: <message>`, then the backtrace.
fn panic_text(thread: &str, info: &PanicHookInfo<'_>, backtrace: &str) -> String {
    let location = info.location().map_or_else(
        || "unknown".to_owned(),
        |l| format!("{}:{}", l.file(), l.line()),
    );
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-text payload");
    format!("PANIC thread={thread} at {location}: {message}\n{backtrace}\n")
}

/// A log file that starts a new one when it reaches its cap. Cloning shares the file. Write
/// errors are swallowed: a log must never take the launcher down.
#[derive(Clone)]
pub struct SizeCappedFile(Arc<Mutex<State>>);

struct State {
    path: PathBuf,
    old: PathBuf,
    cap: u64,
    file: Option<File>,
    written: u64,
}

impl SizeCappedFile {
    /// Opens `path` for appending; `None` when that fails.
    pub fn open(path: PathBuf, old: PathBuf, cap: u64) -> Option<Self> {
        let file = append(&path)?;
        let written = file.metadata().map_or(0, |m| m.len());
        Some(Self(Arc::new(Mutex::new(State {
            path,
            old,
            cap,
            file: Some(file),
            written,
        }))))
    }
}

fn append(path: &Path) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(path).ok()
}

impl Write for SizeCappedFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if cfg!(debug_assertions) {
            let _ = io::stderr().write_all(buf);
        }
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if state.written > 0 && state.written + buf.len() as u64 > state.cap {
            state.file = None;
            let _ = fs::rename(&state.path, &state.old);
            state.file = append(&state.path);
            state.written = 0;
        }
        if let Some(file) = state.file.as_mut()
            && file.write_all(buf).is_ok()
        {
            state.written += buf.len() as u64;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(file) = state.file.as_mut() {
            let _ = file.flush();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_file_moves_to_the_old_name() {
        let dir = tempfile::tempdir().unwrap();
        let (log, old) = (dir.path().join("l.log"), dir.path().join("l.log.1"));
        let mut file = SizeCappedFile::open(log.clone(), old.clone(), 10).unwrap();
        file.write_all(b"aaaaaa\n").unwrap();
        file.write_all(b"bbbbbb\n").unwrap();
        assert_eq!(fs::read_to_string(&old).unwrap(), "aaaaaa\n");
        assert_eq!(fs::read_to_string(&log).unwrap(), "bbbbbb\n");
        file.write_all(b"cccccc\n").unwrap();
        // The older file is replaced, never stacked.
        assert_eq!(fs::read_to_string(&old).unwrap(), "bbbbbb\n");
        assert_eq!(fs::read_to_string(&log).unwrap(), "cccccc\n");
    }

    #[test]
    fn an_existing_file_counts_towards_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let (log, old) = (dir.path().join("l.log"), dir.path().join("l.log.1"));
        fs::write(&log, "0123456789").unwrap();
        let mut file = SizeCappedFile::open(log.clone(), old.clone(), 12).unwrap();
        file.write_all(b"abc").unwrap();
        assert_eq!(fs::read_to_string(&old).unwrap(), "0123456789");
        assert_eq!(fs::read_to_string(&log).unwrap(), "abc");
    }

    #[test]
    fn the_panic_line_names_thread_and_place() {
        let seen = Arc::new(Mutex::new(String::new()));
        let slot = seen.clone();
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            *slot.lock().unwrap() = panic_text("worker", info, "BT");
        }));
        let _ = std::panic::catch_unwind(|| panic!("boom {}", 7));
        std::panic::set_hook(previous);
        let text = seen.lock().unwrap().clone();
        assert!(
            text.starts_with("PANIC thread=worker at ") && text.contains("logging.rs:"),
            "{text}"
        );
        assert!(text.ends_with(": boom 7\nBT\n"), "{text}");
    }
}
