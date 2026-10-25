//! Cisco's OpenH264 for the share worker on Linux: the software H.264 it encodes and decodes
//! with (network ADR 0027).
//!
//! The launcher carries none of it. Cisco's licence for the codec's patents covers only Cisco's
//! own build, downloaded separately to the player's PC, with the player able to turn its use off
//! and on, and the text [`NOTICE`] shown where they do. So the library is fetched from Cisco's
//! server, checked by the SHA-256 of the archive and of the library inside, and written beside the
//! share worker (`Network/bin`), where the worker looks for it under Cisco's file name. Turning it
//! off removes the file.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::events::CancelToken;
use crate::paths::AppPaths;
use crate::toolkit::{Asset, ToolkitError, download_capped, hex, update_agent};

/// Cisco's file name for the build the share worker loads.
pub const LIBRARY_NAME: &str = "libopenh264-2.6.0-linux64.8.so";
/// The text Cisco's licence asks for where the player turns the codec off and on.
pub const NOTICE: &str = "OpenH264 Video Codec provided by Cisco Systems, Inc.";
/// Cisco's archive of that build.
pub const URL: &str = "https://ciscobinary.openh264.org/libopenh264-2.6.0-linux64.8.so.bz2";
/// Its size in bytes.
pub const ARCHIVE_SIZE: u64 = 634_264;
/// Its SHA-256.
pub const ARCHIVE_SHA256: &str = "27ab53323c110b76214c1c72222f459d17febbcd1e252136cadc292b0308d75b";
/// The SHA-256 of the library inside, the one the share worker accepts.
pub const LIBRARY_SHA256: &str = "2f0cde7c6a6abcf5cae76942894ea42897fa677bce4ed6c91a24dd1b041d5f04";
/// The most the archive may unpack to: the library is about 1.7 MB.
const MAX_LIBRARY: u64 = 16 << 20;

/// Where the library comes from and what it must be. [`Default`] is Cisco's pinned build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub url: String,
    pub archive_size: u64,
    pub archive_sha256: String,
    pub library_sha256: String,
}

impl Default for Source {
    fn default() -> Self {
        Source {
            url: URL.to_owned(),
            archive_size: ARCHIVE_SIZE,
            archive_sha256: ARCHIVE_SHA256.to_owned(),
            library_sha256: LIBRARY_SHA256.to_owned(),
        }
    }
}

/// The variable that tells the share worker where the library is (and `off` turns it off).
pub const LIBRARY_ENV: &str = "ANYPLAY_OPENH264";

/// Where the library goes: beside the share worker.
pub fn library_path(paths: &AppPaths) -> PathBuf {
    library_in(&paths.network_dir())
}

/// The library's place in the net service's folder (`Network`), which the net service's share
/// worker is told through [`LIBRARY_ENV`].
pub fn library_in(network_dir: &Path) -> PathBuf {
    network_dir.join("bin").join(LIBRARY_NAME)
}

/// What the codec is on this PC, as the player's row shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecState {
    /// Not downloaded.
    Missing,
    Downloading,
    /// Downloaded and checked.
    Ready,
    /// The last download failed: why.
    Failed(String),
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
/// Set when the library came or went; the launcher then tells the net service again.
static CHANGED: AtomicBool = AtomicBool::new(false);
/// A file as last checked: where, its size and time, the hash wanted, and whether it matched.
type Checked = (PathBuf, u64, SystemTime, String, bool);
/// The settings page asks every frame; the file is hashed again only when it changed.
static CHECKED: Mutex<Option<Checked>> = Mutex::new(None);
/// The background download's token: turning the codec off cancels it.
static ACTIVE: Mutex<Option<CancelToken>> = Mutex::new(None);
/// Held while the library is put in place and while it is removed, so a download the player
/// turned off never lands after the removal.
static PLACING: Mutex<()> = Mutex::new(());

