//! Add to Steam: put the launcher, and optionally each game, into the player's Steam library as
//! non-Steam shortcuts (docs/DESIGN.md 9.8).
//!
//! The one thing a caller does is [`install`]: it finds the player's Steam, refuses while Steam
//! runs, and for every local Steam user writes the shortcuts into
//! `userdata/<id>/config/shortcuts.vdf` and the artwork into `userdata/<id>/config/grid/`. Nothing
//! else is touched, nothing leaves the PC, and Steam's servers are never contacted.
//!
//! What stays inside the module:
//! - the binary KeyValues format of `shortcuts.vdf` (read and written as a tree, so every field the
//!   player or Steam put there survives a round trip byte for byte),
//! - the app ID Steam derives for a shortcut (CRC-32 of the quoted exe and the name, high bit set),
//!   which also names the artwork files,
//! - where Steam lives: the registry on Windows; `~/.steam/steam`, `~/.local/share/Steam` and the
//!   Flatpak folder on Linux,
//! - "never twice": an entry that already starts this program with these arguments (or carries the
//!   same name and arguments) is updated in place instead of added again.
//!
//! Steam holds `shortcuts.vdf` in memory and writes it back when it exits, so an edit made while
//! Steam runs is lost. [`install`] therefore never writes while Steam runs and never stops Steam.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The app ID Steam gives a non-Steam shortcut: CRC-32 of `exe + name` as Steam stores them (the
/// exe including its quotes), with the high bit set. It is the value of the entry's `appid` field
/// and the prefix of its artwork files.
pub fn app_id(quoted_exe: &str, name: &str) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(quoted_exe.as_bytes());
    hasher.update(name.as_bytes());
    hasher.finalize() | 0x8000_0000
}

/// The artwork of one shortcut, as image files' bytes (PNG). Whatever is `None` is left as it is.
#[derive(Clone, Debug, Default)]
pub struct Art {
    /// Tall capsule, 600 x 900 (`<id>p.png`).
    pub portrait: Option<Vec<u8>>,
    /// Wide capsule, 920 x 430 (`<id>.png`).
    pub wide: Option<Vec<u8>>,
    /// Hero banner, 1920 x 620 (`<id>_hero.png`).
    pub hero: Option<Vec<u8>>,
    /// Logo over the hero, transparent (`<id>_logo.png`).
    pub logo: Option<Vec<u8>>,
    /// Small icon (`<id>_icon.png`), also named in the entry's `icon` field.
    pub icon: Option<Vec<u8>>,
}

/// One entry for the Steam library.
#[derive(Clone, Debug)]
pub struct Shortcut {
    /// The name Steam shows. Never a platform's trademark; the caller names it.
    pub name: String,
    /// The program Steam starts.
    pub exe: PathBuf,
    /// The arguments Steam passes (Steam's "launch options").
    pub arguments: String,
    pub art: Art,
}

/// A Steam installation on this PC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub root: PathBuf,
    /// Steam from Flatpak (Linux): its programs run in a sandbox.
    pub flatpak: bool,
}

/// What [`install`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Steam users whose library was written.
    pub users: usize,
    /// Entries added, over all users.
    pub added: usize,
    /// Entries that were there and were brought up to date, over all users.
    pub updated: usize,
}

/// Why nothing was written.
#[derive(Debug)]
pub enum Error {
    /// No Steam folder was found.
    NoSteam,
    /// Steam runs. It would write its own copy over ours when it exits.
    SteamRunning,
    /// Steam is there but has no user folder yet (nobody has signed in).
    NoUsers,
    /// The shortcuts file is not in the form this module knows; it was left untouched.
    Unreadable(PathBuf),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoSteam => f.write_str("no Steam folder found"),
            Error::SteamRunning => f.write_str("Steam is running"),
            Error::NoUsers => f.write_str("Steam has no user folder yet"),
            Error::Unreadable(path) => write!(f, "cannot read {}", path.display()),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

/// Where Steam is on this PC, found from the real registry and home folder.
pub fn locate() -> Vec<Place> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    places(home.as_deref(), registry::steam_path())
}

