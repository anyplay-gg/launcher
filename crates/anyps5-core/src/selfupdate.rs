//! The launcher updating itself from its own GitHub releases.
//!
//! The flow has four steps and each is one call:
//!
//! 1. [`check`] asks GitHub for the latest release and returns it when it is newer than the running
//!    version (semver, pre-releases included).
//! 2. [`stage`] downloads the portable `AnyPlay-Launcher-<v>-x64.exe` (or, from a release that has
//!    only that, the same exe under its old name `AnyPS5-Launcher-<v>-x64.exe`) into
//!    `%LOCALAPPDATA%\AnyPS5\Updates`, verifies it against the release's `SHA256SUMS.txt` and
//!    returns a [`Staged`] update. A missing or wrong checksum refuses the update and leaves
//!    nothing behind. It also refuses early when the running exe's folder is not writable.
//! 3. [`Staged::swap_in`] replaces the running exe. Windows cannot overwrite a running exe but can
//!    rename it: the running exe moves to `<name>.old`, the new one takes its place, and a failed
//!    second step moves the old one back. [`Staged::restart`] does that and starts the new exe.
//! 4. [`cleanup`] at the next start deletes the `.old` files and stale downloads.
//!
//! [`pending`] finds an update staged by an earlier run (the player closed the launcher before the
//! swap could happen, or it failed), so it is offered again without a second download. For a signed
//! update the manifest and its signature are kept beside the staged exe and checked again, in full,
//! before it is offered; nothing on disk is trusted just for being there.
//!
//! Nothing here keeps state: the host owns the threads and decides when to swap.
//!
//! # Signed updates, and the migration rule
//!
//! Trusting whatever a GitHub release holds lets anyone who can publish one run code on every
//! player's PC. The launcher therefore embeds a list of trusted public keys
//! (`data/update-keys.txt`) and, **once that list holds a key, takes updates only through the
//! release's signed manifest** (`latest.json` and `latest.json.sig`, see
//! [`anyplay_update_manifest`]): the signature, the expiry, a version newer than the running one
//! and the download's SHA-256 and size are all checked, and the release's tag, its
//! `SHA256SUMS.txt` and its asset names are not used. The download is cut off as soon as it is
//! longer than the manifest's size, a stalled one times out, and every address must be https (the
//! manifest and its signature also from GitHub's own hosts).
//!
//! The rule is a single switch with no way back, [`signing_required`]:
//!
//! - **No key in the list:** signing is not switched on. Behaviour is exactly the unsigned flow
//!   described above, checksum check included.
//! - **One key or more:** a valid signed manifest is required. A release without a manifest, a
//!   bad or missing signature, an expired or older manifest is refused. There is no fallback to
//!   the unsigned flow, and an unreadable key list is not "no key" but a refusal.
//!
//! A refused update only logs; the installed launcher is never touched, and offline the check
//! simply fails and the launcher runs as it is.
//!
//! # Platforms
//!
//! The same flow updates the Windows portable exe and the Linux AppImage; the manifest carries one
//! entry per platform ([`Platform::key`]). A manifest without a Linux entry
//! (every release before the AppImage was signed) offers nothing: a quiet `Ok(None)`, not an
//! error. Linux has **no unsigned flow at all**: with an empty key list nothing is offered.
//!
//! The AppImage replaces itself in place ([`install_path`] finds it from `APPIMAGE`): the staged
//! file is copied to a sibling temp file, hash-checked against the manifest, made executable
//! and renamed over the original, which Linux allows while the old image runs. A launcher
//! that is not an AppImage (the `.tar.gz`) has no install path and stays manual.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyplay_update_manifest::{
    Artifact, LINUX_X64, MAX_MANIFEST_BYTES, MAX_SIGNATURE_BYTES, Manifest, ManifestError,
    TrustedKeys, WINDOWS_X64, verify_signed,
};
pub use anyplay_update_manifest::{Version, is_newer};
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use ureq::ResponseExt as _;
use url::Url;

use crate::error::CancelToken;
use crate::paths::AppPaths;
use crate::toolkit::{self, Asset, ToolkitError};

/// Where the releases live.
pub const REPOSITORY: &str = "anyplaynet/launcher";

const SUMS_NAME: &str = "SHA256SUMS.txt";
/// The signed manifest of a release and its signature.
const MANIFEST_NAME: &str = "latest.json";
const MANIFEST_SIG_NAME: &str = "latest.json.sig";
/// Beside a staged signed exe: the manifest and the signature that vouched for it, checked again
/// by [`pending`].
const MANIFEST_SUFFIX: &str = ".manifest";
const SIGNATURE_SUFFIX: &str = ".manifest.sig";
/// What earlier builds wrote beside a staged exe: a bare hash that authenticated nothing. It is
/// ignored, and cleaned up.
const LEGACY_MARKER_SUFFIX: &str = ".verified";
/// The longest a download may take as a whole; a transfer that stops moving is cut off sooner.
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// The keys this build trusts. Comments only until the operator adds one.
const EMBEDDED_KEYS: &str = include_str!("../../../data/update-keys.txt");
/// The portable exe is `AnyPlay-Launcher-<v>-x64.exe`.
const EXE_PREFIX: &str = "AnyPlay-Launcher-";
/// Its name up to v0.1.3, when the launcher was AnyPS5 Launcher. Those launchers update only from
/// exactly this name, so every release also carries the same exe under it; this launcher falls
/// back to it when a release has only that one.
const LEGACY_EXE_PREFIX: &str = "AnyPS5-Launcher-";
const EXE_SUFFIX: &str = "-x64.exe";

/// The AppImage is `AnyPlay-Launcher-<v>-x86_64.AppImage`.
const APPIMAGE_SUFFIX: &str = "-x86_64.AppImage";

/// Which kind of file the launcher replaces itself with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// The portable exe; the running exe is moved aside and the new one takes its place.
    Windows,
    /// The AppImage; the new file is renamed over the running one.
    Linux,
}

impl Platform {
    /// The platform this build updates, `None` where the launcher does not update itself.
    pub const fn current() -> Option<Platform> {
        if cfg!(windows) {
            Some(Platform::Windows)
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Some(Platform::Linux)
        } else {
            None
        }
    }

