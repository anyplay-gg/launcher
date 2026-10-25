//! Game identifiers, path normalisation and the Windows locations the launcher reads and writes.
//!
//! [`AppPaths`] is built once from the known folders and handed to everything that touches disk, so
//! tests point it at a temporary directory (PORT.md section 7).

use std::{
    fmt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

/// Identifies a game by its dump folder: the absolute path with the `\\?\` prefix removed, `\`
/// turned into `/`, no trailing `/`, lower-cased (D23). Two spellings of one folder are equal.
/// No file system access is involved, so it also works for folders that no longer exist.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GameId(String);

impl GameId {
    pub fn from_path(path: &Path) -> GameId {
        GameId(normalise(&path.to_string_lossy()))
    }

    /// Adopts an already normalised key (read from a preferences file).
    pub fn from_key(key: impl Into<String>) -> GameId {
        GameId(key.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for GameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&Path> for GameId {
    fn from(path: &Path) -> GameId {
        GameId::from_path(path)
    }
}

fn normalise(raw: &str) -> String {
    let mut text = raw.to_string();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        text = format!(r"\\{rest}");
    } else if let Some(rest) = text
        .strip_prefix(r"\\?\")
        .or_else(|| text.strip_prefix(r"\\.\"))
    {
        text = rest.to_string();
    }
    if !looks_absolute(&text)
        && let Ok(absolute) = std::path::absolute(&text)
    {
        text = absolute.to_string_lossy().into_owned();
        if let Some(rest) = text.strip_prefix(r"\\?\") {
            text = rest.to_string();
        }
    }
    let mut text = text.replace('\\', "/").to_lowercase();
    // A drive root keeps its slash: "c:" alone would mean "the current folder of drive C".
    while text.len() > 1 && text.ends_with('/') && !(text.len() == 3 && text.ends_with(":/")) {
        text.pop();
    }
    text
}

fn looks_absolute(text: &str) -> bool {
    let bytes = text.as_bytes();
    text.starts_with('/')
        || text.starts_with('\\')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

/// FNV-1a over the UTF-8 bytes, 64 bit.
pub fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

/// The folder name of a game's unlocked copy: `<alphanumerics of the title ID>-<hex of the low 24
/// bits of the FNV-1a hash of the game ID>`, lower-case hex without zero padding (PORT.md 6.4).
pub fn unlocked_copy_name(title_id: &str, id: &GameId) -> String {
    let alnum: String = title_id.chars().filter(|c| c.is_alphanumeric()).collect();
    format!("{alnum}-{:x}", fnv1a64(id.as_str()) & 0xFF_FFFF)
}

/// `org.anyps5.` plus the lower-cased alphanumerics of the title ID (cache folder names).
pub fn bundle_identifier(title_id: &str) -> String {
    let tail: String = title_id
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    format!("org.anyps5.{tail}")
}

/// The roots all launcher data lives under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    /// `%APPDATA%\AnyPS5` (Linux: `~/.config/anyps5`): preferences and input files.
    pub config: PathBuf,
    /// `%LOCALAPPDATA%\AnyPS5` (Linux: `~/.local/share/anyps5`): toolkit, logs, caches.
    pub data: PathBuf,
    /// The Pictures known folder.
    pub pictures: PathBuf,
}

impl AppPaths {
    /// The real locations of this system (see [`crate::platform::roots`]). `None` when the
    /// environment names no home.
    pub fn from_env() -> Option<AppPaths> {
        use crate::platform::{self, Os};
        let os = Os::current();
        let user_dirs = (os == Os::Linux)
            .then(|| platform::read_user_dirs(&platform::process_vars))
            .flatten();
        let roots = platform::roots(os, &platform::process_vars, user_dirs.as_deref())?;
        Some(AppPaths {
            config: roots.config,
            data: roots.data,
            pictures: roots.pictures,
        })
    }

    /// Everything below `root` (tests, portable mode).
    pub fn under(root: &Path) -> AppPaths {
        AppPaths {
            config: root.join("Roaming").join("AnyPS5"),
            data: root.join("Local").join("AnyPS5"),
            pictures: root.join("Pictures"),
        }
    }

    pub fn preferences_file(&self) -> PathBuf {
        self.config.join("preferences.json")
    }

    pub fn controls_dir(&self) -> PathBuf {
        self.config.join("Controls")
    }

    /// The input file of one unlocked copy, named after the copy's folder.
    pub fn input_file(&self, unlocked_copy_name: &str) -> PathBuf {
        self.controls_dir()
            .join(format!("{unlocked_copy_name}.ini"))
    }

    pub fn toolkit_dir(&self) -> PathBuf {
        self.data.join("Toolkit")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data.join("Logs")
    }

    /// `Logs\launcher.log`: what the launcher itself logs, with panics.
    pub fn launcher_log(&self) -> PathBuf {
        self.logs_dir().join("launcher.log")
    }

    /// `Logs\launcher.log.1`: the previous launcher log, kept when the current one fills up.
    pub fn launcher_log_old(&self) -> PathBuf {
        self.logs_dir().join("launcher.log.1")
    }

    pub fn log_file(&self, title_id: &str) -> PathBuf {
        self.logs_dir().join(format!("{title_id}.log"))
    }

    pub fn reports_dir(&self) -> PathBuf {
        self.logs_dir().join("Reports")
    }

    /// `Backups`: one folder per title ID below it.
    pub fn backups_dir(&self) -> PathBuf {
        self.data.join("Backups")
    }

    /// The save backups of one title (`Backups<TITLEID>`); only letters and digits of the ID
    /// name the folder.
    pub fn game_backups_dir(&self, title_id: &str) -> PathBuf {
        let name: String = title_id.chars().filter(|c| c.is_alphanumeric()).collect();
        self.backups_dir()
            .join(if name.is_empty() { "unknown" } else { &name })
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.data.join("Cache")
    }

    /// `Network`: the net service's own folder (its endpoint key, observation logs).
    pub fn network_dir(&self) -> PathBuf {
        self.data.join("Network")
    }

    /// `Logs\network.log`: what the net service writes to its standard error.
    pub fn network_log(&self) -> PathBuf {
        self.logs_dir().join("network.log")
    }

    /// `Logs\<TITLEID>-network.jsonl`: the bridge's observation log of one game.
    pub fn network_record(&self, title_id: &str) -> PathBuf {
        let name: String = title_id.chars().filter(|c| c.is_alphanumeric()).collect();
        let name = if name.is_empty() { "unknown" } else { &name };
        self.logs_dir().join(format!("{name}-network.jsonl"))
    }

    /// `Cache\org.anyps5.<titleid>`: what "clear shader cache" deletes.
    pub fn game_cache_dir(&self, title_id: &str) -> PathBuf {
        self.cache_dir().join(bundle_identifier(title_id))
    }

    /// The value for `ANYPS5_SHADER_CACHE_DIR`.
    pub fn shader_cache_dir(&self, title_id: &str) -> PathBuf {
        self.game_cache_dir(title_id).join("shader_cache")
    }

    pub fn default_screenshots(&self) -> PathBuf {
        self.pictures.join("AnyPS5")
    }
}

/// The hidden store inside a games folder (`<root>\.AnyPS5`).
pub fn store_dir(root: &Path) -> PathBuf {
    root.join(".AnyPS5")
}

/// Where the unlocked copy of a game lives: `<root>\.AnyPS5\Games\<name>`.
pub fn unlocked_copy_dir(root: &Path, title_id: &str, id: &GameId) -> PathBuf {
    store_dir(root)
        .join("Games")
        .join(unlocked_copy_name(title_id, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_log_lives_in_the_logs_folder() {
        let paths = AppPaths::under(Path::new("data"));
        assert_eq!(paths.launcher_log(), paths.logs_dir().join("launcher.log"));
        assert_eq!(
            paths.launcher_log_old(),
            paths.logs_dir().join("launcher.log.1")
        );
    }

    #[test]
    fn spellings_of_one_folder_are_one_id() {
        let a = GameId::from_path(Path::new(r"C:\Games\Stray\"));
        let b = GameId::from_path(Path::new("c:/games/stray"));
        let c = GameId::from_path(Path::new(r"\\?\C:\Games\Stray"));
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert_eq!(a.as_str(), "c:/games/stray");
    }

    #[test]
    fn unc_and_drive_root() {
        assert_eq!(
            GameId::from_path(Path::new(r"\\?\UNC\Server\Share\Dump")).as_str(),
            "//server/share/dump"
        );
        assert_eq!(GameId::from_path(Path::new(r"D:\")).as_str(), "d:/");
    }

    #[test]
    fn unicode_ids_lowercase() {
        assert_eq!(
            GameId::from_path(Path::new(r"C:\Spiele\ÄÖÜ Straße")).as_str(),
            "c:/spiele/äöü straße"
        );
    }

    #[test]
    fn id_serialises_as_plain_string() {
        let id = GameId::from_key("c:/g");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"c:/g\"");
        assert_eq!(serde_json::from_str::<GameId>("\"c:/g\"").unwrap(), id);
    }

    #[test]
    fn unlocked_copy_names_match_the_spec_vectors() {
        let id = GameId::from_path(Path::new("c:/games/ppsa01234 - stray"));
        assert_eq!(unlocked_copy_name("PPSA01234", &id), "PPSA01234-d356b9");
        let root = GameId::from_path(Path::new("c:/games"));
        assert_eq!(
            format!("{:x}", fnv1a64(root.as_str()) & 0xFF_FFFF),
            "602bcc"
        );
        // the macOS formula from the spec: full hash f8de232e6e06d6bf
        assert_eq!(fnv1a64("/Users/x/Games/Stray"), 0xf8de_232e_6e06_d6bf);
    }

    #[test]
    fn copy_name_drops_punctuation_and_does_not_pad() {
        let id = GameId::from_key("c:/x");
        let name = unlocked_copy_name("PP-SA 01", &id);
        assert!(name.starts_with("PPSA01-"));
        assert!(!name.contains("--"));
    }

    #[test]
    fn bundle_identifier_is_lowercase_alphanumeric() {
        assert_eq!(bundle_identifier("PPSA02100"), "org.anyps5.ppsa02100");
        assert_eq!(bundle_identifier("CUSA-1_X"), "org.anyps5.cusa1x");
    }

    #[test]
    fn app_paths_layout() {
        let p = AppPaths::under(Path::new("/t"));
        assert!(p.preferences_file().ends_with("AnyPS5/preferences.json"));
        assert!(p.input_file("PPSA1-ab").ends_with("Controls/PPSA1-ab.ini"));
        assert!(p.log_file("PPSA1").ends_with("Logs/PPSA1.log"));
        assert!(p.reports_dir().ends_with("Logs/Reports"));
        assert!(
            p.game_backups_dir("PPSA-01 234")
                .ends_with("Backups/PPSA01234")
        );
        assert!(p.game_backups_dir("../").ends_with("Backups/unknown"));
        assert!(
            p.shader_cache_dir("PPSA02100")
                .ends_with("Cache/org.anyps5.ppsa02100/shader_cache")
        );
        assert!(p.default_screenshots().ends_with("Pictures/AnyPS5"));
        assert_eq!(
            unlocked_copy_dir(Path::new("/g"), "PPSA1", &GameId::from_key("c:/x")),
            Path::new("/g/.AnyPS5/Games")
                .join(unlocked_copy_name("PPSA1", &GameId::from_key("c:/x")))
        );
    }
}
