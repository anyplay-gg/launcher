//! Moves the unlocked copies from one store to another when the player changes the unlocked-games
//! location (docs/DESIGN.md 5.7).
//!
//! The rule is the unlock's: a copy is either whole at the old place or whole at the new one.
//! On one volume the copy's folder is renamed (instant, hard links stay). Across volumes it is
//! copied into `<name>.partial` at the new place, checked against the old tree, renamed to its
//! name, and only then removed from the old place. Saves and trophies live inside the copy and
//! travel with it. An interrupted or cancelled move leaves the unmoved copies where they were;
//! `.partial` folders are swept by [`crate::unlock::remove_leftovers`]. The dumps are never touched.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::CancelToken;
use crate::fsx::{self, LinkMode, Linker, TreeError, TreeOpts};
use crate::paths;

const PARTIAL: &str = ".partial";
const PREVIOUS: &str = ".previous";
/// Room kept free on the target besides the copy itself.
const HEADROOM: u64 = 256 * 1024 * 1024;

/// Why a copy stayed where it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Left {
    /// A game that runs now; never moved.
    Running,
    /// The new drive cannot hold it: how much it needs and how much is free.
    NoSpace { needed: u64, free: u64 },
    /// The move was cancelled before it.
    Cancelled,
    /// A file operation failed.
    Failed(String),
}

/// What a move did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Copies now whole at the new place.
    pub moved: Vec<String>,
    /// Copies still at the old place, and why.
    pub left: Vec<(String, Left)>,
    /// The old copy of a moved game could not be removed (a program holds a file); it is a
    /// duplicate the player can delete.
    pub duplicates: Vec<String>,
    pub cancelled: bool,
}

impl Report {
    /// Nothing is left to move: the old place may be forgotten.
    pub fn complete(&self) -> bool {
        self.left.is_empty() && !self.cancelled
    }
}

/// How far a move is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// Copies finished, and all that were to be moved.
    pub done: usize,
    pub total: usize,
    /// The folder name of the copy being moved.
    pub current: String,
}

/// The unlocked copies in a store (folder names), without half-made ones.
pub fn pending(root: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(paths::copies_dir(root)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !name.ends_with(PARTIAL) && !name.ends_with(PREVIOUS))
        .collect();
    names.sort();
    names
}

/// Moves every copy of the store `from` to the store `to`. `running` names copies that must stay
/// (a game runs from them). `mode` is [`LinkMode::ForceCopy`] to skip the rename of a same-volume
/// move (tests, and the way to exercise the cross-volume path). `progress` is called from this thread. Stops at the first copy that
/// fails for lack of space or an error, and at a cancel; whatever is left stays whole in `from`.
pub fn move_all(
    from: &Path,
    to: &Path,
    mode: LinkMode,
    running: &dyn Fn(&str) -> bool,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(Progress),
) -> Report {
    let names = pending(from);
    let sizes: Vec<u64> = names
        .iter()
        .map(|name| {
            fsx::plan_tree(&paths::copies_dir(from).join(name), &[]).map_or(0, |plan| plan.bytes)
        })
        .collect();
    let total_bytes: u64 = sizes.iter().sum();
    let mut report = Report::default();
    let mut done_bytes = 0u64;
    let mut stopped = false;
    for (index, name) in names.iter().enumerate() {
        if stopped || cancel.is_cancelled() {
            let reason = if stopped {
                continue_reason(&report)
            } else {
                report.cancelled = true;
                Left::Cancelled
            };
            if running(name) {
                report.left.push((name.clone(), Left::Running));
            } else {
                report.left.push((name.clone(), reason));
            }
            stopped = true;
            continue;
        }
        if running(name) {
            report.left.push((name.clone(), Left::Running));
            done_bytes += sizes[index];
            continue;
        }
        let moved_now = report.moved.len();
        let mut emit = |share: f64| {
            progress(Progress {
                done_bytes: done_bytes + (share.clamp(0.0, 1.0) * sizes[index] as f64) as u64,
                total_bytes,
                done: moved_now,
                total: names.len(),
                current: name.clone(),
            });
        };
        emit(0.0);
        match move_one(from, to, name, mode, cancel, &mut emit) {
            Ok(Moved { duplicate }) => {
                report.moved.push(name.clone());
                if duplicate {
                    report.duplicates.push(name.clone());
                }
                done_bytes += sizes[index];
            }
            Err(MoveError::Cancelled) => {
                report.cancelled = true;
                report.left.push((name.clone(), Left::Cancelled));
                stopped = true;
            }
            Err(MoveError::NoSpace { needed, free }) => {
                report
                    .left
                    .push((name.clone(), Left::NoSpace { needed, free }));
                stopped = true;
            }
            Err(MoveError::Io(error)) => {
                report
                    .left
                    .push((name.clone(), Left::Failed(error.to_string())));
                stopped = true;
            }
        }
    }
    if report.complete() {
        tidy(from);
    }
    report
}