    /// The key of this platform in the manifest.
    pub const fn key(self) -> &'static str {
        match self {
            Platform::Windows => WINDOWS_X64,
            Platform::Linux => LINUX_X64,
        }
    }

    /// The file names a manifest may give this platform's download for `version`.
    fn names(self, version: &str) -> Vec<String> {
        match self {
            Platform::Windows => vec![exe_name(version), legacy_exe_name(version)],
            Platform::Linux => vec![appimage_name(version)],
        }
    }

    /// The version in a staged file's name.
    fn version_of(self, name: &str) -> Option<&str> {
        match self {
            Platform::Windows => version_of_exe_name(name),
            Platform::Linux => name.strip_prefix(EXE_PREFIX)?.strip_suffix(APPIMAGE_SUFFIX),
        }
    }

    /// A cheap check that a download is a program of this platform and not a saved error page.
    fn check_program(self, path: &Path) -> Result<(), SelfUpdateError> {
        let mut head = [0u8; 11];
        let read = io::Read::read(&mut fs::File::open(path)?, &mut head)?;
        let ok = match self {
            Platform::Windows => read >= 2 && &head[..2] == b"MZ",
            // A 64-bit ELF with the AppImage type 2 magic after the ELF identification.
            Platform::Linux => {
                read == 11 && head.starts_with(b"\x7fELF") && &head[8..] == b"AI\x02"
            }
        };
        if ok {
            Ok(())
        } else {
            Err(SelfUpdateError::NotAnExecutable)
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SelfUpdateError {
    #[error("{0}")]
    Network(String),
    #[error("the release has no portable exe or no checksum list")]
    NoRelease,
    #[error("the release lists no checksum for {0}")]
    NoChecksum(String),
    #[error("the download does not match its checksum")]
    Digest,
    #[error("the download does not have the size the signed manifest states")]
    Size,
    /// The signed manifest (or the key list that checks it) is refused. Never falls back.
    #[error("the signed update manifest is refused: {0}")]
    Manifest(#[from] ManifestError),
    #[error("the release has no signed update manifest")]
    NoManifest,
    #[error("the download is not a program for this system")]
    NotAnExecutable,
    /// The folder of the running exe refuses new files (a read-only or protected location).
    #[error("cannot write to {}", .0.display())]
    NotWritable(PathBuf),
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("cancelled")]
    Cancelled,
}

impl From<ToolkitError> for SelfUpdateError {
    fn from(error: ToolkitError) -> Self {
        match error {
            ToolkitError::NoRelease => SelfUpdateError::NoRelease,
            ToolkitError::Digest => SelfUpdateError::Digest,
            ToolkitError::TooLarge => SelfUpdateError::Size,
            ToolkitError::Cancelled => SelfUpdateError::Cancelled,
            ToolkitError::Io(error) => SelfUpdateError::Io(error),
            other => SelfUpdateError::Network(other.to_string()),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Releases

/// What vouches for a release's exe.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Vouch {
    /// Not signed: the release's `SHA256SUMS.txt`. Only while no key is trusted.
    Checksums(Asset),
    /// A verified manifest; the exe asset carries the manifest's hash and size. The manifest's
    /// exact bytes and its signature are kept, to be stored beside the staged exe.
    Manifest {
        manifest: Vec<u8>,
        signature: String,
    },
}

/// A newer launcher release with what [`stage`] needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// With the leading `v`. For a signed release it is made from the manifest's version, never
    /// taken from the release's tag.
    pub tag: String,
    platform: Platform,
    exe: Asset,
    vouch: Vouch,
}

impl Release {
    /// The version without the leading `v`: what the player reads.
    pub fn version(&self) -> &str {
        self.tag.trim_start_matches('v')
    }
}

/// The name of the AppImage of a version.
fn appimage_name(version: &str) -> String {
    format!("{EXE_PREFIX}{version}{APPIMAGE_SUFFIX}")
}

/// The name of the portable exe of a version.
fn exe_name(version: &str) -> String {
    format!("{EXE_PREFIX}{version}{EXE_SUFFIX}")
}

/// Its name in releases up to v0.1.3.
fn legacy_exe_name(version: &str) -> String {
    format!("{LEGACY_EXE_PREFIX}{version}{EXE_SUFFIX}")
}

/// The version in a staged file's name, under either name.
fn version_of_exe_name(name: &str) -> Option<&str> {
    name.strip_prefix(EXE_PREFIX)
        .or_else(|| name.strip_prefix(LEGACY_EXE_PREFIX))?
        .strip_suffix(EXE_SUFFIX)
}

/// Reads the JSON of `releases/latest`: the release when its tag is newer than `current`, `None`
/// when it is not (its files are then not looked at), an error when it is newer but lacks the
/// portable exe or the checksum list.
pub(crate) fn parse_latest(json: &str, current: &str) -> Result<Option<Release>, SelfUpdateError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| SelfUpdateError::Network(format!("bad release data: {e}")))?;
    let text =
        |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).map(str::to_owned);
    let tag = text(&value, "tag_name").ok_or(SelfUpdateError::NoRelease)?;
    if !is_newer(&tag, current) {
        return Ok(None);
    }
    let assets = value
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or(SelfUpdateError::NoRelease)?;
    let find = |name: &str| {
        assets
            .iter()
            .find(|a| text(a, "name").as_deref() == Some(name))
            .and_then(|a| {
                Some(Asset {
                    name: name.to_owned(),
                    url: text(a, "browser_download_url")?,
                    size: a.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
                    sha256: None,
                })
            })
    };
    let version = tag.trim_start_matches('v');
    let exe = find(&exe_name(version))
        .or_else(|| find(&legacy_exe_name(version)))
        .ok_or(SelfUpdateError::NoRelease)?;
    let sums = find(SUMS_NAME).ok_or(SelfUpdateError::NoRelease)?;
    Ok(Some(Release {
        tag,
        platform: Platform::Windows,
        exe,
        vouch: Vouch::Checksums(sums),
    }))
}

/// The keys this build trusts. An unreadable list is an error, never an empty one.
fn embedded_keys() -> Result<TrustedKeys, SelfUpdateError> {
    Ok(TrustedKeys::parse(EMBEDDED_KEYS)?)
}

/// Whether this build takes updates only through a signed manifest: it does as soon as the key
/// list holds a key, and also when the list cannot be read (failing closed, so a damaged list can
/// never send the launcher back to the unsigned flow).
pub fn signing_required() -> bool {
    embedded_keys().map_or(true, |keys| !keys.is_empty())
}

/// Asks GitHub whether a release newer than `current` exists for this platform. Blocking: call
/// from a worker thread. `Ok(None)` also where the launcher does not update itself at all.
pub fn check(current: &str) -> Result<Option<Release>, SelfUpdateError> {
    let Some(platform) = Platform::current() else {
        return Ok(None);
    };
    check_with(
        toolkit::API_BASE,
        REPOSITORY,
        current,
        &embedded_keys()?,
        platform,
    )
}

/// [`check`] with the API host, repository, trusted keys and platform given. An empty list is the
/// unsigned flow (Windows only; Linux offers nothing), any key makes the signed manifest
/// mandatory (see the module documentation).
pub fn check_with(
    api_base: &str,
    repo: &str,
    current: &str,
    keys: &TrustedKeys,
    platform: Platform,
) -> Result<Option<Release>, SelfUpdateError> {
    let json = toolkit::latest_release_json(api_base, repo)?;
    if keys.is_empty() {
        return match platform {
            Platform::Windows => parse_latest(&json, current),
            Platform::Linux => Ok(None),
        };
    }
    parse_signed(
        &json,
        current,
        platform,
        keys,
        OffsetDateTime::now_utc(),
        &mut get_small,
    )
}

