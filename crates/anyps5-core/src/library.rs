//! Ported from utkuhalis/AnyPS5-ARM (GameLibrary.swift, GPL-2.0-only) and changed since; see README "Credits".
//! Game library: scanning the games folder for dumps, their metadata, and a watcher that tells the
//! app when the folder changed (PORT.md 6.2, GameLibrary.swift).

use std::{
    collections::{BTreeMap, HashMap},
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Sender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde_json::Value;

use crate::{
    events::{CancelToken, CoreEvent, Notifier},
    paths::{self, GameId},
};

/// What a dump's `eboot.bin` is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExecutableKind {
    /// A plain ELF.
    Elf,
    /// A SELF container (decrypted ones can be unwrapped).
    Container,
    /// Neither: encrypted or unknown.
    Unreadable,
}

/// A game found in the games folder.
#[derive(Clone, Debug)]
pub struct Game {
    pub id: GameId,
    pub folder: PathBuf,
    pub title_id: String,
    /// The name in the dump's default language (title ID when the dump has none).
    pub name: String,
    /// Names by language tag (`en-US` and so on), empty for param.sfo dumps.
    pub names: BTreeMap<String, String>,
    pub version: String,
    pub content_id: Option<String>,
    pub icon: Option<PathBuf>,
    pub background: Option<PathBuf>,
    pub music: Option<PathBuf>,
    pub executable: ExecutableKind,
    /// Bytes on disk once measured (`folder_size`), `None` until then.
    pub size: Option<u64>,
    /// The toolkit version this game's unlocked copy was made with. Filled by [`scan`], which knows
    /// the games folder; `None` when the game is not unlocked (or the game came from [`inspect`]).
    pub unlocked_with: Option<String>,
}

impl Game {
    /// `org.anyps5.` plus the lower-case alphanumerics of the title ID.
    pub fn bundle_identifier(&self) -> String {
        paths::bundle_identifier(&self.title_id)
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked_with.is_some()
    }
}

impl PartialEq for Game {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Game {}

impl std::hash::Hash for Game {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

const SKIPPED: [&str; 7] = [
    ".anyps5",
    "sce_sys",
    "sce_module",
    "sce_modules",
    "prx",
    "out",
    "rel",
];
const MAX_DEPTH: u32 = 3;

/// All games below `root`, sorted by name (case-insensitive). A found game is not searched further.
pub fn scan(root: &Path) -> Vec<Game> {
    scan_with_store(root, root)
}

/// [`scan`] for unlocked copies kept elsewhere: `store_root` is the folder that holds the
/// `.AnyPS5` store (the player's unlocked-games location). A store that is not reachable (a drive
/// that is unplugged) simply finds no copies.
pub fn scan_with_store(root: &Path, store_root: &Path) -> Vec<Game> {
    let mut games = Vec::new();
    visit(root, 0, &mut games);
    for game in &mut games {
        game.unlocked_with = unlocked_with(store_root, game);
    }
    games.sort_by_cached_key(|g| (g.name.to_lowercase(), g.name.clone()));
    games
}

fn visit(folder: &Path, depth: u32, games: &mut Vec<Game>) {
    if let Some(game) = inspect(folder) {
        games.push(game);
        return;
    }
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) && !is_hidden(e)
        })
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    for sub in folders {
        visit(&sub, depth + 1, games);
    }
}

