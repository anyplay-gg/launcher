//! The AnyPS5 toolkit on this PC: finding it, checking it, getting a new release, and the
//! version string unlocked copies are compared against (PORT.md 6.3).
//!
//! Interface: [`Toolkit::locate`], [`Toolkit::open`], [`adopt_folder`], [`check_latest`],
//! [`download_and_install`], [`update_available`]. Release lookup, zip extraction and the
//! fingerprint are hidden behind them.

use std::cmp::Ordering;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::CancelToken;
use crate::paths::AppPaths;
use crate::platform::Os;

mod macos;
mod pins;

pub use pins::{
    GameToolkit, InstalledRelease, game_toolkit, installed_tags, installed_version,
    installed_with_sizes, open_for_game, pin_tag, remove_release,
};

/// The MinGW runtime the libraries need beside them (without it `LoadLibrary` fails with 126).
pub const MINGW_DLLS: [&str; 3] = [
    "libgcc_s_seh-1.dll",
    "libstdc++-6.dll",
    "libwinpthread-1.dll",
];

/// Default release repository (`toolkitRepository` setting).
pub const DEFAULT_REPOSITORY: &str = if cfg!(target_os = "macos") {
    "utkuhalis/AnyPS5-ARM"
} else {
    "boykopovar/AnyPS5"
};

pub(crate) const API_BASE: &str = "https://api.github.com";

#[derive(Debug, thiserror::Error)]
pub enum ToolkitError {
    #[error("{0} not found in the toolkit folder")]
    Missing(String),
    #[error("the toolkit lacks the MinGW runtime: {}", .0.join(", "))]
    MissingRuntime(Vec<String>),
    #[error("{0}")]
    Network(String),
    #[error("the downloaded archive is not usable: {0}")]
    BadArchive(String),
    #[error("the download does not match its checksum")]
    Digest,
    /// The body is longer than the size the caller was promised.
    #[error("the download is larger than expected")]
    TooLarge,
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("the release has no relinker or libraries for this system")]
    NoRelease,
    #[error("cancelled")]
    Cancelled,
    /// The release a game stays on is not installed (or not usable).
    #[error("the toolkit {0} chosen for this game is not installed")]
    PinMissing(String),
    /// An installed release that a game or the preferences still use cannot be removed.
    #[error("the toolkit {0} is in use")]
    InUse(String),
}

/// What the preferences remember about the player's toolkit choice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ToolkitChoice {
    /// An installed release under `%LOCALAPPDATA%\AnyPS5\Toolkit\<tag>`.
    Release { tag: String },
    /// A folder the player picked (a self-built toolkit); nothing is copied.
    Folder { relinker: PathBuf, libs: PathBuf },
}

/// A toolkit that is ready to use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toolkit {
    root: PathBuf,
    relinker: PathBuf,
    libs: PathBuf,
    patches: PathBuf,
    version: String,
    tag: Option<String>,
    /// MinGW DLLs found outside `libs` (adopted toolkits only).
    extra_runtime: Vec<PathBuf>,
    adopted: bool,
    /// SHA-256 of the relinker and the libraries zip as the release published them.
    digests: Option<Digests>,
}

/// The checksums a downloaded release was verified against (kept in `toolkit.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Digests {
    pub relinker: String,
    pub libs: String,
}

impl Toolkit {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn relinker(&self) -> &Path {
        &self.relinker
    }

    pub fn libs(&self) -> &Path {
        &self.libs
    }

    /// The per-title patch files shipped with the toolkit (may not exist).
    pub fn patches(&self) -> &Path {
        &self.patches
    }

    /// `<tag>-<fingerprint>`, or `local-<fingerprint>` for an adopted folder.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The release tag without `v`; `None` for an adopted folder.
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }

    pub fn is_adopted(&self) -> bool {
        self.adopted
    }

    /// The release checksums; `None` for adopted folders and installs from before they were kept.
    pub fn digests(&self) -> Option<&Digests> {
        self.digests.as_ref()
    }

    /// MinGW DLLs that must be copied into an unlocked copy besides the contents of `libs`.
    pub fn extra_runtime(&self) -> &[PathBuf] {
        &self.extra_runtime
    }

    /// What to store in the preferences so `locate` finds this toolkit again.
    pub fn choice(&self) -> ToolkitChoice {
        match &self.tag {
            Some(tag) if !self.adopted => ToolkitChoice::Release { tag: tag.clone() },
            _ => ToolkitChoice::Folder {
                relinker: self.relinker.clone(),
                libs: self.libs.clone(),
            },
        }
    }

    /// Opens an installed toolkit folder: `relinker.exe`, `libs\` with `.prx` files and the MinGW
    /// DLLs, optionally `version.txt`, `toolkit.json` and `patches\`.
    pub fn open(root: &Path) -> Result<Toolkit, ToolkitError> {
        Self::open_for(Os::current(), root)
    }

    fn open_for(os: Os, root: &Path) -> Result<Toolkit, ToolkitError> {
        let relinker = root.join(os.relinker_file());
        let libs = root.join("libs");
        let mut toolkit = Self::from_parts(os, root, relinker, libs, false)?;
        if let Ok(text) = fs::read_to_string(root.join("version.txt")) {
            let line = text.trim();
            toolkit.version = if line.is_empty() {
                "unknown".into()
            } else {
                line.into()
            };
        }
        toolkit.tag = fs::read_to_string(root.join("toolkit.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| {
                v.get("tag")?
                    .as_str()
                    .map(|t| t.trim_start_matches('v').to_string())
            })
            .or_else(|| {
                // `version.txt` without `toolkit.json`: the tag is what comes before the dash.
                toolkit
                    .version
                    .split_once('-')
                    .map(|(tag, _)| tag.to_string())
                    .filter(|tag| tag != "local" && tag != "unknown")
            });
        toolkit.digests = fs::read_to_string(root.join("toolkit.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| {
                Some(Digests {
                    relinker: v.get("relinkerSha256")?.as_str()?.to_owned(),
                    libs: v.get("libsSha256")?.as_str()?.to_owned(),
                })
            });
        Ok(toolkit)
    }

    fn from_parts(
        os: Os,
        root: &Path,
        relinker: PathBuf,
        libs: PathBuf,
        adopted: bool,
    ) -> Result<Toolkit, ToolkitError> {
        if !relinker.is_file() {
            return Err(ToolkitError::Missing(Os::current().relinker_file().into()));
        }
        if !has_prx(&libs) {
            return Err(ToolkitError::Missing("libs\\*.prx".into()));
        }
        if os == Os::Macos {
            macos::validate(&relinker, root)?;
        } else {
            use std::io::Read;
            let mut magic = [0u8; 4];
            let count = fs::File::open(&relinker)?.read(&mut magic)?;
            let macho = matches!(
                magic,
                [0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe] | [0xca, 0xfe, 0xba, 0xbf]
            );
            if count == 4
                && (macho
                    || (os == Os::Windows && magic == *b"\x7fELF")
                    || (os == Os::Linux && magic.starts_with(b"MZ")))
            {
                return Err(ToolkitError::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "{} is not a {} relinker",
                        relinker.display(),
                        os.platform_key()
                    ),
                )));
            }
        }
        let mut extra_runtime = Vec::new();
        let mut missing = Vec::new();
        for dll in os.runtime_libs() {
            if libs.join(dll).is_file() {
                continue;
            }
            match adopted.then(|| find_runtime_dll(root, dll)).flatten() {
                Some(path) => extra_runtime.push(path),
                None => missing.push(dll.to_string()),
            }
        }
        if !missing.is_empty() {
            return Err(ToolkitError::MissingRuntime(missing));
        }
        let patches = root.join("patches");
        let fp = fingerprint(&relinker, &libs, &patches)?;
        Ok(Toolkit {
            root: root.to_path_buf(),
            relinker,
            libs,
            patches,
            version: format!("local-{fp}"),
            tag: None,
            extra_runtime,
            adopted,
            digests: None,
        })
    }

    /// The first toolkit that opens: `ANYPS5_TOOLKIT`, the remembered choice, then the newest
    /// installed release.
    pub fn locate(paths: &AppPaths, choice: Option<&ToolkitChoice>) -> Option<Toolkit> {
        let env = std::env::var_os("ANYPS5_TOOLKIT").map(PathBuf::from);
        Self::locate_with(paths, choice, env.as_deref())
    }

    /// [`locate`](Self::locate) with the environment override given explicitly.
    pub fn locate_with(
        paths: &AppPaths,
        choice: Option<&ToolkitChoice>,
        env_override: Option<&Path>,
    ) -> Option<Toolkit> {
        if let Some(folder) = env_override
            && let Ok(toolkit) = Self::open(folder).or_else(|_| adopt_folder(folder))
        {
            return Some(toolkit);
        }
        match choice {
            Some(ToolkitChoice::Release { tag }) => {
                if let Ok(toolkit) =
                    Self::open(&paths.toolkit_dir().join(tag.trim_start_matches('v')))
                {
                    return Some(toolkit);
                }
            }
            Some(ToolkitChoice::Folder { relinker, libs }) => {
                if let Some(root) = relinker.parent()
                    && let Ok(toolkit) = from_chosen(root, relinker, libs)
                {
                    return Some(toolkit);
                }
            }
            None => {}
        }
        installed_releases(paths)
            .into_iter()
            .find_map(|(_, dir)| Self::open(&dir).ok())
    }
}