/// Whether the library is there and is the build `source` names, hashing the file only when its
/// size or time changed since the last call.
pub fn installed(paths: &AppPaths, source: &Source) -> bool {
    let path = library_path(paths);
    let Ok(meta) = fs::metadata(&path) else {
        return false;
    };
    let time = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let mut checked = CHECKED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, len, t, sha, good)) = &*checked
        && (p, *len, *t, sha) == (&path, meta.len(), time, &source.library_sha256)
    {
        return *good;
    }
    let good = present(paths, source);
    *checked = Some((path, meta.len(), time, source.library_sha256.clone(), good));
    good
}

/// The codec's state for the player's row.
pub fn state(paths: &AppPaths, source: &Source) -> CodecState {
    if RUNNING.load(Ordering::SeqCst) {
        return CodecState::Downloading;
    }
    if installed(paths, source) {
        return CodecState::Ready;
    }
    match LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        Some(why) => CodecState::Failed(why),
        None => CodecState::Missing,
    }
}

/// True once after the library came or went.
pub fn take_changed() -> bool {
    CHANGED.swap(false, Ordering::SeqCst)
}

/// Starts the download on a worker thread unless the library is there or a download runs.
/// `done` is called when the thread ends. Returns whether a download was started.
pub fn install_in_background(
    paths: &AppPaths,
    source: Source,
    done: Option<Arc<dyn Fn() + Send + Sync>>,
) -> bool {
    if installed(paths, &source)
        || RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return false;
    }
    *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = None;
    let cancel = CancelToken::new();
    *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) = Some(cancel.clone());
    let paths = paths.clone();
    let spawned = std::thread::Builder::new()
        .name("openh264".into())
        .spawn(move || {
            let outcome = install(&paths, &source, &cancel);
            match &outcome {
                Ok(()) => tracing::info!("Cisco's OpenH264 is installed"),
                Err(error) => tracing::warn!("Cisco's OpenH264 was not installed: {error}"),
            }
            *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) = None;
            // Turned off while it ran: not a failure the row should show.
            *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = match outcome {
                Err(ToolkitError::Cancelled) => None,
                other => other.err().map(|e| e.to_string()),
            };
            RUNNING.store(false, Ordering::SeqCst);
            CHANGED.store(true, Ordering::SeqCst);
            if let Some(done) = done {
                done();
            }
        });
    if spawned.is_err() {
        *ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) = None;
        RUNNING.store(false, Ordering::SeqCst);
        return false;
    }
    true
}

/// Whether the library is there and is the build `source` names.
pub fn present(paths: &AppPaths, source: &Source) -> bool {
    fs::read(library_path(paths))
        .is_ok_and(|bytes| hex(&Sha256::digest(bytes)) == source.library_sha256)
}

/// Downloads and places the library. Nothing is left behind when a check fails, and a library
/// already in place is not touched.
pub fn install(
    paths: &AppPaths,
    source: &Source,
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    if present(paths, source) {
        return Ok(());
    }
    let target = library_path(paths);
    let dir = target.parent().expect("the library has a folder");
    fs::create_dir_all(dir)?;
    let archive = dir.join(".openh264.bz2.partial");
    let result = install_from(&archive, &target, source, cancel);
    let _ = fs::remove_file(&archive);
    let _ = fs::remove_file(partial(&target));
    result
}

fn partial(target: &Path) -> PathBuf {
    target.with_extension("so.partial")
}