/// Downloads at most `max` bytes (the manifest and its signature).
fn get_small(url: &str, max: u64) -> Result<Vec<u8>, SelfUpdateError> {
    let mut response = toolkit::update_agent(Some(std::time::Duration::from_secs(20)))
        .get(url)
        .header("User-Agent", toolkit::user_agent())
        .call()
        .map_err(|e| SelfUpdateError::Network(e.to_string()))?;
    // After the redirects the answer must still come from a host we trust.
    vet_url(&response.get_uri().to_string(), true)?;
    if response.status().as_u16() != 200 {
        return Err(SelfUpdateError::Network(format!(
            "download failed: HTTP {}",
            response.status().as_u16()
        )));
    }
    response
        .body_mut()
        .with_config()
        .limit(max)
        .read_to_vec()
        .map_err(|e| SelfUpdateError::Network(e.to_string()))
}

/// Fetches a URL, at most the given number of bytes.
type Fetch<'a> = &'a mut dyn FnMut(&str, u64) -> Result<Vec<u8>, SelfUpdateError>;

/// The signed flow on the JSON of `releases/latest`: finds the manifest and its signature among
/// the assets, fetches both with `fetch(url, max_bytes)`, verifies them at `now` and returns the
/// release the manifest offers. `Ok(None)` when the manifest is genuine but not newer than
/// `current` (nothing to update, or an older release served as "latest"); every other refusal is
/// an error. The tag and the other assets of the release are not trusted and not used.
fn parse_signed(
    json: &str,
    current: &str,
    platform: Platform,
    keys: &TrustedKeys,
    now: OffsetDateTime,
    fetch: Fetch,
) -> Result<Option<Release>, SelfUpdateError> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| SelfUpdateError::Network(format!("bad release data: {e}")))?;
    let url_of = |name: &str| {
        value
            .get("assets")?
            .as_array()?
            .iter()
            .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(name))?
            .get("browser_download_url")?
            .as_str()
            .map(str::to_owned)
    };
    let (Some(manifest_url), Some(sig_url)) = (url_of(MANIFEST_NAME), url_of(MANIFEST_SIG_NAME))
    else {
        return Err(SelfUpdateError::NoManifest);
    };
    // The release JSON is not signed: where it sends us is checked before anything is fetched.
    vet_url(&manifest_url, true)?;
    vet_url(&sig_url, true)?;
    let manifest_bytes = fetch(&manifest_url, MAX_MANIFEST_BYTES)?;
    let signature = fetch(&sig_url, MAX_SIGNATURE_BYTES)?;
    let signature = String::from_utf8_lossy(&signature).into_owned();
    let manifest = verify_signed(&manifest_bytes, &signature, keys)?;
    // Newer first: a launcher that is up to date must not call a stale manifest an error.
    match manifest.allows_update_from(current) {
        Ok(()) => {}
        Err(ManifestError::NotNewer { .. }) => return Ok(None),
        Err(other) => return Err(other.into()),
    }
    manifest.check_time(now)?;
    // A manifest from before the AppImage was signed has no Linux file: nothing, and no error.
    let Some(artifact) = signed_artifact(&manifest, platform)? else {
        return Ok(None);
    };
    Ok(Some(Release {
        tag: format!("v{}", manifest.version),
        platform,
        exe: Asset {
            name: artifact.name.clone(),
            url: artifact.url.clone(),
            size: artifact.size,
            sha256: Some(artifact.sha256.clone()),
        },
        vouch: Vouch::Manifest {
            manifest: manifest_bytes,
            signature,
        },
    }))
}

/// Plain http to this machine: only the tests' servers speak it. A release build has no such
/// allowance, so it takes https and nothing else.
#[cfg(test)]
fn loopback_http_allowed(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(name)) => name == "localhost",
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

#[cfg(not(test))]
fn loopback_http_allowed(_: &Url) -> bool {
    false
}

/// Whether `host` is one of GitHub's: the site, or the hosts its release files are served from.
fn github_host(host: &str) -> bool {
    host == "github.com" || host.ends_with(".githubusercontent.com")
}

/// Parses an address of an update and applies the rules for all of them: https, no user name or
/// password in it (`https://github.com@evil.example/` is a request to evil.example), and, with
/// `github_only`, a GitHub host on the default port. The host is the parsed one, never a prefix
/// of the text.
fn vet_url(raw: &str, github_only: bool) -> Result<Url, SelfUpdateError> {
    let refuse = |why: &str| Err(ManifestError::Malformed(why.to_owned()).into());
    let Ok(url) = Url::parse(raw) else {
        return refuse("an address is not a URL");
    };
    if !url.username().is_empty() || url.password().is_some() {
        return refuse("an address carries a user name");
    }
    let local = loopback_http_allowed(&url);
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return refuse("an address is not https");
    }
    let Some(host) = url.host_str() else {
        return refuse("an address has no host");
    };
    if github_only && !local && !(github_host(host) && url.port().is_none()) {
        return refuse("an address is not on a GitHub host");
    }
    Ok(url)
}

/// The exe of a verified manifest, once its name and address pass: the name must be exactly one
/// the launcher stages under (so it cannot point outside the updates folder), and the address
/// must be https (see [`vet_url`]).
/// `None` when the manifest has no file for `platform`.
fn signed_artifact(
    manifest: &Manifest,
    platform: Platform,
) -> Result<Option<&Artifact>, SelfUpdateError> {
    let Some(artifact) = manifest.artifact(platform.key()) else {
        // Only Linux is optional: every release has its Windows exe, so a manifest without it is
        // broken. A manifest from before the AppImage was signed simply has nothing for Linux.
        return match platform {
            Platform::Linux => Ok(None),
            Platform::Windows => Err(SelfUpdateError::NoRelease),
        };
    };
    if !platform.names(&manifest.version).contains(&artifact.name) {
        return Err(ManifestError::Malformed(
            "the download has an unexpected file name".to_owned(),
        )
        .into());
    }
    // The hash is signed, so the exe may sit on any https host.
    vet_url(&artifact.url, false)?;
    Ok(Some(artifact))
}

/// The SHA-256 (lower case hex) that a `SHA256SUMS.txt` lists for `file`. Lines look like
/// `<hex> *<file>` (binary) or `<hex>  <file>` (text).
pub fn checksum_for(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, name) = line.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        let valid = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
        (valid && name == file).then(|| hash.to_ascii_lowercase())
    })
}

// ---------------------------------------------------------------------------------------------
// Staging

/// A downloaded, verified update waiting for the swap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    /// Without the leading `v`.
    pub version: String,
    pub path: PathBuf,
    /// The SHA-256 the file must have when it is installed (a signed update only): the swap hashes
    /// its own copy against it, whatever happened to the staged file since it was checked.
    pub sha256: Option<String>,
    /// How the swap is done.
    pub platform: Platform,
}

/// Where downloads wait: `%LOCALAPPDATA%\AnyPS5\Updates`.
pub fn updates_dir(paths: &AppPaths) -> PathBuf {
    paths.data.join("Updates")
}

