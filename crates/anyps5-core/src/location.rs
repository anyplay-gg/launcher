//! Where the unlocked copies live (docs/DESIGN.md 5.7 "Unlocked Games Location").
//!
//! By default the store (`.AnyPS5`) sits in the games folder, on the dumps' volume, so the copy is
//! made of hard links. The player can pick another folder, on any drive: the store is then
//! `<folder>\.AnyPS5`, the copies are full copies (hard links only where it happens to be the same
//! volume) and the dumps are not touched. This module decides whether a folder is acceptable, what
//! state it is in and what to tell the player about it. Moving existing copies is [`crate::relocate`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::fsx;
use crate::paths::{self, GameId};
use crate::unlock::STORE_NAME;

/// Why a folder cannot hold the unlocked games.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not an absolute path.
    NotAbsolute,
    /// Missing, or a file.
    NotAFolder,
    /// Inside the games folder: the dumps' folder is never written to.
    InsideGamesFolder,
    /// Inside a store, so inside an unlocked copy (or the store itself).
    InsideStore,
    /// The launcher cannot create files there.
    NotWritable(String),
}

/// What the location is like right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    /// The folder is there (and its store).
    Ready,
    /// The folder or its drive is not there (a disconnected drive, a share that is down).
    Missing,
}

/// Everything the settings row says about the location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The folder that holds the store (the games folder by default).
    pub root: PathBuf,
    /// Chosen by the player, not the default.
    pub custom: bool,
    pub availability: Availability,
    /// Free bytes on the drive; `None` when missing or unknown.
    pub free: Option<u64>,
    /// A network drive or share: copies work, but are slow and only as safe as the network.
    pub network: bool,
}

/// The state of the location the preferences name. `None` without a games folder.
pub fn report(games_folder: Option<&Path>, unlocked_folder: Option<&Path>) -> Option<Report> {
    let games = games_folder?;
    let (root, custom) = match unlocked_folder {
        Some(folder) => (folder, true),
        None => (games, false),
    };
    let availability = availability(root, custom);
    let free = (availability == Availability::Ready)
        .then(|| fsx::free_space(root))
        .flatten();
    Some(Report {
        root: root.to_path_buf(),
        custom,
        availability,
        free,
        network: is_network(root),
    })
}

/// Whether the store root can be used now. A custom location also needs its store: the folder
/// alone may be an unmounted mount point (Linux) that writes to the wrong disk.
pub fn availability(root: &Path, custom: bool) -> Availability {
    if root.is_dir() && (!custom || paths::store_dir(root).is_dir()) {
        Availability::Ready
    } else {
        Availability::Missing
    }
}