fn from_chosen(root: &Path, relinker: &Path, libs: &Path) -> Result<Toolkit, ToolkitError> {
    Toolkit::from_parts(
        Os::current(),
        root,
        relinker.to_path_buf(),
        libs.to_path_buf(),
        true,
    )
}

fn has_prx(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            e.path()
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("prx"))
        })
    })
}

/// Installed release folders, newest first, `.partial` folders left out.
fn installed_releases(paths: &AppPaths) -> Vec<(Vec<u64>, PathBuf)> {
    let Ok(entries) = fs::read_dir(paths.toolkit_dir()) else {
        return Vec::new();
    };
    let mut found: Vec<(Vec<u64>, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(".partial") {
                return None;
            }
            Some((parse_tag(&name)?, e.path()))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found
}

/// Looks for a MinGW DLL next to the compiler recorded in the build's CMake cache, then on PATH.
fn find_runtime_dll(root: &Path, name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for cache in cmake_caches(root) {
        if let Ok(text) = fs::read_to_string(cache) {
            for line in text.lines() {
                if let Some(value) = line.strip_prefix("CMAKE_CXX_COMPILER:")
                    && let Some((_, path)) = value.split_once('=')
                    && let Some(dir) = Path::new(path.trim()).parent()
                {
                    dirs.push(dir.to_path_buf());
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    dirs.into_iter().map(|d| d.join(name)).find(|p| p.is_file())
}

fn cmake_caches(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut dir = Some(root);
    for _ in 0..4 {
        let Some(d) = dir else { break };
        for candidate in [d.join("CMakeCache.txt"), d.join("build/CMakeCache.txt")] {
            if candidate.is_file() {
                found.push(candidate);
            }
        }
        dir = d.parent();
    }
    found
}

/// Accepts a folder the player picked: a flat folder with `relinker.exe` and `libs\`, or a build
/// tree with `core\relinker\relinker.exe` and `core\libs\libs\` (also below `build\`), searched
/// four levels deep. Nothing is copied; the folder is never written.
pub fn adopt_folder(folder: &Path) -> Result<Toolkit, ToolkitError> {
    let relinker = find_below(folder, 4, &|p| {
        p.file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case(Os::current().relinker_file()))
            && p.is_file()
    })
    .ok_or_else(|| ToolkitError::Missing(Os::current().relinker_file().into()))?;
    let libs = find_below(folder, 5, &|p| {
        p.is_dir()
            && p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("libs"))
            && has_prx(p)
    })
    .ok_or_else(|| ToolkitError::Missing("libs\\*.prx".into()))?;
    Toolkit::from_parts(Os::current(), folder, relinker, libs, true)
}

/// Breadth-first search, shallow matches first, names in order.
fn find_below(root: &Path, depth: usize, wanted: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut level = vec![root.to_path_buf()];
    for _ in 0..=depth {
        let mut next = Vec::new();
        for dir in &level {
            let Ok(entries) = fs::read_dir(dir) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                if wanted(&path) {
                    return Some(path);
                }
                if path.is_dir() {
                    next.push(path);
                }
            }
        }
        level = next;
    }
    None
}

/// First 12 hex digits of the SHA-256 over `relinker.exe`, every regular file of `libs` in
/// ordinal order, then the `*.toml` and `*.json` files of `patches` in ordinal order.
pub fn fingerprint(relinker: &Path, libs: &Path, patches: &Path) -> io::Result<String> {
    let mut hasher = Sha256::new();
    feed(&mut hasher, relinker)?;
    for file in sorted_files(libs, |_| true) {
        feed(&mut hasher, &file)?;
    }
    for file in sorted_files(patches, |p| {
        p.extension().is_some_and(|x| x == "toml" || x == "json")
    }) {
        feed(&mut hasher, &file)?;
    }
    // Runtime changes must also mark an adopted macOS toolkit as changed.
    if let Some(root) = libs.parent() {
        for file in sorted_files(&root.join("vulkan"), |_| true) {
            feed(&mut hasher, &file)?;
        }
    }
    let digest = hasher.finalize();
    Ok(hex(&digest)[..12].to_string())
}

fn sorted_files(dir: &Path, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(_, p)| keep(p))
        .collect();
    files.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    files.into_iter().map(|(_, p)| p).collect()
}

fn feed(hasher: &mut Sha256, path: &Path) -> io::Result<()> {
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            return Ok(());
        }
        hasher.update(&buffer[..n]);
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------------------------
// Tags

/// `v0.1.10` -> `[0, 1, 10]`; `None` for anything that is not dotted numbers.
fn parse_tag(tag: &str) -> Option<Vec<u64>> {
    let mut parts = Vec::new();
    let tag = tag.trim().trim_start_matches('v');
    // The macOS runtime publishes versions such as 0.2.15-arm. Other suffixes stay invalid.
    let tag = tag.strip_suffix("-arm").unwrap_or(tag);
    for part in tag.split('.') {
        parts.push(part.parse().ok()?);
    }
    Some(parts)
}

/// Compares release tags as dotted numbers (`0.1.10 > 0.1.9`, `0.2 == 0.2.0`).
pub fn compare_tags(a: &str, b: &str) -> Option<Ordering> {
    let (mut a, mut b) = (parse_tag(a)?, parse_tag(b)?);
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    Some(a.cmp(&b))
}

/// Whether `latest` is a newer release than `installed`; garbage is never newer.
pub fn is_newer(latest: &str, installed: &str) -> bool {
    compare_tags(latest, installed) == Some(Ordering::Greater)
}

/// Whether a release is newer than the installed toolkit. An adopted folder never has one.
pub fn update_available(latest: &ReleaseInfo, installed: &Toolkit) -> bool {
    match installed.tag() {
        Some(tag) if !installed.is_adopted() => is_newer(&latest.tag, tag),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Releases

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lower-case hex from the API's `digest` (`sha256:<hex>`), when it gave one.
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub os: Os,
    /// As published, with the leading `v`.
    pub tag: String,
    pub published: String,
    /// Separate relinker on Windows/Linux; macOS includes it in `libs`.
    pub relinker: Option<Asset>,
    /// Libraries archive, or the complete macOS toolkit archive.
    pub libs: Asset,
    pub notes_url: String,
    /// The full list of changes (`release-notes.md`), when the release has one.
    pub notes: Option<Asset>,
}

impl ReleaseInfo {
    /// The tag without a leading `v`: the toolkit's folder and version prefix.
    pub fn plain_tag(&self) -> &str {
        self.tag.trim_start_matches('v')
    }
}

/// Reads the JSON of `GET /repos/<repo>/releases?per_page=N`: the releases that have a relinker and
/// libraries for this system, in the API's order (newest first). Drafts and unusable ones are left out.
pub fn parse_releases(json: &str) -> Result<Vec<ReleaseInfo>, ToolkitError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| ToolkitError::Network(format!("bad release data: {e}")))?;
    let list = value
        .as_array()
        .ok_or_else(|| ToolkitError::Network("bad release data".into()))?;
    Ok(list
        .iter()
        .filter(|r| r.get("draft").and_then(|d| d.as_bool()) != Some(true))
        .filter_map(|r| parse_release(&r.to_string()).ok())
        .collect())
}

