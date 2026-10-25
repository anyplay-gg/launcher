//! The report: a text file with the PC, the versions, the game's settings, the diagnosis and the
//! log of the last start, for the player to attach to a bug report (PORT.md 6.11).

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::paths::AppPaths;

/// The report keeps this much of the session's end.
pub const LOG_KEEP: usize = 400 * 1024;

/// The report also keeps this much of the end of the launcher's own log.
pub const LAUNCHER_LOG_KEEP: usize = 100 * 1024;

/// What the PC is, as the report lists it. Collected once; injected into tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemInfo {
    pub cpu: String,
    pub threads: usize,
    pub ram_gb: u64,
    pub gpus: Vec<String>,
    /// The display driver of each GPU in `gpus`, empty where unknown.
    pub drivers: Vec<String>,
    /// `<ProductName> <DisplayVersion> (build <CurrentBuild>.<UBR>)`
    pub windows: String,
}

impl SystemInfo {
    /// Reads this PC. Slow enough (DXGI, registry) to belong on the `report` thread.
    pub fn collect() -> SystemInfo {
        collect_system()
    }

    /// One entry per GPU: its name and, when known, `(driver <version>)`.
    pub fn gpu_lines(&self) -> Vec<String> {
        self.gpus
            .iter()
            .enumerate()
            .map(
                |(i, name)| match self.drivers.get(i).filter(|d| !d.is_empty()) {
                    Some(driver) => format!("{name} (driver {driver})"),
                    None => name.clone(),
                },
            )
            .collect()
    }
}

/// Everything a report says.
#[derive(Clone, Debug)]
pub struct ReportInput {
    pub game_name: String,
    pub title_id: String,
    pub version: String,
    pub system: SystemInfo,
    pub launcher_version: String,
    /// `None`: no toolkit.
    pub toolkit_version: Option<String>,
    /// The diagnosis' English summary, if one was found.
    pub reason: Option<String>,
    /// The game's settings as JSON; printed pretty with sorted keys.
    pub settings: serde_json::Value,
    /// The exception name for the `[Crash report: ...]` line.
    pub crash: Option<String>,
    /// The log text of the last start.
    pub session: String,
    /// RFC 3339, UTC.
    pub now: String,
}

/// The report text: the lines in the order of `DiagnosticReport.text`, with a trailing newline.
pub fn diagnostic_text(input: &ReportInput) -> String {
    let system = &input.system;
    let settings = serde_json::to_string_pretty(&input.settings).unwrap_or_else(|_| "{}".into());
    let mut lines = vec![
        format!("AnyPlay Launcher report, {}", input.now),
        String::new(),
        format!(
            "Game: {} ({}, version {})",
            input.game_name, input.title_id, input.version
        ),
        format!(
            "PC: {}, {} threads, {} GB",
            or_unknown(&system.cpu),
            system.threads,
            system.ram_gb
        ),
        format!(
            "GPU: {}",
            if system.gpus.is_empty() {
                "unknown".to_string()
            } else {
                system.gpu_lines().join("; ")
            }
        ),
        format!(
            "{}: {}",
            if system.windows.starts_with("Windows") || (system.windows.is_empty() && cfg!(windows))
            {
                "Windows"
            } else {
                "System"
            },
            or_unknown(&system.windows)
        ),
        format!("Launcher: {}", input.launcher_version),
        format!(
            "Toolkit: {}",
            input.toolkit_version.as_deref().unwrap_or("none")
        ),
        format!(
            "Reason: {}",
            input.reason.as_deref().unwrap_or("none found")
        ),
        String::new(),
        "Settings:".to_string(),
        settings,
    ];
    if let Some(crash) = &input.crash {
        lines.push(String::new());
        lines.push(format!("Crash report: {crash}"));
    }
    lines.push(String::new());
    lines.push("Log of the last start:".to_string());
    lines.push(tail(&input.session, LOG_KEEP).to_string());
    lines.join("\n") + "\n"
}

fn or_unknown(text: &str) -> &str {
    if text.is_empty() { "unknown" } else { text }
}

