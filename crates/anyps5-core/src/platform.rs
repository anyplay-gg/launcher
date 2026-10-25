//! Everything the pipeline needs to know about the operating system, in one place: the names of
//! the toolkit's files and release assets, where launcher data lives, where Discord's socket can
//! be, how a folder is shown, and how a game's process tree is ended.
//!
//! Interface: [`Os`] (and [`Os::current`]) answers the "what is it called here" questions as pure
//! values, so both operating systems are tested on either host. [`open_folder`], [`reveal`],
//! [`isolate_group`] and [`kill_group`] are the few calls that touch the system. Windows keeps its
//! job objects in [`crate::win`]; Linux and other Unix systems use process groups.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Refuse Linux/Windows executables before macOS turns them into an opaque spawn failure.
pub fn require_macho(path: &Path) -> io::Result<()> {
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)?.read_exact(&mut magic)?;
    if matches!(
        magic,
        [0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] | [0xca, 0xfe, 0xba, 0xbf]
    ) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is not a macOS Mach-O executable; select a macOS toolkit and unlock again",
                path.display()
            ),
        ))
    }
}

/// An operating system the launcher can run games on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Linux,
    Macos,
}

impl Os {
    /// The system this build runs on. macOS is distinct from Linux.
    pub const fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }

    /// The relinked game executable inside an unlocked copy.
    pub const fn executable(self) -> &'static str {
        match self {
            Os::Windows => "eboot.exe",
            Os::Linux | Os::Macos => "eboot",
        }
    }

    /// The key of this system in `release.json` of the toolkit's download pages.
    pub const fn platform_key(self) -> &'static str {
        match self {
            Os::Windows => "windows",
            Os::Linux => "linux",
            Os::Macos => "macos",
        }
    }

    /// The extra relinker flags (flag and value pairs) that apply to this system: the flags of
    /// another system's output format (`--windows-...`) are left out.
    pub fn relink_flags(self, flags: &[String]) -> Vec<String> {
        let allowed = |flag: &str| {
            [
                ("--windows", Os::Windows),
                ("--linux", Os::Linux),
                ("--macos", Os::Macos),
            ]
            .iter()
            .all(|(prefix, os)| !flag.starts_with(prefix) || *os == self)
        };
        flags
            .chunks(2)
            .filter(|pair| allowed(&pair[0]))
            .flatten()
            .cloned()
            .collect()
    }

    /// The relinker inside a toolkit folder.
    pub const fn relinker_file(self) -> &'static str {
        match self {
            Os::Windows => "relinker.exe",
            Os::Linux | Os::Macos => "relinker",
        }
    }

    /// Options that pick the output format. The relinker writes a Linux ELF unless told otherwise.
    pub const fn relinker_args(self) -> &'static [&'static str] {
        match self {
            Os::Windows => &["--windows"],
            Os::Linux => &[],
            Os::Macos => &["--macos"],
        }
    }

    /// Runtime libraries the system libraries need beside them. Only the MinGW build has any.
    pub const fn runtime_libs(self) -> &'static [&'static str] {
        match self {
            Os::Windows => &crate::toolkit::MINGW_DLLS,
            Os::Linux | Os::Macos => &[],
        }
    }

    /// The release asset with the relinker, `tag` as published (`v0.1.3`).
    pub fn relinker_asset(self, tag: &str) -> Option<String> {
        match self {
            Os::Windows => Some(format!("relinker-{tag}.exe")),
            Os::Linux => Some(format!("relinker-{tag}")),
            Os::Macos => None,
        }
    }

    /// The release asset with the system libraries (`libs/*.prx`).
    pub fn libs_asset(self, tag: &str) -> String {
        match self {
            Os::Windows => format!("prx-windows-{tag}.zip"),
            Os::Linux => format!("prx-linux-{tag}.zip"),
            Os::Macos => format!("AnyPS5-toolkit-macos-{}.zip", tag.trim_start_matches('v')),
        }
    }

    /// The name of the launcher's own folders below the config and data roots.
    const fn app_folder(self) -> &'static str {
        match self {
            Os::Windows | Os::Macos => "AnyPS5",
            Os::Linux => "anyps5",
        }
    }
}