/// The reason the copies after a stop stay: the one that stopped the move.
fn continue_reason(report: &Report) -> Left {
    report
        .left
        .iter()
        .rev()
        .map(|(_, reason)| reason.clone())
        .find(|reason| !matches!(reason, Left::Running))
        .unwrap_or(Left::Cancelled)
}

/// Removes the emptied store (never anything else): the old `.AnyPS5` goes when nothing is left.
fn tidy(root: &Path) {
    let store = paths::store_dir(root);
    let _ = fs::remove_dir(store.join("Work"));
    let _ = fs::remove_dir(paths::copies_dir(root));
    let _ = fs::remove_dir(&store);
}

struct Moved {
    /// The old copy could not be removed after the new one was complete.
    duplicate: bool,
}

enum MoveError {
    Cancelled,
    NoSpace { needed: u64, free: u64 },
    Io(io::Error),
}

impl From<io::Error> for MoveError {
    fn from(error: io::Error) -> Self {
        MoveError::Io(error)
    }
}

fn move_one(
    from: &Path,
    to: &Path,
    name: &str,
    mode: LinkMode,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f64),
) -> Result<Moved, MoveError> {
    let source = paths::copies_dir(from).join(name);
    let target_dir = paths::copies_dir(to);
    let target = target_dir.join(name);
    let partial = target_dir.join(format!("{name}{PARTIAL}"));
    fs::create_dir_all(&target_dir)?;

    if target.exists() {
        // A move that was cut off after the new copy was whole but before the old one was
        // removed. Only an identical tree counts as that; anything else stays untouched.
        let (old, new) = (fsx::plan_tree(&source, &[])?, fsx::plan_tree(&target, &[])?);
        if (old.files, old.bytes) == (new.files, new.bytes) {
            let duplicate = fsx::remove_tree_retry(&source).is_err();
            return Ok(Moved { duplicate });
        }
        return Err(MoveError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "a different copy of this game is already at the new place",
        )));
    }
    let _ = fsx::remove_tree(&partial);

    // Same volume: one rename, nothing is copied, hard links stay hard links.
    if mode == LinkMode::Auto && fs::rename(&source, &target).is_ok() {
        progress(1.0);
        return Ok(Moved { duplicate: false });
    }

    let plan = fsx::plan_tree(&source, &[])?;
    if let Some(free) = fsx::free_space(&target_dir) {
        let needed = plan.bytes + HEADROOM;
        if free < needed {
            return Err(MoveError::NoSpace { needed, free });
        }
    }
    let linker = Linker::new(LinkMode::ForceCopy);
    let opts = TreeOpts::new(cancel);
    let result = fsx::link_tree_planned(&source, &plan, &partial, &linker, &opts, progress);
    let finish = match result {
        Ok(()) => verify(&plan, &partial),
        Err(TreeError::Cancelled) => {
            let _ = fsx::remove_tree(&partial);
            return Err(MoveError::Cancelled);
        }
        Err(TreeError::Io { relative, source }) => Err(io::Error::new(
            source.kind(),
            format!("{relative}: {source}"),
        )),
    };
    if let Err(error) = finish {
        let _ = fsx::remove_tree(&partial);
        return Err(error.into());
    }
    if let Err(error) = fsx::rename_retry(&partial, &target) {
        let _ = fsx::remove_tree(&partial);
        return Err(error.into());
    }
    // Whole at the new place; the old one goes last.
    let duplicate = fsx::remove_tree_retry(&source).is_err();
    Ok(Moved { duplicate })
}

/// The copy at the new place has every file of the old one, in full.
fn verify(old: &fsx::TreePlan, new_root: &Path) -> io::Result<()> {
    let new = fsx::plan_tree(new_root, &[])?;
    if (old.files, old.bytes) == (new.files, new.bytes) {
        Ok(())
    } else {
        Err(io::Error::other(
            "the copy at the new place is not the size of the old one",
        ))
    }
}

