//! Ported from utkuhalis/AnyPS5-ARM (Diagnosis.swift, GPL-2.0-only) and changed since; see README "Credits".
//! Why a game stopped, read from what its last start wrote to the log and from how its process
//! ended (PORT.md 6.11, `Diagnosis.swift`). All regular expressions of the engine's output
//! formats live here.

use std::fs;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;

/// How the process ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitInfo {
    /// The exit status; `None` when the launcher ended the process.
    pub code: Option<u32>,
    pub had_window: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Diagnosis {
    /// A module (DLL or PRX) could not be loaded.
    MissingModule(String),
    /// The game called a system function the engine does not implement yet.
    Unsupported(String),
    /// The GPU device was lost.
    GpuLost,
    /// The engine or the GPU ran out of memory.
    OutOfMemory,
    /// Any other error the engine stopped on, with its message.
    EngineError(String),
    /// The process died by an exception or abort.
    Crashed(String),
    /// The host CPU lacks extensions the game needs (the list).
    HostCpu(String),
    /// Linux: the user may not open `/dev/udmabuf`, so GPU stores through shared memory fail.
    NoUdmabuf,
}

const MINGW_RUNTIME: &str =
    "MinGW runtime (libgcc_s_seh-1.dll, libstdc++-6.dll, libwinpthread-1.dll)";

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid pattern"))
}

/// Capture group 1 of the first match.
fn first(re: &Regex, text: &str) -> Option<String> {
    re.captures(text)?.get(1).map(|m| m.as_str().to_string())
}