fn install_from(
    archive: &Path,
    target: &Path,
    source: &Source,
    cancel: &CancelToken,
) -> Result<(), ToolkitError> {
    let asset = Asset {
        name: "OpenH264".to_owned(),
        url: source.url.clone(),
        size: source.archive_size,
        sha256: Some(source.archive_sha256.clone()),
    };
    // Checked against the SHA-256 while it streams in; nothing is unpacked before that matches.
    download_capped(
        &update_agent(None),
        &asset,
        archive,
        Some(source.archive_size),
        &mut |_| {},
        cancel,
    )?;
    let mut library = Vec::new();
    bzip2::read::BzDecoder::new(fs::File::open(archive)?)
        .take(MAX_LIBRARY + 1)
        .read_to_end(&mut library)
        .map_err(|e| ToolkitError::BadArchive(e.to_string()))?;
    if library.len() as u64 > MAX_LIBRARY {
        return Err(ToolkitError::TooLarge);
    }
    if hex(&Sha256::digest(&library)) != source.library_sha256 {
        return Err(ToolkitError::Digest);
    }
    let partial = partial(target);
    fs::write(&partial, &library)?;
    let _placing = PLACING.lock().unwrap_or_else(|e| e.into_inner());
    if cancel.is_cancelled() {
        return Err(ToolkitError::Cancelled);
    }
    crate::fsx::rename_retry(&partial, target)?;
    Ok(())
}