#[cfg(windows)]
fn is_hidden(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    entry
        .metadata()
        .map(|m| m.file_attributes() & 0x2 != 0)
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn is_hidden(_entry: &fs::DirEntry) -> bool {
    false
}

/// The game in `folder`, or `None` when it is not a dump (no `eboot.bin`, or no readable metadata).
pub fn inspect(folder: &Path) -> Option<Game> {
    let eboot = folder.join("eboot.bin");
    if !eboot.is_file() {
        return None;
    }
    let sys = folder.join("sce_sys");
    let folder_name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let meta = read_param_json(&sys.join("param.json"), &folder_name).or_else(|| {
        fs::read(sys.join("param.sfo"))
            .ok()
            .and_then(|b| parse_sfo(&b))
    })?;
    let existing = |name: &str| Some(sys.join(name)).filter(|p| p.is_file());
    Some(Game {
        id: GameId::from_path(folder),
        folder: folder.to_path_buf(),
        title_id: meta.title_id,
        name: meta.name,
        names: meta.names,
        version: meta.version,
        content_id: meta.content_id,
        icon: existing("icon0.png"),
        background: existing("pic0.png"),
        music: existing("snd0.at9"),
        executable: executable_kind(folder),
        size: None,
        unlocked_with: None,
    })
}

struct Meta {
    title_id: String,
    name: String,
    names: BTreeMap<String, String>,
    version: String,
    content_id: Option<String>,
}

fn read_param_json(file: &Path, folder_name: &str) -> Option<Meta> {
    let text = fs::read(file).ok()?;
    let text = text.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(&text);
    let Value::Object(params) = serde_json::from_slice::<Value>(text).ok()? else {
        return None;
    };
    let string = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
    let title_id = string(params.get("titleId")).unwrap_or_else(|| folder_name.to_string());
    let empty = serde_json::Map::new();
    let localized = params
        .get("localizedParameters")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let language = string(localized.get("defaultLanguage"))
        .or_else(|| string(params.get("defaultLanguage")))
        .unwrap_or_else(|| "en-US".to_string());
    let names_of = |tag: &str| localized.get(tag).and_then(Value::as_object);
    let chosen = names_of(&language)
        .or_else(|| names_of("en-US"))
        .unwrap_or(&empty);
    let name = string(chosen.get("titleName")).unwrap_or_else(|| title_id.clone());
    let names = localized
        .iter()
        .filter_map(|(tag, v)| Some((tag.clone(), string(v.as_object()?.get("titleName"))?)))
        .collect();
    Some(Meta {
        name,
        names,
        version: string(params.get("contentVersion")).unwrap_or_else(|| "1.0".to_string()),
        content_id: string(params.get("contentId")),
        title_id,
    })
}

/// Reads `TITLE`, `TITLE_ID`, `APP_VER` and `CONTENT_ID` from a PSF (param.sfo) file.
fn parse_sfo(data: &[u8]) -> Option<Meta> {
    let u16_at = |o: usize| {
        data.get(o..o + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
    };
    let u32_at = |o: usize| {
        data.get(o..o + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
    };
    if data.get(..4)? != [0x00, b'P', b'S', b'F'] {
        return None;
    }
    let (key_table, data_table, count) = (u32_at(8)?, u32_at(12)?, u32_at(16)?);
    if count > 4096 {
        return None;
    }
    let mut strings: HashMap<String, String> = HashMap::new();
    for i in 0..count {
        let entry = 20 + i * 16;
        let key_offset = u16_at(entry)?;
        let format = u16_at(entry + 2)?;
        let length = u32_at(entry + 4)?;
        let value_at = data_table.checked_add(u32_at(entry + 12)?)?;
        let key_start = key_table.checked_add(key_offset)?;
        let key_bytes = data.get(key_start..)?;
        let key_end = key_bytes.iter().position(|&b| b == 0)?;
        let key = String::from_utf8_lossy(&key_bytes[..key_end]).into_owned();
        if format == 0x0204 {
            let raw = data.get(value_at..value_at.checked_add(length)?)?;
            let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
            strings.insert(key, String::from_utf8_lossy(&raw[..end]).into_owned());
        }
    }
    let non_empty = |key: &str| strings.get(key).filter(|v| !v.is_empty()).cloned();
    let name = non_empty("TITLE")?;
    let title_id = non_empty("TITLE_ID")?;
    Some(Meta {
        title_id,
        names: BTreeMap::new(),
        version: non_empty("APP_VER").unwrap_or_else(|| "1.0".to_string()),
        content_id: non_empty("CONTENT_ID"),
        name,
    })
}

fn executable_kind(folder: &Path) -> ExecutableKind {
    const ELF: [u8; 4] = [0x7F, b'E', b'L', b'F'];
    let head = |name: &str| {
        let mut buf = [0u8; 4];
        fs::File::open(folder.join(name))
            .ok()?
            .read_exact(&mut buf)
            .ok()?;
        Some(buf)
    };
    match head("eboot.bin") {
        Some(ELF) => ExecutableKind::Elf,
        magic if magic.is_some_and(is_self_magic) || head("eboot.bin.esbak") == Some(ELF) => {
            ExecutableKind::Container
        }
        _ => ExecutableKind::Unreadable,
    }
}

/// The first four bytes of the SELF containers: `4F 15 3D 1D` (PS4/PS5 SELF, little-endian
/// `0x1D3D154F`) and `54 14 F5 EE` (PS3-style fSELF header, `0xEEF51454`).
fn is_self_magic(magic: [u8; 4]) -> bool {
    matches!(magic, [0x4F, 0x15, 0x3D, 0x1D] | [0x54, 0x14, 0xF5, 0xEE])
}

/// The toolkit version recorded in the unlocked copy of `game`, if there is a copy with a record.
/// Same rules as the unlock record (the relinked executable must exist; `anyps5-unlock.json`, or the macOS
/// name `anyps5-install.json`, must parse).
pub fn unlocked_with(root: &Path, game: &Game) -> Option<String> {
    let copy = paths::unlocked_copy_dir(root, &game.title_id, &game.id);
    if !copy.join(crate::unlock::EXECUTABLE).is_file() {
        return None;
    }
    ["anyps5-unlock.json", "anyps5-install.json"]
        .iter()
        .find_map(|name| {
            let value: Value = serde_json::from_slice(&fs::read(copy.join(name)).ok()?).ok()?;
            value.get("toolkit")?.as_str().map(str::to_string)
        })
}

/// Sum of the regular files below `folder`; symlinks are not followed. Stops early (returning what
/// it has) when `cancel` is set.
pub fn folder_size(folder: &Path, cancel: &CancelToken) -> u64 {
    let mut total = 0;
    let mut stack = vec![folder.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if cancel.is_cancelled() {
            return total;
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

/// What changed between two scans, by game ID.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryDiff {
    pub added: Vec<GameId>,
    pub removed: Vec<GameId>,
    /// Same folder, different name, version, artwork, executable kind or unlock state.
    pub changed: Vec<GameId>,
}

impl LibraryDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

pub fn diff(old: &[Game], new: &[Game]) -> LibraryDiff {
    let old_by_id: HashMap<&GameId, &Game> = old.iter().map(|g| (&g.id, g)).collect();
    let new_by_id: HashMap<&GameId, &Game> = new.iter().map(|g| (&g.id, g)).collect();
    let mut out = LibraryDiff::default();
    for game in new {
        match old_by_id.get(&game.id) {
            None => out.added.push(game.id.clone()),
            Some(before) if differs(before, game) => out.changed.push(game.id.clone()),
            Some(_) => {}
        }
    }
    out.removed = old
        .iter()
        .filter(|g| !new_by_id.contains_key(&g.id))
        .map(|g| g.id.clone())
        .collect();
    out
}

fn differs(a: &Game, b: &Game) -> bool {
    (
        &a.title_id,
        &a.name,
        &a.names,
        &a.version,
        &a.content_id,
        &a.icon,
        &a.background,
        &a.music,
        a.executable,
        &a.unlocked_with,
    ) != (
        &b.title_id,
        &b.name,
        &b.names,
        &b.version,
        &b.content_id,
        &b.icon,
        &b.background,
        &b.music,
        b.executable,
        &b.unlocked_with,
    )
}

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("cannot watch {path}: {source}")]
    Notify {
        path: PathBuf,
        source: notify::Error,
    },
}

const SETTLE: Duration = Duration::from_millis(500);
const MIN_RESCAN_GAP: Duration = Duration::from_secs(2);

/// Watches the games folder (non-recursive, D18) and sends [`CoreEvent::LibraryChanged`] after a
/// change has settled. Dropping it stops the thread.
pub struct LibraryWatcher {
    _watcher: RecommendedWatcher,
    stop: Arc<AtomicBool>,
    last_scan: Arc<Mutex<Option<Instant>>>,
    thread: Option<JoinHandle<()>>,
}

impl LibraryWatcher {
    /// `busy` is true while an unlock is queued or running: changes are then ignored (the unlock
    /// itself writes into the games folder).
    pub fn start(
        root: PathBuf,
        tx: Sender<CoreEvent>,
        notify: Notifier,
        busy: Arc<AtomicBool>,
    ) -> Result<Self, WatchError> {
        let (raw_tx, raw_rx) = mpsc::channel::<()>();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event
                    && !matches!(event.kind, EventKind::Access(_))
                {
                    let _ = raw_tx.send(());
                }
            })
            .map_err(|source| WatchError::Notify {
                path: root.clone(),
                source,
            })?;
        watcher
            .watch(&root, RecursiveMode::NonRecursive)
            .map_err(|source| WatchError::Notify {
                path: root.clone(),
                source,
            })?;

        let stop = Arc::new(AtomicBool::new(false));
        let last_scan = Arc::new(Mutex::new(None::<Instant>));
        let thread = {
            let (stop, last_scan) = (stop.clone(), last_scan.clone());
            thread::Builder::new()
                .name("watch".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        if raw_rx.recv_timeout(Duration::from_millis(100)).is_err() {
                            continue;
                        }
                        // Settle: further events inside the window are dropped.
                        thread::sleep(SETTLE);
                        while raw_rx.try_recv().is_ok() {}
                        let mut last = last_scan.lock().unwrap_or_else(|e| e.into_inner());
                        let due = last.is_none_or(|t| t.elapsed() > MIN_RESCAN_GAP);
                        if due && !busy.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
                            *last = Some(Instant::now());
                            drop(last);
                            let _ = tx.send(CoreEvent::LibraryChanged);
                            notify();
                        }
                    }
                })
                .expect("spawn watch thread")
        };
        Ok(LibraryWatcher {
            _watcher: watcher,
            stop,
            last_scan,
            thread: Some(thread),
        })
    }

    /// The app calls this when it starts a scan, so changes the scan itself causes do not
    /// trigger another one within two seconds.
    pub fn mark_scanned(&self) {
        *self.last_scan.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }
}

