//! The AnyPlay engine channel (docs/DESIGN.md 3.7): the toolkit of the fork anyplaynet/AnyPS5
//! that carries the network bridge, taken from its moving pre-release `nightly`.
//!
//! The tag never changes, so a new build is told apart by its archive's checksum (and, when the
//! API gave none, by the version the archive names). An archive is installed only when its
//! SHA-256 is in the release's `SHA256SUMS`; a missing entry, a mismatch, or a release that
//! disagrees with its own list refuses the install. The folder is `Toolkit\engine`, a name that is
//! not a release number, so the clean-up of old releases, the pinned list and the fallback to the
//! newest installed release never touch it.

use std::collections::BTreeMap;

use super::*;
use crate::feed;

/// The only repository this channel trusts. It is not a setting.
pub const REPOSITORY: &str = "anyplaynet/AnyPS5";
/// The moving pre-release.
pub const TAG: &str = "nightly";
/// The folder below `Toolkit\`.
pub const FOLDER: &str = "engine";

const SUMS_FILE: &str = "SHA256SUMS";
/// A release answer younger than this is not asked for again, however often the player presses
/// "Check Now". Older ones are revalidated with the ETag, which costs one small reply.
const MIN_AGE: Duration = Duration::from_secs(60);
const MAX_ARCHIVE: u64 = 1024 * 1024 * 1024;
const MAX_UNPACKED: u64 = 3 * 1024 * 1024 * 1024;

/// Where to ask and download from. [`Endpoints::github`] is the real thing.
#[derive(Clone, Debug)]
pub struct Endpoints {
    api: String,
    download: String,
}

impl Endpoints {
    pub fn github() -> Endpoints {
        Endpoints {
            api: API_BASE.to_owned(),
            download: format!("https://github.com/{REPOSITORY}/releases/download/{TAG}"),
        }
    }

    #[cfg(test)]
    fn local(base: &str) -> Endpoints {
        Endpoints {
            api: base.to_owned(),
            download: format!("{base}/dl"),
        }
    }
}

/// One nightly build as the release lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// What the archive's `version.txt` must say: `anyplay-30d0fe9`.
    pub version: String,
    /// The day the archive was uploaded, `2026-10-25` (as far as the API said).
    pub published: String,
    /// The archive for this system. Its `sha256` is the API's digest, when it gave one.
    pub archive: Asset,
}

/// The build in `Toolkit\engine`, read from the files it was installed with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    /// SHA-256 of the archive it was installed from.
    pub archive_sha256: Option<String>,
    /// The day it was built, `2026-10-25`.
    pub built: Option<String>,
}

/// `AnyPlay-toolkit-windows-30d0fe9.zip` gives `30d0fe9` on Windows; nothing else does.
fn short_of(os: Os, name: &str) -> Option<&str> {
    let short = match os {
        Os::Windows => name
            .strip_prefix("AnyPlay-toolkit-windows-")?
            .strip_suffix(".zip")?,
        Os::Linux => name
            .strip_prefix("AnyPlay-toolkit-linux-")?
            .strip_suffix(".tar.gz")?,
        Os::Macos => return None,
    };
    let hex = (7..=40).contains(&short.len())
        && short
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    hex.then_some(short)
}

fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Reads the JSON of `GET /repos/anyplaynet/AnyPS5/releases/tags/nightly` and picks the archive of
/// `os`. The download address is built from the asset's name, never taken from the data.
pub fn parse_build(os: Os, endpoints: &Endpoints, json: &str) -> Result<Build, ToolkitError> {
    if os == Os::Macos {
        return Err(ToolkitError::NoEngine(
            "The AnyPlay engine is not available on macOS yet.".into(),
        ));
    }
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| ToolkitError::Network(format!("bad release data: {e}")))?;
    let assets = value
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or(ToolkitError::NoRelease)?;
    let text =
        |v: &serde_json::Value, key: &str| v.get(key).and_then(|x| x.as_str()).map(str::to_owned);
    let mut found = assets.iter().filter_map(|asset| {
        let name = text(asset, "name")?;
        let short = short_of(os, &name)?.to_owned();
        Some((name, short, asset))
    });
    let (name, short, asset) = found.next().ok_or_else(|| {
        ToolkitError::NoEngine("The nightly build has no engine for this system.".into())
    })?;
    if found.next().is_some() {
        return Err(ToolkitError::NoEngine(
            "The nightly build lists more than one engine for this system.".into(),
        ));
    }
    let sha256 = text(asset, "digest")
        .and_then(|d| d.strip_prefix("sha256:").map(str::to_ascii_lowercase))
        .filter(|hash| valid_sha256(hash));
    let published = text(asset, "created_at")
        .or_else(|| text(&value, "published_at"))
        .map(|stamp| stamp.chars().take(10).collect())
        .unwrap_or_default();
    Ok(Build {
        version: format!("anyplay-{short}"),
        published,
        archive: Asset {
            url: format!("{}/{name}", endpoints.download),
            size: asset.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
            name,
            sha256,
        },
    })
}

