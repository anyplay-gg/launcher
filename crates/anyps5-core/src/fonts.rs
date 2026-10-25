//! The system fonts a game gets: the player's folder, else the Noto substitutes the launcher
//! carries and keeps in its data folder (`Fonts/noto-<version>`).
//!
//! The engine reads font files from the folder in `ANYPS5_SYSTEM_FONTS` (names in
//! [`BUNDLED_FILES`] and [`CJK_FILES`]). Latin, Mono, Thai and Arabic are small and travel inside
//! the launcher binary: the caller hands them in as a [`Bundle`], [`resolve`] writes them once.
//! Japanese, Chinese and Korean are one 100 MB download, fetched on request or in the background
//! the first time a game starts without them ([`download_cjk_in_background`]).
//!
//! Interface: [`resolve`], [`ensure_bundled`], [`cjk_state`], [`install_cjk`],
//! [`download_cjk_in_background`].

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::CancelToken;
use crate::paths::AppPaths;
use crate::toolkit::{Asset, ToolkitError, download_capped, update_agent};

/// The files the launcher carries, by the names the engine reads.
pub const BUNDLED_FILES: [&str; 20] = [
    "NotoSans-Light.ttf",
    "NotoSans-Regular.ttf",
    "NotoSans-Medium.ttf",
    "NotoSans-Bold.ttf",
    "NotoSans-LightItalic.ttf",
    "NotoSans-Italic.ttf",
    "NotoSans-MediumItalic.ttf",
    "NotoSans-BoldItalic.ttf",
    "NotoSansMono-Light.ttf",
    "NotoSansMono-Regular.ttf",
    "NotoSansMono-Medium.ttf",
    "NotoSansMono-Bold.ttf",
    "NotoSansThai-Light.ttf",
    "NotoSansThai-Regular.ttf",
    "NotoSansThai-Medium.ttf",
    "NotoSansThai-Bold.ttf",
    "NotoSansArabic-Light.ttf",
    "NotoSansArabic-Regular.ttf",
    "NotoSansArabic-Medium.ttf",
    "NotoSansArabic-Bold.ttf",
];

/// The Japanese, Chinese and Korean files of the download.
pub const CJK_FILES: [&str; 4] = [
    "NotoSansCJK-Light.ttc",
    "NotoSansCJK-Regular.ttc",
    "NotoSansCJK-Medium.ttc",
    "NotoSansCJK-Bold.ttc",
];

/// Written beside the fonts last: the licence the fonts are distributed under.
pub const LICENCE_FILE: &str = "OFL.txt";

/// Written after the four CJK files; holds the SHA-256 of the archive they came from.
const CJK_MARKER: &str = "cjk.complete";

/// The pinned CJK archive (Noto CJK release Sans2.004, OTC for all weights).
pub const CJK_URL: &str =
    "https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/03_NotoSansCJK-OTC.zip";
pub const CJK_SIZE: u64 = 104_680_673;
pub const CJK_SHA256: &str = "528f4e1b25ff3badb0321b38d015d954c4c0de926c7830ef50e4a1948f6a3eed";

/// The bundled files. `version` names the folder: change it whenever a file changes.
#[derive(Clone, Copy, Debug)]
pub struct Bundle<'a> {
    pub version: &'a str,
    pub files: &'a [(&'a str, &'a [u8])],
}

/// Where the CJK archive comes from (the pinned values by default; tests serve their own).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CjkSource {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

impl Default for CjkSource {
    fn default() -> Self {
        Self {
            url: CJK_URL.to_owned(),
            sha256: CJK_SHA256.to_owned(),
            size: CJK_SIZE,
        }
    }
}

/// Whether the managed folder holds the Japanese, Chinese and Korean fonts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CjkState {
    Present,
    Missing,
    Downloading,
}

/// The folder the bundled fonts are written to.
pub fn managed_dir(paths: &AppPaths, bundle: &Bundle<'_>) -> PathBuf {
    paths
        .data
        .join("Fonts")
        .join(format!("noto-{}", bundle.version))
}