/// Downloads `release` and verifies it, so the swap can be done at any time. `exe` is the running
/// exe: when its folder cannot be written the update is refused before anything is downloaded.
/// `progress` gets the share (0..=1) of the download. A file left by an earlier run that has the
/// right size is reused (only verified files get their final name).
pub fn stage(
    release: &Release,
    exe: &Path,
    paths: &AppPaths,
    progress: &mut dyn FnMut(f64),
    cancel: &CancelToken,
) -> Result<Staged, SelfUpdateError> {
    ensure_writable(exe.parent().unwrap_or(Path::new(".")))?;
    let dir = updates_dir(paths);
    fs::create_dir_all(&dir)?;
    let mut staged = Staged {
        version: release.version().to_owned(),
        path: dir.join(&release.exe.name),
        sha256: None,
        platform: release.platform,
    };
    let sums_asset = match &release.vouch {
        Vouch::Checksums(sums) => sums,
        Vouch::Manifest {
            manifest,
            signature,
        } => {
            staged.sha256 = release.exe.sha256.clone();
            return stage_signed(release, staged, (manifest, signature), progress, cancel);
        }
    };
    let size = release.exe.size;
    if let Ok(meta) = fs::metadata(&staged.path)
        && meta.is_file()
        && (size == 0 || meta.len() == size)
    {
        return Ok(staged);
    }

    let sums = toolkit::download_text(&sums_asset.url)?;
    let want = checksum_for(&sums, &release.exe.name)
        .ok_or_else(|| SelfUpdateError::NoChecksum(release.exe.name.clone()))?;
    let verified = Asset {
        sha256: Some(want),
        ..release.exe.clone()
    };
    let part = sidecar(&staged.path, ".part");
    let result = toolkit::download_to(&download_agent(), &verified, &part, progress, cancel)
        .map_err(SelfUpdateError::from)
        .and_then(|()| release.platform.check_program(&part));
    match result {
        Ok(()) => {
            fs::rename(&part, &staged.path)?;
            Ok(staged)
        }
        Err(error) => {
            let _ = fs::remove_file(&part);
            Err(error)
        }
    }
}

/// The agent for the exe: connect and stall timeouts, https only, few redirects, and a limit on
/// the whole transfer.
fn download_agent() -> ureq::Agent {
    toolkit::update_agent(Some(DOWNLOAD_TIMEOUT))
}

/// [`stage`] for a release whose manifest was verified: the download must have exactly the
/// SHA-256 and the size the manifest states, and is cut off as soon as it is longer. The manifest
/// and its signature are written beside the exe before it gets its final name, so a staged exe
/// never exists without the proof that [`pending`] checks. A file left by an earlier run is
/// reused only when it hashes to the manifest's value.
fn stage_signed(
    release: &Release,
    staged: Staged,
    (manifest, signature): (&[u8], &str),
    progress: &mut dyn FnMut(f64),
    cancel: &CancelToken,
) -> Result<Staged, SelfUpdateError> {
    let want = release.exe.sha256.clone().unwrap_or_default();
    if let Ok(meta) = fs::metadata(&staged.path)
        && meta.is_file()
        && meta.len() == release.exe.size
        && hash_file(&staged.path).is_ok_and(|have| have == want)
    {
        write_proof(&staged.path, manifest, signature)?;
        return Ok(staged);
    }
    let part = sidecar(&staged.path, ".part");
    let result = toolkit::download_capped(
        &download_agent(),
        &release.exe,
        &part,
        Some(release.exe.size),
        progress,
        cancel,
    )
    .map_err(SelfUpdateError::from)
    .and_then(|()| {
        if fs::metadata(&part)?.len() == release.exe.size {
            Ok(())
        } else {
            Err(SelfUpdateError::Size)
        }
    })
    .and_then(|()| release.platform.check_program(&part));
    match result {
        Ok(()) => {
            if let Err(error) = write_proof(&staged.path, manifest, signature) {
                let _ = fs::remove_file(&part);
                return Err(error);
            }
            if let Err(error) = fs::rename(&part, &staged.path) {
                remove_proof(&staged.path);
                let _ = fs::remove_file(&part);
                return Err(error.into());
            }
            Ok(staged)
        }
        Err(error) => {
            let _ = fs::remove_file(&part);
            Err(error)
        }
    }
}

/// `<exe><suffix>`: a file kept beside a staged exe.
fn sidecar(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Keeps the manifest and the signature that vouched for a staged exe beside it.
fn write_proof(exe: &Path, manifest: &[u8], signature: &str) -> Result<(), SelfUpdateError> {
    let result = fs::write(sidecar(exe, MANIFEST_SUFFIX), manifest)
        .and_then(|()| fs::write(sidecar(exe, SIGNATURE_SUFFIX), signature));
    if let Err(error) = result {
        remove_proof(exe);
        return Err(error.into());
    }
    Ok(())
}

fn remove_proof(exe: &Path) {
    for suffix in [MANIFEST_SUFFIX, SIGNATURE_SUFFIX, LEGACY_MARKER_SUFFIX] {
        let _ = fs::remove_file(sidecar(exe, suffix));
    }
}

/// A small file, or `None` when it is missing or longer than `max`.
fn read_small(path: &Path, max: u64) -> Option<Vec<u8>> {
    let meta = fs::metadata(path).ok()?;
    (meta.is_file() && meta.len() <= max)
        .then(|| fs::read(path).ok())
        .flatten()
}

/// Checks a staged exe the way a fresh download is checked: its manifest and signature (kept beside
/// it) must verify against `keys` and not be expired, name a version newer than `current` and the
/// version and file name of this exe, and the exe must have the size and SHA-256 the manifest
/// states. Returns that SHA-256. A marker or any other file next to the exe proves nothing.
fn vouched_hash(
    platform: Platform,
    exe: &Path,
    version: &str,
    current: &str,
    keys: &TrustedKeys,
    now: OffsetDateTime,
) -> Option<String> {
    let manifest_bytes = read_small(&sidecar(exe, MANIFEST_SUFFIX), MAX_MANIFEST_BYTES)?;
    let signature = read_small(&sidecar(exe, SIGNATURE_SUFFIX), MAX_SIGNATURE_BYTES)?;
    let signature = String::from_utf8(signature).ok()?;
    let manifest = verify_signed(&manifest_bytes, &signature, keys).ok()?;
    manifest.allows_update_from(current).ok()?;
    manifest.check_time(now).ok()?;
    let artifact = signed_artifact(&manifest, platform).ok()??;
    let file_name = exe.file_name()?.to_str()?;
    if manifest.version != version || artifact.name != file_name {
        return None;
    }
    let meta = fs::metadata(exe).ok()?;
    (meta.is_file() && meta.len() == artifact.size && hash_file(exe).ok()? == artifact.sha256)
        .then(|| artifact.sha256.clone())
}

fn hash_file(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(toolkit::hex(&hasher.finalize()))
}

/// The file the launcher would replace with an update, `None` when it cannot update itself in
/// place and an update stays manual: on Windows the running exe; on Linux the AppImage that is
/// running (see [`appimage_path`]), so a launcher started from the `.tar.gz` has none.
pub fn install_path() -> Option<PathBuf> {
    match Platform::current()? {
        Platform::Windows => std::env::current_exe().ok(),
        Platform::Linux => appimage_path(
            std::env::var_os("APPIMAGE").as_deref(),
            std::env::var_os("APPDIR").as_deref(),
            &std::env::current_exe().ok()?,
        ),
    }
}

/// The AppImage that is running, from the `APPIMAGE` and `APPDIR` variables its runtime sets, or
/// `None`. Both are inherited by child processes, so they are not taken on trust: `running` (the
/// program's own path) must lie inside `appdir`, or this program is not that AppImage (a `.tar.gz`
/// launcher started from some other AppImage's terminal would otherwise overwrite that one). The
/// path must be absolute, without `..`, end in `.AppImage` and be a regular file, not a link.
/// Whether its folder accepts a new file is [`can_install`] and [`stage`].
fn appimage_path(
    appimage: Option<&std::ffi::OsStr>,
    appdir: Option<&std::ffi::OsStr>,
    running: &Path,
) -> Option<PathBuf> {
    use std::path::Component;
    let (image, appdir) = (Path::new(appimage?), Path::new(appdir?));
    let clean = |path: &Path| {
        path.is_absolute() && !path.components().any(|part| part == Component::ParentDir)
    };
    if !clean(image) || !clean(appdir) || appdir.parent().is_none() || !running.starts_with(appdir)
    {
        return None;
    }
    let name = image.file_name()?.to_str()?;
    if !name.ends_with(".AppImage") || name.len() == ".AppImage".len() {
        return None;
    }
    fs::symlink_metadata(image)
        .ok()?
        .file_type()
        .is_file()
        .then(|| image.to_path_buf())
}

/// Whether an update can be installed over `exe`: its folder accepts new files.
pub fn can_install(exe: &Path) -> bool {
    ensure_writable(exe.parent().unwrap_or(Path::new("."))).is_ok()
}

/// Whether new files can be created in `dir`.
fn ensure_writable(dir: &Path) -> Result<(), SelfUpdateError> {
    let probe = dir.join(format!(".anyps5-write-test-{}", std::process::id()));
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            Ok(())
        }
        Err(_) => Err(SelfUpdateError::NotWritable(dir.to_path_buf())),
    }
}