/// Reads one environment variable; empty values count as unset. A parameter so tests can fake it.
pub type Vars<'a> = &'a dyn Fn(&str) -> Option<OsString>;

/// The real environment.
pub fn process_vars(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|v| !v.is_empty())
}

/// The three roots of [`crate::paths::AppPaths`]: config, data, pictures.
pub struct Roots {
    pub config: PathBuf,
    pub data: PathBuf,
    pub pictures: PathBuf,
}

/// Native macOS locations. XDG variables belong to Linux and do not override these folders.
pub fn macos_roots(var: Vars) -> Option<Roots> {
    let home = PathBuf::from(var("HOME")?);
    if !is_unix_absolute(&home) {
        return None;
    }
    let support = home.join("Library/Application Support/AnyPlay Launcher");
    Some(Roots {
        config: support.clone(),
        data: support,
        pictures: home.join("Pictures"),
    })
}

/// Where the launcher keeps its files. Windows: `%APPDATA%\AnyPS5` and `%LOCALAPPDATA%\AnyPS5`.
/// Linux: the XDG base directories (`$XDG_CONFIG_HOME` or `~/.config`, `$XDG_DATA_HOME` or
/// `~/.local/share`), and the Pictures folder from `$XDG_PICTURES_DIR` or `user-dirs.dirs`.
/// `None` when no home can be found. `user_dirs` is the text of `user-dirs.dirs`, if any.
pub fn roots(os: Os, var: Vars, user_dirs: Option<&str>) -> Option<Roots> {
    let path = |name: &str| var(name).map(PathBuf::from);
    let absolute = |name: &str| path(name).filter(|p| is_unix_absolute(p));
    match os {
        Os::Macos => macos_roots(var),
        Os::Windows => {
            let profile = path("USERPROFILE").or_else(|| path("HOME"));
            let config = path("APPDATA")
                .or_else(|| profile.as_ref().map(|p| p.join("AppData").join("Roaming")))?;
            let data = path("LOCALAPPDATA")
                .or_else(|| profile.as_ref().map(|p| p.join("AppData").join("Local")))?;
            let pictures = profile
                .map(|p| p.join("Pictures"))
                .unwrap_or_else(|| data.clone());
            Some(Roots {
                config: config.join(os.app_folder()),
                data: data.join(os.app_folder()),
                pictures,
            })
        }
        Os::Linux => {
            let home = absolute("HOME");
            let config =
                absolute("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|h| h.join(".config")))?;
            let data = absolute("XDG_DATA_HOME")
                .or_else(|| home.as_ref().map(|h| h.join(".local").join("share")))?;
            let pictures = absolute("XDG_PICTURES_DIR")
                .or_else(|| {
                    let home = home.as_ref()?;
                    user_dirs.and_then(|text| pictures_from_user_dirs(text, home))
                })
                .or_else(|| home.as_ref().map(|h| h.join("Pictures")))
                .unwrap_or_else(|| data.clone());
            Some(Roots {
                config: config.join(os.app_folder()),
                data: data.join(os.app_folder()),
                pictures,
            })
        }
    }
}

/// Whether a path starts at the root of a Unix file system. (`Path::is_absolute` asks the host, so
/// it would call these paths relative on Windows and the tests could not run there.)
fn is_unix_absolute(path: &Path) -> bool {
    path.to_str().is_some_and(|p| p.starts_with('/'))
}

/// The text of `~/.config/user-dirs.dirs`, or `$XDG_CONFIG_HOME/user-dirs.dirs`.
pub fn read_user_dirs(var: Vars) -> Option<String> {
    let base = var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| is_unix_absolute(p))
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    std::fs::read_to_string(base.join("user-dirs.dirs")).ok()
}

/// `XDG_PICTURES_DIR="$HOME/Pictures"` out of a `user-dirs.dirs` file. Only `$HOME` and absolute
/// values are valid there; anything else is ignored.
pub fn pictures_from_user_dirs(text: &str, home: &Path) -> Option<PathBuf> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("XDG_PICTURES_DIR="))?;
    let value = line.split_once('=')?.1.trim().trim_matches('"').to_string();
    if value == "$HOME" {
        return Some(home.to_path_buf());
    }
    if let Some(rest) = value.strip_prefix("$HOME/") {
        return Some(home.join(rest));
    }
    let absolute = PathBuf::from(&value);
    is_unix_absolute(&absolute).then_some(absolute)
}