/// The candidate Steam folders that exist, without doubles (`~/.steam/steam` is usually a link to
/// `~/.local/share/Steam`). `registry` is Windows' `SteamPath`.
pub fn places(home: Option<&Path>, registry: Option<PathBuf>) -> Vec<Place> {
    let mut found: Vec<Place> = Vec::new();
    let mut candidates: Vec<(PathBuf, bool)> = Vec::new();
    if let Some(path) = registry {
        candidates.push((path, false));
    }
    if let Some(home) = home {
        candidates.push((home.join(".steam/steam"), false));
        candidates.push((home.join(".local/share/Steam"), false));
        candidates.push((
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
            true,
        ));
    }
    let mut seen = BTreeSet::new();
    for (path, flatpak) in candidates {
        if !path.join("userdata").is_dir() {
            continue;
        }
        let key = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if seen.insert(key) {
            found.push(Place {
                root: path,
                flatpak,
            });
        }
    }
    found
}

/// Whether Steam runs now (best effort: a missing sign means "no").
pub fn is_running(place: &Place, home: Option<&Path>) -> bool {
    registry::steam_running() || pid_file_alive(place, home) || process_named_steam()
}

/// A process called `steam` (Linux). The pid file of Flatpak Steam holds a number from inside its
/// sandbox, so the processes are looked at as well.
fn process_named_steam() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
            && fs::read_to_string(entry.path().join("comm"))
                .is_ok_and(|name| name.trim_end() == "steam")
    })
}

fn pid_file_alive(place: &Place, home: Option<&Path>) -> bool {
    let mut files = vec![place.root.join("steam.pid")];
    if let Some(home) = home {
        files.push(home.join(".steam/steam.pid"));
        files.push(home.join(".var/app/com.valvesoftware.Steam/.steam/steam.pid"));
    }
    files.iter().any(|file| {
        fs::read_to_string(file)
            .ok()
            .and_then(|t| t.trim().parse::<u32>().ok())
            .is_some_and(|pid| pid != 0 && Path::new(&format!("/proc/{pid}")).exists())
    })
}

/// Writes `shortcuts` into every local Steam user of the Steam installations found on this PC.
pub fn install(shortcuts: &[Shortcut]) -> Result<Outcome, Error> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    let found = locate();
    install_into(&found, shortcuts, |place| {
        is_running(place, home.as_deref())
    })
}

/// [`install`] on given places; `running` says whether Steam of that place runs.
pub fn install_into(
    places: &[Place],
    shortcuts: &[Shortcut],
    running: impl Fn(&Place) -> bool,
) -> Result<Outcome, Error> {
    if places.is_empty() {
        return Err(Error::NoSteam);
    }
    if places.iter().any(&running) {
        return Err(Error::SteamRunning);
    }
    let mut outcome = Outcome::default();
    let mut first_error = None;
    for place in places {
        for user in users(place) {
            match write_user(&user.join("config"), shortcuts) {
                Ok((added, updated)) => {
                    outcome.users += 1;
                    outcome.added += added;
                    outcome.updated += updated;
                }
                Err(e) => {
                    tracing::warn!("Steam user {}: {e}", user.display());
                    first_error.get_or_insert(e);
                }
            }
        }
    }
    if outcome.users > 0 {
        Ok(outcome)
    } else if let Some(e) = first_error {
        Err(e)
    } else {
        Err(Error::NoUsers)
    }
}

/// `userdata/<account id>` folders. `0` is Steam's placeholder, not an account.
fn users(place: &Place) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(place.root.join("userdata")) else {
        return Vec::new();
    };
    let mut users: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            !name.is_empty()
                && name != "0"
                && name.bytes().all(|b| b.is_ascii_digit())
                && e.path().is_dir()
        })
        .map(|e| e.path())
        .collect();
    users.sort();
    users
}