/// Resolves links and spelling where the path exists, else makes it absolute.
fn resolved(path: &Path) -> PathBuf {
    fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// `inner` is `outer` or lies below it (compared like game IDs: case-insensitive, `/`).
fn is_within(inner: &Path, outer: &Path) -> bool {
    let inner = GameId::from_path(inner);
    let outer = GameId::from_path(outer);
    let (inner, outer) = (inner.as_str(), outer.as_str().trim_end_matches('/'));
    inner == outer
        || inner
            .strip_prefix(outer)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether `candidate` is the games folder itself (picking it means "next to the games").
pub fn is_games_folder(candidate: &Path, games_folder: &Path) -> bool {
    GameId::from_path(&resolved(candidate)) == GameId::from_path(&resolved(games_folder))
}

/// Checks a folder the player picked, without writing anything. The games folder itself is
/// accepted here (the caller turns it into the default).
pub fn check(candidate: &Path, games_folder: &Path) -> Result<(), Refusal> {
    if !candidate.is_absolute() {
        return Err(Refusal::NotAbsolute);
    }
    if !candidate.is_dir() {
        return Err(Refusal::NotAFolder);
    }
    let candidate = resolved(candidate);
    let games = resolved(games_folder);
    if is_games_folder(&candidate, &games) {
        return Ok(());
    }
    if is_within(&candidate, &games) {
        return Err(Refusal::InsideGamesFolder);
    }
    if candidate.components().any(|part| {
        part.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(STORE_NAME)
    }) {
        return Err(Refusal::InsideStore);
    }
    Ok(())
}

/// Accepts `candidate` as the new location: checks it, then makes its store and proves that a
/// file can be written there. Returns the folder to keep (`None`: the games folder, the default).
/// Only ever writes below `candidate`, never into the games folder.
pub fn adopt(candidate: &Path, games_folder: &Path) -> Result<Option<PathBuf>, Refusal> {
    check(candidate, games_folder)?;
    if is_games_folder(candidate, games_folder) {
        return Ok(None);
    }
    let store = paths::store_dir(candidate);
    prove_writable(&store).map_err(|e| Refusal::NotWritable(e.to_string()))?;
    Ok(Some(candidate.to_path_buf()))
}

fn prove_writable(store: &Path) -> io::Result<()> {
    fs::create_dir_all(store)?;
    let probe = store.join(format!(".anyps5-write-probe-{}", std::process::id()));
    fs::write(&probe, b"x")?;
    fs::remove_file(&probe)
}

/// A network drive or share. Windows: `\\server\share`, a mapped drive letter. Linux: a mount of
/// a network file system.
pub fn is_network(path: &Path) -> bool {
    network_impl(path)
}

#[cfg(windows)]
fn network_impl(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    use windows::core::PCWSTR;
    const DRIVE_REMOTE: u32 = 4;
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return false;
    };
    match prefix.kind() {
        Prefix::UNC(..) | Prefix::VerbatimUNC(..) => true,
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
            let root = format!("{}:\\", letter as char);
            let wide: Vec<u16> = std::ffi::OsStr::new(&root)
                .encode_wide()
                .chain(Some(0))
                .collect();
            // SAFETY: `wide` is NUL terminated and outlives the call.
            unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) == DRIVE_REMOTE }
        }
        _ => false,
    }
}