/// Reads the JSON of `GET /repos/<repo>/releases/latest`, picking the assets of this system.
pub fn parse_release(json: &str) -> Result<ReleaseInfo, ToolkitError> {
    parse_release_for(Os::current(), json)
}

/// [`parse_release`] for another system (tests).
pub fn parse_release_for(os: Os, json: &str) -> Result<ReleaseInfo, ToolkitError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| ToolkitError::Network(format!("bad release data: {e}")))?;
    let text =
        |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).map(str::to_string);
    let tag = text(&value, "tag_name").ok_or(ToolkitError::NoRelease)?;
    if parse_tag(&tag).is_none() {
        return Err(ToolkitError::NoRelease);
    }
    let assets = value
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or(ToolkitError::NoRelease)?;
    let find = |name: &str| -> Option<Asset> {
        assets
            .iter()
            .find(|a| text(a, "name").as_deref() == Some(name))
            .and_then(|a| {
                Some(Asset {
                    name: name.to_string(),
                    url: text(a, "browser_download_url")?,
                    size: a.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                    sha256: text(a, "digest")
                        .and_then(|d| d.strip_prefix("sha256:").map(|h| h.to_ascii_lowercase())),
                })
            })
    };
    let relinker = os
        .relinker_asset(&tag)
        .map(|name| find(&name).ok_or(ToolkitError::NoRelease))
        .transpose()?;
    let libs = find(&os.libs_asset(&tag)).ok_or(ToolkitError::NoRelease)?;
    if os == Os::Macos
        && libs
            .sha256
            .as_ref()
            .is_none_or(|hash| hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(ToolkitError::Digest);
    }
    Ok(ReleaseInfo {
        os,
        notes: find("release-notes.md"),
        published: text(&value, "published_at").unwrap_or_default(),
        notes_url: text(&value, "html_url").unwrap_or_default(),
        tag,
        relinker,
        libs,
    })
}

pub(crate) fn agent(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(timeout)
        .http_status_as_error(false)
        .build()
        .into()
}

/// How long a connection may take to open, and how long a transfer may make no progress.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const STALL_TIMEOUT: Duration = Duration::from_secs(30);

/// The agent for downloads that end up in the launcher's own update: a connect timeout, a stall
/// (read) timeout, at most three redirects, and https only. The test build also speaks plain http,
/// because its servers run on this machine.
pub(crate) fn update_agent(global: Option<Duration>) -> ureq::Agent {
    update_agent_with(global, !cfg!(test), STALL_TIMEOUT)
}

/// [`update_agent`] with the policy given (tests use a short stall time, or force https).
pub(crate) fn update_agent_with(
    global: Option<Duration>,
    https_only: bool,
    stall: Duration,
) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(global)
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_per_read(Some(stall))
        .https_only(https_only)
        .max_redirects(3)
        .max_redirects_will_error(true)
        .http_status_as_error(false)
        .build()
        .into()
}

pub(crate) fn user_agent() -> String {
    format!("anyplay-launcher/{}", env!("CARGO_PKG_VERSION"))
}

/// Asks GitHub for the latest release of `repo`. Blocking: call from the `toolkit` thread.
/// Sends only the request below; no token, nothing else.
pub fn check_latest(repo: &str) -> Result<ReleaseInfo, ToolkitError> {
    check_latest_at(API_BASE, repo)
}

/// [`check_latest`] against another API host (tests).
pub fn check_latest_at(api_base: &str, repo: &str) -> Result<ReleaseInfo, ToolkitError> {
    parse_release(&latest_release_json(api_base, repo)?)
}

/// The JSON of `GET /repos/<repo>/releases/latest` (shared with the launcher's self-update).
pub(crate) fn latest_release_json(api_base: &str, repo: &str) -> Result<String, ToolkitError> {
    let url = format!("{api_base}/repos/{repo}/releases/latest");
    let mut response = agent(Some(Duration::from_secs(10)))
        .get(&url)
        .header("User-Agent", user_agent())
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| ToolkitError::Network(e.to_string()))?;
    let status = response.status().as_u16();
    match status {
        200 => {}
        403 | 429 => {
            return Err(ToolkitError::Network(
                "rate limited, try again later".into(),
            ));
        }
        404 => return Err(ToolkitError::NoRelease),
        other => return Err(ToolkitError::Network(format!("GitHub answered {other}"))),
    }
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| ToolkitError::Network(e.to_string()))
}

/// Downloads a small text file (a checksum list).
pub(crate) fn download_text(url: &str) -> Result<String, ToolkitError> {
    let mut response = update_agent(Some(Duration::from_secs(20)))
        .get(url)
        .header("User-Agent", user_agent())
        .call()
        .map_err(|e| ToolkitError::Network(e.to_string()))?;
    if response.status().as_u16() != 200 {
        return Err(ToolkitError::Network(format!(
            "download failed: HTTP {}",
            response.status().as_u16()
        )));
    }
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| ToolkitError::Network(e.to_string()))
}

// ---------------------------------------------------------------------------------------------
// Download and installation

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadStage {
    Relinker,
    Libraries,
    Extracting,
    Finishing,
}

/// Downloads a release into `%LOCALAPPDATA%\AnyPS5\Toolkit\<tag>`: the relinker, the libraries
/// zip (verified when the API gave a digest, otherwise by size), extracted into `libs\`, the
/// launcher's per-title `patches_source` copied beside, the version fingerprint written. The
/// folder appears only when complete; an existing one is replaced then. Afterwards all but the
/// newest two releases are deleted. `progress` gets the share (0..=1) of the current stage.
pub fn download_and_install(
    info: &ReleaseInfo,
    paths: &AppPaths,
    patches_source: Option<&Path>,
    progress: &mut dyn FnMut(DownloadStage, f64),
    cancel: &CancelToken,
) -> Result<Toolkit, ToolkitError> {
    download_and_install_keeping(info, paths, patches_source, &[], progress, cancel)
}

/// [`download_and_install`] that leaves the releases in `keep` (the tags games stay on) alone
/// when it deletes the old ones.
pub fn download_and_install_keeping(
    info: &ReleaseInfo,
    paths: &AppPaths,
    patches_source: Option<&Path>,
    keep: &[String],
    progress: &mut dyn FnMut(DownloadStage, f64),
    cancel: &CancelToken,
) -> Result<Toolkit, ToolkitError> {
    let tag = info.plain_tag().to_string();
    if parse_tag(&tag).is_none() {
        return Err(ToolkitError::NoRelease);
    }
    if (info.os == Os::Macos) != info.relinker.is_none() {
        return Err(ToolkitError::NoRelease);
    }
    let base = paths.toolkit_dir();
    let partial = base.join(format!("{tag}.partial"));
    let final_dir = base.join(&tag);
    crate::fsx::remove_tree(&partial)?;
    fs::create_dir_all(&partial)?;
    let result = install_into(info, &partial, patches_source, progress, cancel);
    if let Err(error) = result {
        let _ = crate::fsx::remove_tree(&partial);
        return Err(error);
    }
    let previous = base.join(format!("{tag}.previous"));
    if !final_dir.exists() && previous.exists() {
        crate::fsx::rename_retry(&previous, &final_dir)?;
    }
    crate::fsx::remove_tree_retry(&previous)?;
    let replacing = final_dir.exists();
    if replacing {
        crate::fsx::rename_retry(&final_dir, &previous)?;
    }
    if let Err(error) = crate::fsx::rename_retry(&partial, &final_dir) {
        if replacing {
            crate::fsx::rename_retry(&previous, &final_dir)?;
        }
        return Err(error.into());
    }
    if let Err(error) = crate::fsx::remove_tree_retry(&previous) {
        tracing::warn!(
            "could not remove previous toolkit {}: {error}",
            previous.display()
        );
    }
    prune_old_releases(paths, &tag, keep);
    Toolkit::open_for(info.os, &final_dir)
}