/// One user's `config` folder. Returns how many entries were added and updated.
fn write_user(config: &Path, shortcuts: &[Shortcut]) -> Result<(usize, usize), Error> {
    let file = config.join("shortcuts.vdf");
    let existing = match fs::read(&file) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let mut root = match &existing {
        Some(bytes) => vdf::parse(bytes).map_err(|_| Error::Unreadable(file.clone()))?,
        None => Vec::new(),
    };
    let list = vdf::child_map(&mut root, "shortcuts");

    let (mut added, mut updated) = (0, 0);
    let mut ids = Vec::with_capacity(shortcuts.len());
    for shortcut in shortcuts {
        let (id, was_there) = upsert(list, shortcut, config);
        ids.push(id);
        if was_there {
            updated += 1;
        } else {
            added += 1;
        }
    }
    let bytes = vdf::write(&root);

    fs::create_dir_all(config)?;
    // The player's own file, kept once before this module first changes it.
    if let Some(old) = &existing {
        let backup = config.join("shortcuts.vdf.anyplay-backup");
        if !backup.exists() {
            fs::write(&backup, old)?;
        }
    }
    let grid = config.join("grid");
    for (shortcut, id) in shortcuts.iter().zip(&ids) {
        write_art(&grid, *id, &shortcut.art)?;
    }
    write_atomic(&file, &bytes)?;
    Ok((added, updated))
}

/// Adds the shortcut to the list or brings its entry up to date. Returns its app ID (the one the
/// entry already has when it is there) and whether it was there.
fn upsert(list: &mut Vec<(String, vdf::Node)>, shortcut: &Shortcut, config: &Path) -> (u32, bool) {
    let exe = format!("\"{}\"", shortcut.exe.display());
    let start_dir = shortcut
        .exe
        .parent()
        .map(|p| format!("\"{}\"", p.display()))
        .unwrap_or_default();

    let position = list.iter().position(|(_, node)| {
        let vdf::Node::Map(fields) = node else {
            return false;
        };
        let text = |key: &str| vdf::text_of(fields, key).unwrap_or_default();
        let arguments = text("LaunchOptions");
        arguments == shortcut.arguments
            && (same_path(text("Exe"), &exe) || text("AppName") == shortcut.name)
    });

    let was_there = position.is_some();
    let index = position.unwrap_or_else(|| {
        let key = list
            .iter()
            .filter_map(|(k, _)| k.parse::<u64>().ok())
            .max()
            .map_or(0, |m| m + 1);
        list.push((key.to_string(), vdf::Node::Map(Vec::new())));
        list.len() - 1
    });
    let vdf::Node::Map(fields) = &mut list[index].1 else {
        unreachable!("the entry is a map");
    };

    let name = if was_there {
        vdf::text_of(fields, "AppName")
            .map(str::to_owned)
            .unwrap_or_else(|| shortcut.name.clone())
    } else {
        shortcut.name.clone()
    };
    let id = vdf::number_of(fields, "appid").unwrap_or_else(|| app_id(&exe, &name));

    vdf::set_number(fields, "appid", id);
    vdf::set_text(fields, "AppName", &name);
    vdf::set_text(fields, "Exe", &exe);
    vdf::set_text(fields, "StartDir", &start_dir);
    if shortcut.art.icon.is_some() {
        let icon = config.join("grid").join(format!("{id}_icon.png"));
        vdf::set_text(fields, "icon", &icon.display().to_string());
    }
    vdf::set_text(fields, "LaunchOptions", &shortcut.arguments);
    if !was_there {
        vdf::set_text(fields, "ShortcutPath", "");
        vdf::set_number(fields, "IsHidden", 0);
        vdf::set_number(fields, "AllowDesktopConfig", 1);
        vdf::set_number(fields, "AllowOverlay", 1);
        vdf::set_number(fields, "OpenVR", 0);
        vdf::set_number(fields, "Devkit", 0);
        vdf::set_text(fields, "DevkitGameID", "");
        vdf::set_number(fields, "DevkitOverrideAppID", 0);
        vdf::set_number(fields, "LastPlayTime", 0);
        vdf::set_text(fields, "FlatpakAppID", "");
        vdf::ensure_map(fields, "tags");
    }
    (id, was_there)
}

fn same_path(a: &str, b: &str) -> bool {
    let normal = |s: &str| s.replace('\\', "/").to_lowercase();
    !a.is_empty() && normal(a) == normal(b)
}