/// Reads a `SHA256SUMS` file (`<hex>  <name>` per line, `*` before a binary file's name). Any line
/// that is not one, or a name with two different checksums, makes the whole list unusable.
pub fn parse_sums(text: &str) -> Result<BTreeMap<String, String>, ToolkitError> {
    let bad =
        || ToolkitError::Engine("The checksum list of the nightly build is unreadable.".into());
    let mut sums = BTreeMap::new();
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (hash, name) = line.split_once(char::is_whitespace).ok_or_else(bad)?;
        let name = name.trim().trim_start_matches('*');
        let hash = hash.to_ascii_lowercase();
        if !valid_sha256(&hash) || name.is_empty() {
            return Err(bad());
        }
        if sums
            .insert(name.to_owned(), hash.clone())
            .is_some_and(|old| old != hash)
        {
            return Err(bad());
        }
    }
    Ok(sums)
}

/// The checksum `build`'s archive must have: the list's entry. No entry, or an entry the release's
/// own digest contradicts, is a refusal.
pub fn expected_digest(
    build: &Build,
    sums: &BTreeMap<String, String>,
) -> Result<String, ToolkitError> {
    let name = &build.archive.name;
    let listed = sums.get(name).ok_or_else(|| {
        ToolkitError::Engine(format!(
            "The checksum list of the nightly build has no entry for {name}. Nothing was installed."
        ))
    })?;
    if build
        .archive
        .sha256
        .as_ref()
        .is_some_and(|api| api != listed)
    {
        return Err(ToolkitError::Engine(format!(
            "The checksum list and the release disagree about {name}. Nothing was installed."
        )));
    }
    Ok(listed.clone())
}

/// Whether `build` is not what is installed: no engine yet, a different archive checksum, or (with
/// no checksum on either side) a different version.
pub fn update_available(installed: Option<&Installed>, build: &Build) -> bool {
    let Some(installed) = installed else {
        return true;
    };
    match (&installed.archive_sha256, &build.archive.sha256) {
        (Some(have), Some(offered)) => have != offered,
        _ => installed.version != build.version,
    }
}

/// The folder of the installed engine.
fn folder(paths: &AppPaths) -> PathBuf {
    paths.toolkit_dir().join(FOLDER)
}

/// The installed engine opened as a toolkit; `None` when there is none or it is incomplete.
pub fn open(paths: &AppPaths) -> Option<Toolkit> {
    Toolkit::open(&folder(paths)).ok()
}

