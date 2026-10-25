//! Persistent launcher preferences and per-game settings, stored as JSON in the locations of
//! [`AppPaths`] (PORT.md 6.6, GameSettings.swift).
//!
//! Every field has a serde default and unknown fields are kept, so a file written by an older or
//! newer launcher loads without loss. The per-game JSON uses the Swift launcher's `Codable` names,
//! so settings files can be exchanged with the macOS launcher.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::{
    controls::{Changes, Controls, InputTuning},
    library::Game,
    paths::{AppPaths, GameId},
};

/// Deserialises a value, falling back to the default when the file holds something this version
/// does not know (an unknown theme number, say) instead of rejecting the whole file.
fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    Ok(T::deserialize(Value::deserialize(deserializer)?).unwrap_or_default())
}

// ---------------------------------------------------------------------------------------------
// per-game settings

/// Where the performance overlay sits (`ANYPS5_OVERLAY_POS`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Overlay {
    #[default]
    Off,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Overlay {
    pub const ALL: [Overlay; 5] = [
        Overlay::Off,
        Overlay::TopLeft,
        Overlay::TopRight,
        Overlay::BottomLeft,
        Overlay::BottomRight,
    ];

    /// The value of `ANYPS5_OVERLAY_POS`; `None` for off.
    pub fn position(self) -> Option<&'static str> {
        match self {
            Overlay::Off => None,
            Overlay::TopLeft => Some("tl"),
            Overlay::TopRight => Some("tr"),
            Overlay::BottomLeft => Some("bl"),
            Overlay::BottomRight => Some("br"),
        }
    }
}

fn yes() -> bool {
    true
}

fn default_window_size() -> u32 {
    75
}

/// How long a game was played: total seconds, and how many sessions they came from. A session is
/// counted when the game's window showed, and its seconds are added while it runs, so a crash or a
/// closed launcher keeps what was seen.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Playtime {
    #[serde(default)]
    pub seconds: u64,
    #[serde(default)]
    pub sessions: u32,
}

impl Playtime {
    /// The game was played at all.
    pub fn is_played(self) -> bool {
        self.seconds > 0 || self.sessions > 0
    }
}

/// How many days of [`Preferences::playtime_days`] are kept.
pub const PLAYTIME_DAYS: i64 = 60;

/// The UTC day of a Unix time, as `YYYY-MM-DD`.
pub fn utc_day(unix_secs: i64) -> String {
    let date = time::OffsetDateTime::from_unix_timestamp(unix_secs)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .date();
    format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    )
}

/// The per-game choices passed to a started game. Fields are in alphabetical order of their JSON
/// names so serialised files have sorted keys like the Swift launcher's.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSettings {
    /// Keys moved from the launcher's layout, by pad action name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controls: Option<Changes>,
    /// Variables a recommendation set (checked again against the allowlist at every start).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub fullscreen: bool,
    /// Windows only: a Vulkan device name for `ANYPS5_GPU`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu: Option<String>,
    /// Console language number (`ANYPS5_LANGUAGE`); `None` follows the launcher's language.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<i32>,
    #[serde(default, deserialize_with = "lenient")]
    pub overlay: Overlay,
    /// Relinker flags a recommendation set, as flag and value pairs; used when the game is unlocked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relink: Vec<String>,
    /// `WIDTHxHEIGHT` the title's patches can set; `None` for the title's default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// `Some(false)` turns the shader and pipeline caches off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shader_cache: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tuning: Option<InputTuning>,
    #[serde(default = "yes")]
    pub vsync: bool,
    /// Share of the display the window takes, in percent (30 to 100 when used).
    #[serde(default = "default_window_size")]
    pub window_size: u32,
}

impl Default for GameSettings {
    fn default() -> Self {
        GameSettings {
            controls: None,
            env: BTreeMap::new(),
            fullscreen: false,
            gpu: None,
            language: None,
            overlay: Overlay::Off,
            relink: Vec::new(),
            resolution: None,
            shader_cache: None,
            tuning: None,
            vsync: true,
            window_size: 75,
        }
    }
}

/// What [`GameSettings::environment`] needs from outside.
#[derive(Clone, Debug, Default)]
pub struct EnvContext {
    /// The launcher's console language number (19 Turkish, 1 English).
    pub launcher_language: i32,
    /// The `anyps5-input.ini` written for this start; `None` starts with the engine's own keys.
    pub input_file: Option<PathBuf>,
    pub screenshots: Option<PathBuf>,
    pub fonts: Option<PathBuf>,
    /// `ANYPS5_SHADER_CACHE_DIR`.
    pub shader_cache: Option<PathBuf>,
}

impl GameSettings {
    /// The window size in percent, clamped to 30..=100. Used by the launcher's window placement.
    pub fn window_size_clamped(&self) -> u32 {
        self.window_size.clamp(30, 100)
    }