/// An update staged by an earlier run: the newest staged exe that is newer than `current`. Once
/// signing is required ([`signing_required`]) only an exe that a signed manifest vouches for
/// counts: the manifest and signature kept beside it are verified again (signature, expiry,
/// version) and the exe must hash to the manifest's value. What an unsigned launcher left behind,
/// or anything planted in the folder, is not offered.
pub fn pending(paths: &AppPaths, current: &str) -> Option<Staged> {
    let platform = Platform::current()?;
    let now = OffsetDateTime::now_utc();
    match embedded_keys() {
        Ok(keys) if keys.is_empty() => pending_with(paths, current, platform, None, now),
        Ok(keys) => pending_with(paths, current, platform, Some(&keys), now),
        Err(_) => None,
    }
}

/// [`pending`] with the platform, the trusted keys (`None`: signing is not required, which only
/// Windows allows) and the time given.
fn pending_with(
    paths: &AppPaths,
    current: &str,
    platform: Platform,
    keys: Option<&TrustedKeys>,
    now: OffsetDateTime,
) -> Option<Staged> {
    if keys.is_none() && platform == Platform::Linux {
        return None;
    }
    fs::read_dir(updates_dir(paths))
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let version = platform.version_of(&name)?.to_owned();
            if !is_newer(&version, current) || !entry.path().is_file() {
                return None;
            }
            let expect = match keys {
                Some(keys) => Some(vouched_hash(
                    platform,
                    &entry.path(),
                    &version,
                    current,
                    keys,
                    now,
                )?),
                None => None,
            };
            Some((Version::parse(&version)?, version, entry.path(), expect))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, version, path, expect)| Staged {
            version,
            path,
            sha256: expect,
            platform,
        })
}

// ---------------------------------------------------------------------------------------------
// The swap

impl Staged {
    /// Replaces the program at `exe` (normally the running one) with the staged file.
    ///
    /// On Linux the AppImage is replaced by a rename over it, see [`Staged::swap_in_image`]. On
    /// Windows: the staged file is first copied beside `exe` (it may sit on another drive, and a rename
    /// needs one), then `exe` is renamed to `<name>.old` and the copy to `exe`. If the second
    /// rename fails the first is undone, so `exe` is never left missing. The staged file is
    /// deleted afterwards. The running process keeps running from the `.old` file.
    ///
    /// For a signed update the copy is hashed before it is renamed into place and refused if it
    /// is not the exact file the manifest vouched for; the staged file may have been changed
    /// since it was checked.
    pub fn swap_in(&self, exe: &Path) -> Result<(), SelfUpdateError> {
        let dir = exe.parent().unwrap_or(Path::new("."));
        let name = exe
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?
            .to_string_lossy()
            .into_owned();
        let incoming = dir.join(format!("{name}.new"));
        let _ = fs::remove_file(&incoming);
        fs::copy(&self.path, &incoming).map_err(|e| writable_or(e, dir))?;
        self.check_copy(&incoming)?;
        if self.platform == Platform::Linux {
            return self.swap_in_image(exe, &incoming);
        }
        let old = free_old_name(dir, &name);
        if let Err(error) = fs::rename(exe, &old) {
            let _ = fs::remove_file(&incoming);
            return Err(writable_or(error, dir));
        }
        if let Err(error) = fs::rename(&incoming, exe) {
            let _ = fs::rename(&old, exe);
            let _ = fs::remove_file(&incoming);
            return Err(writable_or(error, dir));
        }
        let _ = fs::remove_file(&self.path);
        remove_proof(&self.path);
        Ok(())
    }

    /// A signed update's copy must be the exact file the manifest vouched for: it is hashed before
    /// it is renamed into place, and removed when it is not.
    fn check_copy(&self, incoming: &Path) -> Result<(), SelfUpdateError> {
        let Some(want) = &self.sha256 else {
            // Only Windows has an unsigned flow; an AppImage is never installed without a hash.
            return match self.platform {
                Platform::Windows => Ok(()),
                Platform::Linux => {
                    let _ = fs::remove_file(incoming);
                    Err(SelfUpdateError::Digest)
                }
            };
        };
        match hash_file(incoming) {
            Ok(have) if have == *want => Ok(()),
            other => {
                let _ = fs::remove_file(incoming);
                Err(match other {
                    Ok(_) => SelfUpdateError::Digest,
                    Err(error) => error.into(),
                })
            }
        }
    }