/// What `Toolkit\engine` was installed from; `None` when nothing is installed.
pub fn installed(paths: &AppPaths) -> Option<Installed> {
    let root = folder(paths);
    let version = fs::read_to_string(root.join("version.txt")).ok()?;
    let version = version.trim();
    if version.is_empty() {
        return None;
    }
    let record: serde_json::Value = fs::read_to_string(root.join("toolkit.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let text = |key: &str| record.get(key).and_then(|v| v.as_str()).map(str::to_owned);
    Some(Installed {
        version: version.to_owned(),
        archive_sha256: text("archiveSha256").filter(|hash| valid_sha256(hash)),
        built: text("published").filter(|day| !day.is_empty()),
    })
}

/// Asks for the current nightly build. The answer is kept on disk with its ETag: asked again
/// within a minute it costs nothing, later it is revalidated and an unchanged answer is one small
/// reply. A failing network gives the last answer. Blocking: call from a worker.
pub fn check(paths: &AppPaths, endpoints: &Endpoints) -> Result<Build, ToolkitError> {
    check_for(Os::current(), paths, endpoints)
}

fn check_for(os: Os, paths: &AppPaths, endpoints: &Endpoints) -> Result<Build, ToolkitError> {
    let document = feed::fetch(
        &paths.cache_dir().join("Feeds"),
        &feed::Request {
            url: &format!("{}/repos/{REPOSITORY}/releases/tags/{TAG}", endpoints.api),
            name: "engine-nightly.json",
            accept: "application/vnd.github+json",
            max_age: MIN_AGE,
            max_bytes: 4 * 1024 * 1024,
        },
    )?;
    parse_build(os, endpoints, &document.body)
}

/// The checksum list of the release, read over https with a size limit.
fn fetch_sums(endpoints: &Endpoints) -> Result<String, ToolkitError> {
    let mut response = update_agent(Some(Duration::from_secs(30)))
        .get(&format!("{}/{SUMS_FILE}", endpoints.download))
        .header("User-Agent", user_agent())
        .call()
        .map_err(|e| ToolkitError::Network(e.to_string()))?;
    if response.status().as_u16() != 200 {
        return Err(ToolkitError::Network(format!(
            "The checksum list of the nightly build could not be downloaded (HTTP {}).",
            response.status().as_u16()
        )));
    }
    response
        .body_mut()
        .with_config()
        .limit(256 * 1024)
        .read_to_string()
        .map_err(|e| ToolkitError::Network(e.to_string()))
}

/// Downloads `build`, checks it against the release's `SHA256SUMS` and puts it in `Toolkit\engine`.
/// The old engine stays until the new one is complete and comes back if the swap fails. Blocking.
pub fn install(
    build: &Build,
    paths: &AppPaths,
    endpoints: &Endpoints,
    progress: &mut dyn FnMut(DownloadStage, f64),
    cancel: &CancelToken,
) -> Result<Toolkit, ToolkitError> {
    install_for(Os::current(), build, paths, endpoints, progress, cancel)
}

fn install_for(
    os: Os,
    build: &Build,
    paths: &AppPaths,
    endpoints: &Endpoints,
    progress: &mut dyn FnMut(DownloadStage, f64),
    cancel: &CancelToken,
) -> Result<Toolkit, ToolkitError> {
    let expected = expected_digest(build, &parse_sums(&fetch_sums(endpoints)?)?)?;
    let base = paths.toolkit_dir();
    let partial = base.join(format!("{FOLDER}.partial"));
    crate::fsx::remove_tree(&partial)?;
    fs::create_dir_all(&partial)?;
    let result = unpack_into(os, build, &expected, &partial, progress, cancel);
    if let Err(error) = result {
        let _ = crate::fsx::remove_tree(&partial);
        return Err(error);
    }
    if let Err(error) = swap_in(&base, FOLDER, &partial) {
        let _ = crate::fsx::remove_tree(&partial);
        return Err(error);
    }
    Toolkit::open_for(os, &base.join(FOLDER))
}

fn unpack_into(
    os: Os,
    build: &Build,
    expected: &str,
    partial: &Path,
    progress: &mut dyn FnMut(DownloadStage, f64),
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    let archive_path = partial.join("download.bin");
    let asset = Asset {
        sha256: Some(expected.to_owned()),
        ..build.archive.clone()
    };
    download_capped(
        &update_agent(None),
        &asset,
        &archive_path,
        Some(MAX_ARCHIVE),
        &mut |share| progress(DownloadStage::Libraries, share),
        cancel,
    )
    .map_err(|error| match error {
        ToolkitError::Digest => ToolkitError::Engine(format!(
            "{} does not match its checksum. Nothing was installed.",
            build.archive.name
        )),
        other => other,
    })?;
    progress(DownloadStage::Extracting, 0.0);
    match os {
        Os::Windows => extract_zip(&archive_path, partial, cancel)?,
        _ => extract_tar_gz(&archive_path, partial, cancel)?,
    }
    fs::remove_file(&archive_path)?;
    let said = fs::read_to_string(partial.join("version.txt")).unwrap_or_default();
    if said.trim() != build.version {
        return Err(ToolkitError::Engine(format!(
            "The archive says {:?}, the release says {:?}. Nothing was installed.",
            said.trim(),
            build.version
        )));
    }
    let relinker = partial.join(os.relinker_file());
    if !relinker.is_file() {
        return Err(ToolkitError::Missing(os.relinker_file().into()));
    }
    make_executable(&relinker)?;
    let relinker_sha256 = hex(&Sha256::digest(fs::read(&relinker)?));
    let record = serde_json::json!({
        "tag": TAG,
        "channel": "engine",
        "version": build.version,
        "published": build.published,
        "source": build.archive.url,
        "installedAt": now_text(),
        "archiveSha256": expected,
        "relinkerSha256": relinker_sha256,
        "libsSha256": expected,
    });
    fs::write(
        partial.join("toolkit.json"),
        serde_json::to_string_pretty(&record).unwrap_or_default(),
    )?;
    // The folder must be usable, or the player would be left with a broken toolkit.
    Toolkit::open_for(os, partial)?;
    progress(DownloadStage::Finishing, 1.0);
    Ok(())
}

/// Writes one archive entry below `dest`, within what is left of the unpacked-size budget.
fn put(
    dest: &Path,
    name: &str,
    reader: &mut dyn Read,
    budget: &mut u64,
) -> Result<(), ToolkitError> {
    let target = dest.join(name.replace('/', std::path::MAIN_SEPARATOR_STR));
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = fs::File::create(&target)?;
    let copied = io::copy(&mut reader.take(budget.saturating_add(1)), &mut out)
        .map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
    if copied > *budget {
        return Err(ToolkitError::TooLarge);
    }
    *budget -= copied;
    Ok(())
}

/// A normalised entry name; `None` for the folder itself, an error for one that leaves it.
fn entry_name(raw: &str) -> Result<Option<String>, ToolkitError> {
    let name = safe_entry_name(raw)
        .ok_or_else(|| ToolkitError::BadArchive(format!("unsafe entry name {raw}")))?;
    let name = name.trim_start_matches("./").trim_end_matches('/');
    if name.is_empty() || name == "." {
        return Ok(None);
    }
    if name.split('/').any(str::is_empty) {
        return Err(ToolkitError::BadArchive(format!("unsafe entry name {raw}")));
    }
    Ok(Some(name.to_owned()))
}

fn extract_zip(zip_path: &Path, dest: &Path, cancel: &CancelToken) -> Result<(), ToolkitError> {
    let mut archive = zip::ZipArchive::new(fs::File::open(zip_path)?)
        .map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
    // Reject the whole archive before writing anything if one entry is unsafe.
    let mut wanted = Vec::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
        let name = entry_name(entry.name())?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(ToolkitError::BadArchive(format!(
                "{} is a link",
                entry.name()
            )));
        }
        if let (false, Some(name)) = (entry.is_dir(), name) {
            wanted.push((index, name));
        }
    }
    if wanted.is_empty() {
        return Err(ToolkitError::BadArchive("the archive is empty".into()));
    }
    let mut budget = MAX_UNPACKED;
    for (index, name) in wanted {
        if cancel.is_cancelled() {
            return Err(ToolkitError::Cancelled);
        }
        let mut entry = archive
            .by_index(index)
            .map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
        put(dest, &name, &mut entry, &mut budget)?;
    }
    Ok(())
}