    /// The environment variables a game starts with. `fullscreen`, `window_size` and `resolution`
    /// never appear: the first two are done by the launcher's window logic (D9), the third by
    /// patches at unlock time (D10).
    pub fn environment(&self, ctx: &EnvContext) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        let mut set = |key: &str, value: String| {
            env.insert(key.to_string(), value);
        };
        set(
            "ANYPS5_PRESENT_MODE",
            if self.vsync { "fifo" } else { "immediate" }.to_string(),
        );
        set(
            "ANYPS5_LANGUAGE",
            self.language.unwrap_or(ctx.launcher_language).to_string(),
        );
        if let Some(position) = self.overlay.position() {
            set("ANYPS5_OVERLAY", "1".to_string());
            set("ANYPS5_OVERLAY_POS", position.to_string());
        }
        let path = |p: &Path| p.to_string_lossy().into_owned();
        if let Some(file) = &ctx.input_file {
            set("ANYPS5_INPUT_CONFIG", path(file));
        }
        if let Some(dir) = &ctx.screenshots {
            set("ANYPS5_SCREENSHOTS", path(dir));
        }
        if let Some(dir) = &ctx.fonts {
            set("ANYPS5_SYSTEM_FONTS", path(dir));
        }
        if let Some(dir) = &ctx.shader_cache {
            set("ANYPS5_SHADER_CACHE_DIR", path(dir));
        }
        if self.shader_cache == Some(false) {
            set("ANYPS5_NO_SHADER_CACHE", "1".to_string());
        }
        if let Some(gpu) = self.gpu.as_deref().filter(|g| !g.is_empty()) {
            set("ANYPS5_GPU", gpu.to_string());
        }
        for (key, value) in &self.env {
            if crate::recommended::env_allowed(key, value) {
                set(key, value.clone());
            }
        }
        env
    }

    /// The text of this game's `anyps5-input.ini` for the toolkit with this version or tag.
    pub fn input_file_text(&self, toolkit: Option<&str>) -> String {
        Controls::file_for(
            &self.controls.clone().unwrap_or_default(),
            &self.tuning.unwrap_or_default(),
            toolkit,
        )
    }

    /// Writes the game's input file atomically, creating parent folders. On failure the caller
    /// starts the game without `ANYPS5_INPUT_CONFIG`.
    pub fn write_input_file(&self, file: &Path, toolkit: Option<&str>) -> io::Result<()> {
        write_atomic(file, self.input_file_text(toolkit).as_bytes())
    }
}

/// Console language numbers (`sceSystemServiceParamGetInt`) a game can be started with.
pub struct ConsoleLanguage;

impl ConsoleLanguage {
    /// (number, native name), in the order the launcher lists them.
    pub const ALL: [(i32, &'static str); 18] = [
        (19, "Türkçe"),
        (1, "English (US)"),
        (18, "English (UK)"),
        (4, "Deutsch"),
        (2, "Français"),
        (3, "Español"),
        (20, "Español (Latinoamérica)"),
        (5, "Italiano"),
        (17, "Português (Brasil)"),
        (7, "Português"),
        (8, "Русский"),
        (16, "Polski"),
        (6, "Nederlands"),
        (0, "日本語"),
        (9, "한국어"),
        (11, "简体中文"),
        (10, "繁體中文"),
        (21, "العربية"),
    ];

    /// The locale tag of a console language number, as the engine's trophy library maps them.
    pub fn tag(number: i32) -> Option<&'static str> {
        const TAGS: [&str; 22] = [
            "ja-JP", "en-US", "fr-FR", "es-ES", "de-DE", "it-IT", "nl-NL", "pt-PT", "ru-RU",
            "ko-KR", "zh-Hant", "zh-Hans", "fi-FI", "sv-SE", "da-DK", "no-NO", "pl-PL", "pt-BR",
            "en-GB", "tr-TR", "es-419", "ar-AE",
        ];
        usize::try_from(number)
            .ok()
            .and_then(|i| TAGS.get(i).copied())
    }
}

// ---------------------------------------------------------------------------------------------
// export / import

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("not an AnyPlay settings file")]
    NotSettingsFile,
    #[error("settings file is damaged: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
}

const SHARED_FORMAT: &str = "anyps5-settings";

/// The export/import form of one game's settings.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SharedSettings {
    pub format: String,
    pub game: String,
    pub settings: GameSettings,
    #[serde(rename = "titleID")]
    pub title_id: String,
    pub version: u32,
}

impl SharedSettings {
    pub fn new(game: &Game, settings: &GameSettings) -> SharedSettings {
        SharedSettings {
            format: SHARED_FORMAT.to_string(),
            game: game.name.clone(),
            settings: settings.clone(),
            title_id: game.title_id.clone(),
            version: 1,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings serialise")
    }

    /// Parses an exported file. Files from the macOS launcher work; unknown keys are ignored.
    pub fn read(bytes: &[u8]) -> Result<SharedSettings, SettingsError> {
        let value: Value = serde_json::from_slice(bytes)?;
        if value.get("format").and_then(Value::as_str) != Some(SHARED_FORMAT) {
            return Err(SettingsError::NotSettingsFile);
        }
        Ok(serde_json::from_value(value)?)
    }
}

// ---------------------------------------------------------------------------------------------
// app preferences

/// The launcher's interface language.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Language {
    #[serde(rename = "tr")]
    Turkish,
    #[default]
    #[serde(rename = "en")]
    English,
}