/// The folder `ANYPS5_SYSTEM_FONTS` gets: the player's folder, else the managed one with the
/// bundled files written. `None` only when they cannot be written (the game then starts without
/// system fonts, as before).
pub fn resolve(paths: &AppPaths, bundle: &Bundle<'_>, picked: Option<&Path>) -> Option<PathBuf> {
    if let Some(folder) = picked {
        return Some(folder.to_path_buf());
    }
    match ensure_bundled(paths, bundle) {
        Ok(dir) => Some(dir),
        Err(error) => {
            tracing::warn!("cannot write the built-in fonts: {error}");
            None
        }
    }
}

/// Writes the bundled files into the managed folder and returns it. Files that are there with
/// the right length are left alone; the others are written beside their name and renamed.
pub fn ensure_bundled(paths: &AppPaths, bundle: &Bundle<'_>) -> io::Result<PathBuf> {
    for name in BUNDLED_FILES {
        if !bundle.files.iter().any(|(n, _)| *n == name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("the bundle lacks {name}"),
            ));
        }
    }
    let dir = managed_dir(paths, bundle);
    let complete = |(name, data): &(&str, &[u8])| {
        fs::metadata(dir.join(name)).is_ok_and(|m| m.len() == data.len() as u64)
    };
    if bundle.files.iter().all(complete) {
        return Ok(dir);
    }
    fs::create_dir_all(&dir)?;
    for entry in bundle.files.iter().filter(|e| !complete(e)) {
        let (name, data) = entry;
        let partial = dir.join(format!(".{name}.partial"));
        let written = fs::write(&partial, data)
            .and_then(|()| crate::fsx::rename_retry(&partial, &dir.join(name)));
        if written.is_err() {
            let _ = fs::remove_file(&partial);
        }
        written?;
    }
    Ok(dir)
}

/// Whether the four CJK files and their marker are in `dir`.
pub fn cjk_present(dir: &Path) -> bool {
    dir.join(CJK_MARKER).is_file() && CJK_FILES.iter().all(|f| dir.join(f).is_file())
}

/// What the managed folder holds now (a download in this process counts as `Downloading`).
pub fn cjk_state(paths: &AppPaths, bundle: &Bundle<'_>) -> CjkState {
    if cjk_present(&managed_dir(paths, bundle)) {
        CjkState::Present
    } else if RUNNING.load(Ordering::SeqCst) {
        CjkState::Downloading
    } else {
        CjkState::Missing
    }
}

/// Downloads the archive, checks its SHA-256, and puts the four CJK files into `dir` (which must
/// exist), the marker last. On any failure nothing of it stays in `dir`.
pub fn install_cjk(
    dir: &Path,
    source: &CjkSource,
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    let work = dir.join(".cjk-partial");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work)?;
    let result = install_from(dir, &work, source, cancel);
    let _ = fs::remove_dir_all(&work);
    if result.is_err() {
        for name in CJK_FILES.iter().copied().chain([CJK_MARKER]) {
            let _ = fs::remove_file(dir.join(name));
        }
    }
    result
}