/// The places Discord's IPC socket `discord-ipc-<index>` can be on Linux, best first: the runtime
/// folder itself, the folders the Flatpak and Snap builds of Discord (and Vesktop) use below it,
/// then `$TMPDIR` and `/tmp` for old setups. This is the search order of the Discord RPC client
/// libraries (discord-rpc, arRPC).
pub fn discord_socket_candidates(index: u32, var: Vars) -> Vec<PathBuf> {
    let name = format!("discord-ipc-{index}");
    let mut folders: Vec<PathBuf> = Vec::new();
    if let Some(runtime) = var("XDG_RUNTIME_DIR").map(PathBuf::from) {
        folders.push(runtime.clone());
        for sub in [
            "app/com.discordapp.Discord",
            "app/com.discordapp.DiscordCanary",
            "app/dev.vencord.Vesktop",
            "snap.discord",
            "snap.discord-canary",
        ] {
            folders.push(runtime.join(sub));
        }
    }
    if let Some(tmp) = var("TMPDIR").map(PathBuf::from) {
        folders.push(tmp);
    }
    folders.push(PathBuf::from("/tmp"));
    let mut seen = Vec::new();
    folders
        .into_iter()
        .filter(|f| {
            let fresh = !seen.contains(f);
            seen.push(f.clone());
            fresh
        })
        .map(|f| f.join(&name))
        .collect()
}

/// What `xdg-open` should be given to show `target`: the folder itself, or for a file the folder
/// that holds it (a file manager cannot be asked to select a file portably).
pub fn xdg_open_target(target: &Path, reveal: bool) -> PathBuf {
    if reveal && !target.is_dir() {
        target.parent().unwrap_or(target).to_path_buf()
    } else {
        target.to_path_buf()
    }
}

/// Shows a folder in the system's file manager.
pub fn open_folder(folder: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        Command::new("explorer.exe").arg(folder).spawn().map(drop)
    }
    #[cfg(target_os = "macos")]
    {
        desktop_open(&[folder.as_os_str()])
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        xdg_open(xdg_open_target(folder, false))
    }
}

/// Shows a file in the file manager. Windows and macOS select it; elsewhere its folder opens.
pub fn reveal(path: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // `/select,<path>` must be one argument; raw_arg keeps the quoting Explorer expects.
        Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", path.display()))
            .spawn()
            .map(drop)
    }
    #[cfg(target_os = "macos")]
    {
        desktop_open(&[std::ffi::OsStr::new("-R"), path.as_os_str()])
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        xdg_open(xdg_open_target(path, true))
    }
}

/// Opens a web address in the default browser.
pub fn open_url(url: &str) -> io::Result<()> {
    #[cfg(windows)]
    {
        shell_open(url)
    }
    #[cfg(target_os = "macos")]
    {
        desktop_open(&[std::ffi::OsStr::new(url)])
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        xdg_open(url)
    }
}

/// Asks Windows to open `target` with its default program, as a double click would
/// (`ShellExecuteW`, no shell and no Explorer). `explorer.exe <address>` opened the Documents
/// folder next to the browser once the address was longer than a path may be, as the sign-in
/// address is. The call runs on a thread of its own with COM set up, as the shell wants.
#[cfg(windows)]
fn shell_open(target: &str) -> io::Result<()> {
    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};

    let target = HSTRING::from(target);
    std::thread::Builder::new()
        .name("shell-open".into())
        .spawn(move || {
            // SAFETY: COM is set up and torn down on this thread only; the strings outlive the call.
            unsafe {
                let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
                let result = ShellExecuteW(None, w!("open"), &target, None, None, SW_SHOWNORMAL);
                if com.is_ok() {
                    CoUninitialize();
                }
                // Above 32 is success; below it is an error code, with the reason in the last error.
                if result.0 as usize > 32 {
                    Ok(())
                } else {
                    Err(io::Error::last_os_error())
                }
            }
        })?
        .join()
        .unwrap_or_else(|_| Err(io::Error::other("the shell call panicked")))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn xdg_open(target: impl AsRef<std::ffi::OsStr>) -> io::Result<()> {
    spawn_desktop(Command::new("xdg-open").arg(target.as_ref()))
}

