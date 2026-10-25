//! Child-process plumbing: one merged output pipe, line splitting with `\r` rewriting, and
//! killing a process together with its children. Used by the unlock pipeline for the relinker.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// The last lines a tool wrote that are kept for error messages.
pub const KEPT_LINES: usize = 400;

/// Splits a tool's output into lines as it arrives. A progress display rewrites its line with
/// carriage returns: every rewrite is a *segment* (for the progress estimate), only the line's
/// last state becomes a *line* (for the log). Port of `OutputCollector`.
#[derive(Debug, Default)]
pub struct LineSplitter {
    pending: Vec<u8>,
    rewritten: Option<String>,
    lines: VecDeque<String>,
}

/// What one chunk of output produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Split {
    /// Every non-empty piece between `\n` / `\r`, in order.
    pub segments: Vec<String>,
    /// Lines that ended with `\n`, each in the last state it was rewritten to.
    pub lines: Vec<String>,
}

impl LineSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, chunk: &[u8]) -> Split {
        let mut split = Split::default();
        if chunk.is_empty() {
            return split;
        }
        self.pending.extend_from_slice(chunk);
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n' || *b == b'\r') {
            let newline = self.pending[end] == b'\n';
            let text = String::from_utf8_lossy(&self.pending[..end]).into_owned();
            self.pending.drain(..=end);
            if !text.is_empty() {
                split.segments.push(text.clone());
                self.rewritten = Some(text);
            }
            if newline && let Some(line) = self.rewritten.take() {
                split.lines.push(line);
            }
        }
        for line in &split.lines {
            self.keep(line.clone());
        }
        split
    }

    /// Ends the output: the unfinished line, if any.
    pub fn flush(&mut self) -> Option<String> {
        let rest = if self.pending.is_empty() {
            self.rewritten.take()
        } else {
            Some(String::from_utf8_lossy(&self.pending).into_owned())
        };
        self.pending.clear();
        self.rewritten = None;
        if let Some(rest) = &rest {
            self.keep(rest.clone());
        }
        rest
    }

    fn keep(&mut self, line: String) {
        self.lines.push_back(line);
        while self.lines.len() > KEPT_LINES {
            self.lines.pop_front();
        }
    }

    /// The last [`KEPT_LINES`] lines.
    pub fn lines(&self) -> Vec<String> {
        self.lines.iter().cloned().collect()
    }
}

/// What to run.
#[derive(Debug, Clone, Default)]
pub struct ToolSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
    pub priority: Priority,
}

/// How much of the machine a tool may take. The player is waiting for `Low`; `Idle` is for work
/// nobody asked for yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Priority {
    #[default]
    Normal,
    Low,
    Idle,
}

/// Puts the calling thread into the background (low CPU and I/O priority) for the rest of its life.
pub fn enter_background_mode() {
    #[cfg(windows)]
    crate::win::enter_background_mode();
    #[cfg(target_os = "linux")]
    // SAFETY: plain system call; on Linux `who` 0 is the calling thread.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 10);
    }
}

/// How a tool ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolResult {
    /// Exit code; `None` when [`ToolProcess::kill`] ended it.
    pub code: Option<i32>,
    pub killed: bool,
    /// The last lines of output.
    pub lines: Vec<String>,
}

struct Shared {
    child: Mutex<Child>,
    reader: Mutex<Option<io::PipeReader>>,
    killed: AtomicBool,
    #[cfg(windows)]
    job: Option<crate::win::Job>,
}

/// A running tool with stdout and stderr merged into one pipe. Clones are handles to the same
/// process, so one thread can [`run`](Self::run) it while another can [`kill`](Self::kill) it.
#[derive(Clone)]
pub struct ToolProcess(Arc<Shared>);