fn basename(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

/// Names of the exception codes the engine's crash handler and Windows use.
pub fn exception_name(code: u32) -> String {
    match code {
        0xC000_0005 => "EXCEPTION_ACCESS_VIOLATION".into(),
        0xC000_001D => "EXCEPTION_ILLEGAL_INSTRUCTION".into(),
        0xC000_0094 => "EXCEPTION_INT_DIVIDE_BY_ZERO".into(),
        0xC000_00FD => "EXCEPTION_STACK_OVERFLOW".into(),
        0xC000_0409 => "STATUS_FAIL_FAST_EXCEPTION".into(),
        0xC000_0374 => "STATUS_HEAP_CORRUPTION".into(),
        0xC000_0142 => "STATUS_DLL_INIT_FAILED".into(),
        0xC000_007B => "STATUS_INVALID_IMAGE_FORMAT".into(),
        0x8000_0003 => "EXCEPTION_BREAKPOINT".into(),
        other => format!("0x{other:08X}"),
    }
}

/// Whether an exit status looks like a crash (an NTSTATUS error or warning).
fn is_crash_status(code: u32) -> bool {
    matches!(code >> 28, 0xC | 0x8)
}

impl Diagnosis {
    /// The line the launcher writes to a game's log before it starts the game.
    pub const START_MARKER: &'static str = "[anyps5-launcher] start";

    /// The log text of the last start: what follows the last start marker, or the last 64 KiB of
    /// the log when the game was started some other way.
    pub fn last_session(log: &str) -> String {
        if let Some(at) = log.rfind(Self::START_MARKER) {
            return log[at + Self::START_MARKER.len()..].to_string();
        }
        let keep = 64 * 1024;
        if log.len() <= keep {
            return log.to_string();
        }
        let mut start = log.len() - keep;
        while !log.is_char_boundary(start) {
            start += 1;
        }
        log[start..].to_string()
    }

    /// The reason in `session` (the log of one start) and the way the process ended, `None` for
    /// a game that ended without a sign of trouble. The first rule that matches wins.
    pub fn read(session: &str, exit: Option<&ExitInfo>) -> Option<Diagnosis> {
        static LOAD: OnceLock<Regex> = OnceLock::new();
        static DYLD: OnceLock<Regex> = OnceLock::new();
        static IMPORT: OnceLock<Regex> = OnceLock::new();
        static CPU: OnceLock<Regex> = OnceLock::new();
        static TERMINATE: OnceLock<Regex> = OnceLock::new();
        static ARM_UNCAUGHT: OnceLock<Regex> = OnceLock::new();
        static ARM_TERMINATING: OnceLock<Regex> = OnceLock::new();
        static VK4: OnceLock<Regex> = OnceLock::new();
        static VK12: OnceLock<Regex> = OnceLock::new();
        static DRIVER: OnceLock<Regex> = OnceLock::new();
        static FATAL: OnceLock<Regex> = OnceLock::new();
        static UNDELIVERABLE: OnceLock<Regex> = OnceLock::new();

        let code = exit.and_then(|e| e.code);

        // 1. A module that did not load.
        if let Some(module) = first(
            regex(&LOAD, r"(?m)^Failed to load module: (.+?)\s*$"),
            session,
        ) {
            return Some(Diagnosis::MissingModule(basename(&module)));
        }
        if let Some(module) = first(
            regex(&DYLD, r"Library not loaded: (?:@rpath/)?(\S+)"),
            session,
        ) {
            return Some(Diagnosis::MissingModule(basename(&module)));
        }
        if let Some(function) = first(
            regex(&IMPORT, r"(?m)^FAIL: unresolved ELF import (\S+)"),
            session,
        ) {
            return Some(Diagnosis::Unsupported(function));
        }
        if code == Some(0xC000_0135) {
            return Some(Diagnosis::MissingModule(MINGW_RUNTIME.into()));
        }

        // 2. The host CPU, before the generic uncaught exception it arrives in.
        if let Some(list) = first(
            regex(&CPU, r"The host CPU \(.*?\) lacks ([^.\r\n]+)\."),
            session,
        ) {
            return Some(Diagnosis::HostCpu(list.trim().to_string()));
        }

        // 3. An uncaught C++ exception.
        let message = first(
            regex(
                &TERMINATE,
                r"(?m)^terminate called after throwing an instance of '[^'\r\n]+'\r?\n\s*what\(\):\s+([^\r\n]+)",
            ),
            session,
        )
        .or_else(|| {
            first(
                regex(
                    &ARM_UNCAUGHT,
                    r"uncaught C\+\+ exception \S+ of type [^\n,]+, what\(\): ([^\r\n]+)",
                ),
                session,
            )
        })
        .or_else(|| {
            first(
                regex(
                    &ARM_TERMINATING,
                    r"terminating (?:due to|with) uncaught exception of type \S+?: ([^\r\n]+)",
                ),
                session,
            )
        });
        if let Some(message) = message {
            return Some(Self::classify(&message));
        }

        // Linux: the engine says so at start; it matters once GPU stores fail because of it.
        if session.contains("open /dev/udmabuf: Permission denied")
            && session.contains("BDA access failed")
        {
            return Some(Diagnosis::NoUdmabuf);
        }

        // 4 and 5. The GPU.
        if session.contains("VK_ERROR_DEVICE_LOST")
            || regex(&VK4, r"Vulkan result -4\b").is_match(session)
        {
            return Some(Diagnosis::GpuLost);
        }
        if session.contains("VK_ERROR_OUT_OF_DEVICE_MEMORY")
            || session.contains("std::bad_alloc")
            || regex(&VK12, r"Vulkan result -[12]\b").is_match(session)
        {
            return Some(Diagnosis::OutOfMemory);
        }

        // 6. What the engine prints when it dies.
        if let Some(message) = first(
            regex(&DRIVER, r"\[gpu\] driver failure: ([^\r\n]+)"),
            session,
        ) {
            return Some(Self::classify(&message));
        }
        if let Some(found) = regex(
            &FATAL,
            r"(?m)^FATAL: unhandled exception 0x([0-9a-fA-F]{8}) on thread[^\r\n]*",
        )
        .captures(session)
        {
            let code = u32::from_str_radix(&found[1], 16).unwrap_or(0);
            let mut name = exception_name(code);
            // `  rip ...` and `  read of 0x...` follow on indented lines.
            let after = &session[found.get(0).map_or(0, |m| m.end())..];
            let access = after
                .lines()
                .skip(1)
                .take(3)
                .take_while(|l| l.starts_with(' ') || l.starts_with('\t'))
                .find_map(|l| {
                    let l = l.trim();
                    ["read of ", "write of ", "execute of "]
                        .iter()
                        .any(|p| l.starts_with(p))
                        .then(|| l.to_string())
                });
            if let Some(access) = access {
                name = format!("{name} ({access})");
            }
            return Some(Diagnosis::Crashed(name));
        }
        if session.contains("FATAL: abort() (SIGABRT)") {
            return Some(Diagnosis::Crashed("abort()".into()));
        }
        if session.contains("FATAL: UCRT invalid parameter") {
            return Some(Diagnosis::Crashed("invalid parameter".into()));
        }
        if let Some(kind) = first(
            regex(
                &UNDELIVERABLE,
                r"FATAL: C\+\+ exception (\S+) raised on thread \d+ cannot be delivered",
            ),
            session,
        ) {
            return Some(Diagnosis::EngineError(format!(
                "C++ exception {kind} cannot be delivered"
            )));
        }

        // 7. The exit status alone.
        match code {
            Some(0xC000_0017) => Some(Diagnosis::OutOfMemory),
            Some(3) => Some(Diagnosis::Crashed("abort()".into())),
            Some(code) if is_crash_status(code) => Some(Diagnosis::Crashed(exception_name(code))),
            _ => None,
        }
    }

    /// Sorts an engine message into a diagnosis.
    pub fn classify(message: &str) -> Diagnosis {
        static NOT_IMPLEMENTED: OnceLock<Regex> = OnceLock::new();
        static VK12: OnceLock<Regex> = OnceLock::new();
        static CPU_LIST: OnceLock<Regex> = OnceLock::new();
        let trimmed = message.trim_matches([' ', '\t']);
        if let Some(function) = first(regex(&NOT_IMPLEMENTED, r"^(\S+) not implemented"), trimmed) {
            return Diagnosis::Unsupported(function);
        }
        let lower = trimmed.to_lowercase();
        if trimmed.contains("VK_ERROR_DEVICE_LOST")
            || lower.contains("device lost")
            || trimmed.contains("Vulkan result -4")
        {
            return Diagnosis::GpuLost;
        }
        if trimmed.contains("VK_ERROR_OUT_OF_DEVICE_MEMORY")
            || lower.contains("out of memory")
            || regex(&VK12, r"Vulkan result -[12]\b").is_match(trimmed)
        {
            return Diagnosis::OutOfMemory;
        }
        if trimmed.starts_with("The host CPU (") && trimmed.contains(" lacks ") {
            let list = first(regex(&CPU_LIST, r" lacks ([^.]+)"), trimmed)
                .unwrap_or_else(|| trimmed.to_string());
            return Diagnosis::HostCpu(list.trim().to_string());
        }
        if trimmed.chars().count() > 200 {
            let cut: String = trimmed.chars().take(200).collect();
            return Diagnosis::EngineError(format!("{cut}…"));
        }
        Diagnosis::EngineError(trimmed.to_string())
    }

    /// The name of the exception for the report's crash line: from a `FATAL` block, else from an
    /// exit status that is a crash.
    pub fn crash_name(session: &str, exit: Option<&ExitInfo>) -> Option<String> {
        match Self::read(session, exit) {
            Some(Diagnosis::Crashed(name)) => Some(name),
            _ => exit
                .and_then(|e| e.code)
                .filter(|c| is_crash_status(*c))
                .map(exception_name),
        }
    }

    /// The key of the localised text and its arguments (`diagnosisMissingModule`, ...).
    pub fn summary_key(&self) -> (&'static str, Vec<String>) {
        match self {
            Diagnosis::MissingModule(name) => ("diagnosisMissingModule", vec![name.clone()]),
            Diagnosis::Unsupported(function) => ("diagnosisUnsupported", vec![function.clone()]),
            Diagnosis::GpuLost => ("diagnosisGpuLost", vec![]),
            Diagnosis::OutOfMemory => ("diagnosisOutOfMemory", vec![]),
            Diagnosis::EngineError(message) => ("diagnosisEngineError", vec![message.clone()]),
            Diagnosis::Crashed(what) => ("diagnosisCrashed", vec![what.clone()]),
            Diagnosis::HostCpu(list) => ("diagnosisHostCpu", vec![list.clone()]),
            Diagnosis::NoUdmabuf => ("diagnosisNoUdmabuf", vec![]),
        }
    }

    /// The English text (used in the report).
    pub fn summary(&self) -> String {
        match self {
            Diagnosis::MissingModule(name)
                if name.to_lowercase().ends_with(".dll") || name.starts_with("MinGW") =>
            {
                format!(
                    "Missing module: {name}. Install or choose a toolkit that includes the MinGW runtime."
                )
            }
            Diagnosis::MissingModule(name) => {
                format!("Missing module: {name}. Unlocking the game again may fix it.")
            }
            Diagnosis::Unsupported(function) => {
                format!(
                    "The game asked for a system function that is not supported yet: {function}"
                )
            }
            Diagnosis::GpuLost => "The GPU stopped responding.".into(),
            Diagnosis::OutOfMemory => "The game ran out of memory.".into(),
            Diagnosis::EngineError(message) => format!("Engine error: {message}"),
            Diagnosis::Crashed(what) => format!("The game crashed ({what})."),
            Diagnosis::HostCpu(list) => format!("This CPU lacks {list}, which the game needs."),
            Diagnosis::NoUdmabuf => {
                "The game cannot open /dev/udmabuf. Add your user to the kvm group and log in again.".into()
            }
        }
    }
}

const SESSION_READ_LIMIT: u64 = 4_000_000;

/// The last start in a log file: reads the last 4 000 000 bytes and cuts at the last marker.
pub fn read_last_session(path: &Path) -> String {
    read_tail(path, SESSION_READ_LIMIT)
        .map(|bytes| Diagnosis::last_session(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default()
}

/// The last `limit` bytes of a file.
pub fn read_tail(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut file = fs::File::open(path)?;
    let len = file.metadata()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(limit)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Option<Diagnosis> {
        Diagnosis::read(text, None)
    }

    fn exit(code: u32) -> ExitInfo {
        ExitInfo {
            code: Some(code),
            had_window: true,
        }
    }

    #[test]
    fn missing_module_in_all_its_forms() {
        let log = "starting\nFailed to load module: libSceFoo.prx\nGetLastError: 126 - The specified module could not be found.\n";
        assert_eq!(
            read(log),
            Some(Diagnosis::MissingModule("libSceFoo.prx".into()))
        );
        let path = "Failed to load module: C:\\x\\libs\\libSceFoo.prx\r\nGetLastError: 126\r\n";
        assert_eq!(
            read(path),
            Some(Diagnosis::MissingModule("libSceFoo.prx".into()))
        );
        let mac = "dyld: Library not loaded: @rpath/libSceFoo.prx\n  Referenced from: x";
        assert_eq!(
            read(mac),
            Some(Diagnosis::MissingModule("libSceFoo.prx".into()))
        );
        let import = "FAIL: unresolved ELF import sceKernelFoo\n";
        assert_eq!(
            read(import),
            Some(Diagnosis::Unsupported("sceKernelFoo".into()))
        );
        assert_eq!(
            Diagnosis::read("nothing", Some(&exit(0xC000_0135))),
            Some(Diagnosis::MissingModule(MINGW_RUNTIME.into()))
        );
    }

    #[test]
    fn unsupported_function_from_an_uncaught_exception() {
        let log = "terminate called after throwing an instance of 'std::runtime_error'\n  what():  sceFooBar not implemented\n";
        assert_eq!(read(log), Some(Diagnosis::Unsupported("sceFooBar".into())));
        let crlf = log.replace('\n', "\r\n");
        assert_eq!(
            read(&crlf),
            Some(Diagnosis::Unsupported("sceFooBar".into()))
        );
        let mac = "libc++abi: terminating due to uncaught exception of type std::runtime_error: sceFooBar not implemented\n";
        assert_eq!(read(mac), Some(Diagnosis::Unsupported("sceFooBar".into())));
        let arm = "uncaught C++ exception St13runtime_error of type std::runtime_error, what(): sceFooBar not implemented\n";
        assert_eq!(read(arm), Some(Diagnosis::Unsupported("sceFooBar".into())));
    }

    #[test]
    fn gpu_failures() {
        let lost = "[gpu] driver failure: AGC graphics: vkQueueSubmit: Vulkan result -4\n";
        assert_eq!(read(lost), Some(Diagnosis::GpuLost));
        let oom = "[gpu] driver failure: AGC graphics: vkAllocateMemory: Vulkan result -2\n";
        assert_eq!(read(oom), Some(Diagnosis::OutOfMemory));
        let host = "AGC graphics: x: Vulkan result -1\n";
        assert_eq!(read(host), Some(Diagnosis::OutOfMemory));
        assert_eq!(read("VK_ERROR_DEVICE_LOST"), Some(Diagnosis::GpuLost));
        assert_eq!(
            read("VK_ERROR_OUT_OF_DEVICE_MEMORY"),
            Some(Diagnosis::OutOfMemory)
        );
        assert_eq!(
            read("terminate: std::bad_alloc"),
            Some(Diagnosis::OutOfMemory)
        );
        // -40 is not -4.
        assert_eq!(read("Vulkan result -40"), None);
        let wrapped = "[gpu] driver failure: some other thing happened\n";
        assert_eq!(
            read(wrapped),
            Some(Diagnosis::EngineError("some other thing happened".into()))
        );
    }

    #[test]
    fn a_denied_udmabuf_counts_once_gpu_stores_fail() {
        let denied = "[gpu] open /dev/udmabuf: Permission denied\n";
        assert_eq!(read(denied), None, "a copy fallback alone is no failure");
        let failed = format!("{denied}BDA access failed at 0x1000\nVulkan result -4\n");
        assert_eq!(read(&failed), Some(Diagnosis::NoUdmabuf));
    }

    #[test]
    fn host_cpu_beats_the_generic_uncaught_exception() {
        let log = "terminate called after throwing an instance of 'std::runtime_error'\n  what():  The host CPU (Intel(R) Core(TM) i3-1005G1 CPU @ 1.20GHz) lacks AVX2, BMI2. The PS5's Zen 2 CPU has these extensions and titles run their instructions natively, so this CPU cannot run them.\n";
        assert_eq!(read(log), Some(Diagnosis::HostCpu("AVX2, BMI2".into())));
        let message = "The host CPU (X) lacks MOVBE. The rest";
        assert_eq!(
            Diagnosis::classify(message),
            Diagnosis::HostCpu("MOVBE".into())
        );
    }

    #[test]
    fn fatal_blocks() {
        let write = "\nFATAL: unhandled exception 0xc0000005 on thread 12 'GfxQueue'\n  write of 0x0000000000000008\n  rax 0\n";
        assert_eq!(
            read(write),
            Some(Diagnosis::Crashed(
                "EXCEPTION_ACCESS_VIOLATION (write of 0x0000000000000008)".into()
            ))
        );
        let with_rip = "FATAL: unhandled exception 0xC0000005 on thread 1 'main'\n  rip 0x1234 eboot.exe+0x44\n  read of 0x0000000000000010\n";
        assert_eq!(
            read(with_rip),
            Some(Diagnosis::Crashed(
                "EXCEPTION_ACCESS_VIOLATION (read of 0x0000000000000010)".into()
            ))
        );
        let plain = "FATAL: unhandled exception 0xc000001d on thread 3 'x'\n  rip 0x1\n";
        assert_eq!(
            read(plain),
            Some(Diagnosis::Crashed("EXCEPTION_ILLEGAL_INSTRUCTION".into()))
        );
        let unknown = "FATAL: unhandled exception 0xe06d7363 on thread 3 'x'\n";
        assert_eq!(read(unknown), Some(Diagnosis::Crashed("0xE06D7363".into())));
        assert_eq!(
            read("FATAL: abort() (SIGABRT) on thread 3\n"),
            Some(Diagnosis::Crashed("abort()".into()))
        );
        assert_eq!(
            read("FATAL: UCRT invalid parameter on thread 3\n"),
            Some(Diagnosis::Crashed("invalid parameter".into()))
        );
        assert_eq!(
            read(
                "FATAL: C++ exception St9bad_alloc raised on thread 4 cannot be delivered: 0x1 has no unwind data"
            ),
            Some(Diagnosis::EngineError(
                "C++ exception St9bad_alloc cannot be delivered".into()
            ))
        );
    }

    #[test]
    fn priority_follows_the_arm_order() {
        let fatal = "FATAL: unhandled exception 0xc0000005 on thread 1 'x'\n";
        let module = format!("Failed to load module: a.prx\n{fatal}");
        assert!(matches!(read(&module), Some(Diagnosis::MissingModule(_))));
        let gpu = format!("Vulkan result -4\n{fatal}");
        assert_eq!(read(&gpu), Some(Diagnosis::GpuLost));
        // The uncaught exception beats VK_ERROR_DEVICE_LOST in the same log.
        let both = "VK_ERROR_DEVICE_LOST\nterminate called after throwing an instance of 'std::runtime_error'\n  what():  sceX not implemented\n";
        assert_eq!(read(both), Some(Diagnosis::Unsupported("sceX".into())));
        // The log beats the exit status.
        let with_exit = Diagnosis::read("Vulkan result -4", Some(&exit(0xC000_0005)));
        assert_eq!(with_exit, Some(Diagnosis::GpuLost));
    }

    #[test]
    fn exit_codes_alone() {
        let d = |code: Option<u32>| {
            Diagnosis::read(
                "",
                Some(&ExitInfo {
                    code,
                    had_window: true,
                }),
            )
        };
        assert_eq!(
            d(Some(0xC000_0005)),
            Some(Diagnosis::Crashed("EXCEPTION_ACCESS_VIOLATION".into()))
        );
        assert_eq!(d(Some(0xC000_0017)), Some(Diagnosis::OutOfMemory));
        assert_eq!(d(Some(3)), Some(Diagnosis::Crashed("abort()".into())));
        assert_eq!(
            d(Some(0x8000_0003)),
            Some(Diagnosis::Crashed("EXCEPTION_BREAKPOINT".into()))
        );
        assert_eq!(
            d(Some(0xC123_4567)),
            Some(Diagnosis::Crashed("0xC1234567".into()))
        );
        assert_eq!(d(Some(0)), None);
        assert_eq!(d(Some(1)), None);
        assert_eq!(d(None), None);
        assert_eq!(Diagnosis::read("", None), None);
    }

    #[test]
    fn classify_trims_cuts_and_matches_at_the_start_only() {
        assert_eq!(
            Diagnosis::classify("  \tsceFoo not implemented\t "),
            Diagnosis::Unsupported("sceFoo".into())
        );
        assert_eq!(
            Diagnosis::classify("error: sceFoo not implemented"),
            Diagnosis::EngineError("error: sceFoo not implemented".into())
        );
        assert_eq!(
            Diagnosis::classify("The Device Lost event"),
            Diagnosis::GpuLost
        );
        assert_eq!(
            Diagnosis::classify("Out Of Memory in pool"),
            Diagnosis::OutOfMemory
        );
        let long: String = "ü".repeat(300);
        match Diagnosis::classify(&long) {
            Diagnosis::EngineError(text) => {
                assert_eq!(text.chars().count(), 201);
                assert!(text.ends_with('…'));
            }
            other => panic!("{other:?}"),
        }
        let exactly: String = "x".repeat(200);
        assert_eq!(
            Diagnosis::classify(&exactly),
            Diagnosis::EngineError(exactly.clone())
        );
    }

    #[test]
    fn last_session_cuts_at_the_last_marker() {
        let m = Diagnosis::START_MARKER;
        assert_eq!(
            Diagnosis::last_session(&format!("old\n{m} t1 A\nfirst\n{m} t2 A\nsecond")),
            " t2 A\nsecond"
        );
        assert_eq!(Diagnosis::last_session(&format!("x\n{m}")), "");
        let big = "ä".repeat(100_000);
        let tail = Diagnosis::last_session(&big);
        assert!(tail.len() <= 64 * 1024);
        assert!(tail.len() > 64 * 1024 - 4);
        assert_eq!(Diagnosis::last_session("short"), "short");
    }

    #[test]
    fn reads_only_the_tail_of_a_big_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("A.log");
        let mut text = String::from("OLD-START\n");
        text.push_str(&"x".repeat(5_000_000));
        text.push_str(&format!("\n{}\nlast run\n", Diagnosis::START_MARKER));
        fs::write(&path, &text).unwrap();
        assert_eq!(read_last_session(&path), "\nlast run\n");
        let no_marker = dir.path().join("B.log");
        fs::write(&no_marker, "x".repeat(5_000_000)).unwrap();
        assert_eq!(read_last_session(&no_marker).len(), 64 * 1024);
        assert_eq!(read_last_session(&dir.path().join("missing.log")), "");
    }

    #[test]
    fn summaries_and_keys() {
        let (key, args) = Diagnosis::MissingModule("libSceFoo.prx".into()).summary_key();
        assert_eq!(
            (key, args),
            ("diagnosisMissingModule", vec!["libSceFoo.prx".to_string()])
        );
        assert_eq!(
            Diagnosis::MissingModule("libSceFoo.prx".into()).summary(),
            "Missing module: libSceFoo.prx. Unlocking the game again may fix it."
        );
        assert!(
            Diagnosis::MissingModule("libstdc++-6.dll".into())
                .summary()
                .contains("MinGW runtime")
        );
        assert_eq!(Diagnosis::GpuLost.summary(), "The GPU stopped responding.");
        assert_eq!(
            Diagnosis::Crashed("abort()".into()).summary(),
            "The game crashed (abort())."
        );
        assert_eq!(
            Diagnosis::HostCpu("AVX2".into()).summary(),
            "This CPU lacks AVX2, which the game needs."
        );
    }

    #[test]
    fn crash_name_from_block_or_exit_status() {
        let block = "FATAL: unhandled exception 0xc0000005 on thread 1 'x'\n";
        assert_eq!(
            Diagnosis::crash_name(block, None).as_deref(),
            Some("EXCEPTION_ACCESS_VIOLATION")
        );
        assert_eq!(
            Diagnosis::crash_name("", Some(&exit(0xC000_0409))).as_deref(),
            Some("STATUS_FAIL_FAST_EXCEPTION")
        );
        assert_eq!(Diagnosis::crash_name("", Some(&exit(1))), None);
    }
}