fn install_into(
    info: &ReleaseInfo,
    partial: &Path,
    patches_source: Option<&Path>,
    progress: &mut dyn FnMut(DownloadStage, f64),
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    let agent = agent(None);
    if let Some(relinker) = &info.relinker {
        download(
            &agent,
            relinker,
            &partial.join(info.os.relinker_file()),
            DownloadStage::Relinker,
            progress,
            cancel,
        )?;
        make_executable(&partial.join(info.os.relinker_file()))?;
    }
    let zip_path = partial.join("libs.zip");
    if info.os == Os::Macos {
        if info.libs.sha256.is_none() {
            return Err(ToolkitError::Digest);
        }
        download_capped(
            &agent,
            &info.libs,
            &zip_path,
            Some(512 * 1024 * 1024),
            &mut |f| progress(DownloadStage::Libraries, f),
            cancel,
        )?;
    } else {
        download(
            &agent,
            &info.libs,
            &zip_path,
            DownloadStage::Libraries,
            progress,
            cancel,
        )?;
    }
    if info.relinker.is_some() {
        extract_libs(&zip_path, &partial.join("libs"), progress, cancel)?;
    } else {
        macos::extract(&zip_path, partial, info.plain_tag(), progress, cancel)?;
        make_executable(&partial.join("relinker"))?;
    }
    fs::remove_file(&zip_path)?;
    progress(DownloadStage::Finishing, 0.0);
    if let Some(source) = patches_source.filter(|_| info.relinker.is_some()) {
        copy_patches(source, &partial.join("patches"))?;
    }
    let fp = fingerprint(
        &partial.join(info.os.relinker_file()),
        &partial.join("libs"),
        &partial.join("patches"),
    )?;
    let tag = info.plain_tag();
    let installed_version = format!("{tag}-{fp}\n");
    fs::write(partial.join("version.txt"), &installed_version)?;
    let mut record = if info.relinker.is_none() {
        serde_json::from_slice::<serde_json::Value>(&fs::read(partial.join("toolkit.json"))?)
            .map_err(|e| ToolkitError::BadArchive(e.to_string()))?
    } else {
        serde_json::json!({})
    };
    let relinker_digest = info
        .relinker
        .as_ref()
        .and_then(|asset| asset.sha256.clone())
        .or_else(|| record["files"]["relinker"].as_str().map(str::to_owned));
    if info.os == Os::Macos {
        record["version"] = installed_version.trim().into();
        record["files"]["version.txt"] = hex(&Sha256::digest(installed_version.as_bytes())).into();
    }
    let provenance = serde_json::json!({
        "tag": tag,
        "published": info.published,
        "source": info.libs.url,
        "installedAt": now_text(),
        "relinkerSha256": relinker_digest,
        "libsSha256": info.libs.sha256,
    });
    record
        .as_object_mut()
        .ok_or_else(|| ToolkitError::BadArchive("invalid toolkit manifest".into()))?
        .extend(provenance.as_object().expect("provenance object").clone());
    fs::write(
        partial.join("toolkit.json"),
        serde_json::to_string_pretty(&record).unwrap_or_default(),
    )?;
    // The folder must be usable, or the player would be left with a broken toolkit.
    Toolkit::open_for(info.os, partial)?;
    progress(DownloadStage::Finishing, 1.0);
    Ok(())
}

fn now_text() -> String {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .ok()
        .and_then(|t| t.format(&Rfc3339).ok())
        .unwrap_or_default()
}

fn copy_patches(from: &Path, to: &Path) -> io::Result<()> {
    let Ok(entries) = fs::read_dir(from) else {
        return Ok(());
    };
    fs::create_dir_all(to)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|x| x == "toml" || x == "json") && path.is_file() {
            fs::copy(&path, to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// A downloaded tool has no execute bit on Unix.
fn make_executable(file: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o755))
    }
    #[cfg(not(unix))]
    {
        let _ = file;
        Ok(())
    }
}

fn download(
    agent: &ureq::Agent,
    asset: &Asset,
    destination: &Path,
    stage: DownloadStage,
    progress: &mut dyn FnMut(DownloadStage, f64),
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    download_to(
        agent,
        asset,
        destination,
        &mut |share| progress(stage, share),
        cancel,
    )
}

/// Streams `asset` into `destination` and checks it (SHA-256 when the asset has one, else size).
pub(crate) fn download_to(
    agent: &ureq::Agent,
    asset: &Asset,
    destination: &Path,
    progress: &mut dyn FnMut(f64),
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    download_capped(agent, asset, destination, None, progress, cancel)
}

/// [`download_to`] that stops with [`ToolkitError::TooLarge`] as soon as the body is longer than
/// `max_bytes` (a Content-Length above it is refused before a byte is read). A file stays at
/// `destination` when it fails; the caller removes it.
pub(crate) fn download_capped(
    agent: &ureq::Agent,
    asset: &Asset,
    destination: &Path,
    max_bytes: Option<u64>,
    progress: &mut dyn FnMut(f64),
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    progress(0.0);
    let mut response = agent
        .get(&asset.url)
        .header("User-Agent", user_agent())
        .call()
        .map_err(|e| ToolkitError::Network(e.to_string()))?;
    if response.status().as_u16() != 200 {
        return Err(ToolkitError::Network(format!(
            "download of {} failed: HTTP {}",
            asset.name,
            response.status().as_u16()
        )));
    }
    let declared = response.body().content_length();
    if let (Some(max), Some(declared)) = (max_bytes, declared)
        && declared > max
    {
        return Err(ToolkitError::TooLarge);
    }
    let expected = declared.or((asset.size > 0).then_some(asset.size));
    // One byte more than allowed, so that an overlong body is seen and not just cut off.
    let limit = max_bytes.map_or(u64::MAX, |max| max.saturating_add(1));
    let mut reader = response.body_mut().with_config().limit(limit).reader();
    let mut file = fs::File::create(destination)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Err(ToolkitError::Cancelled);
        }
        let n = reader
            .read(&mut buffer)
            .map_err(|e| ToolkitError::Network(e.to_string()))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if max_bytes.is_some_and(|max| total > max) {
            return Err(ToolkitError::TooLarge);
        }
        file.write_all(&buffer[..n])?;
        hasher.update(&buffer[..n]);
        if let Some(size) = expected.filter(|s| *s > 0) {
            progress((total as f64 / size as f64).min(1.0));
        }
    }
    file.flush()?;
    match &asset.sha256 {
        Some(want) => {
            if hex(&hasher.finalize()) != *want {
                return Err(ToolkitError::Digest);
            }
        }
        None => {
            if asset.size > 0 && total != asset.size {
                return Err(ToolkitError::Digest);
            }
        }
    }
    progress(1.0);
    Ok(())
}

/// Normalises a zip entry name; `None` when it could leave the target folder.
fn safe_entry_name(name: &str) -> Option<String> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.contains(':') {
        return None;
    }
    if name.split('/').any(|part| part == "..") {
        return None;
    }
    Some(name)
}