    /// The Linux half of [`swap_in`](Self::swap_in): `incoming` is the verified copy beside the
    /// AppImage. It is made executable and renamed over the original in one atomic step, which
    /// Linux allows while the old image runs (the running process keeps the old file open). A
    /// failure at any point leaves the original untouched and `incoming` removed. `exe` must be a
    /// regular file, not a link: a link would be replaced instead of the file it points to.
    fn swap_in_image(&self, exe: &Path, incoming: &Path) -> Result<(), SelfUpdateError> {
        let dir = exe.parent().unwrap_or(Path::new("."));
        let result = (|| {
            if !fs::symlink_metadata(exe)?.file_type().is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "the AppImage is not a regular file",
                ));
            }
            make_executable(incoming)?;
            fs::File::open(incoming)?.sync_all()?;
            fs::rename(incoming, exe)
        })();
        if let Err(error) = result {
            let _ = fs::remove_file(incoming);
            return Err(writable_or(error, dir));
        }
        let _ = fs::remove_file(&self.path);
        remove_proof(&self.path);
        Ok(())
    }

    /// [`swap_in`](Self::swap_in), then starts the new exe with `args`. The caller closes this
    /// process afterwards.
    pub fn restart(
        &self,
        exe: &Path,
        args: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<(), SelfUpdateError> {
        self.swap_in(exe)?;
        let mut command = Command::new(exe);
        command.args(args);
        if self.platform == Platform::Linux {
            // The new image sets these for itself; the old image's values must not leak into it.
            command
                .env_remove("APPIMAGE")
                .env_remove("APPDIR")
                .env_remove("ARGV0")
                .env_remove("OWD");
        }
        command.spawn()?;
        Ok(())
    }
}

/// `rwxr-xr-x`.
#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "an AppImage can only be installed on Linux",
    ))
}

fn writable_or(error: io::Error, dir: &Path) -> SelfUpdateError {
    if error.kind() == io::ErrorKind::PermissionDenied {
        SelfUpdateError::NotWritable(dir.to_path_buf())
    } else {
        SelfUpdateError::Io(error)
    }
}

/// `<name>.old`, or `<name>.old1`, ... when an earlier one is still locked by a running process.
fn free_old_name(dir: &Path, name: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{name}.old"));
    for n in 1..100 {
        if !candidate.exists() || fs::remove_file(&candidate).is_ok() {
            break;
        }
        candidate = dir.join(format!("{name}.old{n}"));
    }
    candidate
}

/// At start: deletes what the last swap left. `<name>.old*` and `<name>.new` beside `exe`, and in
/// the updates folder the partial downloads and every staged exe that is not newer than
/// `current`. Best effort: a file that is still in use stays for the next start.
pub fn cleanup(exe: &Path, paths: &AppPaths, current: &str) {
    if let Some(platform) = Platform::current() {
        cleanup_for(platform, exe, paths, current);
    }
}

fn cleanup_for(platform: Platform, exe: &Path, paths: &AppPaths, current: &str) {
    if let (Some(dir), Some(name)) = (exe.parent(), exe.file_name()) {
        let name = name.to_string_lossy().into_owned();
        for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
            let other = entry.file_name().to_string_lossy().into_owned();
            let leftover = other
                .strip_prefix(&name)
                .is_some_and(|rest| rest == ".new" || rest.starts_with(".old"));
            if leftover {
                remove(&entry.path());
            }
        }
    }
    for entry in fs::read_dir(updates_dir(paths))
        .into_iter()
        .flatten()
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        // A manifest, signature or old marker goes with its exe: it is stale when the exe is gone
        // or stale itself.
        let belongs_to = [SIGNATURE_SUFFIX, MANIFEST_SUFFIX, LEGACY_MARKER_SUFFIX]
            .iter()
            .find_map(|suffix| name.strip_suffix(suffix));
        let orphan = belongs_to.is_some_and(|exe| !updates_dir(paths).join(exe).exists());
        let stale = name.ends_with(".part")
            || orphan
            || platform
                .version_of(belongs_to.unwrap_or(&name))
                .is_some_and(|v| !is_newer(v, current));
        if stale {
            remove(&entry.path());
        }
    }
}