impl Language {
    /// Turkish for any locale tag starting with `tr`, English otherwise.
    pub fn from_locale(tag: &str) -> Language {
        if tag.to_ascii_lowercase().starts_with("tr") {
            Language::Turkish
        } else {
            Language::English
        }
    }

    /// The console language number games get by default.
    pub fn console_language(self) -> i32 {
        match self {
            Language::Turkish => 19,
            Language::English => 1,
        }
    }
}

/// The colour theme, stored as the Swift raw integer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    #[default]
    Cosmic,
    Aurora,
    Sunset,
}

impl Serialize for Theme {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u8(match self {
            Theme::Cosmic => 0,
            Theme::Aurora => 1,
            Theme::Sunset => 2,
        })
    }
}

impl<'de> Deserialize<'de> for Theme {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match u64::deserialize(d)? {
            1 => Theme::Aurora,
            2 => Theme::Sunset,
            _ => Theme::Cosmic,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    Blue,
    Mint,
    Coral,
    Violet,
    Gold,
    White,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    #[default]
    Theme,
    Image,
    Video,
}

/// Which CPU conversion the relinker is asked for (D1).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CpuConversion {
    #[default]
    Auto,
    None,
    Intel,
}

fn default_profile_name() -> String {
    "Player".to_string()
}

fn default_repository() -> String {
    "boykopovar/AnyPS5".to_string()
}

fn one() -> u32 {
    1
}

/// The launcher's own settings (`%APPDATA%\AnyPS5\preferences.json`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub onboarded: bool,
    #[serde(default, deserialize_with = "lenient")]
    pub language: Language,
    #[serde(default, deserialize_with = "lenient")]
    pub theme: Theme,
    /// `None` follows the theme's own accent.
    #[serde(default, deserialize_with = "lenient")]
    pub accent: Option<Accent>,
    #[serde(default, deserialize_with = "lenient")]
    pub backdrop: Backdrop,
    #[serde(default)]
    pub backdrop_file: Option<PathBuf>,
    #[serde(default = "yes")]
    pub game_artwork: bool,
    #[serde(default)]
    pub reduced_motion: bool,
    #[serde(default = "yes")]
    pub sounds: bool,
    /// Home screen music.
    #[serde(default = "yes")]
    pub music: bool,
    /// The running game on the player's Discord profile.
    #[serde(default = "yes")]
    pub discord_presence: bool,
    #[serde(default)]
    pub hide_while_playing: bool,
    /// Hidden trophies that are not earned show their name and description in the trophy list.
    /// Local only: the network never sees it.
    #[serde(default)]
    pub show_hidden_trophies: bool,
    /// The launcher fills its screen, like a console; off, it is a maximised window (F11).
    #[serde(default = "yes")]
    pub fullscreen: bool,
    #[serde(default = "default_profile_name")]
    pub profile_name: String,
    /// The games folder; `None` until the player picks one.
    #[serde(default)]
    pub games_folder: Option<PathBuf>,
    #[serde(default)]
    pub screenshots_folder: Option<PathBuf>,
    #[serde(default)]
    pub fonts_folder: Option<PathBuf>,
    /// Settings of each game by [`GameId`] string.
    #[serde(default)]
    pub games: BTreeMap<String, GameSettings>,
    /// RFC 3339 time each game was last played, by [`GameId`] string.
    #[serde(default)]
    pub last_played: BTreeMap<String, String>,
    /// Time played of each game, by [`GameId` string](GameId).
    #[serde(default)]
    pub playtime: BTreeMap<String, Playtime>,
    /// Seconds played of each game on each UTC day (`YYYY-MM-DD`), by [`GameId` string](GameId):
    /// what the network's playtime records are made of. Only the last [`PLAYTIME_DAYS`] days stay.
    #[serde(default)]
    pub playtime_days: BTreeMap<String, BTreeMap<String, u64>>,
    /// A toolkit folder the player picked instead of the downloaded one.
    #[serde(default)]
    pub toolkit: Option<PathBuf>,
    #[serde(default = "yes")]
    pub check_for_updates: bool,
    #[serde(default)]
    pub toolkit_checked_at: Option<String>,
    #[serde(default = "default_repository")]
    pub toolkit_repository: String,
    /// A release tag to stay on (the pinned channel); `None` follows the latest release.
    #[serde(default)]
    pub toolkit_pinned: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    pub cpu_conversion: CpuConversion,
    /// The cloud of the network the player signs in to; `None` until one is set.
    #[serde(default)]
    pub online_cloud: Option<String>,
    /// The player signed in to the network on this PC: the net service starts with the launcher.
    #[serde(default)]
    pub online: bool,
    /// Tunnels to other players go only through a relay, so they never see this PC's address.
    #[serde(default)]
    pub online_relay_only: bool,
    /// Party voice: on or off, the mode, the push-to-talk key, devices and member volumes.
    #[serde(default)]
    pub voice: crate::online::VoicePrefs,
    /// Games write an observation log of their network calls (`Logs\<TITLEID>-network.jsonl`).
    #[serde(default)]
    pub record_network: bool,
    /// Fields written by a newer launcher survive a round trip.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            version: 1,
            onboarded: false,
            language: Language::English,
            theme: Theme::Cosmic,
            accent: None,
            backdrop: Backdrop::Theme,
            backdrop_file: None,
            game_artwork: true,
            reduced_motion: false,
            sounds: true,
            music: true,
            discord_presence: true,
            hide_while_playing: false,
            show_hidden_trophies: false,
            fullscreen: true,
            profile_name: default_profile_name(),
            games_folder: None,
            screenshots_folder: None,
            fonts_folder: None,
            games: BTreeMap::new(),
            last_played: BTreeMap::new(),
            playtime: BTreeMap::new(),
            playtime_days: BTreeMap::new(),
            toolkit: None,
            check_for_updates: true,
            toolkit_checked_at: None,
            toolkit_repository: default_repository(),
            toolkit_pinned: None,
            cpu_conversion: CpuConversion::Auto,
            online_cloud: None,
            online: false,
            online_relay_only: false,
            voice: crate::online::VoicePrefs::default(),
            record_network: false,
            extra: Map::new(),
        }
    }
}