fn extract_libs(
    zip_path: &Path,
    libs: &Path,
    progress: &mut dyn FnMut(DownloadStage, f64),
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    let file = fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
    // Reject the whole archive before writing anything if one name is unsafe.
    let mut wanted = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
        let name = safe_entry_name(entry.name()).ok_or_else(|| {
            ToolkitError::BadArchive(format!("unsafe entry name {}", entry.name()))
        })?;
        if entry.is_dir() {
            continue;
        }
        if let Some(rest) = name.strip_prefix("libs/")
            && !rest.is_empty()
        {
            wanted.push((index, rest.to_string()));
        }
    }
    if wanted.is_empty() {
        return Err(ToolkitError::BadArchive("no libs/ folder".into()));
    }
    fs::create_dir_all(libs)?;
    let total = wanted.len();
    for (done, (index, rest)) in wanted.into_iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(ToolkitError::Cancelled);
        }
        let target = libs.join(rest.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
        let mut out = fs::File::create(&target)?;
        io::copy(&mut entry, &mut out).map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
        progress(DownloadStage::Extracting, (done + 1) as f64 / total as f64);
    }
    Ok(())
}

/// Keeps the newest two installed releases (the one just installed and its predecessor), and
/// every release in `keep`: the ones games stay on.
fn prune_old_releases(paths: &AppPaths, keep_tag: &str, keep: &[String]) {
    let mut found = installed_releases(paths);
    found.retain(|(_, dir)| {
        dir.file_name()
            .is_some_and(|n| n.to_string_lossy() != keep_tag)
    });
    let kept: Vec<String> = keep.iter().filter_map(|tag| pin_tag(tag)).collect();
    // `found` is newest first and no longer holds the new one: keep one predecessor.
    for (_, dir) in found.into_iter().skip(1) {
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
        if !name.is_some_and(|name| kept.contains(&name)) {
            let _ = crate::fsx::remove_tree(&dir);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // The real host entry point must not select a Linux release on macOS.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_does_not_select_linux_release_assets() {
        assert!(matches!(
            parse_release(RELEASE_JSON),
            Err(ToolkitError::NoRelease)
        ));
    }

    const RELINKER: &str = Os::current().relinker_file();

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn native_relinker() -> Vec<u8> {
        if cfg!(target_os = "macos") {
            b"\xcf\xfa\xed\xfeRELINKER".to_vec()
        } else {
            b"RELINKER".to_vec()
        }
    }

    fn macos_runtime(root: &Path) {
        if cfg!(target_os = "macos") {
            for name in ["libvulkan.1.dylib", "libMoltenVK.dylib"] {
                write(&root.join("vulkan").join(name), b"runtime fixture");
            }
            write(
                &root.join("vulkan/MoltenVK_icd.json"),
                br#"{"ICD":{"library_path":"libMoltenVK.dylib"}}"#,
            );
        }
    }

    fn make_toolkit(root: &Path, dlls: bool) {
        write(&root.join(RELINKER), &native_relinker());
        macos_runtime(root);
        write(&root.join("libs/a.prx"), b"AAA");
        if dlls {
            for dll in Os::current().runtime_libs() {
                write(&root.join("libs").join(dll), b"D");
            }
        }
    }

    #[test]
    fn fingerprint_vector() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write(&r.join(RELINKER), b"RELINKER");
        write(&r.join("libs/b.prx"), b"BBBB");
        write(&r.join("libs/a.prx"), b"AAA");
        write(&r.join("libs/libx.dll"), b"D");
        write(&r.join("patches/p.json"), b"patches!");
        write(&r.join("patches/readme.txt"), b"ignored");
        let fp = fingerprint(&r.join(RELINKER), &r.join("libs"), &r.join("patches")).unwrap();
        assert_eq!(fp, "3549fbc1b2f8");
        let full = {
            let mut h = Sha256::new();
            for part in [&b"RELINKER"[..], b"AAA", b"BBBB", b"D", b"patches!"] {
                h.update(part);
            }
            hex(&h.finalize())
        };
        assert_eq!(
            full,
            "3549fbc1b2f847f7155da1e47723b2381ce3bdba0e382998417fd392cb3335c5"
        );
    }

    #[test]
    fn open_requires_relinker_and_prx() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            matches!(Toolkit::open(dir.path()), Err(ToolkitError::Missing(f)) if f == RELINKER)
        );
        write(&dir.path().join(RELINKER), b"R");
        assert!(matches!(
            Toolkit::open(dir.path()),
            Err(ToolkitError::Missing(_))
        ));
        write(&dir.path().join("libs/a.prx"), b"A");
        if Os::current() == Os::Linux {
            assert!(
                Toolkit::open(dir.path()).is_ok(),
                "Linux needs no runtime DLLs"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn open_requires_the_mingw_runtime() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            matches!(Toolkit::open(dir.path()), Err(ToolkitError::Missing(f)) if f == RELINKER)
        );
        write(&dir.path().join(RELINKER), b"R");
        assert!(matches!(
            Toolkit::open(dir.path()),
            Err(ToolkitError::Missing(_))
        ));
        write(&dir.path().join("libs/a.prx"), b"A");
        match Toolkit::open(dir.path()) {
            Err(ToolkitError::MissingRuntime(names)) => assert_eq!(names, MINGW_DLLS),
            other => panic!("{other:?}"),
        }
        for dll in &MINGW_DLLS[..2] {
            write(&dir.path().join("libs").join(dll), b"D");
        }
        match Toolkit::open(dir.path()) {
            Err(ToolkitError::MissingRuntime(names)) => assert_eq!(names, ["libwinpthread-1.dll"]),
            other => panic!("{other:?}"),
        }
        write(&dir.path().join("libs/libwinpthread-1.dll"), b"D");
        let toolkit = Toolkit::open(dir.path()).unwrap();
        assert!(toolkit.version().starts_with("local-"));
        assert_eq!(toolkit.tag(), None);
    }

    #[test]
    fn open_reads_version_and_tag_of_an_installed_release() {
        let dir = tempfile::tempdir().unwrap();
        make_toolkit(dir.path(), true);
        write(&dir.path().join("version.txt"), b"0.1.3-abcdef123456\n");
        let toolkit = Toolkit::open(dir.path()).unwrap();
        assert_eq!(toolkit.version(), "0.1.3-abcdef123456");
        assert_eq!(toolkit.tag(), Some("0.1.3"));
        assert_eq!(
            toolkit.choice(),
            ToolkitChoice::Release {
                tag: "0.1.3".into()
            }
        );
    }

    #[test]
    fn adopts_flat_and_build_tree_layouts() {
        let flat = tempfile::tempdir().unwrap();
        make_toolkit(flat.path(), true);
        let toolkit = adopt_folder(flat.path()).unwrap();
        assert!(toolkit.is_adopted());
        assert_eq!(toolkit.relinker(), flat.path().join(RELINKER));

        let tree = tempfile::tempdir().unwrap();
        let r = tree.path();
        write(
            &r.join(format!("build/core/relinker/{RELINKER}")),
            &native_relinker(),
        );
        macos_runtime(r);
        write(&r.join("build/core/libs/libs/x.prx"), b"X");
        for dll in MINGW_DLLS {
            write(&r.join("build/core/libs/libs").join(dll), b"D");
        }
        let toolkit = adopt_folder(r).unwrap();
        assert_eq!(
            toolkit.relinker(),
            r.join(format!("build/core/relinker/{RELINKER}"))
        );
        assert_eq!(toolkit.libs(), r.join("build/core/libs/libs"));
        assert_eq!(toolkit.tag(), None);
        assert!(matches!(toolkit.choice(), ToolkitChoice::Folder { .. }));

        let empty = tempfile::tempdir().unwrap();
        assert!(matches!(
            adopt_folder(empty.path()),
            Err(ToolkitError::Missing(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn adopted_toolkit_finds_the_runtime_next_to_the_recorded_compiler() {
        let tree = tempfile::tempdir().unwrap();
        let r = tree.path();
        write(&r.join(format!("build/core/relinker/{RELINKER}")), b"R");
        write(&r.join("build/core/libs/libs/x.prx"), b"X");
        let compiler = r.join("mingw64/bin");
        for dll in MINGW_DLLS {
            write(&compiler.join(dll), b"D");
        }
        let cache = format!(
            "CMAKE_CXX_COMPILER:FILEPATH={}\n",
            compiler.join("g++.exe").display()
        );
        write(&r.join("build/CMakeCache.txt"), cache.as_bytes());
        let toolkit = adopt_folder(r).unwrap();
        assert_eq!(toolkit.extra_runtime().len(), 3);
    }

    #[test]
    fn locate_prefers_override_then_choice_then_newest() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        assert!(Toolkit::locate_with(&paths, None, None).is_none());
        for tag in ["0.1.9", "0.1.10"] {
            let root = paths.toolkit_dir().join(tag);
            make_toolkit(&root, true);
            write(&root.join("version.txt"), format!("{tag}-x\n").as_bytes());
        }
        // A half-made folder is ignored.
        make_toolkit(&paths.toolkit_dir().join("0.9.0.partial"), true);
        assert_eq!(
            Toolkit::locate_with(&paths, None, None).unwrap().tag(),
            Some("0.1.10")
        );
        let choice = ToolkitChoice::Release {
            tag: "0.1.9".into(),
        };
        assert_eq!(
            Toolkit::locate_with(&paths, Some(&choice), None)
                .unwrap()
                .tag(),
            Some("0.1.9")
        );
        let own = dir.path().join("own");
        make_toolkit(&own, true);
        let found = Toolkit::locate_with(&paths, Some(&choice), Some(&own)).unwrap();
        assert!(found.is_adopted() || found.root() == own);
    }

    #[test]
    fn tags_compare_as_dotted_numbers() {
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("v0.2.0", "0.1.99"));
        assert!(!is_newer("0.2", "0.2.0"));
        assert_eq!(compare_tags("0.2", "0.2.0"), Some(Ordering::Equal));
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(!is_newer("0.1.0", "garbage"));
        let toolkit = {
            let dir = tempfile::tempdir().unwrap();
            make_toolkit(dir.path(), true);
            write(&dir.path().join("version.txt"), b"0.1.3-aa\n");
            let t = Toolkit::open(dir.path()).unwrap();
            (dir, t)
        };
        let release = |tag: &str| ReleaseInfo {
            os: Os::Windows,
            tag: tag.into(),
            published: String::new(),
            relinker: Some(asset("x")),
            libs: asset("y"),
            notes_url: String::new(),
            notes: None,
        };
        assert!(update_available(&release("v0.1.4"), &toolkit.1));
        assert!(!update_available(&release("v0.1.3"), &toolkit.1));
    }

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.into(),
            url: String::new(),
            size: 0,
            sha256: None,
        }
    }

    const RELEASE_JSON: &str = r#"{
      "tag_name": "v0.1.3", "published_at": "2026-10-20T10:00:00Z",
      "html_url": "https://github.com/boykopovar/AnyPS5/releases/tag/v0.1.3",
      "assets": [
        {"name": "relinker-v0.1.3", "browser_download_url": "https://x/linux", "size": 1},
        {"name": "relinker-v0.1.3.exe", "browser_download_url": "https://x/relinker.exe", "size": 5400000,
         "digest": "sha256:ABCDEF"},
        {"name": "prx-windows-v0.1.3.zip", "browser_download_url": "https://x/prx.zip", "size": 96600000},
        {"name": "prx-linux-v0.1.3.zip", "browser_download_url": "https://x/prx-linux.zip", "size": 102000000},
        {"name": "USAGE.md", "browser_download_url": "https://x/USAGE.md", "size": 10}
      ]}"#;

    #[test]
    fn parses_the_release_assets_by_exact_name() {
        let info = parse_release_for(Os::Windows, RELEASE_JSON).unwrap();
        assert_eq!(info.tag, "v0.1.3");
        assert_eq!(info.plain_tag(), "0.1.3");
        assert_eq!(
            info.relinker.as_ref().unwrap().url,
            "https://x/relinker.exe"
        );
        assert_eq!(
            info.relinker.as_ref().and_then(|a| a.sha256.as_deref()),
            Some("abcdef")
        );
        assert_eq!(info.libs.size, 96_600_000);
        assert_eq!(info.libs.sha256, None);
        assert_eq!(info.published, "2026-10-20T10:00:00Z");
    }

    #[test]
    fn the_linux_assets_are_the_ones_without_an_extension_and_the_linux_zip() {
        let info = parse_release_for(Os::Linux, RELEASE_JSON).unwrap();
        assert_eq!(info.relinker.as_ref().unwrap().url, "https://x/linux");
        assert_eq!(info.libs.name, "prx-linux-v0.1.3.zip");
        let json = RELEASE_JSON.replace("prx-linux-v0.1.3.zip", "prx-other.zip");
        assert!(matches!(
            parse_release_for(Os::Linux, &json),
            Err(ToolkitError::NoRelease)
        ));
    }

    #[test]
    fn a_release_without_the_windows_assets_is_no_release() {
        let json = RELEASE_JSON.replace("prx-windows-v0.1.3.zip", "prx-other.zip");
        assert!(matches!(
            parse_release_for(Os::Windows, &json),
            Err(ToolkitError::NoRelease)
        ));
        assert!(matches!(parse_release("{}"), Err(ToolkitError::NoRelease)));
        assert!(matches!(
            parse_release("nope"),
            Err(ToolkitError::Network(_))
        ));
    }

    #[test]
    fn zip_names_are_checked() {
        assert_eq!(safe_entry_name("libs/a.prx").as_deref(), Some("libs/a.prx"));
        assert_eq!(
            safe_entry_name("libs\\a.prx").as_deref(),
            Some("libs/a.prx")
        );
        assert!(safe_entry_name("../x").is_none());
        assert!(safe_entry_name("libs/../../x").is_none());
        assert!(safe_entry_name("/etc/x").is_none());
        assert!(safe_entry_name("C:/x").is_none());
    }

    pub(crate) fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut cursor = io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut cursor);
            let options = zip::write::SimpleFileOptions::default();
            for (name, data) in entries {
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        cursor.into_inner()
    }

    fn extract(entries: &[(&str, &[u8])]) -> (tempfile::TempDir, Result<(), ToolkitError>) {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("libs.zip");
        fs::write(&zip, build_zip(entries)).unwrap();
        let result = extract_libs(
            &zip,
            &dir.path().join("libs"),
            &mut |_, _| {},
            &CancelToken::new(),
        );
        (dir, result)
    }

    #[test]
    fn extraction_lands_in_libs_and_rejects_traversal() {
        let (dir, result) = extract(&[
            ("libs/a.prx", b"A"),
            ("libs/sub/b.dll", b"B"),
            ("other/x", b"X"),
        ]);
        result.unwrap();
        assert_eq!(fs::read(dir.path().join("libs/a.prx")).unwrap(), b"A");
        assert_eq!(fs::read(dir.path().join("libs/sub/b.dll")).unwrap(), b"B");
        assert!(!dir.path().join("other").exists());

        for bad in [
            "../evil.prx",
            "libs/../../evil.prx",
            "/abs.prx",
            "C:/evil.prx",
        ] {
            let (dir, result) = extract(&[("libs/ok.prx", b"A"), (bad, b"X")]);
            assert!(matches!(result, Err(ToolkitError::BadArchive(_))), "{bad}");
            // Nothing was written before the rejection.
            assert!(!dir.path().join("libs").exists());
        }
        let (_, result) = extract(&[("nolibs/a.prx", b"A")]);
        assert!(matches!(result, Err(ToolkitError::BadArchive(_))));
    }

    /// Serves fixed bodies on localhost for the download tests.
    pub(crate) fn serve(
        routes: Vec<(&'static str, Vec<u8>, u16)>,
    ) -> (
        String,
        std::thread::JoinHandle<()>,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.server_addr().to_ip().unwrap());
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = stop.clone();
        let handle = std::thread::spawn(move || {
            while !flag.load(std::sync::atomic::Ordering::SeqCst) {
                let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                    continue;
                };
                let path = request.url().to_string();
                match routes.iter().find(|(p, _, _)| *p == path) {
                    Some((_, body, status)) => {
                        let response =
                            tiny_http::Response::from_data(body.clone()).with_status_code(*status);
                        let _ = request.respond(response);
                    }
                    None => {
                        let _ = request.respond(tiny_http::Response::empty(404));
                    }
                }
            }
        });
        (base, handle, stop)
    }

    pub(crate) fn sha_hex(data: &[u8]) -> String {
        hex(&Sha256::digest(data))
    }

    fn release_for(base: &str, relinker: &[u8], zip: &[u8], digests: bool) -> ReleaseInfo {
        ReleaseInfo {
            os: if Os::current() == Os::Macos {
                Os::Windows
            } else {
                Os::current()
            },
            tag: "v9.9.9".into(),
            published: "2026-10-22T00:00:00Z".into(),
            relinker: Some(Asset {
                name: "relinker-v9.9.9.exe".into(),
                url: format!("{base}/relinker.exe"),
                size: relinker.len() as u64,
                sha256: digests.then(|| sha_hex(relinker)),
            }),
            libs: Asset {
                name: "prx-windows-v9.9.9.zip".into(),
                url: format!("{base}/prx.zip"),
                size: zip.len() as u64,
                sha256: digests.then(|| sha_hex(zip)),
            },
            notes_url: String::new(),
            notes: None,
        }
    }

    fn libs_zip() -> Vec<u8> {
        build_zip(&[
            ("libs/a.prx", b"AAA"),
            ("libs/libgcc_s_seh-1.dll", b"1"),
            ("libs/libstdc++-6.dll", b"2"),
            ("libs/libwinpthread-1.dll", b"3"),
        ])
    }

    #[test]
    fn downloads_verifies_extracts_and_installs() {
        let zip = libs_zip();
        let relinker = b"RELINKER-EXE".to_vec();
        let (base, handle, stop) = serve(vec![
            ("/relinker.exe", relinker.clone(), 200),
            ("/prx.zip", zip.clone(), 200),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let patches = dir.path().join("patches-src");
        write(&patches.join("PPSA00001.json"), b"{}");
        let mut events: Vec<(DownloadStage, f64)> = Vec::new();
        let info = release_for(&base, &relinker, &zip, true);
        let toolkit = download_and_install(
            &info,
            &paths,
            Some(&patches),
            &mut |s, f| events.push((s, f)),
            &CancelToken::new(),
        )
        .unwrap();
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        handle.join().unwrap();

        assert_eq!(toolkit.tag(), Some("9.9.9"));
        assert!(toolkit.version().starts_with("9.9.9-"));
        assert_eq!(toolkit.version().len(), "9.9.9-".len() + 12);
        assert_eq!(fs::read(toolkit.relinker()).unwrap(), relinker);
        assert!(toolkit.patches().join("PPSA00001.json").is_file());
        assert!(!paths.toolkit_dir().join("9.9.9.partial").exists());
        assert!(
            events
                .iter()
                .any(|(s, f)| *s == DownloadStage::Libraries && *f >= 1.0)
        );
        assert!(events.iter().any(|(s, _)| *s == DownloadStage::Extracting));
        // locate finds it again, by choice and without.
        if info.os != Os::current() {
            assert!(
                Toolkit::locate_with(&paths, Some(&toolkit.choice()), None).is_none(),
                "a foreign toolkit must not be selected by this host"
            );
            assert_eq!(
                Toolkit::open_for(info.os, toolkit.root())
                    .unwrap()
                    .version(),
                toolkit.version()
            );
            return;
        }
        let found = Toolkit::locate_with(&paths, Some(&toolkit.choice()), None).unwrap();
        assert_eq!(found.version(), toolkit.version());
        assert_eq!(
            Toolkit::locate_with(&paths, None, None).unwrap().version(),
            toolkit.version()
        );
    }

    #[test]
    fn a_wrong_checksum_is_refused_and_leaves_nothing() {
        let zip = libs_zip();
        let relinker = b"RELINKER-EXE".to_vec();
        let (base, handle, stop) = serve(vec![
            ("/relinker.exe", b"TAMPERED".to_vec(), 200),
            ("/prx.zip", zip.clone(), 200),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let info = release_for(&base, &relinker, &zip, true);
        let result = download_and_install(&info, &paths, None, &mut |_, _| {}, &CancelToken::new());
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        handle.join().unwrap();
        assert!(matches!(result, Err(ToolkitError::Digest)));
        assert!(fs::read_dir(paths.toolkit_dir()).unwrap().next().is_none());
    }

    #[test]
    fn a_size_mismatch_without_digest_is_refused_and_http_errors_are_network_errors() {
        let zip = libs_zip();
        let (base, handle, stop) = serve(vec![("/relinker.exe", b"SHORT".to_vec(), 200)]);
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let info = release_for(&base, b"SHORT-BUT-EXPECTED-LONGER", &zip, false);
        let result = download_and_install(&info, &paths, None, &mut |_, _| {}, &CancelToken::new());
        assert!(matches!(result, Err(ToolkitError::Digest)));
        let info = release_for(&base, b"SHORT", &zip, false);
        let result = download_and_install(&info, &paths, None, &mut |_, _| {}, &CancelToken::new());
        assert!(
            matches!(result, Err(ToolkitError::Network(_))),
            "{result:?}"
        );
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        handle.join().unwrap();
        assert!(fs::read_dir(paths.toolkit_dir()).unwrap().next().is_none());
    }

    #[test]
    fn cancelling_mid_download_leaves_no_partial_folder() {
        let zip = libs_zip();
        let relinker = vec![7u8; 200_000];
        let (base, handle, stop) = serve(vec![("/relinker.exe", relinker.clone(), 200)]);
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let info = release_for(&base, &relinker, &zip, true);
        let cancel = CancelToken::new();
        let flag = cancel.clone();
        let result = download_and_install(
            &info,
            &paths,
            None,
            &mut |_, f| {
                if f > 0.0 {
                    flag.cancel()
                }
            },
            &cancel,
        );
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        handle.join().unwrap();
        assert!(matches!(result, Err(ToolkitError::Cancelled)));
        assert!(fs::read_dir(paths.toolkit_dir()).unwrap().next().is_none());
    }

    fn host_release_json() -> String {
        if Os::current() == Os::Macos {
            serde_json::json!({"tag_name":"v0.1.3", "assets":[{"name":Os::Macos.libs_asset("v0.1.3"),"browser_download_url":"https://x/toolkit.zip","size":1,"digest":format!("sha256:{}", "a".repeat(64))}]}).to_string()
        } else {
            RELEASE_JSON.into()
        }
    }

    // Only HTTP transport and executable payloads are fixtures here. These tests cross the real
    // release parser, downloader, manifest verifier and installer, but do not claim to run a game.
    fn macos_archive(tampered: bool) -> Vec<u8> {
        let prefix = "AnyPS5-toolkit-macos-0.2.15-arm";
        let files: Vec<(&str, &[u8])> = vec![
            ("relinker", b"\xcf\xfa\xed\xfemacOS relinker fixture"),
            ("libs/libkernel.prx", b"system library fixture"),
            ("vulkan/libvulkan.1.dylib", b"loader fixture"),
            ("vulkan/libMoltenVK.dylib", b"driver fixture"),
            (
                "vulkan/MoltenVK_icd.json",
                br#"{"ICD":{"library_path":"../../../MacOS/libs/libMoltenVK.dylib"}}"#,
            ),
            ("patches/EXAMPLE.json", br#"{"patches":[]}"#),
            ("version.txt", b"0.2.15-arm-source\n"),
        ];
        let mut hashes = serde_json::Map::new();
        for (name, bytes) in &files {
            hashes.insert((*name).into(), sha_hex(bytes).into());
        }
        if tampered {
            hashes.insert("libs/libkernel.prx".into(), "0".repeat(64).into());
        }
        let manifest = serde_json::to_vec(
            &serde_json::json!({"format":1,"files":hashes,"engineCommit":"fixture"}),
        )
        .unwrap();
        let mut entries: Vec<(String, &[u8])> = files
            .iter()
            .map(|(name, bytes)| (format!("{prefix}/{name}"), *bytes))
            .collect();
        entries.push((format!("{prefix}/toolkit.json"), &manifest));
        build_zip(
            &entries
                .iter()
                .map(|(name, bytes)| (name.as_str(), *bytes))
                .collect::<Vec<_>>(),
        )
    }

    fn macos_release(base: &str, path: &str, bytes: &[u8]) -> ReleaseInfo {
        let json = serde_json::json!({"tag_name":"v0.2.15-arm", "assets":[{
            "name":Os::Macos.libs_asset("v0.2.15-arm"), "browser_download_url":format!("{base}/{path}"),
            "size":bytes.len(), "digest":format!("sha256:{}", sha_hex(bytes))
        }]});
        parse_release_for(Os::Macos, &json.to_string()).unwrap()
    }

    #[test]
    fn macos_bundle_download_is_verified_installed_and_platform_isolated() {
        let bytes = macos_archive(false);
        let (base, server, stop) = serve(vec![("/toolkit.zip", bytes.clone(), 200)]);
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let info = macos_release(&base, "toolkit.zip", &bytes);
        assert!(info.relinker.is_none());
        assert_eq!(info.os, Os::Macos);
        let toolkit =
            download_and_install(&info, &paths, None, &mut |_, _| {}, &CancelToken::new()).unwrap();
        assert_eq!(toolkit.tag(), Some("0.2.15-arm"));
        assert!(toolkit.root().join("vulkan/libMoltenVK.dylib").is_file());
        assert!(toolkit.patches().join("EXAMPLE.json").is_file());
        assert_eq!(
            Toolkit::open_for(Os::Macos, toolkit.root())
                .unwrap()
                .version(),
            toolkit.version()
        );
        assert!(Toolkit::open_for(Os::Windows, toolkit.root()).is_err());
        // Linux must not adopt this toolkit just because both relinkers have the same file name.
        assert!(Toolkit::open_for(Os::Linux, toolkit.root()).is_err());
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn macos_failed_or_cancelled_reinstall_preserves_the_working_toolkit() {
        let bytes = macos_archive(false);
        let bad = macos_archive(true);
        let (base, server, stop) = serve(vec![
            ("/toolkit.zip", bytes.clone(), 200),
            ("/bad.zip", bad.clone(), 200),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let info = macos_release(&base, "toolkit.zip", &bytes);
        let toolkit =
            download_and_install(&info, &paths, None, &mut |_, _| {}, &CancelToken::new()).unwrap();
        let version = toolkit.version().to_owned();
        let bad = macos_release(&base, "bad.zip", &bad);
        assert!(matches!(
            download_and_install(&bad, &paths, None, &mut |_, _| {}, &CancelToken::new()),
            Err(ToolkitError::Digest)
        ));
        assert_eq!(
            Toolkit::open_for(Os::Macos, toolkit.root())
                .unwrap()
                .version(),
            version
        );
        let cancel = CancelToken::new();
        let result = download_and_install(
            &info,
            &paths,
            None,
            &mut |stage, _| {
                if stage == DownloadStage::Extracting {
                    cancel.cancel();
                }
            },
            &cancel,
        );
        assert!(matches!(result, Err(ToolkitError::Cancelled)));
        assert_eq!(
            Toolkit::open_for(Os::Macos, toolkit.root())
                .unwrap()
                .version(),
            version
        );
        assert!(!paths.toolkit_dir().join("0.2.15-arm.partial").exists());
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        server.join().unwrap();
    }

    #[test]
    fn macos_archive_rejects_absolute_or_traversing_entries_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("unpack");
        fs::create_dir_all(&destination).unwrap();
        let escaped = dir.path().join("escaped");
        for name in [
            "AnyPS5-toolkit-macos-0.2.15-arm/../escaped".into(),
            format!("AnyPS5-toolkit-macos-0.2.15-arm/{}", escaped.display()),
        ] {
            let zip = build_zip(&[
                ("AnyPS5-toolkit-macos-0.2.15-arm/valid", b"valid"),
                (&name, b"invalid"),
            ]);
            let zip_path = dir.path().join("test.zip");
            fs::write(&zip_path, zip).unwrap();
            assert!(matches!(
                macos::extract(
                    &zip_path,
                    &destination,
                    "0.2.15-arm",
                    &mut |_, _| {},
                    &CancelToken::new()
                ),
                Err(ToolkitError::BadArchive(_))
            ));
            assert!(!escaped.exists());
            assert_eq!(fs::read_dir(&destination).unwrap().count(), 0);
        }
    }

    #[test]
    fn macos_rejects_a_linux_toolkit_even_when_its_file_names_match() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("relinker"),
            b"\x7fELFLinux relinker fixture",
        );
        write(&dir.path().join("libs/a.prx"), b"library fixture");
        assert!(
            matches!(Toolkit::open_for(Os::Macos, dir.path()), Err(ToolkitError::Io(e)) if e.kind() == io::ErrorKind::InvalidData)
        );
    }

    #[test]
    fn windows_and_linux_keep_their_split_downloads_and_runtime_layouts() {
        let zip = libs_zip();
        for os in [Os::Windows, Os::Linux] {
            let relinker = if os == Os::Windows {
                b"MZ\0\0Windows fixture".to_vec()
            } else {
                b"\x7fELFLinux fixture".to_vec()
            };
            let (base, server, stop) = serve(vec![
                ("/relinker.exe", relinker.clone(), 200),
                ("/prx.zip", zip.clone(), 200),
            ]);
            let mut info = release_for(&base, &relinker, &zip, true);
            info.os = os;
            let dir = tempfile::tempdir().unwrap();
            let paths = AppPaths::under(dir.path());
            let toolkit =
                download_and_install(&info, &paths, None, &mut |_, _| {}, &CancelToken::new())
                    .unwrap();
            assert_eq!(
                fs::read(toolkit.root().join(os.relinker_file())).unwrap(),
                relinker
            );
            assert!(toolkit.libs().join("a.prx").is_file());
            for file in os.runtime_libs() {
                assert!(toolkit.libs().join(file).is_file());
            }
            assert!(!toolkit.root().join("vulkan").exists());
            assert_eq!(
                Toolkit::open_for(os, toolkit.root()).unwrap().version(),
                toolkit.version()
            );
            assert!(Toolkit::open_for(Os::Macos, toolkit.root()).is_err());
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
            server.join().unwrap();
        }
    }

    #[test]
    fn check_latest_reads_the_api_and_maps_rate_limits() {
        let (base, handle, stop) = serve(vec![
            (
                "/repos/o/ok/releases/latest",
                host_release_json().into_bytes(),
                200,
            ),
            ("/repos/o/limited/releases/latest", b"{}".to_vec(), 403),
            ("/repos/o/gone/releases/latest", b"{}".to_vec(), 404),
        ]);
        assert_eq!(check_latest_at(&base, "o/ok").unwrap().tag, "v0.1.3");
        match check_latest_at(&base, "o/limited") {
            Err(ToolkitError::Network(m)) => assert!(m.contains("rate limited")),
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            check_latest_at(&base, "o/gone"),
            Err(ToolkitError::NoRelease)
        ));
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        handle.join().unwrap();
        assert!(matches!(
            check_latest_at("http://127.0.0.1:1", "o/ok"),
            Err(ToolkitError::Network(_))
        ));
    }

    #[test]
    fn only_the_newest_two_releases_stay() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        for tag in ["0.1.0", "0.1.1", "0.1.2", "0.1.3"] {
            make_toolkit(&paths.toolkit_dir().join(tag), true);
        }
        prune_old_releases(&paths, "0.1.3", &[]);
        let left: Vec<String> = installed_releases(&paths)
            .into_iter()
            .map(|(_, d)| d.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["0.1.3", "0.1.2"]);
    }
}