fn install_from(
    dir: &Path,
    work: &Path,
    source: &CjkSource,
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    let archive_path = work.join("cjk.zip");
    let asset = Asset {
        name: "Noto Sans CJK".to_owned(),
        url: source.url.clone(),
        size: source.size,
        sha256: Some(source.sha256.clone()),
    };
    // Checked against the SHA-256 while it streams in; nothing is opened before that matches.
    download_capped(
        &update_agent(None),
        &asset,
        &archive_path,
        Some(source.size),
        &mut |_| {},
        cancel,
    )?;
    let mut archive = zip::ZipArchive::new(fs::File::open(&archive_path)?)
        .map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
    for name in CJK_FILES {
        let mut entry = archive
            .by_name(name)
            .map_err(|_| ToolkitError::BadArchive(format!("the archive has no {name}")))?;
        let mut out = fs::File::create(work.join(name))?;
        io::copy(&mut entry, &mut out).map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
        out.flush()?;
    }
    drop(archive);
    for name in CJK_FILES {
        crate::fsx::rename_retry(&work.join(name), &dir.join(name))?;
    }
    fs::write(dir.join(CJK_MARKER), source.sha256.as_bytes())?;
    Ok(())
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static AUTOMATIC_TRIED: AtomicBool = AtomicBool::new(false);

struct RunningGuard;

impl Drop for RunningGuard {
    fn drop(&mut self) {
        RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Starts the CJK download on a worker thread unless the fonts are there or a download runs.
/// `automatic` is the start of a game: it tries once per launcher session. `done` is called when
/// the thread ends. Returns whether a download was started.
pub fn download_cjk_in_background(
    paths: &AppPaths,
    bundle: Bundle<'static>,
    automatic: bool,
    done: Option<Arc<dyn Fn() + Send + Sync>>,
) -> bool {
    if cjk_present(&managed_dir(paths, &bundle)) {
        return false;
    }
    if automatic && AUTOMATIC_TRIED.swap(true, Ordering::SeqCst) {
        return false;
    }
    if RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return false;
    }
    let paths = paths.clone();
    let spawned = std::thread::Builder::new()
        .name("fonts cjk".into())
        .spawn(move || {
            let _guard = RunningGuard;
            let outcome = ensure_bundled(&paths, &bundle)
                .map_err(ToolkitError::from)
                .and_then(|dir| install_cjk(&dir, &CjkSource::default(), &CancelToken::new()));
            match outcome {
                Ok(()) => tracing::info!("the Japanese, Chinese and Korean fonts are installed"),
                Err(error) => tracing::warn!("the CJK fonts were not installed: {error}"),
            }
            if let Some(done) = done {
                done();
            }
        });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::SeqCst);
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{EnvContext, GameSettings};
    use crate::toolkit::tests::{build_zip, serve, sha_hex};
    use std::sync::atomic::Ordering as AtomicOrdering;

    fn files() -> Vec<(&'static str, Vec<u8>)> {
        BUNDLED_FILES
            .iter()
            .chain([&LICENCE_FILE])
            .map(|n| (*n, format!("font {n}").into_bytes()))
            .collect()
    }

    fn with_bundle<T>(version: &str, f: impl FnOnce(Bundle<'_>) -> T) -> T {
        let owned = files();
        let refs: Vec<(&str, &[u8])> = owned.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        f(Bundle {
            version,
            files: &refs,
        })
    }

    fn env_fonts(fonts: Option<PathBuf>) -> Option<String> {
        let ctx = EnvContext {
            launcher_language: 1,
            input_file: None,
            screenshots: None,
            fonts,
            shader_cache: None,
        };
        GameSettings::default()
            .environment(&ctx)
            .get("ANYPS5_SYSTEM_FONTS")
            .cloned()
    }

    #[test]
    fn a_game_without_a_picked_folder_gets_the_managed_one() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        with_bundle("t1", |bundle| {
            let fonts = resolve(&paths, &bundle, None);
            let want = managed_dir(&paths, &bundle);
            assert_eq!(fonts.as_deref(), Some(want.as_path()));
            assert_eq!(
                env_fonts(fonts).as_deref(),
                Some(want.to_string_lossy().as_ref())
            );
            for name in BUNDLED_FILES {
                assert!(want.join(name).is_file(), "{name} is written");
            }
            assert!(want.join(LICENCE_FILE).is_file());
        });
    }

    #[test]
    fn a_picked_folder_wins_and_nothing_is_written() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        let picked = root.path().join("MyFonts");
        with_bundle("t1", |bundle| {
            let fonts = resolve(&paths, &bundle, Some(&picked));
            assert_eq!(env_fonts(fonts).as_deref(), picked.to_str());
            assert!(!managed_dir(&paths, &bundle).exists());
        });
    }

    #[test]
    fn writing_the_bundle_twice_changes_nothing_and_a_partial_set_is_completed() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        with_bundle("t1", |bundle| {
            let dir = ensure_bundled(&paths, &bundle).unwrap();
            let stamp = |n: &str| fs::metadata(dir.join(n)).unwrap().modified().unwrap();
            let before = stamp("NotoSans-Regular.ttf");
            std::thread::sleep(std::time::Duration::from_millis(20));
            assert_eq!(ensure_bundled(&paths, &bundle).unwrap(), dir);
            assert_eq!(stamp("NotoSans-Regular.ttf"), before);
            // One file lost, one cut short: both come back.
            fs::remove_file(dir.join("NotoSansThai-Bold.ttf")).unwrap();
            fs::write(dir.join("NotoSansArabic-Light.ttf"), b"cut").unwrap();
            ensure_bundled(&paths, &bundle).unwrap();
            assert_eq!(
                fs::read(dir.join("NotoSansThai-Bold.ttf")).unwrap(),
                b"font NotoSansThai-Bold.ttf"
            );
            assert_eq!(
                fs::read(dir.join("NotoSansArabic-Light.ttf")).unwrap(),
                b"font NotoSansArabic-Light.ttf"
            );
            let names: Vec<_> = fs::read_dir(&dir)
                .unwrap()
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            assert!(!names.iter().any(|n| n.ends_with(".partial")), "{names:?}");
        });
    }

    #[test]
    fn a_bundle_without_a_required_file_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        let bundle = Bundle {
            version: "t1",
            files: &[("NotoSans-Regular.ttf", b"x")],
        };
        assert!(ensure_bundled(&paths, &bundle).is_err());
        assert!(resolve(&paths, &bundle, None).is_none());
    }

    fn cjk_zip(skip: Option<&str>) -> Vec<u8> {
        let owned: Vec<(&str, Vec<u8>)> = CJK_FILES
            .iter()
            .filter(|n| Some(**n) != skip)
            .map(|n| (*n, format!("ttc {n}").into_bytes()))
            .chain([("LICENSE", b"licence".to_vec())])
            .collect();
        let refs: Vec<(&str, &[u8])> = owned.iter().map(|(n, d)| (*n, d.as_slice())).collect();
        build_zip(&refs)
    }

    fn install(zip: Vec<u8>, sha: Option<String>) -> (tempfile::TempDir, Result<(), ToolkitError>) {
        let dir = tempfile::tempdir().unwrap();
        let source = CjkSource {
            sha256: sha.unwrap_or_else(|| sha_hex(&zip)),
            size: zip.len() as u64,
            url: String::new(),
        };
        let (base, handle, stop) = serve(vec![("/cjk.zip", zip, 200)]);
        let source = CjkSource {
            url: format!("{base}/cjk.zip"),
            ..source
        };
        let result = install_cjk(dir.path(), &source, &CancelToken::new());
        stop.store(true, AtomicOrdering::SeqCst);
        handle.join().unwrap();
        (dir, result)
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_cjk_download_installs_four_files_and_the_marker() {
        let (dir, result) = install(cjk_zip(None), None);
        result.unwrap();
        assert!(cjk_present(dir.path()));
        let mut want: Vec<String> = CJK_FILES.iter().map(|s| (*s).to_owned()).collect();
        want.push(CJK_MARKER.to_owned());
        want.sort();
        assert_eq!(listing(dir.path()), want);
        assert_eq!(
            fs::read(dir.path().join("NotoSansCJK-Bold.ttc")).unwrap(),
            b"ttc NotoSansCJK-Bold.ttc"
        );
    }

    #[test]
    fn a_wrong_checksum_is_refused_and_leaves_nothing() {
        let (dir, result) = install(cjk_zip(None), Some("0".repeat(64)));
        assert!(matches!(result, Err(ToolkitError::Digest)), "{result:?}");
        assert!(listing(dir.path()).is_empty(), "{:?}", listing(dir.path()));
    }

    #[test]
    fn an_archive_missing_one_font_is_refused_and_leaves_nothing() {
        let (dir, result) = install(cjk_zip(Some("NotoSansCJK-Medium.ttc")), None);
        assert!(
            matches!(result, Err(ToolkitError::BadArchive(_))),
            "{result:?}"
        );
        assert!(listing(dir.path()).is_empty(), "{:?}", listing(dir.path()));
    }

    #[test]
    fn a_failed_download_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (base, handle, stop) = serve(vec![]);
        let source = CjkSource {
            url: format!("{base}/missing.zip"),
            sha256: "0".repeat(64),
            size: 10,
        };
        let result = install_cjk(dir.path(), &source, &CancelToken::new());
        stop.store(true, AtomicOrdering::SeqCst);
        handle.join().unwrap();
        assert!(
            matches!(result, Err(ToolkitError::Network(_))),
            "{result:?}"
        );
        assert!(listing(dir.path()).is_empty());
        assert!(!cjk_present(dir.path()));
    }

    #[test]
    fn the_pinned_values_are_well_formed() {
        let source = CjkSource::default();
        assert!(
            source
                .url
                .starts_with("https://github.com/notofonts/noto-cjk/")
        );
        assert_eq!(source.sha256.len(), 64);
        assert!(source.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(source.sha256, source.sha256.to_lowercase());
    }
}