/// Reads one 512-byte block; `false` at a clean end of the stream.
fn read_block(reader: &mut dyn Read, block: &mut [u8; 512]) -> Result<bool, ToolkitError> {
    let mut filled = 0;
    while filled < block.len() {
        match reader.read(&mut block[filled..]) {
            Ok(0) if filled == 0 => return Ok(false),
            Ok(0) => return Err(ToolkitError::BadArchive("the archive is cut short".into())),
            Ok(n) => filled += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(ToolkitError::BadArchive(e.to_string())),
        }
    }
    Ok(true)
}

fn skip(reader: &mut dyn Read, count: u64) -> Result<(), ToolkitError> {
    let skipped = io::copy(&mut reader.take(count), &mut io::sink())
        .map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
    if skipped == count {
        Ok(())
    } else {
        Err(ToolkitError::BadArchive("the archive is cut short".into()))
    }
}

fn octal(field: &[u8]) -> Result<u64, ToolkitError> {
    let bad = || ToolkitError::BadArchive("a tar header is damaged".into());
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        return Err(bad());
    }
    let text: String = field
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as char)
        .collect();
    let text = text.trim();
    if text.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(text, 8).map_err(|_| bad())
}

fn c_string(field: &[u8]) -> String {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// A gzipped tar of plain files and folders, the only thing the nightly build packs. Links,
/// devices and extended headers are refused: the reader stays small enough to be read whole.
fn extract_tar_gz(path: &Path, dest: &Path, cancel: &CancelToken) -> Result<(), ToolkitError> {
    let mut reader = flate2::read::GzDecoder::new(io::BufReader::new(fs::File::open(path)?));
    let mut budget = MAX_UNPACKED;
    let mut long_name: Option<String> = None;
    let mut files = 0usize;
    let mut header = [0u8; 512];
    while read_block(&mut reader, &mut header)? {
        if cancel.is_cancelled() {
            return Err(ToolkitError::Cancelled);
        }
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let stored = octal(&header[148..156])?;
        let sum: u64 = header
            .iter()
            .enumerate()
            .map(|(i, b)| {
                if (148..156).contains(&i) {
                    32
                } else {
                    u64::from(*b)
                }
            })
            .sum();
        if sum != stored {
            return Err(ToolkitError::BadArchive("a tar header is damaged".into()));
        }
        let size = octal(&header[124..136])?;
        let padding = (512 - size % 512) % 512;
        let kind = header[156];
        if kind == b'L' {
            if size > 4096 {
                return Err(ToolkitError::BadArchive("a tar name is too long".into()));
            }
            let mut name = vec![0u8; size as usize];
            reader
                .read_exact(&mut name)
                .map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
            long_name = Some(c_string(&name));
            skip(&mut reader, padding)?;
            continue;
        }
        let mut name = c_string(&header[..100]);
        if &header[257..263] == b"ustar\0" && header[345] != 0 {
            name = format!("{}/{name}", c_string(&header[345..500]));
        }
        let name = long_name.take().unwrap_or(name);
        match kind {
            b'5' => {
                skip(&mut reader, size + padding)?;
                entry_name(&name)?;
            }
            b'0' | 0 => {
                let Some(name) = entry_name(&name)? else {
                    return Err(ToolkitError::BadArchive("a file without a name".into()));
                };
                put(dest, &name, &mut (&mut reader).take(size), &mut budget)?;
                files += 1;
                skip(&mut reader, padding)?;
            }
            _ => {
                return Err(ToolkitError::BadArchive(format!(
                    "{name} is not a plain file or folder"
                )));
            }
        }
    }
    if files == 0 {
        return Err(ToolkitError::BadArchive("the archive is empty".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    const SHORT: &str = "30d0fe9";
    const NEXT: &str = "a1b2c3d";

    fn release(short: &str, windows_digest: &str, linux_digest: &str) -> String {
        format!(
            r#"{{"tag_name":"nightly","prerelease":true,"published_at":"2026-10-20T10:00:00Z","assets":[
            {{"name":"AnyPlay-toolkit-linux-{short}.tar.gz","size":10,"created_at":"2026-10-25T16:48:05Z","digest":"sha256:{linux_digest}"}},
            {{"name":"AnyPlay-toolkit-windows-{short}.zip","size":20,"created_at":"2026-10-25T16:48:05Z","digest":"sha256:{windows_digest}"}},
            {{"name":"prx-windows-nightly.zip","size":30,"digest":"sha256:{windows_digest}"}},
            {{"name":"relinker-nightly","size":40}},
            {{"name":"SHA256SUMS","size":553}}]}}"#
        )
    }

    fn hash(c: char) -> String {
        c.to_string().repeat(64)
    }

    #[test]
    fn each_system_gets_its_own_archive() {
        let endpoints = Endpoints::github();
        let json = release(SHORT, &hash('a'), &hash('b'));
        let windows = parse_build(Os::Windows, &endpoints, &json).unwrap();
        assert_eq!(windows.archive.name, "AnyPlay-toolkit-windows-30d0fe9.zip");
        assert_eq!(
            windows.archive.url,
            "https://github.com/anyplaynet/AnyPS5/releases/download/nightly/AnyPlay-toolkit-windows-30d0fe9.zip"
        );
        assert_eq!(windows.archive.sha256, Some(hash('a')));
        assert_eq!(windows.version, "anyplay-30d0fe9");
        assert_eq!(windows.published, "2026-10-25");
        let linux = parse_build(Os::Linux, &endpoints, &json).unwrap();
        assert_eq!(linux.archive.name, "AnyPlay-toolkit-linux-30d0fe9.tar.gz");
        assert_eq!(linux.archive.sha256, Some(hash('b')));
    }

    #[test]
    fn macos_has_no_engine_and_says_so() {
        let json = release(SHORT, &hash('a'), &hash('b'));
        let error = parse_build(Os::Macos, &Endpoints::github(), &json).unwrap_err();
        assert!(error.to_string().contains("macOS"), "{error}");
    }

    #[test]
    fn a_release_without_an_archive_for_this_system_is_refused() {
        let json = r#"{"assets":[{"name":"AnyPlay-toolkit-windows-30d0fe9.zip"},{"name":"prx-linux-nightly.zip"}]}"#;
        assert!(parse_build(Os::Linux, &Endpoints::github(), json).is_err());
        assert!(parse_build(Os::Windows, &Endpoints::github(), r#"{"assets":[]}"#).is_err());
    }

    #[test]
    fn two_archives_for_one_system_are_refused() {
        let json = r#"{"assets":[{"name":"AnyPlay-toolkit-linux-30d0fe9.tar.gz"},{"name":"AnyPlay-toolkit-linux-a1b2c3d.tar.gz"}]}"#;
        assert!(parse_build(Os::Linux, &Endpoints::github(), json).is_err());
    }

    #[test]
    fn a_name_that_is_not_a_commit_is_not_an_archive() {
        for name in [
            "AnyPlay-toolkit-linux-../x.tar.gz",
            "AnyPlay-toolkit-linux-30D0FE9.tar.gz",
            "AnyPlay-toolkit-linux-abc.tar.gz",
            "AnyPlay-toolkit-linux-30d0fe9.zip",
        ] {
            assert_eq!(short_of(Os::Linux, name), None, "{name}");
        }
        assert_eq!(
            short_of(Os::Linux, "AnyPlay-toolkit-linux-30d0fe9.tar.gz"),
            Some("30d0fe9")
        );
    }

    #[test]
    fn a_bad_api_digest_is_ignored_not_trusted() {
        let json = release(SHORT, "short", &hash('b'));
        let build = parse_build(Os::Windows, &Endpoints::github(), &json).unwrap();
        assert_eq!(build.archive.sha256, None);
    }

    fn build_with(sha: Option<String>) -> Build {
        Build {
            version: format!("anyplay-{SHORT}"),
            published: "2026-10-25".into(),
            archive: Asset {
                name: format!("AnyPlay-toolkit-windows-{SHORT}.zip"),
                url: "https://example.invalid/x".into(),
                size: 1,
                sha256: sha,
            },
        }
    }

    #[test]
    fn the_checksum_list_is_read_strictly() {
        let sums = parse_sums(&format!(
            "{}  AnyPlay-toolkit-windows-30d0fe9.zip\n{} *relinker-nightly\n\n",
            hash('a'),
            hash('B')
        ))
        .unwrap();
        assert_eq!(sums["AnyPlay-toolkit-windows-30d0fe9.zip"], hash('a'));
        assert_eq!(sums["relinker-nightly"], hash('b'));
        for bad in [
            "nonsense\n",
            "abc  file\n",
            &format!("{}  \n", hash('a')),
            &format!("{}  f\n{}  f\n", hash('a'), hash('b')),
        ] {
            assert!(parse_sums(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_missing_entry_refuses_with_its_name() {
        let sums = parse_sums(&format!("{}  other.zip\n", hash('a'))).unwrap();
        let error = expected_digest(&build_with(None), &sums).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("no entry for AnyPlay-toolkit-windows-30d0fe9.zip"),
            "{error}"
        );
    }

    #[test]
    fn a_release_that_contradicts_its_list_is_refused() {
        let name = format!("AnyPlay-toolkit-windows-{SHORT}.zip");
        let sums = parse_sums(&format!("{}  {name}\n", hash('a'))).unwrap();
        assert_eq!(
            expected_digest(&build_with(Some(hash('a'))), &sums).unwrap(),
            hash('a')
        );
        assert_eq!(
            expected_digest(&build_with(None), &sums).unwrap(),
            hash('a')
        );
        assert!(expected_digest(&build_with(Some(hash('c'))), &sums).is_err());
    }

    fn installed_with(version: &str, sha: Option<String>) -> Installed {
        Installed {
            version: version.into(),
            archive_sha256: sha,
            built: None,
        }
    }

    #[test]
    fn a_new_nightly_is_told_by_its_checksum_not_its_tag() {
        let build = build_with(Some(hash('a')));
        assert!(update_available(None, &build), "nothing installed yet");
        assert!(!update_available(
            Some(&installed_with("anyplay-30d0fe9", Some(hash('a')))),
            &build
        ));
        // Same commit, rebuilt: another archive.
        assert!(update_available(
            Some(&installed_with("anyplay-30d0fe9", Some(hash('b')))),
            &build
        ));
    }

    #[test]
    fn without_checksums_the_version_decides() {
        let build = build_with(None);
        assert!(!update_available(
            Some(&installed_with("anyplay-30d0fe9", None)),
            &build
        ));
        assert!(update_available(
            Some(&installed_with("anyplay-0000000", None)),
            &build
        ));
        let with_sha = build_with(Some(hash('a')));
        assert!(
            update_available(Some(&installed_with("anyplay-0000000", None)), &with_sha),
            "an install without a record of its checksum is compared by version"
        );
    }

    // --- the network --------------------------------------------------------------------------

    struct Server {
        base: String,
        hits: Arc<AtomicUsize>,
    }

    /// Serves `files` by path and counts the requests; no ETag.
    fn serve(files: Vec<(String, Vec<u8>)>) -> Server {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", server.server_addr());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                counter.fetch_add(1, AtomicOrdering::SeqCst);
                let found = files.iter().find(|(path, _)| path == request.url());
                let _ = match found {
                    Some((_, body)) => {
                        request.respond(tiny_http::Response::from_data(body.clone()))
                    }
                    None => request.respond(tiny_http::Response::empty(404)),
                };
            }
        });
        Server { base, hits }
    }

    fn paths() -> (tempfile::TempDir, AppPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        (dir, paths)
    }

    fn files_of(short: &str) -> Vec<(&'static str, Vec<u8>)> {
        let mut files: Vec<(&'static str, Vec<u8>)> = vec![
            (
                Os::current().relinker_file(),
                if cfg!(target_os = "macos") {
                    b"\xcf\xfa\xed\xfe".to_vec()
                } else {
                    b"RELINKER".to_vec()
                },
            ),
            ("libs/a.prx", b"AAA".to_vec()),
            ("version.txt", format!("anyplay-{short}\n").into_bytes()),
        ];
        let dlls: Vec<&'static str> = Os::current().runtime_libs().to_vec();
        for dll in dlls {
            files.push((
                Box::leak(format!("libs/{dll}").into_boxed_str()),
                b"D".to_vec(),
            ));
        }
        files
    }

    fn tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar = Vec::new();
        for (name, data) in files {
            tar.extend(tar_entry(name, data, b'0'));
        }
        tar.extend([0u8; 1024]);
        let mut out = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        out.write_all(&tar).unwrap();
        out.finish().unwrap()
    }

    fn tar_entry(name: &str, data: &[u8], kind: u8) -> Vec<u8> {
        let mut header = [0u8; 512];
        header[..name.len().min(100)].copy_from_slice(&name.as_bytes()[..name.len().min(100)]);
        header[100..107].copy_from_slice(b"0000644");
        header[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        header[156] = kind;
        header[257..265].copy_from_slice(b"ustar  \0");
        header[148..156].copy_from_slice(b"        ");
        let sum: u64 = header.iter().map(|b| u64::from(*b)).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        let mut out = header.to_vec();
        out.extend(data);
        out.extend(vec![0u8; (512 - data.len() % 512) % 512]);
        out
    }

    /// The archive for this system, its name and the checksum list that goes with it.
    fn archive_for_this_system(files: &[(&str, &[u8])], short: &str) -> (String, Vec<u8>, String) {
        let (name, bytes) = if Os::current() == Os::Windows {
            (
                format!("AnyPlay-toolkit-windows-{short}.zip"),
                super::super::tests::build_zip(files),
            )
        } else {
            (
                format!("AnyPlay-toolkit-linux-{short}.tar.gz"),
                tar_gz(files),
            )
        };
        let sums = format!("{}  {name}\n", hex(&Sha256::digest(&bytes)));
        (name, bytes, sums)
    }

    fn build_of(name: &str, bytes: &[u8], short: &str) -> Build {
        Build {
            version: format!("anyplay-{short}"),
            published: "2026-10-25".into(),
            archive: Asset {
                name: name.into(),
                url: String::new(),
                size: bytes.len() as u64,
                sha256: None,
            },
        }
    }

    fn install_from(
        paths: &AppPaths,
        files: &[(&str, &[u8])],
        short: &str,
        sums: Option<String>,
        archive: Option<Vec<u8>>,
    ) -> Result<Toolkit, ToolkitError> {
        let (name, bytes, own_sums) = archive_for_this_system(files, short);
        let bytes = archive.unwrap_or(bytes);
        let server = serve(vec![
            (format!("/dl/{name}"), bytes.clone()),
            (
                "/dl/SHA256SUMS".into(),
                sums.unwrap_or(own_sums).into_bytes(),
            ),
        ]);
        let endpoints = Endpoints::local(&server.base);
        let mut build = build_of(&name, &bytes, short);
        build.archive.url = format!("{}/{name}", endpoints.download);
        install_for(
            Os::current(),
            &build,
            paths,
            &endpoints,
            &mut |_, _| {},
            &CancelToken::new(),
        )
    }

    fn borrowed(files: &[(&'static str, Vec<u8>)]) -> Vec<(&'static str, Vec<u8>)> {
        files.to_vec()
    }

    // macOS has no engine build; these install one for this system.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_verified_archive_is_installed_with_its_record() {
        let (_dir, paths) = paths();
        let files = borrowed(&files_of(SHORT));
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        let toolkit = install_from(&paths, &refs, SHORT, None, None).unwrap();
        assert_eq!(toolkit.version(), "anyplay-30d0fe9");
        let installed = installed(&paths).unwrap();
        assert_eq!(installed.version, "anyplay-30d0fe9");
        assert_eq!(installed.built.as_deref(), Some("2026-10-25"));
        assert!(installed.archive_sha256.is_some());
        assert!(open(&paths).is_some());
        assert!(!paths.toolkit_dir().join("engine.partial").exists());
        assert!(!paths.toolkit_dir().join("engine.previous").exists());
    }

    // macOS has no engine build; these install one for this system.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn the_engine_folder_is_not_a_release() {
        let (_dir, paths) = paths();
        let files = borrowed(&files_of(SHORT));
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        install_from(&paths, &refs, SHORT, None, None).unwrap();
        assert!(installed_tags(&paths).is_empty());
        assert!(Toolkit::locate_with(&paths, None, None).is_none());
    }

    // macOS has no engine build; these install one for this system.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_wrong_checksum_installs_nothing_and_keeps_the_old_engine() {
        let (_dir, paths) = paths();
        let old = borrowed(&files_of(SHORT));
        let refs: Vec<(&str, &[u8])> = old.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        install_from(&paths, &refs, SHORT, None, None).unwrap();
        let next = borrowed(&files_of(NEXT));
        let refs: Vec<(&str, &[u8])> = next.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        let (name, _, _) = archive_for_this_system(&refs, NEXT);
        let lying = format!("{}  {name}\n", hash('0'));
        let error = install_from(&paths, &refs, NEXT, Some(lying), None).unwrap_err();
        assert!(
            error.to_string().contains("does not match its checksum"),
            "{error}"
        );
        assert_eq!(installed(&paths).unwrap().version, "anyplay-30d0fe9");
        assert!(open(&paths).is_some());
        assert!(!paths.toolkit_dir().join("engine.partial").exists());
    }

    // macOS has no engine build; these install one for this system.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_swapped_archive_is_refused() {
        let (_dir, paths) = paths();
        let good = borrowed(&files_of(SHORT));
        let refs: Vec<(&str, &[u8])> = good.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        let (_, other, _) = archive_for_this_system(&refs, SHORT);
        let evil = borrowed(&files_of(NEXT));
        let evil_refs: Vec<(&str, &[u8])> = evil.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        let (_, evil_bytes, _) = archive_for_this_system(&evil_refs, NEXT);
        // The list vouches for the good archive; the server hands out another.
        let _ = other;
        let error = install_from(&paths, &refs, SHORT, None, Some(evil_bytes)).unwrap_err();
        assert!(error.to_string().contains("does not match"), "{error}");
        assert!(installed(&paths).is_none());
    }

    // macOS has no engine build; these install one for this system.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_list_without_the_archive_installs_nothing() {
        let (_dir, paths) = paths();
        let files = borrowed(&files_of(SHORT));
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        let sums = format!("{}  something-else.zip\n", hash('a'));
        let error = install_from(&paths, &refs, SHORT, Some(sums), None).unwrap_err();
        assert!(error.to_string().contains("no entry for"), "{error}");
        assert!(!paths.toolkit_dir().join("engine").exists());
        assert!(!paths.toolkit_dir().join("engine.partial").exists());
    }

    // macOS has no engine build; these install one for this system.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn an_archive_naming_another_build_is_refused_and_the_old_engine_stays() {
        let (_dir, paths) = paths();
        let old = borrowed(&files_of(SHORT));
        let refs: Vec<(&str, &[u8])> = old.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        install_from(&paths, &refs, SHORT, None, None).unwrap();
        // Release says NEXT, the archive's version.txt says SHORT.
        let (name, bytes, sums) = archive_for_this_system(&refs, SHORT);
        let renamed = name.replace(SHORT, NEXT);
        let sums = sums.replace(SHORT, NEXT);
        let server = serve(vec![
            (format!("/dl/{renamed}"), bytes.clone()),
            ("/dl/SHA256SUMS".into(), sums.into_bytes()),
        ]);
        let endpoints = Endpoints::local(&server.base);
        let mut build = build_of(&renamed, &bytes, NEXT);
        build.archive.url = format!("{}/{renamed}", endpoints.download);
        let error = install_for(
            Os::current(),
            &build,
            &paths,
            &endpoints,
            &mut |_, _| {},
            &CancelToken::new(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("Nothing was installed"),
            "{error}"
        );
        assert_eq!(installed(&paths).unwrap().version, "anyplay-30d0fe9");
    }

    // macOS has no engine build; these install one for this system.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_newer_build_replaces_the_old_one() {
        let (_dir, paths) = paths();
        for short in [SHORT, NEXT] {
            let files = borrowed(&files_of(short));
            let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
            install_from(&paths, &refs, short, None, None).unwrap();
        }
        assert_eq!(installed(&paths).unwrap().version, "anyplay-a1b2c3d");
        assert!(!paths.toolkit_dir().join("engine.previous").exists());
    }

    // macOS has no engine build; these install one for this system.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_failed_swap_brings_the_old_engine_back() {
        let (_dir, paths) = paths();
        let files = borrowed(&files_of(SHORT));
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        install_from(&paths, &refs, SHORT, None, None).unwrap();
        // The finished folder is missing, so moving it into place fails after the old one moved aside.
        let gone = paths.toolkit_dir().join("engine.partial");
        assert!(swap_in(&paths.toolkit_dir(), FOLDER, &gone).is_err());
        assert_eq!(installed(&paths).unwrap().version, "anyplay-30d0fe9");
        assert!(open(&paths).is_some());
    }

    // macOS has no engine build; these install one for this system.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn a_crash_between_the_two_moves_is_repaired() {
        let (_dir, paths) = paths();
        let files = borrowed(&files_of(SHORT));
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        install_from(&paths, &refs, SHORT, None, None).unwrap();
        let base = paths.toolkit_dir();
        fs::rename(base.join(FOLDER), base.join("engine.previous")).unwrap();
        let fresh = base.join("engine.partial");
        write_tree(&fresh, &borrowed(&files_of(NEXT)));
        swap_in(&base, FOLDER, &fresh).unwrap();
        assert_eq!(installed(&paths).unwrap().version, "anyplay-a1b2c3d");
    }

    fn write_tree(root: &Path, files: &[(&'static str, Vec<u8>)]) {
        for (name, data) in files {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, data).unwrap();
        }
    }

    #[test]
    fn unsafe_tar_entries_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.tar.gz");
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        for name in ["../evil", "/abs", "libs/../../evil"] {
            fs::write(&archive, tar_gz(&[(name, b"x")])).unwrap();
            assert!(
                extract_tar_gz(&archive, &out, &CancelToken::new()).is_err(),
                "{name}"
            );
        }
        assert!(!dir.path().join("evil").exists());
    }

    #[test]
    fn tar_links_and_damage_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.tar.gz");
        let out = dir.path().join("out");
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar_entry("link", b"", b'2')).unwrap();
        gz.write_all(&[0u8; 1024]).unwrap();
        fs::write(&archive, gz.finish().unwrap()).unwrap();
        assert!(extract_tar_gz(&archive, &out, &CancelToken::new()).is_err());
        let mut broken = tar_entry("f", b"data", b'0');
        broken[10] ^= 0x55;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&broken).unwrap();
        fs::write(&archive, gz.finish().unwrap()).unwrap();
        assert!(extract_tar_gz(&archive, &out, &CancelToken::new()).is_err());
    }

    #[test]
    fn tar_reads_folders_dot_prefixes_and_long_names() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.tar.gz");
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        let long = format!("libs/{}.prx", "n".repeat(120));
        let mut tar = Vec::new();
        tar.extend(tar_entry("./", b"", b'5'));
        tar.extend(tar_entry("./version.txt", b"anyplay-1234567\n", b'0'));
        tar.extend(tar_entry(
            "././@LongLink",
            format!("{long}\0").as_bytes(),
            b'L',
        ));
        tar.extend(tar_entry("truncated", b"long", b'0'));
        tar.extend([0u8; 1024]);
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gz.write_all(&tar).unwrap();
        fs::write(&archive, gz.finish().unwrap()).unwrap();
        extract_tar_gz(&archive, &out, &CancelToken::new()).unwrap();
        assert_eq!(
            fs::read_to_string(out.join("version.txt")).unwrap(),
            "anyplay-1234567\n"
        );
        assert_eq!(fs::read(out.join(&long)).unwrap(), b"long");
    }

    #[test]
    fn zip_links_and_escapes_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.zip");
        let out = dir.path().join("out");
        fs::create_dir_all(&out).unwrap();
        fs::write(
            &archive,
            super::super::tests::build_zip(&[("../evil", b"x")]),
        )
        .unwrap();
        assert!(extract_zip(&archive, &out, &CancelToken::new()).is_err());
        assert!(!dir.path().join("evil").exists());
    }

    // --- asking -------------------------------------------------------------------------------

    #[test]
    fn the_release_is_not_asked_for_again_within_a_minute() {
        let (_dir, paths) = paths();
        let json = release(SHORT, &hash('a'), &hash('b'));
        let server = serve(vec![(
            "/repos/anyplaynet/AnyPS5/releases/tags/nightly".into(),
            json.into_bytes(),
        )]);
        let endpoints = Endpoints::local(&server.base);
        let os = if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        };
        let first = check_for(os, &paths, &endpoints).unwrap();
        let second = check_for(os, &paths, &endpoints).unwrap();
        assert_eq!(first, second);
        assert_eq!(server.hits.load(AtomicOrdering::SeqCst), 1);
    }

    #[test]
    fn offline_the_last_answer_still_serves() {
        let (_dir, paths) = paths();
        let json = release(SHORT, &hash('a'), &hash('b'));
        let os = if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        };
        let server = serve(vec![(
            "/repos/anyplaynet/AnyPS5/releases/tags/nightly".into(),
            json.into_bytes(),
        )]);
        let endpoints = Endpoints::local(&server.base);
        check_for(os, &paths, &endpoints).unwrap();
        // Age the copy past the minute, then ask a server that is not there.
        let stored = paths.cache_dir().join("Feeds").join("engine-nightly.json");
        let old = std::time::SystemTime::now() - Duration::from_secs(3600);
        fs::File::options()
            .write(true)
            .open(&stored)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let gone = Endpoints::local("http://127.0.0.1:1");
        assert!(check_for(os, &paths, &gone).is_ok());
    }
}