/// The path of a copy (for messages and tests).
pub fn copy_path(root: &Path, name: &str) -> PathBuf {
    paths::copies_dir(root).join(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn copy(root: &Path, name: &str, save: &str) {
        let dir = copy_path(root, name);
        write(&dir.join("eboot.exe"), "game");
        write(&dir.join("data/big.bin"), &"x".repeat(2000));
        write(&dir.join("_sd/slot0.sav"), save);
    }

    fn quiet() -> impl FnMut(Progress) {
        |_| {}
    }

    #[test]
    fn same_volume_move_is_a_rename_and_keeps_saves_and_links() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("games"), dir.path().join("fast"));
        fs::create_dir_all(&to).unwrap();
        let dump = dir.path().join("games/Stray/data.pak");
        write(&dump, "dump bytes");
        copy(&from, "PPSA1-a", "SAVE ONE");
        let linked = copy_path(&from, "PPSA1-a").join("data.pak");
        fs::hard_link(&dump, &linked).unwrap();

        let report = move_all(
            &from,
            &to,
            LinkMode::Auto,
            &|_| false,
            &CancelToken::new(),
            &mut quiet(),
        );
        assert_eq!(report.moved, ["PPSA1-a"]);
        assert!(report.complete());
        let moved = copy_path(&to, "PPSA1-a");
        assert_eq!(
            fs::read_to_string(moved.join("_sd/slot0.sav")).unwrap(),
            "SAVE ONE"
        );
        assert!(!copy_path(&from, "PPSA1-a").exists());
        assert!(
            !paths::store_dir(&from).exists(),
            "the emptied store is tidied away"
        );
        assert_eq!(
            fs::read_to_string(&dump).unwrap(),
            "dump bytes",
            "the dump is untouched"
        );
    }

    #[test]
    fn a_cancel_leaves_the_rest_whole_at_the_old_place() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a"), dir.path().join("b"));
        copy(&from, "G1-1", "one");
        copy(&from, "G2-2", "two");
        copy(&from, "G3-3", "three");
        let cancel = CancelToken::new();
        // Cancels when the second copy starts.
        let mut starts = 0;
        let mut progress = |p: Progress| {
            if p.done_bytes == 0 || p.current == "G2-2" {
                starts += 1;
            }
            if p.current == "G2-2" {
                cancel.cancel();
            }
        };
        let report = move_all(
            &from,
            &to,
            LinkMode::ForceCopy,
            &|_| false,
            &cancel,
            &mut progress,
        );
        assert!(report.cancelled && !report.complete());
        assert_eq!(report.moved, ["G1-1"]);
        assert_eq!(pending(&to), ["G1-1"]);
        assert_eq!(pending(&from), ["G2-2", "G3-3"]);
        for name in ["G2-2", "G3-3"] {
            assert!(copy_path(&from, name).join("_sd/slot0.sav").is_file());
        }
        assert!(
            !paths::copies_dir(&to).join("G2-2.partial").exists(),
            "no half-made copy at the new place"
        );
        assert!(report.left.iter().all(|(_, why)| *why == Left::Cancelled));
        assert!(starts > 0);

        // A later move picks up where this one stopped.
        let again = move_all(
            &from,
            &to,
            LinkMode::Auto,
            &|_| false,
            &CancelToken::new(),
            &mut quiet(),
        );
        assert!(again.complete());
        assert_eq!(pending(&to), ["G1-1", "G2-2", "G3-3"]);
        assert!(pending(&from).is_empty());
    }

    #[test]
    fn a_running_game_is_never_moved() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a"), dir.path().join("b"));
        copy(&from, "G1-1", "one");
        copy(&from, "G2-2", "two");
        let report = move_all(
            &from,
            &to,
            LinkMode::Auto,
            &|name| name == "G1-1",
            &CancelToken::new(),
            &mut quiet(),
        );
        assert_eq!(report.moved, ["G2-2"]);
        assert_eq!(report.left, [("G1-1".to_string(), Left::Running)]);
        assert!(!report.complete());
        assert!(copy_path(&from, "G1-1").join("eboot.exe").is_file());
    }

    #[test]
    fn a_copy_cut_off_midway_is_swept_and_the_source_is_whole() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a"), dir.path().join("b"));
        copy(&from, "G1-1", "one");
        // What a killed move leaves: a half-made copy at the new place.
        write(
            &paths::copies_dir(&to).join("G1-1.partial/eboot.exe"),
            "gam",
        );
        assert!(pending(&to).is_empty(), "a partial is not a copy");
        crate::unlock::remove_leftovers(&to);
        assert!(!paths::copies_dir(&to).join("G1-1.partial").exists());
        assert!(copy_path(&from, "G1-1").join("_sd/slot0.sav").is_file());
        let report = move_all(
            &from,
            &to,
            LinkMode::Auto,
            &|_| false,
            &CancelToken::new(),
            &mut quiet(),
        );
        assert!(report.complete());
    }

    #[test]
    fn a_move_cut_off_after_the_new_copy_was_whole_finishes_by_removing_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a"), dir.path().join("b"));
        copy(&from, "G1-1", "one");
        copy(&to, "G1-1", "one");
        let report = move_all(
            &from,
            &to,
            LinkMode::Auto,
            &|_| false,
            &CancelToken::new(),
            &mut quiet(),
        );
        assert_eq!(report.moved, ["G1-1"]);
        assert!(report.duplicates.is_empty());
        assert!(pending(&from).is_empty());
    }

    #[test]
    fn a_different_copy_at_the_new_place_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a"), dir.path().join("b"));
        copy(&from, "G1-1", "old save");
        copy(&to, "G1-1", "new save, longer");
        let report = move_all(
            &from,
            &to,
            LinkMode::Auto,
            &|_| false,
            &CancelToken::new(),
            &mut quiet(),
        );
        assert!(matches!(report.left[0].1, Left::Failed(_)));
        assert_eq!(
            fs::read_to_string(copy_path(&from, "G1-1").join("_sd/slot0.sav")).unwrap(),
            "old save"
        );
        assert_eq!(
            fs::read_to_string(copy_path(&to, "G1-1").join("_sd/slot0.sav")).unwrap(),
            "new save, longer"
        );
    }

    #[test]
    fn across_volumes_the_copy_is_copied_whole_then_the_old_one_goes() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a"), dir.path().join("b"));
        let dump = dir.path().join("dump.pak");
        write(&dump, "dump bytes");
        copy(&from, "G1-1", "SAVE");
        let linked = copy_path(&from, "G1-1").join("data.pak");
        fs::hard_link(&dump, &linked).unwrap();
        let report = move_all(
            &from,
            &to,
            LinkMode::ForceCopy,
            &|_| false,
            &CancelToken::new(),
            &mut quiet(),
        );
        assert_eq!(report.moved, ["G1-1"]);
        assert!(report.complete() && report.duplicates.is_empty());
        let new = copy_path(&to, "G1-1");
        assert_eq!(
            fs::read_to_string(new.join("_sd/slot0.sav")).unwrap(),
            "SAVE"
        );
        assert_eq!(
            fs::read_to_string(new.join("data.pak")).unwrap(),
            "dump bytes"
        );
        assert!(!copy_path(&from, "G1-1").exists());
        assert!(!paths::copies_dir(&to).join("G1-1.partial").exists());
        // A full copy: it shares no storage with the dump, so removing the dump keeps the game whole.
        fs::remove_file(&dump).unwrap();
        assert_eq!(
            fs::read_to_string(new.join("data.pak")).unwrap(),
            "dump bytes"
        );
    }

    #[test]
    fn a_cancel_in_the_middle_of_a_copy_removes_the_partial_and_keeps_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a"), dir.path().join("b"));
        copy(&from, "G1-1", "one");
        copy(&from, "G2-2", "two");
        let cancel = CancelToken::new();
        let report = move_all(
            &from,
            &to,
            LinkMode::ForceCopy,
            &|_| false,
            &cancel,
            &mut |p| {
                // The first copy has begun: cancel before its files are done.
                if p.current == "G1-1" {
                    cancel.cancel();
                }
            },
        );
        assert!(report.cancelled);
        assert!(report.moved.is_empty());
        assert_eq!(pending(&from), ["G1-1", "G2-2"]);
        assert!(copy_path(&from, "G1-1").join("_sd/slot0.sav").is_file());
        assert!(pending(&to).is_empty());
        assert!(!paths::copies_dir(&to).join("G1-1.partial").exists());
    }

    #[test]
    fn progress_reaches_the_total() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a"), dir.path().join("b"));
        copy(&from, "G1-1", "one");
        copy(&from, "G2-2", "two");
        let last = AtomicUsize::new(0);
        let mut seen = Vec::new();
        let report = move_all(
            &from,
            &to,
            LinkMode::Auto,
            &|_| false,
            &CancelToken::new(),
            &mut |p| {
                last.store(p.total, Ordering::Relaxed);
                seen.push(p);
            },
        );
        assert!(report.complete());
        assert_eq!(last.load(Ordering::Relaxed), 2);
        assert!(seen.windows(2).all(|w| w[0].done_bytes <= w[1].done_bytes));
        assert!(seen.iter().all(|p| p.done_bytes <= p.total_bytes));
    }
}