fn write_art(grid: &Path, id: u32, art: &Art) -> io::Result<()> {
    let files = [
        (&art.portrait, format!("{id}p.png")),
        (&art.wide, format!("{id}.png")),
        (&art.hero, format!("{id}_hero.png")),
        (&art.logo, format!("{id}_logo.png")),
        (&art.icon, format!("{id}_icon.png")),
    ];
    if files.iter().all(|(bytes, _)| bytes.is_none()) {
        return Ok(());
    }
    fs::create_dir_all(grid)?;
    for (bytes, name) in files {
        if let Some(bytes) = bytes {
            // Steam shows a custom image over its own for the same app ID and kind. A JPEG the
            // player chose earlier would win over our PNG, so it goes.
            let stem = name.trim_end_matches(".png");
            for other in ["jpg", "jpeg"] {
                let _ = fs::remove_file(grid.join(format!("{stem}.{other}")));
            }
            write_atomic(&grid.join(name), bytes)?;
        }
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temp = path.as_os_str().to_owned();
    temp.push(".anyplay-tmp");
    let temp = PathBuf::from(temp);
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// Steam's binary KeyValues, as far as `shortcuts.vdf` uses it: maps (type 0), strings (1) and
/// 32-bit integers (2), each map closed by 8.
mod vdf {
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Node {
        Map(Vec<(String, Node)>),
        Text(String),
        Number(u32),
    }

    pub struct Malformed;

    pub fn parse(bytes: &[u8]) -> Result<Vec<(String, Node)>, Malformed> {
        let mut at = 0;
        let root = parse_map(bytes, &mut at)?;
        // The closing 8 of the root is consumed by `parse_map`; nothing may follow it.
        if at != bytes.len() {
            return Err(Malformed);
        }
        Ok(root)
    }

    fn parse_map(bytes: &[u8], at: &mut usize) -> Result<Vec<(String, Node)>, Malformed> {
        let mut items = Vec::new();
        loop {
            let kind = *bytes.get(*at).ok_or(Malformed)?;
            *at += 1;
            if kind == 8 {
                return Ok(items);
            }
            let key = cstring(bytes, at)?;
            let node = match kind {
                0 => Node::Map(parse_map(bytes, at)?),
                1 => Node::Text(cstring(bytes, at)?),
                2 => {
                    let raw = bytes.get(*at..*at + 4).ok_or(Malformed)?;
                    *at += 4;
                    Node::Number(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
                }
                _ => return Err(Malformed),
            };
            items.push((key, node));
        }
    }

    fn cstring(bytes: &[u8], at: &mut usize) -> Result<String, Malformed> {
        let rest = bytes.get(*at..).ok_or(Malformed)?;
        let len = rest.iter().position(|b| *b == 0).ok_or(Malformed)?;
        let text = std::str::from_utf8(&rest[..len]).map_err(|_| Malformed)?;
        *at += len + 1;
        Ok(text.to_owned())
    }

    pub fn write(root: &[(String, Node)]) -> Vec<u8> {
        let mut out = Vec::new();
        write_map(root, &mut out);
        out
    }

    fn write_map(items: &[(String, Node)], out: &mut Vec<u8>) {
        for (key, node) in items {
            let kind = match node {
                Node::Map(_) => 0,
                Node::Text(_) => 1,
                Node::Number(_) => 2,
            };
            out.push(kind);
            out.extend_from_slice(key.as_bytes());
            out.push(0);
            match node {
                Node::Map(inner) => write_map(inner, out),
                Node::Text(text) => {
                    out.extend_from_slice(text.as_bytes());
                    out.push(0);
                }
                Node::Number(n) => out.extend_from_slice(&n.to_le_bytes()),
            }
        }
        out.push(8);
    }

    /// The map under `key` (any letter case), created at the end when missing.
    pub fn child_map<'a>(
        items: &'a mut Vec<(String, Node)>,
        key: &str,
    ) -> &'a mut Vec<(String, Node)> {
        let index = match items.iter().position(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some(i) if matches!(items[i].1, Node::Map(_)) => i,
            _ => {
                items.push((key.to_owned(), Node::Map(Vec::new())));
                items.len() - 1
            }
        };
        match &mut items[index].1 {
            Node::Map(inner) => inner,
            _ => unreachable!("checked above"),
        }
    }

    pub fn ensure_map(items: &mut Vec<(String, Node)>, key: &str) {
        child_map(items, key);
    }

    pub fn text_of<'a>(items: &'a [(String, Node)], key: &str) -> Option<&'a str> {
        items.iter().find_map(|(k, v)| match v {
            Node::Text(t) if k.eq_ignore_ascii_case(key) => Some(t.as_str()),
            _ => None,
        })
    }

    pub fn number_of(items: &[(String, Node)], key: &str) -> Option<u32> {
        items.iter().find_map(|(k, v)| match v {
            Node::Number(n) if k.eq_ignore_ascii_case(key) => Some(*n),
            _ => None,
        })
    }

    pub fn set_text(items: &mut Vec<(String, Node)>, key: &str, value: &str) {
        set(items, key, Node::Text(value.to_owned()));
    }

    pub fn set_number(items: &mut Vec<(String, Node)>, key: &str, value: u32) {
        set(items, key, Node::Number(value));
    }

    /// Replaces the value under `key` (any letter case, so an older lower-case key is kept as it
    /// is) or appends it.
    fn set(items: &mut Vec<(String, Node)>, key: &str, value: Node) {
        match items.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some((_, slot)) => *slot = value,
            None => items.push((key.to_owned(), value)),
        }
    }
}