impl Drop for LibraryWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;

    fn dump(folder: &Path, param_json: Option<&str>) {
        fs::create_dir_all(folder.join("sce_sys")).unwrap();
        fs::write(
            folder.join("eboot.bin"),
            [0x7F, b'E', b'L', b'F', 2, 1, 1, 0],
        )
        .unwrap();
        if let Some(text) = param_json {
            fs::write(folder.join("sce_sys/param.json"), text).unwrap();
        }
    }

    const FULL: &str = r#"{"titleId":"PPSA02100","contentId":"EP0000-PPSA02100_00-STRAY00000000000","contentVersion":"01.002",
        "localizedParameters":{"defaultLanguage":"de-DE","de-DE":{"titleName":"Streuner"},"en-US":{"titleName":"Stray"}}}"#;

    /// A param.sfo with the given string entries.
    fn sfo(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut keys = Vec::new();
        let mut values = Vec::new();
        let mut table = Vec::new();
        for (key, value) in entries {
            let key_offset = keys.len() as u16;
            keys.extend_from_slice(key.as_bytes());
            keys.push(0);
            let data_offset = values.len() as u32;
            let mut bytes = value.as_bytes().to_vec();
            bytes.push(0);
            let length = bytes.len() as u32;
            values.extend_from_slice(&bytes);
            table.extend_from_slice(&key_offset.to_le_bytes());
            table.extend_from_slice(&0x0204u16.to_le_bytes());
            table.extend_from_slice(&length.to_le_bytes());
            table.extend_from_slice(&length.to_le_bytes());
            table.extend_from_slice(&data_offset.to_le_bytes());
        }
        let key_table = 20 + table.len() as u32;
        let data_table = key_table + keys.len() as u32;
        let mut out = vec![0x00, b'P', b'S', b'F', 1, 1, 0, 0];
        out.extend_from_slice(&key_table.to_le_bytes());
        out.extend_from_slice(&data_table.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&table);
        out.extend_from_slice(&keys);
        out.extend_from_slice(&values);
        out
    }

    #[test]
    fn param_json_with_all_fields() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Stray");
        dump(&folder, Some(FULL));
        fs::write(folder.join("sce_sys/icon0.png"), b"x").unwrap();
        fs::write(folder.join("sce_sys/pic0.png"), b"x").unwrap();
        let game = inspect(&folder).unwrap();
        assert_eq!(game.title_id, "PPSA02100");
        assert_eq!(game.name, "Streuner");
        assert_eq!(game.names["en-US"], "Stray");
        assert_eq!(game.version, "01.002");
        assert_eq!(
            game.content_id.as_deref(),
            Some("EP0000-PPSA02100_00-STRAY00000000000")
        );
        assert!(game.icon.is_some() && game.background.is_some() && game.music.is_none());
        assert_eq!(game.executable, ExecutableKind::Elf);
        assert_eq!(game.id, GameId::from_path(&folder));
        assert_eq!(game.bundle_identifier(), "org.anyps5.ppsa02100");
        assert!(!game.is_unlocked());
    }

    #[test]
    fn missing_title_id_falls_back_to_folder_name() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("PPSA00001");
        dump(&folder, Some("{}"));
        let game = inspect(&folder).unwrap();
        assert_eq!(
            (
                game.title_id.as_str(),
                game.name.as_str(),
                game.version.as_str()
            ),
            ("PPSA00001", "PPSA00001", "1.0")
        );
    }

    #[test]
    fn language_fallbacks() {
        let dir = tempfile::tempdir().unwrap();
        // top-level defaultLanguage used when localized has none; en-US used when the language is missing
        let folder = dir.path().join("a");
        dump(
            &folder,
            Some(
                r#"{"titleId":"T","defaultLanguage":"fr-FR","localizedParameters":{"en-US":{"titleName":"English"}}}"#,
            ),
        );
        assert_eq!(inspect(&folder).unwrap().name, "English");
        let folder = dir.path().join("b");
        dump(
            &folder,
            Some(
                r#"{"titleId":"T","defaultLanguage":"fr-FR","localizedParameters":{"fr-FR":{"titleName":"Francais"}}}"#,
            ),
        );
        assert_eq!(inspect(&folder).unwrap().name, "Francais");
    }

    #[test]
    fn non_string_values_count_as_absent() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Folder");
        dump(
            &folder,
            Some(
                r#"{"titleId":5,"contentVersion":2,"localizedParameters":{"en-US":{"titleName":7}}}"#,
            ),
        );
        let game = inspect(&folder).unwrap();
        assert_eq!(
            (
                game.title_id.as_str(),
                game.name.as_str(),
                game.version.as_str()
            ),
            ("Folder", "Folder", "1.0")
        );
        assert!(game.names.is_empty());
    }

    #[test]
    fn unicode_names_survive() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("日本語 ゲーム");
        dump(
            &folder,
            Some(
                r#"{"titleId":"PPSA9","localizedParameters":{"defaultLanguage":"ja-JP","ja-JP":{"titleName":"ゲームの名前 ÄÖ"}}}"#,
            ),
        );
        assert_eq!(inspect(&folder).unwrap().name, "ゲームの名前 ÄÖ");
    }

    #[test]
    fn param_json_with_bom_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("g");
        dump(&folder, None);
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(FULL.as_bytes());
        fs::write(folder.join("sce_sys/param.json"), bytes).unwrap();
        assert_eq!(inspect(&folder).unwrap().title_id, "PPSA02100");
    }

    #[test]
    fn json_array_is_not_a_dump() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("g");
        dump(&folder, Some("[1,2]"));
        assert!(inspect(&folder).is_none());
    }

    #[test]
    fn param_sfo_is_the_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("g");
        dump(&folder, None);
        fs::write(
            folder.join("sce_sys/param.sfo"),
            sfo(&[
                ("APP_VER", "01.05"),
                ("CONTENT_ID", "UP0001-X"),
                ("TITLE", "Hollow Ünit"),
                ("TITLE_ID", "PPSA77"),
            ]),
        )
        .unwrap();
        let game = inspect(&folder).unwrap();
        assert_eq!(
            (
                game.title_id.as_str(),
                game.name.as_str(),
                game.version.as_str()
            ),
            ("PPSA77", "Hollow Ünit", "01.05")
        );
        assert_eq!(game.content_id.as_deref(), Some("UP0001-X"));
    }

    #[test]
    fn param_json_wins_over_sfo_and_sfo_defaults_version() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("g");
        dump(&folder, Some(FULL));
        fs::write(
            folder.join("sce_sys/param.sfo"),
            sfo(&[("TITLE", "Other"), ("TITLE_ID", "X")]),
        )
        .unwrap();
        assert_eq!(inspect(&folder).unwrap().title_id, "PPSA02100");
        fs::remove_file(folder.join("sce_sys/param.json")).unwrap();
        assert_eq!(inspect(&folder).unwrap().version, "1.0");
    }

    #[test]
    fn broken_sfo_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("g");
        dump(&folder, None);
        let good = sfo(&[("TITLE", "A"), ("TITLE_ID", "B")]);
        for bad in [
            good[..10].to_vec(),
            good[..good.len() - 3].to_vec(),
            b"not psf at all, no".to_vec(),
            sfo(&[("TITLE", "A")]),
        ] {
            fs::write(folder.join("sce_sys/param.sfo"), bad).unwrap();
            assert!(inspect(&folder).is_none());
        }
        fs::write(
            folder.join("sce_sys/param.sfo"),
            sfo(&[("TITLE", ""), ("TITLE_ID", "B")]),
        )
        .unwrap();
        assert!(inspect(&folder).is_none());
    }

    #[test]
    fn missing_files_are_none() {
        let dir = tempfile::tempdir().unwrap();
        let no_eboot = dir.path().join("a");
        fs::create_dir_all(no_eboot.join("sce_sys")).unwrap();
        fs::write(no_eboot.join("sce_sys/param.json"), FULL).unwrap();
        assert!(inspect(&no_eboot).is_none());
        let no_meta = dir.path().join("b");
        dump(&no_meta, None);
        assert!(inspect(&no_meta).is_none());
        assert!(inspect(&dir.path().join("does-not-exist")).is_none());
    }

    #[test]
    fn executable_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("g");
        dump(&folder, Some(FULL));
        let kind = |bytes: &[u8]| {
            fs::write(folder.join("eboot.bin"), bytes).unwrap();
            executable_kind(&folder)
        };
        assert_eq!(kind(&[0x7F, b'E', b'L', b'F', 0]), ExecutableKind::Elf);
        assert_eq!(
            kind(&[0x4F, 0x15, 0x3D, 0x1D, 0]),
            ExecutableKind::Container
        );
        assert_eq!(
            kind(&[0x54, 0x14, 0xF5, 0xEE, 0]),
            ExecutableKind::Container
        );
        assert_eq!(kind(b"garbage!"), ExecutableKind::Unreadable);
        assert_eq!(kind(b""), ExecutableKind::Unreadable);
        // garbage eboot.bin with an ELF eboot.bin.esbak next to it counts as a container
        fs::write(folder.join("eboot.bin.esbak"), [0x7F, b'E', b'L', b'F']).unwrap();
        assert_eq!(kind(b"garbage!"), ExecutableKind::Container);
    }

    #[test]
    fn scan_depth_skips_hidden_and_sorts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let param = |id: &str, name: &str| {
            format!(
                r#"{{"titleId":"{id}","localizedParameters":{{"en-US":{{"titleName":"{name}"}}}}}}"#
            )
        };
        dump(&root.join("beta"), Some(&param("B", "Beta")));
        dump(&root.join("x/alpha"), Some(&param("A", "alpha")));
        dump(&root.join("c/d/e/charlie"), Some(&param("C", "charlie"))); // depth 4 below root
        dump(&root.join("c/d/delta"), Some(&param("D", "Delta"))); // depth 3
        dump(&root.join("sce_module/fake"), Some(&param("F", "Fake")));
        dump(&root.join(".AnyPS5/Games/copy"), Some(&param("S", "Store")));
        dump(&root.join(".hidden/g"), Some(&param("H", "Hidden")));
        // a game folder is not searched further
        dump(&root.join("beta/nested"), Some(&param("N", "Nested")));
        let names: Vec<_> = scan(root).into_iter().map(|g| g.name).collect();
        assert_eq!(names, ["alpha", "Beta", "Delta"]);
        // a folder found at depth 3 is allowed, one at depth 4 is not
        assert!(!names.contains(&"charlie".to_string()));
    }

    #[test]
    fn scan_of_missing_root_is_empty() {
        assert!(scan(Path::new("Z:/definitely/not/here")).is_empty());
    }

    #[test]
    fn scan_reads_the_unlock_record() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let folder = root.join("Stray");
        dump(&folder, Some(FULL));
        assert!(scan(root)[0].unlocked_with.is_none());
        let id = GameId::from_path(&folder);
        let copy = paths::unlocked_copy_dir(root, "PPSA02100", &id);
        fs::create_dir_all(&copy).unwrap();
        fs::write(
            copy.join("anyps5-unlock.json"),
            r#"{"date":"2026-10-22T09:30:00Z","toolkit":"v0.1.3"}"#,
        )
        .unwrap();
        assert!(scan(root)[0].unlocked_with.is_none(), "no executable yet");
        fs::write(copy.join(crate::unlock::EXECUTABLE), b"MZ").unwrap();
        assert_eq!(scan(root)[0].unlocked_with.as_deref(), Some("v0.1.3"));
        fs::rename(
            copy.join("anyps5-unlock.json"),
            copy.join("anyps5-install.json"),
        )
        .unwrap();
        assert!(scan(root)[0].is_unlocked(), "macOS record name is accepted");
        fs::write(copy.join("anyps5-install.json"), "{broken").unwrap();
        assert!(!scan(root)[0].is_unlocked());
    }

    #[test]
    fn folder_size_sums_nested_files_and_honours_cancel() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        fs::write(dir.path().join("one"), [0u8; 100]).unwrap();
        fs::write(dir.path().join("a/two"), [0u8; 50]).unwrap();
        fs::write(dir.path().join("a/b/three"), [0u8; 7]).unwrap();
        assert_eq!(folder_size(dir.path(), &CancelToken::new()), 157);
        let cancelled = CancelToken::new();
        cancelled.cancel();
        assert_eq!(folder_size(dir.path(), &cancelled), 0);
        assert_eq!(
            folder_size(&dir.path().join("nope"), &CancelToken::new()),
            0
        );
    }

    #[test]
    fn diff_reports_added_removed_changed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        dump(&root.join("a"), Some(r#"{"titleId":"A"}"#));
        dump(&root.join("b"), Some(r#"{"titleId":"B"}"#));
        let before = scan(root);
        assert!(diff(&before, &before).is_empty());
        fs::remove_dir_all(root.join("a")).unwrap();
        dump(&root.join("c"), Some(r#"{"titleId":"C"}"#));
        dump(
            &root.join("b"),
            Some(r#"{"titleId":"B","contentVersion":"2.0"}"#),
        );
        let d = diff(&before, &scan(root));
        assert_eq!(d.added, [GameId::from_path(&root.join("c"))]);
        assert_eq!(d.removed, [GameId::from_path(&root.join("a"))]);
        assert_eq!(d.changed, [GameId::from_path(&root.join("b"))]);
    }

    fn watcher(
        root: &Path,
        busy: bool,
    ) -> (LibraryWatcher, mpsc::Receiver<CoreEvent>, Arc<AtomicBool>) {
        let (tx, rx) = mpsc::channel();
        let busy = Arc::new(AtomicBool::new(busy));
        let w =
            LibraryWatcher::start(root.to_path_buf(), tx, Arc::new(|| {}), busy.clone()).unwrap();
        (w, rx, busy)
    }

    #[test]
    fn watcher_reports_a_new_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (_w, rx, _) = watcher(dir.path(), false);
        fs::create_dir(dir.path().join("new game")).unwrap();
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(3)),
            Ok(CoreEvent::LibraryChanged)
        ));
    }

    #[test]
    fn watcher_is_silent_while_busy_and_after_mark_scanned() {
        let dir = tempfile::tempdir().unwrap();
        let (w, rx, busy) = watcher(dir.path(), true);
        fs::create_dir(dir.path().join("one")).unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(1500)).err(),
            Some(RecvTimeoutError::Timeout)
        );
        busy.store(false, Ordering::Relaxed);
        w.mark_scanned();
        fs::create_dir(dir.path().join("two")).unwrap();
        assert_eq!(
            rx.recv_timeout(Duration::from_millis(1200)).err(),
            Some(RecvTimeoutError::Timeout)
        );
    }

    #[test]
    fn watcher_on_missing_folder_is_an_error() {
        let (tx, _rx) = mpsc::channel();
        let result = LibraryWatcher::start(
            PathBuf::from("Z:/no/such/dir"),
            tx,
            Arc::new(|| {}),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(result.is_err());
    }
}