#[cfg(target_os = "linux")]
fn network_impl(path: &Path) -> bool {
    let Ok(mounts) = fs::read_to_string("/proc/mounts") else {
        return false;
    };
    network_in_mounts(&mounts, &resolved(path))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn network_impl(_path: &Path) -> bool {
    false
}

/// The file system type of the longest mount point that holds `path`, read from `/proc/mounts`
/// text, is a network one.
pub fn network_in_mounts(mounts: &str, path: &Path) -> bool {
    const NETWORK: [&str; 8] = [
        "nfs", "nfs4", "cifs", "smb3", "smbfs", "fuse.sshfs", "9p", "afs",
    ];
    let mut best: Option<(usize, &str)> = None;
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let (Some(_device), Some(point), Some(kind)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        // /proc/mounts escapes spaces as \040.
        let point = point.replace("\\040", " ");
        if path.starts_with(&point) && best.is_none_or(|(len, _)| point.len() >= len) {
            best = Some((point.len(), kind));
        }
    }
    best.is_some_and(|(_, kind)| NETWORK.contains(&kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folders() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let games = dir.path().join("games");
        let other = dir.path().join("fast");
        fs::create_dir_all(games.join("Stray")).unwrap();
        fs::create_dir_all(&other).unwrap();
        (dir, games, other)
    }

    #[test]
    fn another_folder_is_accepted_and_gets_a_store() {
        let (_dir, games, other) = folders();
        let kept = adopt(&other, &games).unwrap();
        assert_eq!(kept.as_deref(), Some(other.as_path()));
        assert!(paths::store_dir(&other).is_dir());
        assert!(!paths::store_dir(&games).exists(), "the games folder is not written to");
        assert!(
            fs::read_dir(paths::store_dir(&other)).unwrap().next().is_none(),
            "the write probe is removed"
        );
    }

    #[test]
    fn the_games_folder_itself_means_the_default() {
        let (_dir, games, _other) = folders();
        assert_eq!(adopt(&games, &games).unwrap(), None);
        assert_eq!(
            adopt(&games.join("..").join("games"), &games).unwrap(),
            None,
            "another spelling of the same folder"
        );
        assert!(!paths::store_dir(&games).exists());
    }

    #[test]
    fn a_folder_inside_the_games_folder_is_refused_and_nothing_is_written() {
        let (_dir, games, _other) = folders();
        let inside = games.join("Stray");
        assert_eq!(adopt(&inside, &games), Err(Refusal::InsideGamesFolder));
        assert_eq!(
            fs::read_dir(&inside).unwrap().count(),
            0,
            "nothing was written into the dump"
        );
        let deeper = games.join("Stray").join("sce_sys");
        fs::create_dir_all(&deeper).unwrap();
        assert_eq!(check(&deeper, &games), Err(Refusal::InsideGamesFolder));
    }

    #[test]
    fn a_sibling_with_the_same_name_start_is_not_inside() {
        let (dir, games, _other) = folders();
        let sibling = dir.path().join("games-fast");
        fs::create_dir_all(&sibling).unwrap();
        assert_eq!(check(&sibling, &games), Ok(()));
    }

    #[test]
    fn a_folder_inside_a_store_or_an_unlocked_copy_is_refused() {
        let (_dir, games, other) = folders();
        let copy = paths::store_dir(&other).join("Games").join("PPSA01234-abc");
        fs::create_dir_all(&copy).unwrap();
        assert_eq!(check(&copy, &games), Err(Refusal::InsideStore));
        assert_eq!(
            check(&paths::store_dir(&other), &games),
            Err(Refusal::InsideStore)
        );
        assert_eq!(adopt(&copy, &games), Err(Refusal::InsideStore));
    }

    #[test]
    fn a_missing_folder_a_file_and_a_relative_path_are_refused() {
        let (dir, games, _other) = folders();
        assert_eq!(
            check(&dir.path().join("gone"), &games),
            Err(Refusal::NotAFolder)
        );
        let file = dir.path().join("file.txt");
        fs::write(&file, b"x").unwrap();
        assert_eq!(check(&file, &games), Err(Refusal::NotAFolder));
        assert_eq!(check(Path::new("fast"), &games), Err(Refusal::NotAbsolute));
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_cannot_be_written_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, games, other) = folders();
        fs::set_permissions(&other, fs::Permissions::from_mode(0o555)).unwrap();
        let writable = fs::write(other.join("probe"), b"x").is_ok();
        let result = adopt(&other, &games);
        fs::set_permissions(&other, fs::Permissions::from_mode(0o755)).unwrap();
        if !writable {
            // (a test run as root can write anywhere and skips this)
            assert!(matches!(result, Err(Refusal::NotWritable(_))));
        }
    }

    #[test]
    fn availability_follows_the_folder_and_its_store() {
        let (_dir, games, other) = folders();
        assert_eq!(availability(&games, false), Availability::Ready);
        assert_eq!(availability(&other, true), Availability::Missing, "no store yet");
        adopt(&other, &games).unwrap();
        assert_eq!(availability(&other, true), Availability::Ready);
        fs::remove_dir_all(&other).unwrap();
        assert_eq!(availability(&other, true), Availability::Missing, "drive gone");
    }

    #[test]
    fn the_report_of_a_missing_location_has_no_free_space() {
        let (_dir, games, other) = folders();
        let ready = report(Some(&games), None).unwrap();
        assert!(!ready.custom);
        assert_eq!(ready.availability, Availability::Ready);
        assert!(ready.free.is_some_and(|free| free > 0));
        adopt(&other, &games).unwrap();
        let custom = report(Some(&games), Some(&other)).unwrap();
        assert!(custom.custom && custom.free.is_some());
        fs::remove_dir_all(&other).unwrap();
        let gone = report(Some(&games), Some(&other)).unwrap();
        assert_eq!(gone.availability, Availability::Missing);
        assert_eq!(gone.free, None);
        assert_eq!(report(None, Some(&other)), None);
    }

    #[test]
    fn network_mounts_are_found_by_the_longest_mount_point() {
        let mounts = "/dev/sda1 / ext4 rw 0 0\n\
                      nas:/vol /mnt/nas nfs4 rw 0 0\n\
                      //srv/share /mnt/nas/my\\040games cifs rw 0 0\n\
                      /dev/sdb1 /mnt/nas/local ext4 rw 0 0\n";
        assert!(network_in_mounts(mounts, Path::new("/mnt/nas/dumps")));
        assert!(network_in_mounts(mounts, Path::new("/mnt/nas/my games/x")));
        assert!(!network_in_mounts(mounts, Path::new("/mnt/nas/local/x")));
        assert!(!network_in_mounts(mounts, Path::new("/home/me")));
    }
}