/// The last `limit` bytes of `text`, cut at a character boundary.
fn tail(text: &str, limit: usize) -> &str {
    if text.len() <= limit {
        return text;
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// Writes the report to `%LOCALAPPDATA%\AnyPS5\Logs\Reports\<TITLEID>-<yyyyMMdd-HHmmss>.txt`
/// (local time) and returns the path.
pub fn write_report(paths: &AppPaths, input: &ReportInput) -> io::Result<PathBuf> {
    let dir = paths.reports_dir();
    fs::create_dir_all(&dir)?;
    let safe: String = input
        .title_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let file = dir.join(format!("{safe}-{}.txt", file_stamp()));
    let mut text = diagnostic_text(input);
    if let Some(section) = launcher_log_section(&paths.launcher_log()) {
        text.push('\n');
        text.push_str(&section);
    }
    fs::write(&file, text)?;
    Ok(file)
}

/// The "Launcher log" section: the last `LAUNCHER_LOG_KEEP` bytes of the launcher's own log,
/// read from the end so a full file costs no more than its tail. `None` when there is no log.
fn launcher_log_section(path: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(
        len.saturating_sub(LAUNCHER_LOG_KEEP as u64),
    ))
    .ok()?;
    let mut bytes = Vec::new();
    file.take(LAUNCHER_LOG_KEEP as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // A cut lands in the middle of a line (and maybe of a character): drop that first fragment.
    let text = if len > LAUNCHER_LOG_KEEP as u64 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    Some(format!(
        "Launcher log (last 100 KiB):\n{}\n",
        tail(text, LAUNCHER_LOG_KEEP).trim_end()
    ))
}

fn file_stamp() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let format = time::macros::format_description!("[year][month][day]-[hour][minute][second]");
    now.format(&format).unwrap_or_else(|_| "report".into())
}

/// D24: shows a file in Explorer (`explorer.exe /select,<path>`, no shell). Elsewhere its folder opens.
pub fn reveal_in_explorer(path: &std::path::Path) -> io::Result<()> {
    crate::platform::reveal(path)
}

#[cfg(windows)]
fn collect_system() -> SystemInfo {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: the structure is initialised with its size.
    let ram = unsafe { GlobalMemoryStatusEx(&mut status) }
        .map(|()| status.ullTotalPhys / (1 << 30))
        .unwrap_or(0);
    let adapters = gpu_adapters();
    SystemInfo {
        cpu: crate::hostcpu::HostCpu::detect().brand,
        threads: std::thread::available_parallelism().map_or(0, |n| n.get()),
        ram_gb: ram,
        gpus: adapters.iter().map(|a| a.0.clone()).collect(),
        drivers: adapters.into_iter().map(|a| a.1).collect(),
        windows: windows_version(),
    }
}

#[cfg(not(windows))]
fn collect_system() -> SystemInfo {
    let read = |file: &str| std::fs::read_to_string(file).unwrap_or_default();
    SystemInfo {
        cpu: crate::hostcpu::HostCpu::detect().brand,
        threads: std::thread::available_parallelism().map_or(0, |n| n.get()),
        ram_gb: meminfo_gb(&read("/proc/meminfo")),
        windows: os_release_name(&read("/etc/os-release")),
        ..SystemInfo::default()
    }
}

/// Total memory in GiB from the text of `/proc/meminfo`; 0 when it has no `MemTotal`.
#[cfg(any(not(windows), test))]
fn meminfo_gb(text: &str) -> u64 {
    text.lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))
        .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
        .map_or(0, |kib| kib / (1 << 20))
}

/// `PRETTY_NAME` out of the text of `/etc/os-release`; empty when it has none.
#[cfg(any(not(windows), test))]
fn os_release_name(text: &str) -> String {
    text.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim().trim_matches('"').to_string())
        .unwrap_or_default()
}

/// Hardware adapters as (name, driver version); software ones, like the basic render driver,
/// are left out.
#[cfg(windows)]
fn gpu_adapters() -> Vec<(String, String)> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIDevice, IDXGIFactory1,
    };
    use windows::core::Interface;
    let mut names = Vec::new();
    // SAFETY: plain COM calls on interfaces we own.
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return names;
        };
        let mut index = 0;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            index += 1;
            let Ok(desc) = adapter.GetDesc1() else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let end = desc
                .Description
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(desc.Description.len());
            let driver = adapter
                .CheckInterfaceSupport(&IDXGIDevice::IID)
                .map(|v| driver_version(v, desc.VendorId))
                .unwrap_or_default();
            names.push((String::from_utf16_lossy(&desc.Description[..end]), driver));
        }
    }
    names
}