#[cfg(target_os = "macos")]
fn desktop_open(args: &[&std::ffi::OsStr]) -> io::Result<()> {
    spawn_desktop(Command::new("/usr/bin/open").args(args))
}

#[cfg(not(windows))]
fn spawn_desktop(command: &mut Command) -> io::Result<()> {
    use std::process::Stdio;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Reap it when it ends so no zombie stays behind.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Puts the child into a process group of its own, so [`kill_group`] reaches everything it starts.
/// Windows uses job objects instead (see [`crate::win`]).
pub fn isolate_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

/// Ends the process group `pid` leads. Returns whether the signal was delivered.
pub fn kill_group(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // SAFETY: plain system call; a negative pid addresses the process group.
        unsafe { libc::kill(-pid, libc::SIGKILL) == 0 }
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        let pairs = pairs.to_vec();
        move |name| {
            pairs
                .iter()
                .find(|(k, v)| *k == name && !v.is_empty())
                .map(|(_, v)| OsString::from(*v))
        }
    }

    #[test]
    fn names_follow_the_system() {
        assert_eq!(Os::Windows.executable(), "eboot.exe");
        assert_eq!(Os::Linux.executable(), "eboot");
        assert_eq!(Os::Windows.relinker_args(), ["--windows"]);
        assert!(Os::Linux.relinker_args().is_empty());
        assert_eq!(Os::Windows.runtime_libs().len(), 3);
        assert!(Os::Linux.runtime_libs().is_empty());
    }

    #[test]
    fn relink_flags_of_another_output_format_are_dropped() {
        let flags: Vec<String> = ["--windows-fixed-base", "0x400000", "--rpath", "x"]
            .map(String::from)
            .into();
        assert_eq!(Os::Windows.relink_flags(&flags), flags);
        assert_eq!(Os::Linux.relink_flags(&flags), ["--rpath", "x"]);
        assert_eq!(Os::Windows.platform_key(), "windows");
    }

    #[test]
    fn release_assets_match_upstream_names() {
        assert_eq!(
            Os::Windows.relinker_asset("v0.1.3").as_deref(),
            Some("relinker-v0.1.3.exe")
        );
        assert_eq!(Os::Windows.libs_asset("v0.1.3"), "prx-windows-v0.1.3.zip");
        assert_eq!(
            Os::Linux.relinker_asset("v0.1.3").as_deref(),
            Some("relinker-v0.1.3")
        );
        assert_eq!(Os::Linux.libs_asset("v0.1.3"), "prx-linux-v0.1.3.zip");
    }

    #[test]
    fn windows_roots_follow_appdata_with_a_profile_fallback() {
        let r = roots(
            Os::Windows,
            &vars(&[
                ("APPDATA", "C:/R"),
                ("LOCALAPPDATA", "C:/L"),
                ("USERPROFILE", "C:/U"),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(r.config, Path::new("C:/R/AnyPS5"));
        assert_eq!(r.data, Path::new("C:/L/AnyPS5"));
        assert_eq!(r.pictures, Path::new("C:/U/Pictures"));
        let r = roots(Os::Windows, &vars(&[("USERPROFILE", "C:/U")]), None).unwrap();
        assert_eq!(r.config, Path::new("C:/U/AppData/Roaming/AnyPS5"));
        assert_eq!(r.data, Path::new("C:/U/AppData/Local/AnyPS5"));
        assert!(roots(Os::Windows, &vars(&[]), None).is_none());
    }

    #[test]
    fn linux_roots_default_to_the_home_folder() {
        let r = roots(Os::Linux, &vars(&[("HOME", "/home/mira")]), None).unwrap();
        assert_eq!(r.config, Path::new("/home/mira/.config/anyps5"));
        assert_eq!(r.data, Path::new("/home/mira/.local/share/anyps5"));
        assert_eq!(r.pictures, Path::new("/home/mira/Pictures"));
    }

    #[test]
    fn macos_roots_use_application_support_and_require_an_absolute_home() {
        let r = macos_roots(&vars(&[
            ("HOME", "/Users/mira"),
            ("XDG_CONFIG_HOME", "/linux/config"),
            ("XDG_DATA_HOME", "/linux/data"),
        ]))
        .unwrap();
        assert_eq!(
            r.config,
            Path::new("/Users/mira/Library/Application Support/AnyPlay Launcher")
        );
        assert_eq!(r.data, r.config);
        assert_eq!(r.pictures, Path::new("/Users/mira/Pictures"));
        assert!(macos_roots(&vars(&[])).is_none());
        assert!(macos_roots(&vars(&[("HOME", "relative")])).is_none());
    }

    #[test]
    fn linux_roots_honour_the_xdg_variables_but_not_relative_ones() {
        let r = roots(
            Os::Linux,
            &vars(&[
                ("HOME", "/home/mira"),
                ("XDG_CONFIG_HOME", "/cfg"),
                ("XDG_DATA_HOME", "relative/data"),
                ("XDG_PICTURES_DIR", "/pics"),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(r.config, Path::new("/cfg/anyps5"));
        assert_eq!(r.data, Path::new("/home/mira/.local/share/anyps5"));
        assert_eq!(r.pictures, Path::new("/pics"));
        assert!(roots(Os::Linux, &vars(&[]), None).is_none());
    }

    #[test]
    fn the_pictures_folder_comes_from_user_dirs() {
        let home = Path::new("/home/mira");
        let text =
            "# comment\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_PICTURES_DIR=\"$HOME/Bilder\"\n";
        assert_eq!(
            pictures_from_user_dirs(text, home),
            Some(PathBuf::from("/home/mira/Bilder"))
        );
        assert_eq!(
            pictures_from_user_dirs("XDG_PICTURES_DIR=\"$HOME\"\n", home),
            Some(home.to_path_buf())
        );
        assert_eq!(
            pictures_from_user_dirs("XDG_PICTURES_DIR=\"/data/pics\"\n", home),
            Some(PathBuf::from("/data/pics"))
        );
        assert_eq!(
            pictures_from_user_dirs("XDG_PICTURES_DIR=\"pics\"\n", home),
            None
        );
        assert_eq!(pictures_from_user_dirs("nothing\n", home), None);
        let r = roots(Os::Linux, &vars(&[("HOME", "/home/mira")]), Some(text)).unwrap();
        assert_eq!(r.pictures, Path::new("/home/mira/Bilder"));
    }

    #[test]
    fn discord_sockets_are_searched_in_runtime_flatpak_snap_then_tmp_order() {
        let found = discord_socket_candidates(
            3,
            &vars(&[
                ("XDG_RUNTIME_DIR", "/run/user/1000"),
                ("TMPDIR", "/var/tmp"),
            ]),
        );
        let text: Vec<String> = found
            .iter()
            .map(|p| p.display().to_string().replace('\\', "/"))
            .collect();
        assert_eq!(text[0], "/run/user/1000/discord-ipc-3");
        assert_eq!(
            text[1],
            "/run/user/1000/app/com.discordapp.Discord/discord-ipc-3"
        );
        assert!(text.contains(&"/run/user/1000/snap.discord/discord-ipc-3".to_string()));
        assert_eq!(text[text.len() - 2], "/var/tmp/discord-ipc-3");
        assert_eq!(text[text.len() - 1], "/tmp/discord-ipc-3");
    }

    #[test]
    fn discord_sockets_without_a_runtime_folder_fall_back_to_tmp_once() {
        let found = discord_socket_candidates(0, &vars(&[("TMPDIR", "/tmp")]));
        assert_eq!(found, [PathBuf::from("/tmp/discord-ipc-0")]);
    }

    #[test]
    fn revealing_a_file_opens_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("report.txt");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(xdg_open_target(&file, true), dir.path());
        assert_eq!(xdg_open_target(&file, false), file);
        assert_eq!(xdg_open_target(dir.path(), true), dir.path());
    }

    #[cfg(unix)]
    #[test]
    fn a_group_kill_ends_the_children_too() {
        use std::process::Stdio;
        let mut command = Command::new("sh");
        command
            .args(["-c", "sleep 30 & wait"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        isolate_group(&mut command);
        let mut child = command.spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(kill_group(child.id()));
        let status = child.wait().unwrap();
        assert!(!status.success());
    }
}