fn remove(path: &Path) {
    if let Err(error) = fs::remove_file(path) {
        tracing::debug!("cannot delete {}: {error}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
    use std::time::Duration;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap_or_else(|| panic!("{text} should parse"))
    }

    #[test]
    fn versions_compare_as_semver() {
        assert!(is_newer("0.1.2", "0.1.1"));
        assert!(is_newer("v0.1.10", "0.1.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.1", "0.1.1"));
        assert!(!is_newer("0.1.0", "0.1.1"));
        // Build metadata does not count, a leading v is fine.
        assert!(!is_newer("v0.1.1+abc", "0.1.1+def"));
    }

    #[test]
    fn prereleases_sort_below_their_release() {
        assert!(v("0.2.0-rc.1") < v("0.2.0"));
        assert!(v("0.2.0") > v("0.2.0-rc.9"));
        assert!(v("0.1.9") < v("0.2.0-alpha"));
        // The examples of the semver spec, in order.
        let order = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0",
        ];
        for pair in order.windows(2) {
            assert!(v(pair[0]) < v(pair[1]), "{} < {}", pair[0], pair[1]);
        }
        // A running pre-release is updated to the release it leads to.
        assert!(is_newer("0.2.0", "0.2.0-rc.1"));
        assert!(!is_newer("0.2.0-rc.1", "0.2.0"));
        assert!(v("0.2.0-rc.1").is_prerelease() && !v("0.2.0").is_prerelease());
    }

    #[test]
    fn garbage_is_never_newer() {
        for bad in [
            "",
            "latest",
            "1",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.x",
            "1.2.3-",
            "1.2.3-a..b",
        ] {
            assert!(Version::parse(bad).is_none(), "{bad:?}");
            assert!(!is_newer(bad, "0.0.1"), "{bad:?}");
        }
        assert!(!is_newer("9.9.9", "nightly"));
    }

    const SUMS_HASH: &str = "ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12";

    fn release_json(
        tag: &str,
        base: &str,
        with_sums: bool,
        with_exe: bool,
        with_legacy_exe: bool,
    ) -> String {
        let version = tag.trim_start_matches('v');
        let mut assets = vec![format!(
            r#"{{"name": "AnyPlay-Launcher-{version}-Setup.exe", "size": 5, "browser_download_url": "{base}/setup"}}"#
        )];
        // Before the new name, so the choice is shown to go by name, not by order.
        if with_legacy_exe {
            assets.push(format!(
                r#"{{"name": "AnyPS5-Launcher-{version}-x64.exe", "size": 4, "browser_download_url": "{base}/legacy"}}"#
            ));
        }
        if with_exe {
            assets.push(format!(
                r#"{{"name": "AnyPlay-Launcher-{version}-x64.exe", "size": 4, "browser_download_url": "{base}/exe"}}"#
            ));
        }
        if with_sums {
            assets.push(format!(
                r#"{{"name": "SHA256SUMS.txt", "size": 9, "browser_download_url": "{base}/sums"}}"#
            ));
        }
        format!(
            r#"{{"tag_name": "{tag}", "assets": [{}]}}"#,
            assets.join(",")
        )
    }

    #[test]
    fn the_portable_exe_is_chosen_over_the_installer() {
        let json = release_json("v0.2.0", "http://x", true, true, true);
        let release = parse_latest(&json, "0.1.1").unwrap().unwrap();
        assert_eq!(release.version(), "0.2.0");
        assert_eq!(release.exe.name, "AnyPlay-Launcher-0.2.0-x64.exe");
        assert_eq!(release.exe.url, "http://x/exe");
        assert!(matches!(&release.vouch, Vouch::Checksums(sums) if sums.name == "SHA256SUMS.txt"));
    }

    #[test]
    fn the_new_exe_name_wins_and_the_old_one_is_still_found() {
        let both = release_json("v0.2.0", "http://x", true, true, true);
        let release = parse_latest(&both, "0.1.3").unwrap().unwrap();
        assert_eq!(release.exe.name, "AnyPlay-Launcher-0.2.0-x64.exe");
        assert_eq!(release.exe.url, "http://x/exe");
        let old_only = release_json("v0.2.0", "http://x", true, false, true);
        let release = parse_latest(&old_only, "0.1.3").unwrap().unwrap();
        assert_eq!(release.exe.name, "AnyPS5-Launcher-0.2.0-x64.exe");
        assert_eq!(release.exe.url, "http://x/legacy");
    }

    #[test]
    fn an_old_or_equal_release_needs_no_assets() {
        let json = r#"{"tag_name": "v0.1.1", "assets": []}"#;
        assert_eq!(parse_latest(json, "0.1.1").unwrap(), None);
        assert_eq!(parse_latest(json, "0.3.0").unwrap(), None);
    }

    #[test]
    fn a_newer_release_without_the_files_is_an_error() {
        let no_exe = release_json("v0.2.0", "http://x", true, false, false);
        assert!(matches!(
            parse_latest(&no_exe, "0.1.1"),
            Err(SelfUpdateError::NoRelease)
        ));
        let no_sums = release_json("v0.2.0", "http://x", false, true, true);
        assert!(matches!(
            parse_latest(&no_sums, "0.1.1"),
            Err(SelfUpdateError::NoRelease)
        ));
        assert!(parse_latest("not json", "0.1.1").is_err());
        assert!(parse_latest("{}", "0.1.1").is_err());
    }

    #[test]
    fn checksums_are_found_by_file_name() {
        let sums = format!(
            "{} *AnyPlay-Launcher-0.2.0-Setup.exe\n{} *AnyPlay-Launcher-0.2.0-x64.exe\n{} *AnyPS5-Launcher-0.2.0-x64.exe\n{}  other.txt\n",
            "11".repeat(32),
            SUMS_HASH.to_ascii_uppercase(),
            SUMS_HASH,
            "22".repeat(32),
        );
        assert_eq!(
            checksum_for(&sums, "AnyPlay-Launcher-0.2.0-x64.exe").as_deref(),
            Some(SUMS_HASH)
        );
        assert_eq!(
            checksum_for(&sums, "AnyPS5-Launcher-0.2.0-x64.exe").as_deref(),
            Some(SUMS_HASH)
        );
        assert_eq!(checksum_for(&sums, "other.txt"), Some("22".repeat(32)));
        assert_eq!(checksum_for(&sums, "missing.exe"), None);
        // A short or non-hex hash is not a checksum.
        assert_eq!(checksum_for("abc *f.exe\n", "f.exe"), None);
        assert_eq!(
            checksum_for(&format!("{} *f.exe", "zz".repeat(32)), "f.exe"),
            None
        );
    }

    /// Serves fixed bodies on localhost until the returned flag is set.
    fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> (String, Arc<AtomicBool>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.server_addr().to_ip().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            while !flag.load(AtomicOrdering::SeqCst) {
                let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                    continue;
                };
                let path = request.url().to_string();
                let _ = match routes.iter().find(|(p, _)| *p == path) {
                    Some((_, body)) => {
                        request.respond(tiny_http::Response::from_data(body.clone()))
                    }
                    None => request.respond(tiny_http::Response::empty(404)),
                };
            }
        });
        (base, stop)
    }

    fn sha_hex(data: &[u8]) -> String {
        toolkit::hex(&Sha256::digest(data))
    }

    /// A served release 0.2.0 whose exe is `body`, with `sums` as its checksum list.
    fn serve_release(body: &[u8], sums: String) -> (String, Arc<AtomicBool>, Release) {
        let (base, stop) = serve(vec![("/exe", body.to_vec()), ("/sums", sums.into_bytes())]);
        let json = release_json("v0.2.0", &base, true, true, false)
            .replace("\"size\": 4", &format!("\"size\": {}", body.len()));
        let release = parse_latest(&json, "0.1.1").unwrap().unwrap();
        (base, stop, release)
    }

    const PROGRAM: &[u8] = b"MZ-new-launcher";

    fn fixture() -> (tempfile::TempDir, AppPaths, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        let exe = root.path().join("app").join("AnyPlay-Launcher.exe");
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"MZ-old-launcher").unwrap();
        (root, paths, exe)
    }

    /// The unsigned flow these tests are about, whatever the embedded key list holds.
    fn unsigned() -> TrustedKeys {
        TrustedKeys::parse("").unwrap()
    }

    /// [`pending`] in the unsigned flow.
    fn pending_unsigned(paths: &AppPaths, current: &str) -> Option<Staged> {
        pending_with(
            paths,
            current,
            Platform::Windows,
            None,
            OffsetDateTime::now_utc(),
        )
    }

    #[test]
    fn check_reads_the_latest_release_from_the_api() {
        let json = release_json("v0.2.0", "http://x", true, true, true);
        let (base, stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
        let newer = check_with(&base, "o/r", "0.1.1", &unsigned(), Platform::Windows).unwrap();
        assert_eq!(newer.map(|r| r.tag), Some("v0.2.0".into()));
        assert_eq!(
            check_with(&base, "o/r", "0.2.0", &unsigned(), Platform::Windows).unwrap(),
            None
        );
        assert!(matches!(
            check_with(&base, "o/missing", "0.1.1", &unsigned(), Platform::Windows),
            Err(SelfUpdateError::NoRelease)
        ));
        stop.store(true, AtomicOrdering::SeqCst);
    }

    #[test]
    fn a_matching_checksum_stages_the_update() {
        let (_root, paths, exe) = fixture();
        let sums = format!("{} *AnyPlay-Launcher-0.2.0-x64.exe\n", sha_hex(PROGRAM));
        let (_, stop, release) = serve_release(PROGRAM, sums);
        let mut last = 0.0;
        let staged = stage(
            &release,
            &exe,
            &paths,
            &mut |p| last = p,
            &CancelToken::new(),
        )
        .unwrap();
        stop.store(true, AtomicOrdering::SeqCst);
        assert_eq!(staged.version, "0.2.0");
        assert_eq!(fs::read(&staged.path).unwrap(), PROGRAM);
        assert_eq!(
            staged.path,
            updates_dir(&paths).join("AnyPlay-Launcher-0.2.0-x64.exe")
        );
        assert_eq!(last, 1.0);
        // No partial file is left beside it.
        assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 1);
    }

    #[test]
    fn a_wrong_checksum_refuses_the_update() {
        let (_root, paths, exe) = fixture();
        let sums = format!(
            "{} *AnyPlay-Launcher-0.2.0-x64.exe\n",
            sha_hex(b"MZ-something-else")
        );
        let (_, stop, release) = serve_release(PROGRAM, sums);
        let result = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new());
        stop.store(true, AtomicOrdering::SeqCst);
        assert!(matches!(result, Err(SelfUpdateError::Digest)), "{result:?}");
        // Neither the file nor a partial download stays.
        assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
        assert!(pending_unsigned(&paths, "0.1.1").is_none());
    }

    #[test]
    fn a_missing_checksum_refuses_the_update() {
        let (_root, paths, exe) = fixture();
        let sums = format!("{} *AnyPlay-Launcher-0.2.0-Setup.exe\n", sha_hex(PROGRAM));
        let (_, stop, release) = serve_release(PROGRAM, sums);
        let result = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new());
        stop.store(true, AtomicOrdering::SeqCst);
        assert!(
            matches!(result, Err(SelfUpdateError::NoChecksum(_))),
            "{result:?}"
        );
        assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
    }

    #[test]
    fn a_download_that_is_not_a_program_is_refused() {
        let (_root, paths, exe) = fixture();
        let page = b"<html>rate limited</html>";
        let sums = format!("{} *AnyPlay-Launcher-0.2.0-x64.exe\n", sha_hex(page));
        let (_, stop, release) = serve_release(page, sums);
        let result = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new());
        stop.store(true, AtomicOrdering::SeqCst);
        assert!(
            matches!(result, Err(SelfUpdateError::NotAnExecutable)),
            "{result:?}"
        );
        assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
    }

    #[test]
    fn a_folder_that_cannot_be_written_is_reported_before_any_download() {
        let (root, paths, _) = fixture();
        let gone = root
            .path()
            .join("no-such-folder")
            .join("AnyPlay-Launcher.exe");
        // Nothing listens at the asset URLs: reaching the network would be a different error.
        let json = release_json("v0.2.0", "http://127.0.0.1:9", true, true, true);
        let release = parse_latest(&json, "0.1.1").unwrap().unwrap();
        let result = stage(&release, &gone, &paths, &mut |_| {}, &CancelToken::new());
        assert!(
            matches!(result, Err(SelfUpdateError::NotWritable(_))),
            "{result:?}"
        );
    }

    #[test]
    fn a_staged_file_of_the_right_size_is_not_downloaded_again() {
        let (_root, paths, exe) = fixture();
        let sums = format!("{} *AnyPlay-Launcher-0.2.0-x64.exe\n", sha_hex(PROGRAM));
        let (_, stop, release) = serve_release(PROGRAM, sums);
        stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new()).unwrap();
        stop.store(true, AtomicOrdering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        // The server is gone; a second stage must come from disk.
        let again = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new()).unwrap();
        assert_eq!(fs::read(again.path).unwrap(), PROGRAM);
    }

    fn staged_file(paths: &AppPaths, version: &str, body: &[u8]) -> Staged {
        let path = updates_dir(paths).join(exe_name(version));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        Staged {
            version: version.into(),
            path,
            sha256: None,
            platform: Platform::Windows,
        }
    }

    #[test]
    fn the_swap_renames_the_old_exe_and_cleanup_removes_it() {
        let (_root, paths, exe) = fixture();
        let staged = staged_file(&paths, "0.2.0", PROGRAM);
        staged.swap_in(&exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), PROGRAM);
        let old = exe.with_file_name("AnyPlay-Launcher.exe.old");
        assert_eq!(fs::read(&old).unwrap(), b"MZ-old-launcher");
        assert!(!staged.path.exists(), "the staged file is consumed");
        assert!(!exe.with_file_name("AnyPlay-Launcher.exe.new").exists());

        cleanup_for(Platform::Windows, &exe, &paths, "0.2.0");
        assert!(!old.exists());
        assert!(exe.exists());
    }

    #[test]
    fn a_second_swap_works_while_the_first_old_file_is_still_in_use() {
        let (_root, paths, exe) = fixture();
        let dir = exe.parent().unwrap();
        // Keep the first `.old` locked: open without share-delete is not expressible portably,
        // so make it a directory, which can never be removed as a file.
        fs::create_dir(dir.join("AnyPlay-Launcher.exe.old")).unwrap();
        staged_file(&paths, "0.2.0", PROGRAM).swap_in(&exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), PROGRAM);
        assert_eq!(
            fs::read(dir.join("AnyPlay-Launcher.exe.old1")).unwrap(),
            b"MZ-old-launcher"
        );
    }

    #[test]
    fn a_missing_staged_file_leaves_the_exe_alone() {
        let (_root, paths, exe) = fixture();
        let staged = Staged {
            version: "0.2.0".into(),
            path: updates_dir(&paths).join(exe_name("0.2.0")),
            sha256: None,
            platform: Platform::Windows,
        };
        assert!(staged.swap_in(&exe).is_err());
        assert_eq!(fs::read(&exe).unwrap(), b"MZ-old-launcher");
        assert_eq!(fs::read_dir(exe.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn pending_finds_the_newest_staged_update() {
        let (_root, paths, _) = fixture();
        assert!(pending_unsigned(&paths, "0.1.1").is_none());
        staged_file(&paths, "0.1.1", b"MZ");
        staged_file(&paths, "0.1.10", b"MZ");
        staged_file(&paths, "0.1.9", b"MZ");
        fs::write(updates_dir(&paths).join("notes.txt"), b"x").unwrap();
        assert_eq!(
            pending_unsigned(&paths, "0.1.1").map(|s| s.version),
            Some("0.1.10".into())
        );
        assert!(pending_unsigned(&paths, "0.1.10").is_none());
    }

    #[test]
    fn a_staged_exe_of_the_old_name_is_found_and_cleaned_up() {
        let (_root, paths, exe) = fixture();
        let dir = updates_dir(&paths);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("AnyPS5-Launcher-0.2.0-x64.exe"), b"MZ").unwrap();
        fs::write(dir.join("AnyPS5-Launcher-0.1.1-x64.exe"), b"MZ").unwrap();
        assert_eq!(
            pending_unsigned(&paths, "0.1.1").map(|s| s.version),
            Some("0.2.0".into())
        );
        cleanup_for(Platform::Windows, &exe, &paths, "0.2.0");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
    }

    #[test]
    fn cleanup_removes_stale_downloads_and_keeps_newer_ones() {
        let (_root, paths, exe) = fixture();
        staged_file(&paths, "0.1.1", b"MZ");
        let newer = staged_file(&paths, "0.3.0", b"MZ");
        fs::write(
            updates_dir(&paths).join("AnyPlay-Launcher-0.3.0-x64.exe.part"),
            b"MZ",
        )
        .unwrap();
        fs::write(exe.with_file_name("AnyPlay-Launcher.exe.old2"), b"x").unwrap();
        cleanup_for(Platform::Windows, &exe, &paths, "0.2.0");
        let left: Vec<_> = fs::read_dir(updates_dir(&paths))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        assert_eq!(left, vec![newer.path]);
        assert!(!exe.with_file_name("AnyPlay-Launcher.exe.old2").exists());
        assert!(exe.exists(), "the exe itself is never touched");
    }
}

#[cfg(test)]
mod signed_tests;

#[cfg(test)]
mod appimage_tests;