/// Steam's own state in the registry (Windows); nothing elsewhere.
mod registry {
    use std::path::PathBuf;

    #[cfg(windows)]
    pub fn steam_path() -> Option<PathBuf> {
        imp::string("SteamPath").map(|s| PathBuf::from(s.replace('/', "\\")))
    }

    #[cfg(not(windows))]
    pub fn steam_path() -> Option<PathBuf> {
        None
    }

    /// Steam writes its process ID to `ActiveProcess\pid` while it runs.
    #[cfg(windows)]
    pub fn steam_running() -> bool {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let Some(pid) = imp::dword("ActiveProcess", "pid").filter(|p| *p != 0) else {
            return false;
        };
        // SAFETY: a plain query on a process ID; the handle is closed.
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return false;
            };
            let mut code = 0u32;
            let alive = GetExitCodeProcess(handle, &mut code).is_ok() && code == 259;
            let _ = CloseHandle(handle);
            alive
        }
    }

    #[cfg(not(windows))]
    pub fn steam_running() -> bool {
        false
    }

    #[cfg(windows)]
    mod imp {
        use windows::Win32::Foundation::ERROR_SUCCESS;
        use windows::Win32::System::Registry::{
            HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW,
        };
        use windows::core::PCWSTR;

        fn wide(text: &str) -> Vec<u16> {
            text.encode_utf16().chain(std::iter::once(0)).collect()
        }

        /// A string value of `HKCU\Software\Valve\Steam`.
        pub fn string(name: &str) -> Option<String> {
            let key = wide("Software\\Valve\\Steam");
            let name = wide(name);
            let mut buffer = [0u16; 1024];
            let mut size = std::mem::size_of_val(&buffer) as u32;
            // SAFETY: the buffer and its size describe the same memory; the strings end in NUL.
            let status = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(key.as_ptr()),
                    PCWSTR(name.as_ptr()),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buffer.as_mut_ptr().cast()),
                    Some(&mut size),
                )
            };
            if status != ERROR_SUCCESS {
                return None;
            }
            let len = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
            let text = String::from_utf16_lossy(&buffer[..len]);
            (!text.is_empty()).then_some(text)
        }

        /// A DWORD value under `HKCU\Software\Valve\Steam\<sub>`.
        pub fn dword(sub: &str, name: &str) -> Option<u32> {
            let key = wide(&format!("Software\\Valve\\Steam\\{sub}"));
            let name = wide(name);
            let mut value = 0u32;
            let mut size = 4u32;
            // SAFETY: `value` is four bytes, as `size` says.
            let status = unsafe {
                RegGetValueW(
                    HKEY_CURRENT_USER,
                    PCWSTR(key.as_ptr()),
                    PCWSTR(name.as_ptr()),
                    RRF_RT_REG_DWORD,
                    None,
                    Some((&mut value as *mut u32).cast()),
                    Some(&mut size),
                )
            };
            (status == ERROR_SUCCESS).then_some(value)
        }
    }
}

#[cfg(test)]
mod tests;