/// Removes the library and stops a download that is still running: the player turned the codec
/// off.
pub fn remove(paths: &AppPaths) -> io::Result<()> {
    let _placing = PLACING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cancel) = &*ACTIVE.lock().unwrap_or_else(|e| e.into_inner()) {
        cancel.cancel();
    }
    *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = None;
    CHANGED.store(true, Ordering::SeqCst);
    match fs::remove_file(library_path(paths)) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::Ordering as AtomicOrdering;

    use crate::toolkit::tests::{serve, sha_hex};

    const LIBRARY: &[u8] = b"\x7fELF a library of Cisco's";

    /// The download state is process-wide; tests that start downloads or remove run one at a time.
    static GLOBALS: Mutex<()> = Mutex::new(());

    fn bz2(data: &[u8]) -> Vec<u8> {
        let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    /// Serves `archive` and installs from it; `source` is the pinned values for it unless
    /// changed by `change`.
    fn install_served(
        archive: Vec<u8>,
        change: impl FnOnce(&mut Source),
    ) -> (tempfile::TempDir, AppPaths, Result<(), ToolkitError>) {
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        let (base, handle, stop) = serve(vec![("/lib.so.bz2", archive.clone(), 200)]);
        let mut source = Source {
            url: format!("{base}/lib.so.bz2"),
            archive_size: archive.len() as u64,
            archive_sha256: sha_hex(&archive),
            library_sha256: sha_hex(LIBRARY),
        };
        change(&mut source);
        let result = install(&paths, &source, &CancelToken::new());
        stop.store(true, AtomicOrdering::SeqCst);
        handle.join().unwrap();
        (root, paths, result)
    }

    fn bin_listing(paths: &AppPaths) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(paths.network_dir().join("bin"))
            .map(|dir| {
                dir.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn the_library_lands_beside_the_share_worker() {
        let (_root, paths, result) = install_served(bz2(LIBRARY), |_| {});
        result.unwrap();
        assert_eq!(fs::read(library_path(&paths)).unwrap(), LIBRARY);
        assert_eq!(bin_listing(&paths), [LIBRARY_NAME]);
        let source = Source {
            library_sha256: sha_hex(LIBRARY),
            ..Source::default()
        };
        assert!(present(&paths, &source));
    }

    #[test]
    fn an_archive_with_another_checksum_is_refused_and_leaves_nothing() {
        let (_root, paths, result) =
            install_served(bz2(LIBRARY), |s| s.archive_sha256 = "0".repeat(64));
        assert!(matches!(result, Err(ToolkitError::Digest)), "{result:?}");
        assert!(bin_listing(&paths).is_empty(), "{:?}", bin_listing(&paths));
    }

    #[test]
    fn a_library_with_another_checksum_is_refused_and_leaves_nothing() {
        let (_root, paths, result) =
            install_served(bz2(LIBRARY), |s| s.library_sha256 = "0".repeat(64));
        assert!(matches!(result, Err(ToolkitError::Digest)), "{result:?}");
        assert!(bin_listing(&paths).is_empty(), "{:?}", bin_listing(&paths));
    }

    #[test]
    fn an_archive_that_is_not_bzip2_is_refused_and_leaves_nothing() {
        let (_root, paths, result) = install_served(b"not bzip2".to_vec(), |_| {});
        assert!(
            matches!(result, Err(ToolkitError::BadArchive(_))),
            "{result:?}"
        );
        assert!(bin_listing(&paths).is_empty(), "{:?}", bin_listing(&paths));
    }

    /// One test for everything that goes through the process-wide download state.
    #[test]
    fn the_row_s_state_follows_a_download_in_the_background_and_its_removal() {
        let _globals = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        let archive = bz2(LIBRARY);
        let (base, handle, stop) = serve(vec![
            ("/lib.so.bz2", archive.clone(), 200),
            ("/broken.bz2", b"not the archive".to_vec(), 200),
        ]);
        let source = Source {
            url: format!("{base}/lib.so.bz2"),
            archive_size: archive.len() as u64,
            archive_sha256: sha_hex(&archive),
            library_sha256: sha_hex(LIBRARY),
        };
        let wait = |source: Source| {
            let (tx, rx) = std::sync::mpsc::channel();
            let done: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let _ = tx.send(());
            });
            assert!(install_in_background(&paths, source, Some(done)));
            rx.recv_timeout(std::time::Duration::from_secs(10))
                .expect("the download ends");
        };
        assert_eq!(state(&paths, &source), CodecState::Missing);

        // A download that fails says why, and leaves nothing.
        wait(Source {
            url: format!("{base}/broken.bz2"),
            ..source.clone()
        });
        assert!(matches!(state(&paths, &source), CodecState::Failed(_)));
        assert!(!library_path(&paths).exists());

        wait(source.clone());
        assert_eq!(state(&paths, &source), CodecState::Ready);
        assert!(take_changed(), "the net service is told again");
        // Nothing to download while it is there.
        assert!(!install_in_background(&paths, source.clone(), None));

        // A file changed in place is checked again.
        fs::write(library_path(&paths), b"\x7fELF another library").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        fs::File::options()
            .write(true)
            .open(library_path(&paths))
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(state(&paths, &source), CodecState::Missing);

        remove(&paths).unwrap();
        assert!(take_changed());
        assert_eq!(state(&paths, &source), CodecState::Missing);
        stop.store(true, AtomicOrdering::SeqCst);
        handle.join().unwrap();
    }

    #[test]
    fn turning_it_off_removes_the_library_and_twice_is_fine() {
        let _globals = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let (_root, paths, result) = install_served(bz2(LIBRARY), |_| {});
        result.unwrap();
        remove(&paths).unwrap();
        assert!(!library_path(&paths).exists());
        remove(&paths).unwrap();
    }

    #[test]
    fn turning_it_off_while_it_downloads_leaves_no_library() {
        let _globals = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let root = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(root.path());
        let archive = bz2(LIBRARY);
        let (base, handle, stop) = serve(vec![("/lib.so.bz2", archive.clone(), 200)]);
        let source = Source {
            url: format!("{base}/lib.so.bz2"),
            archive_size: archive.len() as u64,
            archive_sha256: sha_hex(&archive),
            library_sha256: sha_hex(LIBRARY),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let done: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = tx.send(());
        });
        assert!(install_in_background(&paths, source.clone(), Some(done)));
        remove(&paths).unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("the download ends");
        stop.store(true, AtomicOrdering::SeqCst);
        handle.join().unwrap();
        assert!(!library_path(&paths).exists(), "off means no library");
        assert_eq!(state(&paths, &source), CodecState::Missing);
    }

    #[test]
    fn the_pinned_values_are_well_formed() {
        for sha in [ARCHIVE_SHA256, LIBRARY_SHA256] {
            assert_eq!(sha.len(), 64);
            assert!(
                sha.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
        }
        assert!(URL.starts_with("https://ciscobinary.openh264.org/"));
        assert!(URL.ends_with(&format!("{LIBRARY_NAME}.bz2")));
    }
}