/// Which toolkit the launcher follows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolkitChannel {
    /// The latest release; updates are offered.
    Stable,
    /// One chosen release; no update is offered.
    Pinned,
    /// A folder the player built or picked.
    Local,
}

impl Preferences {
    pub fn toolkit_channel(&self) -> ToolkitChannel {
        if self.toolkit.is_some() {
            ToolkitChannel::Local
        } else if self.toolkit_pinned.is_some() {
            ToolkitChannel::Pinned
        } else {
            ToolkitChannel::Stable
        }
    }

    /// Defaults for a first start: language from the Windows user locale, profile name from the
    /// user name.
    pub fn first_run() -> Preferences {
        let mut prefs = Preferences::default();
        if let Some(locale) = user_locale() {
            prefs.language = Language::from_locale(&locale);
        }
        if let Some(name) = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .ok()
            .and_then(|n| n.split_whitespace().next().map(str::to_string))
        {
            prefs.profile_name = name;
        }
        prefs
    }

    /// Loads the preferences. A missing file gives [`Preferences::first_run`]; a file that does
    /// not parse is moved to `preferences.json.bad` and defaults are used.
    pub fn load(paths: &AppPaths) -> Preferences {
        let file = paths.preferences_file();
        let bytes = match fs::read(&file) {
            Ok(bytes) => bytes,
            Err(_) => return Preferences::first_run(),
        };
        let bytes = bytes
            .strip_prefix(&[0xEF, 0xBB, 0xBF][..])
            .unwrap_or(&bytes);
        match serde_json::from_slice(bytes) {
            Ok(prefs) => prefs,
            Err(error) => {
                tracing::warn!(
                    "preferences.json is damaged ({error}); moved to preferences.json.bad"
                );
                let mut bad = file.clone().into_os_string();
                bad.push(".bad");
                let _ = fs::remove_file(&bad);
                let _ = fs::rename(&file, &bad);
                Preferences::first_run()
            }
        }
    }

    /// Writes the preferences atomically.
    pub fn save(&self, paths: &AppPaths) -> io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        write_atomic(&paths.preferences_file(), text.as_bytes())
    }

    /// The settings of a game (defaults when it has none yet).
    pub fn game_settings(&self, id: &GameId) -> GameSettings {
        self.games.get(id.as_str()).cloned().unwrap_or_default()
    }

    pub fn set_game_settings(&mut self, id: &GameId, settings: GameSettings) {
        self.games.insert(id.as_str().to_string(), settings);
    }

    /// What is known of the time a game was played (nothing before its first session).
    pub fn playtime(&self, id: &GameId) -> Playtime {
        self.playtime.get(id.as_str()).copied().unwrap_or_default()
    }

    /// Counts a session that was seen to start.
    pub fn count_session(&mut self, id: &GameId) {
        let entry = self.playtime.entry(id.as_str().to_owned()).or_default();
        entry.sessions = entry.sessions.saturating_add(1);
    }

    /// Adds seconds played to a game.
    pub fn add_playtime(&mut self, id: &GameId, seconds: u64) {
        let entry = self.playtime.entry(id.as_str().to_owned()).or_default();
        entry.seconds = entry.seconds.saturating_add(seconds);
    }

    /// Adds seconds played to a game's day (`now` in Unix seconds), and forgets days older than
    /// [`PLAYTIME_DAYS`].
    pub fn add_playtime_day(&mut self, id: &GameId, seconds: u64, now: i64) {
        let today = utc_day(now);
        let oldest = utc_day(now - PLAYTIME_DAYS * 86_400);
        let days = self
            .playtime_days
            .entry(id.as_str().to_owned())
            .or_default();
        let day = days.entry(today).or_default();
        *day = day.saturating_add(seconds);
        days.retain(|day, _| *day >= oldest);
        self.playtime_days.retain(|_, days| !days.is_empty());
    }

    /// The folder for F12 screenshots: the chosen one or `Pictures\AnyPS5`.
    pub fn screenshots_dir(&self, paths: &AppPaths) -> PathBuf {
        self.screenshots_folder
            .clone()
            .unwrap_or_else(|| paths.default_screenshots())
    }
}