/// The user-mode driver version DXGI reports (`a.b.c.d`); for NVIDIA also the number on the
/// driver's download page, the last five digits of `c.d` (31.0.15.5222 is 552.22).
#[cfg(windows)]
fn driver_version(raw: i64, vendor: u32) -> String {
    if raw <= 0 {
        return String::new();
    }
    let part = |shift: u32| (raw >> shift) & 0xFFFF;
    let (a, b, c, d) = (part(48), part(32), part(16), part(0));
    if vendor == 0x10DE {
        let digits = format!("{c}{d:04}");
        let tail = &digits[digits.len().saturating_sub(5)..];
        if tail.len() == 5 {
            return format!("{}.{} ({a}.{b}.{c}.{d})", &tail[..3], &tail[3..]);
        }
    }
    format!("{a}.{b}.{c}.{d}")
}

#[cfg(windows)]
fn windows_version() -> String {
    use windows::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows::core::{PCWSTR, w};

    const KEY: PCWSTR = w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion");
    let text = |name: PCWSTR| -> Option<String> {
        let mut buffer = [0u16; 256];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: the buffer and its size describe the same memory.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                KEY,
                name,
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr() as *mut _),
                Some(&mut size),
            )
        };
        if status.is_err() {
            return None;
        }
        let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len]))
    };
    let ubr = {
        let mut value = 0u32;
        let mut size = 4u32;
        // SAFETY: a DWORD value into a u32.
        let status = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                KEY,
                w!("UBR"),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut value as *mut u32 as *mut _),
                Some(&mut size),
            )
        };
        status.is_ok().then_some(value)
    };
    let product = text(w!("ProductName")).unwrap_or_default();
    let display = text(w!("DisplayVersion")).unwrap_or_default();
    let build = text(w!("CurrentBuild")).unwrap_or_default();
    let mut out = format!("{product} {display}").trim().to_string();
    if !build.is_empty() {
        match ubr {
            Some(ubr) => out.push_str(&format!(" (build {build}.{ubr})")),
            None => out.push_str(&format!(" (build {build})")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_system_facts_come_from_proc_and_os_release() {
        assert_eq!(meminfo_gb("MemTotal:  32768000 kB\nMemFree: 1 kB\n"), 31);
        assert_eq!(meminfo_gb("nothing"), 0);
        let release = "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\n";
        assert_eq!(os_release_name(release), "Arch Linux");
        assert_eq!(os_release_name(""), "");
    }

    fn input() -> ReportInput {
        ReportInput {
            game_name: "Stray".into(),
            title_id: "PPSA02100".into(),
            version: "01.000.000".into(),
            system: SystemInfo {
                cpu: "AMD Ryzen 5 3600 6-Core Processor".into(),
                threads: 12,
                ram_gb: 31,
                gpus: vec![
                    "NVIDIA GeForce RTX 3060".into(),
                    "AMD Radeon(TM) Graphics".into(),
                ],
                drivers: vec!["552.22 (31.0.15.5222)".into(), String::new()],
                windows: "Windows 11 Home 25H2 (build 26200.1000)".into(),
            },
            launcher_version: "v0.1.0-3-gabc".into(),
            toolkit_version: Some("0.1.3-abcdef123456".into()),
            reason: Some("The GPU stopped responding.".into()),
            settings: serde_json::json!({"windowSize": 75, "fullscreen": false, "vsync": true, "overlay": "off"}),
            crash: Some("EXCEPTION_ACCESS_VIOLATION".into()),
            session: " 2026-10-22T09:30:00Z PPSA02100\nline 1\nline 2".into(),
            now: "2026-10-22T09:31:00Z".into(),
        }
    }

    #[test]
    fn golden_report() {
        let expected = "\
AnyPlay Launcher report, 2026-10-22T09:31:00Z

Game: Stray (PPSA02100, version 01.000.000)
PC: AMD Ryzen 5 3600 6-Core Processor, 12 threads, 31 GB
GPU: NVIDIA GeForce RTX 3060 (driver 552.22 (31.0.15.5222)); AMD Radeon(TM) Graphics
Windows: Windows 11 Home 25H2 (build 26200.1000)
Launcher: v0.1.0-3-gabc
Toolkit: 0.1.3-abcdef123456
Reason: The GPU stopped responding.

Settings:
{
  \"fullscreen\": false,
  \"overlay\": \"off\",
  \"vsync\": true,
  \"windowSize\": 75
}

Crash report: EXCEPTION_ACCESS_VIOLATION

Log of the last start:
 2026-10-22T09:30:00Z PPSA02100
line 1
line 2
";
        assert_eq!(diagnostic_text(&input()), expected);
    }

    #[test]
    fn unknowns_and_no_crash_line() {
        let mut i = input();
        i.crash = None;
        i.reason = None;
        i.toolkit_version = None;
        i.system = SystemInfo::default();
        let text = diagnostic_text(&i);
        assert!(text.contains("PC: unknown, 0 threads, 0 GB\n"));
        assert!(text.contains("GPU: unknown\n"));
        let label = if cfg!(windows) { "Windows" } else { "System" };
        assert!(text.contains(&format!("{label}: unknown\n")));
        assert!(text.contains("Toolkit: none\n"));
        assert!(text.contains("Reason: none found\n"));
        assert!(!text.contains("Crash report"));
    }

    #[test]
    fn the_log_is_cut_to_its_last_400_kib() {
        let mut i = input();
        i.session = format!("START{}END", "é".repeat(300_000));
        let text = diagnostic_text(&i);
        let log = text.split("Log of the last start:\n").nth(1).unwrap();
        assert!(log.len() <= LOG_KEEP + 1);
        assert!(log.ends_with("END\n"));
        assert!(!log.contains("START"));
    }

    #[test]
    fn writes_the_file_under_the_reports_folder() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let file = write_report(&paths, &input()).unwrap();
        assert!(file.starts_with(paths.reports_dir()));
        let name = file.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("PPSA02100-") && name.ends_with(".txt"),
            "{name}"
        );
        assert_eq!(name.len(), "PPSA02100-20261022-093100.txt".len());
        assert_eq!(fs::read_to_string(file).unwrap(), diagnostic_text(&input()));
    }

    #[test]
    fn no_launcher_log_no_section() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let file = write_report(&paths, &input()).unwrap();
        assert!(!fs::read_to_string(file).unwrap().contains("Launcher log"));
    }

    #[test]
    fn the_report_ends_with_the_launcher_log_tail() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        fs::create_dir_all(paths.logs_dir()).unwrap();
        fs::write(
            paths.launcher_log(),
            "short\nPANIC thread=main at a.rs:1: x\n",
        )
        .unwrap();
        let text = fs::read_to_string(write_report(&paths, &input()).unwrap()).unwrap();
        assert!(text.starts_with(&diagnostic_text(&input())));
        assert!(
            text.ends_with("Launcher log (last 100 KiB):\nshort\nPANIC thread=main at a.rs:1: x\n")
        );
    }

    #[test]
    fn a_long_launcher_log_is_cut_at_a_character_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        fs::create_dir_all(paths.logs_dir()).unwrap();
        let log = format!("START\n{}END\n", "é\n".repeat(120_000));
        fs::write(paths.launcher_log(), &log).unwrap();
        let section = launcher_log_section(&paths.launcher_log()).unwrap();
        let body = section
            .strip_prefix("Launcher log (last 100 KiB):\n")
            .unwrap();
        assert!(body.len() <= LAUNCHER_LOG_KEEP + 1);
        assert!(body.ends_with("END\n"));
        assert!(!body.contains("START"));
        assert!(!body.contains('\u{fffd}'));
        assert!(body.starts_with("é\n"));
    }

    #[cfg(windows)]
    #[test]
    fn this_pc_can_be_described() {
        let info = SystemInfo::collect();
        assert!(info.threads > 0);
        assert!(info.ram_gb > 0);
        assert!(info.windows.starts_with("Windows"), "{}", info.windows);
        assert!(info.windows.contains("build"), "{}", info.windows);
        eprintln!("{info:?}");
    }
}