impl ToolProcess {
    pub fn spawn(spec: &ToolSpec) -> io::Result<ToolProcess> {
        let (reader, writer) = io::pipe()?;
        let mut command = Command::new(&spec.program);
        command.args(&spec.args);
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &spec.env {
            command.env(key, value);
        }
        command
            .stdin(Stdio::null())
            .stdout(writer.try_clone()?)
            .stderr(writer);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW, and the priority class the tool starts in.
            let class = match spec.priority {
                Priority::Normal => 0,
                Priority::Low => 0x0000_4000, // BELOW_NORMAL_PRIORITY_CLASS
                Priority::Idle => 0x0000_0040, // IDLE_PRIORITY_CLASS
            };
            command.creation_flags(0x0800_0000 | class);
        }
        crate::platform::isolate_group(&mut command);
        let child = command.spawn()?;
        // The command still holds the write end; it must close for the pipe to reach EOF.
        drop(command);
        #[cfg(windows)]
        let job = {
            use std::os::windows::io::AsRawHandle;
            crate::win::Job::new(true)
                .and_then(|job| job.assign(child.as_raw_handle()).map(|()| job))
                .ok()
        };
        let process = ToolProcess(Arc::new(Shared {
            child: Mutex::new(child),
            reader: Mutex::new(Some(reader)),
            killed: AtomicBool::new(false),
            #[cfg(windows)]
            job,
        }));
        #[cfg(unix)]
        process.set_priority(spec.priority);
        Ok(process)
    }

    /// Changes how much of the machine the tool may take, while it runs (an unlock the player
    /// now waits for stops being a background job).
    pub fn set_priority(&self, priority: Priority) {
        #[cfg(windows)]
        if let Some(job) = &self.0.job {
            job.set_priority(crate::win::priority_class(
                priority == Priority::Low,
                priority == Priority::Idle,
            ));
        }
        #[cfg(unix)]
        {
            let pid = self.id() as libc::id_t;
            let nice = match priority {
                Priority::Normal => 0,
                Priority::Low => 5,
                Priority::Idle => 15,
            };
            // SAFETY: plain system call on a process of ours; a failure changes nothing.
            unsafe {
                libc::setpriority(libc::PRIO_PROCESS, pid, nice);
            }
        }
        #[cfg(not(any(windows, unix)))]
        let _ = priority;
    }

    /// Freezes the tool and its children without losing what they did.
    pub fn pause(&self) {
        #[cfg(windows)]
        if let Some(job) = &self.0.job {
            job.suspend();
        }
        #[cfg(unix)]
        self.signal_group(libc::SIGSTOP);
    }

    /// Lets a paused tool go on.
    pub fn resume(&self) {
        #[cfg(windows)]
        if let Some(job) = &self.0.job {
            job.resume();
        }
        #[cfg(unix)]
        self.signal_group(libc::SIGCONT);
    }

    #[cfg(unix)]
    fn signal_group(&self, signal: libc::c_int) {
        if let Ok(pid) = libc::pid_t::try_from(self.id()) {
            // SAFETY: plain system call; a negative pid addresses the process group.
            unsafe {
                libc::kill(-pid, signal);
            }
        }
    }

    pub fn id(&self) -> u32 {
        self.0.child.lock().expect("child lock").id()
    }

    /// Ends the process and its children. Safe to call from any thread, at any time.
    pub fn kill(&self) {
        self.0.killed.store(true, Ordering::SeqCst);
        #[cfg(windows)]
        if let Some(job) = &self.0.job {
            job.terminate(1);
            return;
        }
        crate::platform::kill_group(self.id());
        let _ = self.0.child.lock().expect("child lock").kill();
    }

    /// Reads the output until the process ends. `on_segment` gets every rewrite of a progress
    /// line, `on_line` every finished line. Call once.
    pub fn run(
        &self,
        on_segment: &mut dyn FnMut(&str),
        on_line: &mut dyn FnMut(&str),
    ) -> io::Result<ToolResult> {
        let mut reader = self
            .0
            .reader
            .lock()
            .expect("reader lock")
            .take()
            .ok_or_else(|| io::Error::other("output already consumed"))?;
        let mut splitter = LineSplitter::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => n,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                // A broken pipe is the normal end on some platforms.
                Err(_) => break,
            };
            let split = splitter.push(&buffer[..n]);
            split.segments.iter().for_each(|s| on_segment(s));
            split.lines.iter().for_each(|l| on_line(l));
        }
        if let Some(rest) = splitter.flush() {
            on_line(&rest);
        }
        let status = loop {
            if let Some(status) = self.0.child.lock().expect("child lock").try_wait()? {
                break status;
            }
            thread::sleep(Duration::from_millis(5));
        };
        let killed = self.0.killed.load(Ordering::SeqCst);
        Ok(ToolResult {
            code: if killed { None } else { status.code() },
            killed,
            lines: splitter.lines(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(chunks: &[&[u8]]) -> (Vec<String>, Vec<String>, Option<String>, Vec<String>) {
        let mut s = LineSplitter::new();
        let mut segments = Vec::new();
        let mut lines = Vec::new();
        for c in chunks {
            let r = s.push(c);
            segments.extend(r.segments);
            lines.extend(r.lines);
        }
        let rest = s.flush();
        (segments, lines, rest, s.lines())
    }

    #[test]
    fn carriage_return_rewrites_become_segments_and_one_line() {
        let (segments, lines, rest, kept) = split(&[
            b"Instruction scan | eboot.bin |  10%\rInstruction scan | eboot.bin | 100%\r\nnext\n",
        ]);
        assert_eq!(
            segments,
            [
                "Instruction scan | eboot.bin |  10%",
                "Instruction scan | eboot.bin | 100%",
                "next"
            ]
        );
        assert_eq!(lines, ["Instruction scan | eboot.bin | 100%", "next"]);
        assert_eq!(rest, None);
        assert_eq!(kept, lines);
    }

    #[test]
    fn a_line_can_arrive_in_pieces() {
        let (segments, lines, rest, _) = split(&[b"hel", b"lo wor", b"ld\nsec", b"ond"]);
        assert_eq!(segments, ["hello world"]);
        assert_eq!(lines, ["hello world"]);
        assert_eq!(rest.as_deref(), Some("second"));
    }

    #[test]
    fn crlf_and_empty_lines() {
        let (_, lines, _, _) = split(&[b"a\r\n\r\nb\r\n"]);
        // The blank line between is empty and not kept, as in the original.
        assert_eq!(lines, ["a", "b"]);
    }

    #[test]
    fn flush_returns_the_last_rewritten_state() {
        let (_, lines, rest, kept) = split(&[b"p 10%\rp 50%\r"]);
        assert!(lines.is_empty());
        assert_eq!(rest.as_deref(), Some("p 50%"));
        assert_eq!(kept, ["p 50%"]);
    }

    #[test]
    fn keeps_the_last_400_lines() {
        let mut s = LineSplitter::new();
        for i in 0..450 {
            s.push(format!("line {i}\n").as_bytes());
        }
        let kept = s.lines();
        assert_eq!(kept.len(), KEPT_LINES);
        assert_eq!(kept.first().unwrap(), "line 50");
        assert_eq!(kept.last().unwrap(), "line 449");
    }

    #[cfg(windows)]
    fn shell(script: &str) -> ToolSpec {
        ToolSpec {
            program: "cmd".into(),
            args: vec!["/c".into(), script.into()],
            ..ToolSpec::default()
        }
    }

    #[cfg(not(windows))]
    fn shell(script: &str) -> ToolSpec {
        ToolSpec {
            program: "sh".into(),
            args: vec!["-c".into(), script.into()],
            ..ToolSpec::default()
        }
    }

    #[test]
    fn merges_stdout_and_stderr_and_reports_the_exit_code() {
        let process = ToolProcess::spawn(&shell("echo one && echo two 1>&2 && exit 3")).unwrap();
        let mut lines = Vec::new();
        let result = process
            .run(&mut |_| {}, &mut |l| lines.push(l.trim().to_string()))
            .unwrap();
        assert_eq!(result.code, Some(3));
        assert!(!result.killed);
        assert_eq!(lines, ["one", "two"]);
    }

    #[test]
    fn kill_ends_a_running_process_from_another_thread() {
        #[cfg(windows)]
        let script = "ping -n 30 127.0.0.1 > nul";
        #[cfg(not(windows))]
        let script = "sleep 30";
        let process = ToolProcess::spawn(&shell(script)).unwrap();
        let killer = process.clone();
        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            killer.kill();
        });
        let started = std::time::Instant::now();
        let result = process.run(&mut |_| {}, &mut |_| {}).unwrap();
        handle.join().unwrap();
        assert!(result.killed);
        assert_eq!(result.code, None);
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