#[cfg(windows)]
fn user_locale() -> Option<String> {
    unsafe extern "system" {
        fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
    }
    let mut buffer = [0u16; 85];
    // SAFETY: the buffer is 85 u16 long, the documented LOCALE_NAME_MAX_LENGTH, and the length passed matches.
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    (written > 1).then(|| String::from_utf16_lossy(&buffer[..written as usize - 1]))
}

#[cfg(not(windows))]
fn user_locale() -> Option<String> {
    std::env::var("LANG").ok()
}

/// Writes `bytes` to `file` through a temporary file in the same folder and a rename, creating
/// parent folders. A reader never sees a half-written file.
pub(crate) fn write_atomic(file: &Path, bytes: &[u8]) -> io::Result<()> {
    use io::Write;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut temp = file.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = PathBuf::from(temp);
    let result = (|| {
        let mut handle = fs::File::create(&temp)?;
        handle.write_all(bytes)?;
        handle.sync_all()?;
        drop(handle);
        fs::rename(&temp, file)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::ExecutableKind;
    use serde_json::json;

    fn game() -> Game {
        Game {
            id: GameId::from_key("c:/games/stray"),
            folder: PathBuf::from("C:/Games/Stray"),
            title_id: "PPSA02100".into(),
            name: "Stray".into(),
            names: BTreeMap::new(),
            version: "1.0".into(),
            content_id: None,
            icon: None,
            background: None,
            music: None,
            executable: ExecutableKind::Elf,
            size: None,
            unlocked_with: None,
        }
    }

    fn ctx() -> EnvContext {
        EnvContext {
            launcher_language: 1,
            ..Default::default()
        }
    }

    #[test]
    fn defaults_environment_is_minimal() {
        let env = GameSettings::default().environment(&ctx());
        assert_eq!(env.len(), 2);
        assert_eq!(env["ANYPS5_PRESENT_MODE"], "fifo");
        assert_eq!(env["ANYPS5_LANGUAGE"], "1");
    }

    #[test]
    fn environment_table() {
        for vsync in [true, false] {
            for overlay in Overlay::ALL {
                for language in [None, Some(4)] {
                    for shader_cache in [None, Some(true), Some(false)] {
                        let s = GameSettings {
                            vsync,
                            overlay,
                            language,
                            shader_cache,
                            ..Default::default()
                        };
                        let env = s.environment(&EnvContext {
                            launcher_language: 19,
                            ..Default::default()
                        });
                        assert_eq!(
                            env["ANYPS5_PRESENT_MODE"],
                            if vsync { "fifo" } else { "immediate" }
                        );
                        assert_eq!(env["ANYPS5_LANGUAGE"], language.unwrap_or(19).to_string());
                        assert_eq!(
                            env.get("ANYPS5_OVERLAY").map(String::as_str),
                            overlay.position().map(|_| "1")
                        );
                        assert_eq!(
                            env.get("ANYPS5_OVERLAY_POS").map(String::as_str),
                            overlay.position()
                        );
                        assert_eq!(
                            env.contains_key("ANYPS5_NO_SHADER_CACHE"),
                            shader_cache == Some(false)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn overlay_positions() {
        let pos: Vec<_> = Overlay::ALL.iter().map(|o| o.position()).collect();
        assert_eq!(pos, [None, Some("tl"), Some("tr"), Some("bl"), Some("br")]);
    }

    #[test]
    fn paths_and_gpu_in_environment() {
        let c = EnvContext {
            launcher_language: 1,
            input_file: Some(PathBuf::from(r"C:\x\a.ini")),
            screenshots: Some(PathBuf::from(r"C:\Pictures\AnyPS5")),
            fonts: Some(PathBuf::from(r"C:\Windows\Fonts")),
            shader_cache: Some(PathBuf::from(r"C:\cache")),
        };
        let s = GameSettings {
            gpu: Some("NVIDIA".into()),
            ..Default::default()
        };
        let env = s.environment(&c);
        assert_eq!(env["ANYPS5_INPUT_CONFIG"], r"C:\x\a.ini");
        assert_eq!(env["ANYPS5_SCREENSHOTS"], r"C:\Pictures\AnyPS5");
        assert_eq!(env["ANYPS5_SYSTEM_FONTS"], r"C:\Windows\Fonts");
        assert_eq!(env["ANYPS5_SHADER_CACHE_DIR"], r"C:\cache");
        assert_eq!(env["ANYPS5_GPU"], "NVIDIA");
        let empty_gpu = GameSettings {
            gpu: Some(String::new()),
            ..Default::default()
        };
        assert!(!empty_gpu.environment(&c).contains_key("ANYPS5_GPU"));
    }

    #[test]
    fn launcher_only_values_never_leak() {
        let s = GameSettings {
            fullscreen: true,
            window_size: 5,
            resolution: Some("1920x1080".into()),
            ..Default::default()
        };
        let env = s.environment(&ctx());
        for key in ["ANYPS5_FULLSCREEN", "ANYPS5_WINDOW_SIZE", "APS5_RESOLUTION"] {
            assert!(!env.contains_key(key), "{key}");
        }
        assert_eq!(s.window_size_clamped(), 30);
        assert_eq!(
            GameSettings {
                window_size: 400,
                ..Default::default()
            }
            .window_size_clamped(),
            100
        );
    }

    #[test]
    fn game_settings_json_has_the_swift_keys() {
        let value = serde_json::to_value(GameSettings::default()).unwrap();
        assert_eq!(
            value,
            json!({"fullscreen": false, "overlay": "off", "vsync": true, "windowSize": 75})
        );
        let s = GameSettings {
            overlay: Overlay::TopRight,
            ..Default::default()
        };
        assert_eq!(serde_json::to_value(&s).unwrap()["overlay"], "topRight");
    }

    #[test]
    fn game_settings_keys_are_sorted() {
        let s = GameSettings {
            controls: Some(Changes::new()),
            gpu: Some("g".into()),
            language: Some(1),
            resolution: Some("1x1".into()),
            shader_cache: Some(false),
            tuning: Some(InputTuning::default()),
            ..Default::default()
        };
        let value = serde_json::to_value(&s).unwrap();
        let keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted);
        assert_eq!(serde_json::from_value::<GameSettings>(value).unwrap(), s);
    }

    #[test]
    fn old_game_settings_get_defaults() {
        let s: GameSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, GameSettings::default());
        let s: GameSettings =
            serde_json::from_str(r#"{"fullscreen":true,"unknownLater":3,"overlay":"hologram"}"#)
                .unwrap();
        assert!(s.fullscreen && s.vsync && s.window_size == 75);
        assert_eq!(s.overlay, Overlay::Off);
    }

    /// What the Swift launcher's `JSONEncoder` (sorted keys) writes for a game's settings.
    const MAC_EXPORT: &str = r#"{
  "format" : "anyps5-settings",
  "game" : "Stray",
  "settings" : {
    "controls" : {
      "Circle" : [
        "KEY:C"
      ]
    },
    "fullscreen" : true,
    "language" : 4,
    "overlay" : "bottomLeft",
    "resolution" : "2560x1440",
    "shaderCache" : false,
    "tuning" : {
      "leftDeadzone" : 20
    },
    "vsync" : false,
    "windowSize" : 60
  },
  "titleID" : "PPSA02100",
  "version" : 1
}"#;

    #[test]
    fn imports_a_macos_export() {
        let shared = SharedSettings::read(MAC_EXPORT.as_bytes()).unwrap();
        assert_eq!(shared.title_id, "PPSA02100");
        assert!(shared.settings.fullscreen && !shared.settings.vsync);
        assert_eq!(shared.settings.window_size, 60);
        assert_eq!(shared.settings.overlay, Overlay::BottomLeft);
        assert_eq!(
            shared.settings.controls.as_ref().unwrap()["Circle"],
            ["KEY:C"]
        );
        assert_eq!(shared.settings.tuning.unwrap().left_deadzone, 20);
        assert_eq!(shared.settings.tuning.unwrap().mouse_decay_ms, 30);
    }

    #[test]
    fn shared_settings_round_trip_and_errors() {
        let settings = GameSettings {
            fullscreen: true,
            language: Some(19),
            ..Default::default()
        };
        let shared = SharedSettings::new(&game(), &settings);
        let text = shared.to_json();
        assert!(text.find("\"format\"").unwrap() < text.find("\"titleID\"").unwrap());
        assert_eq!(SharedSettings::read(text.as_bytes()).unwrap(), shared);
        assert!(matches!(
            SharedSettings::read(br#"{"format":"other"}"#),
            Err(SettingsError::NotSettingsFile)
        ));
        assert!(matches!(
            SharedSettings::read(b"[]"),
            Err(SettingsError::NotSettingsFile)
        ));
        assert!(matches!(
            SharedSettings::read(b"{nope"),
            Err(SettingsError::Json(_))
        ));
        let extra = text.replacen('{', r#"{"future": 1,"#, 1);
        assert!(SharedSettings::read(extra.as_bytes()).is_ok());
    }

    #[test]
    fn console_languages() {
        assert_eq!(ConsoleLanguage::ALL.len(), 18);
        assert_eq!(ConsoleLanguage::ALL[0], (19, "Türkçe"));
        assert_eq!(ConsoleLanguage::tag(19), Some("tr-TR"));
        assert_eq!(ConsoleLanguage::tag(1), Some("en-US"));
        assert_eq!(ConsoleLanguage::tag(4), Some("de-DE"));
        assert_eq!(ConsoleLanguage::tag(22), None);
        assert_eq!(ConsoleLanguage::tag(-1), None);
        for (number, _) in ConsoleLanguage::ALL {
            assert!(ConsoleLanguage::tag(number).is_some());
        }
    }

    #[test]
    fn language_from_locale() {
        assert_eq!(Language::from_locale("tr-TR"), Language::Turkish);
        assert_eq!(Language::from_locale("TR"), Language::Turkish);
        assert_eq!(Language::from_locale("de-DE"), Language::English);
        assert_eq!(Language::Turkish.console_language(), 19);
        assert_eq!(Language::English.console_language(), 1);
    }

    #[test]
    fn preferences_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let mut prefs = Preferences {
            onboarded: true,
            language: Language::Turkish,
            theme: Theme::Sunset,
            accent: Some(Accent::Gold),
            backdrop: Backdrop::Image,
            games_folder: Some(PathBuf::from(r"D:\Dumps")),
            cpu_conversion: CpuConversion::Intel,
            ..Default::default()
        };
        prefs.set_game_settings(
            &GameId::from_key("d:/dumps/stray"),
            GameSettings {
                fullscreen: true,
                ..Default::default()
            },
        );
        prefs
            .last_played
            .insert("d:/dumps/stray".into(), "2026-10-22T09:30:00Z".into());
        let stray = GameId::from_key("d:/dumps/stray");
        prefs.count_session(&stray);
        prefs.add_playtime(&stray, 3725);
        prefs.save(&paths).unwrap();
        assert!(!paths.preferences_file().with_extension("json.tmp").exists());
        assert_eq!(Preferences::load(&paths), prefs);
        assert!(
            prefs
                .game_settings(&GameId::from_key("d:/dumps/stray"))
                .fullscreen
        );
        assert_eq!(
            prefs.game_settings(&GameId::from_key("other")),
            GameSettings::default()
        );
    }

    #[test]
    fn days_are_utc_dates() {
        assert_eq!(utc_day(0), "1970-01-01");
        assert_eq!(utc_day(1_792_800_000), "2026-10-24");
        assert_eq!(utc_day(1_792_800_000 + 86_399), "2026-10-24");
        assert_eq!(utc_day(951_782_400), "2000-02-29");
    }

    #[test]
    fn playtime_per_day_adds_up_and_old_days_go() {
        let mut prefs = Preferences::default();
        let id = GameId::from_key("d:/dumps/stray");
        let day = 1_792_800_000;
        prefs.add_playtime_day(&id, 60, day);
        prefs.add_playtime_day(&id, 90, day + 3_600);
        prefs.add_playtime_day(&id, 30, day + 86_400);
        let days = &prefs.playtime_days[id.as_str()];
        assert_eq!(days["2026-10-24"], 150);
        assert_eq!(days["2026-10-25"], 30);
        // Sixty-one days later the first two are gone.
        prefs.add_playtime_day(&id, 10, day + 61 * 86_400);
        let days = &prefs.playtime_days[id.as_str()];
        assert_eq!(
            days.keys().collect::<Vec<_>>(),
            ["2026-10-25", "2026-12-24"]
        );
        let old: Preferences = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert!(old.playtime_days.is_empty());
    }

    #[test]
    fn playtime_adds_up_and_old_files_have_none() {
        let mut prefs = Preferences::default();
        let id = GameId::from_key("d:/dumps/stray");
        assert_eq!(prefs.playtime(&id), Playtime::default());
        assert!(!prefs.playtime(&id).is_played());
        prefs.count_session(&id);
        prefs.add_playtime(&id, 40);
        prefs.add_playtime(&id, 80);
        prefs.count_session(&id);
        let time = prefs.playtime(&id);
        assert_eq!((time.seconds, time.sessions), (120, 2));
        assert!(time.is_played());
        // Another game is untouched, and the sums cannot overflow.
        assert!(!prefs.playtime(&GameId::from_key("other")).is_played());
        prefs.add_playtime(&id, u64::MAX);
        assert_eq!(prefs.playtime(&id).seconds, u64::MAX);
        // A file from before playtime was recorded loads without it.
        let old: Preferences = serde_json::from_str(r#"{"version":1,"lastPlayed":{}}"#).unwrap();
        assert!(old.playtime.is_empty());
        // The key is the game's id; a half-written entry keeps what it has.
        let json = serde_json::to_value(&prefs).unwrap();
        assert_eq!(json["playtime"]["d:/dumps/stray"]["sessions"], 2);
        let half: Playtime = serde_json::from_str(r#"{"seconds":9}"#).unwrap();
        assert_eq!((half.seconds, half.sessions), (9, 0));
    }

    #[test]
    fn preferences_json_shape() {
        let value = serde_json::to_value(Preferences::default()).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["theme"], 0);
        assert_eq!(value["language"], "en");
        assert_eq!(value["backdrop"], "theme");
        assert_eq!(value["toolkitRepository"], "boykopovar/AnyPS5");
        assert_eq!(value["gamesFolder"], Value::Null);
        assert_eq!(value["cpuConversion"], "auto");
    }

    #[test]
    fn old_version_file_gets_defaults_and_unknown_fields_survive() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        fs::create_dir_all(&paths.config).unwrap();
        fs::write(
            paths.preferences_file(),
            r#"{"onboarded": true, "theme": 2, "futureThing": {"a": [1,2]}, "games": {"c:/g": {"vsync": false}}}"#,
        )
        .unwrap();
        let prefs = Preferences::load(&paths);
        assert!(prefs.onboarded);
        assert_eq!(prefs.theme, Theme::Sunset);
        assert!(prefs.sounds && prefs.music && prefs.discord_presence && prefs.game_artwork);
        assert!(prefs.fullscreen);
        assert!(prefs.check_for_updates);
        assert_eq!(prefs.profile_name, "Player");
        assert_eq!(prefs.toolkit_repository, "boykopovar/AnyPS5");
        assert!(!prefs.games["c:/g"].vsync);
        assert_eq!(prefs.games["c:/g"].window_size, 75);
        prefs.save(&paths).unwrap();
        let again: Value =
            serde_json::from_slice(&fs::read(paths.preferences_file()).unwrap()).unwrap();
        assert_eq!(again["futureThing"], json!({"a": [1, 2]}));
    }

    #[test]
    fn unknown_enum_values_fall_back() {
        let prefs: Preferences =
            serde_json::from_str(r#"{"theme": 9, "accent": "teal", "backdrop": "gif", "language": "fr", "cpuConversion": "amd"}"#).unwrap();
        assert_eq!(prefs.theme, Theme::Cosmic);
        assert_eq!(prefs.accent, None);
        assert_eq!(prefs.backdrop, Backdrop::Theme);
        assert_eq!(prefs.language, Language::English);
        assert_eq!(prefs.cpu_conversion, CpuConversion::Auto);
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        fs::create_dir_all(&paths.config).unwrap();
        fs::write(paths.preferences_file(), "{ this is not json").unwrap();
        let prefs = Preferences::load(&paths);
        assert!(!prefs.onboarded);
        assert!(!paths.preferences_file().exists());
        assert_eq!(
            fs::read_to_string(paths.config.join("preferences.json.bad")).unwrap(),
            "{ this is not json"
        );
    }

    #[test]
    fn missing_file_is_a_first_run() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = Preferences::load(&AppPaths::under(dir.path()));
        assert!(!prefs.onboarded && prefs.games.is_empty());
    }

    #[test]
    fn screenshots_dir_defaults_to_pictures() {
        let paths = AppPaths::under(Path::new("/t"));
        let mut prefs = Preferences::default();
        assert!(prefs.screenshots_dir(&paths).ends_with("Pictures/AnyPS5"));
        prefs.screenshots_folder = Some(PathBuf::from("/shots"));
        assert_eq!(prefs.screenshots_dir(&paths), PathBuf::from("/shots"));
    }

    #[test]
    fn input_file_is_written_atomically_with_the_games_controls() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("Controls/PPSA1-abc.ini");
        let mut controls = Changes::new();
        controls.insert("Circle".into(), vec!["KEY:C".into()]);
        let s = GameSettings {
            controls: Some(controls),
            tuning: Some(InputTuning {
                left_deadzone: 10,
                ..Default::default()
            }),
            ..Default::default()
        };
        s.write_input_file(&file, None).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert_eq!(text, s.input_file_text(None));
        assert!(text.starts_with("# Written by the AnyPlay Launcher"));
        assert!(text.contains("Circle = KEY:C\n") && text.ends_with("LeftStickDeadzone = 10\n"));
        assert!(!dir.path().join("Controls/PPSA1-abc.ini.tmp").exists());
        // rewriting replaces
        GameSettings::default()
            .write_input_file(&file, None)
            .unwrap();
        assert!(
            fs::read_to_string(&file)
                .unwrap()
                .contains("Circle = KEY:L")
        );
    }

    #[test]
    fn input_file_write_failure_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("blocker");
        fs::write(&blocker, "x").unwrap();
        assert!(
            GameSettings::default()
                .write_input_file(&blocker.join("x.ini"), None)
                .is_err()
        );
    }
}
