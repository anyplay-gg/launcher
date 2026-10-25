//! File system helpers of the unlock pipeline: hard links with a copy fallback, tree linking with
//! progress, retrying removal and renaming, size accounting.
//!
//! Hazard (D4): a hard-linked file *is* the dump's file. Nothing may open a file under `app0` for
//! writing; this module only ever creates new files, removes, or renames.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use crate::error::CancelToken;

/// Test hook: forces the copy fallback without needing a second volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LinkMode {
    #[default]
    Auto,
    ForceCopy,
}

/// Links files where the volume allows it and copies them where it does not. Remembers why it
/// first fell back, so the caller can log one note (PR3786's "files copied, not linked").
#[derive(Debug, Default)]
pub struct Linker {
    mode: LinkMode,
    first_fallback: Option<String>,
    linked: u64,
    copied: u64,
}

impl Linker {
    pub fn new(mode: LinkMode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }

    /// Number of files linked and copied so far.
    pub fn counts(&self) -> (u64, u64) {
        (self.linked, self.copied)
    }

    /// The reason of the first fallback to copying, once; later calls return `None`.
    pub fn take_note(&mut self) -> Option<String> {
        self.first_fallback
            .take()
            .map(|reason| format!("files copied, not linked: {reason}"))
    }

    /// Puts `src` at `dst`. The destination must not exist (see [`replace_file`]).
    pub fn link_or_copy(&mut self, src: &Path, dst: &Path, cancel: &CancelToken) -> io::Result<()> {
        if cancel.is_cancelled() {
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        if self.mode == LinkMode::Auto {
            match share_file(src, dst) {
                Ok(()) => {
                    self.linked += 1;
                    return Ok(());
                }
                Err(error) => {
                    if !src.exists() {
                        return Err(error);
                    }
                    if self.first_fallback.is_none() {
                        self.first_fallback = Some(error.to_string());
                    }
                }
            }
        } else if self.first_fallback.is_none() {
            self.first_fallback = Some("copy mode forced".to_string());
        }
        copy_cancellable(src, dst, cancel)?;
        self.copied += 1;
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
fn share_file(src: &Path, dst: &Path) -> io::Result<()> {
    fs::hard_link(src, dst)
}

/// APFS clones share storage without sharing writes made by a running game with its dump.
#[cfg(target_os = "macos")]
fn share_file(src: &Path, dst: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let src = CString::new(src.as_os_str().as_bytes())?;
    let dst = CString::new(dst.as_os_str().as_bytes())?;
    // SAFETY: both paths are NUL terminated and live through this synchronous call.
    if unsafe { libc::clonefile(src.as_ptr(), dst.as_ptr(), 0) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// Copies a file in 256 KiB steps, checking `cancel` between steps. A cancelled copy removes
/// its partial destination and fails with `Interrupted`.
pub fn copy_cancellable(src: &Path, dst: &Path, cancel: &CancelToken) -> io::Result<()> {
    let mut input = fs::File::open(src)?;
    let mut output = fs::File::create_new(dst)?;
    let mut buffer = vec![0u8; 256 * 1024];
    let result = (|| -> io::Result<()> {
        loop {
            if cancel.is_cancelled() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = input.read(&mut buffer)?;
            if n == 0 {
                return Ok(());
            }
            output.write_all(&buffer[..n])?;
        }
    })();
    drop(output);
    if result.is_err() {
        let _ = fs::remove_file(dst);
    }
    result
}

/// Removes `dst` if it exists (clearing a read-only flag), so that a link or copy never writes
/// through an existing link to a dump file.
pub fn replace_file(dst: &Path) -> io::Result<()> {
    match fs::symlink_metadata(dst) {
        Ok(meta) => {
            if meta.is_dir() {
                return remove_tree(dst);
            }
            if fs::remove_file(dst).is_err() {
                clear_readonly(dst);
                fs::remove_file(dst)?;
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn clear_readonly(path: &Path) {
    if let Ok(meta) = fs::symlink_metadata(path) {
        let mut perms = meta.permissions();
        if perms.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = fs::set_permissions(path, perms);
        }
    }
}

/// Whether a directory entry is hidden: a dot name, or the Windows hidden attribute.
pub fn is_hidden(entry: &fs::DirEntry) -> bool {
    if entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if let Ok(meta) = entry.metadata() {
            return meta.file_attributes() & 0x2 != 0;
        }
    }
    false
}

/// Recursive delete that clears read-only flags where needed. Hard links mean the dump behind a
/// linked file is untouched.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => {
            clear_readonly_tree(path);
            fs::remove_dir_all(path)
        }
    }
}

fn clear_readonly_tree(path: &Path) {
    clear_readonly(path);
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                clear_readonly_tree(&p);
            } else {
                clear_readonly(&p);
            }
        }
    }
}

/// Retry policy of D22: antivirus and the indexer may hold a file for a moment.
pub const RETRIES: u32 = 8;
pub const RETRY_PAUSE: Duration = Duration::from_millis(150);

/// `remove_tree` with retries (D22).
pub fn remove_tree_retry(path: &Path) -> io::Result<()> {
    retrying(|| remove_tree(path))
}

/// `fs::rename` with retries (D22).
pub fn rename_retry(from: &Path, to: &Path) -> io::Result<()> {
    retrying(|| fs::rename(from, to))
}

fn retrying(mut step: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut last = None;
    for attempt in 0..RETRIES {
        match step() {
            Ok(()) => return Ok(()),
            Err(error) => {
                last = Some(error);
                if attempt + 1 < RETRIES {
                    thread::sleep(RETRY_PAUSE);
                }
            }
        }
    }
    Err(last.expect("at least one attempt"))
}

/// An entry found while walking a tree.
#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub relative: PathBuf,
    pub is_dir: bool,
    pub size: u64,
}

/// Lists a tree recursively in name order, directories before their content. Symbolic links to
/// files count as files (their target's size), links to folders are not followed.
/// `skip_top` names (compared case-insensitively) are left out at the top level.
pub fn walk(root: &Path, include_hidden: bool, skip_top: &[&str]) -> io::Result<Vec<TreeEntry>> {
    fn visit(
        root: &Path,
        relative: &Path,
        include_hidden: bool,
        skip_top: &[&str],
        out: &mut Vec<TreeEntry>,
    ) -> io::Result<()> {
        let mut entries: Vec<fs::DirEntry> =
            fs::read_dir(root.join(relative))?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let name = entry.file_name();
            if relative.as_os_str().is_empty()
                && skip_top
                    .iter()
                    .any(|s| name.to_string_lossy().eq_ignore_ascii_case(s))
            {
                continue;
            }
            if !include_hidden && is_hidden(&entry) {
                continue;
            }
            let rel = relative.join(&name);
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                out.push(TreeEntry {
                    relative: rel.clone(),
                    is_dir: true,
                    size: 0,
                });
                visit(root, &rel, include_hidden, skip_top, out)?;
            } else if file_type.is_file() {
                out.push(TreeEntry {
                    relative: rel,
                    is_dir: false,
                    size: entry.metadata()?.len(),
                });
            } else if file_type.is_symlink()
                && let Ok(meta) = fs::metadata(entry.path())
                && meta.is_file()
            {
                out.push(TreeEntry {
                    relative: rel,
                    is_dir: false,
                    size: meta.len(),
                });
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(root, Path::new(""), include_hidden, skip_top, &mut out)?;
    Ok(out)
}

/// Sum of the sizes of the regular files below `folder` (logical size, D17); links are not
/// followed. Stops early when cancelled.
pub fn tree_size(folder: &Path, cancel: &CancelToken) -> u64 {
    fn visit(dir: &Path, cancel: &CancelToken) -> u64 {
        let Ok(entries) = fs::read_dir(dir) else {
            return 0;
        };
        let mut total = 0;
        for entry in entries.flatten() {
            if cancel.is_cancelled() {
                return total;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                total += visit(&entry.path(), cancel);
            } else if file_type.is_file() {
                total += entry.metadata().map_or(0, |m| m.len());
            }
        }
        total
    }
    visit(folder, cancel)
}

/// Why `link_tree` stopped.
#[derive(Debug)]
pub enum TreeError {
    Cancelled,
    Io { relative: String, source: io::Error },
}

/// Mirrors `source` into `target`: folders are created, every file is removed at the
/// destination and then linked or copied. `progress` receives the share of bytes done (0..=1).
/// Hidden files are included. `skip_top` names are left out at the top level.
pub fn link_tree(
    source: &Path,
    target: &Path,
    linker: &mut Linker,
    skip_top: &[&str],
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f64),
) -> Result<(), TreeError> {
    let io_error = |relative: &Path, source: io::Error| TreeError::Io {
        relative: relative.to_string_lossy().replace('\\', "/"),
        source,
    };
    let entries = walk(source, true, skip_top).map_err(|e| io_error(Path::new(""), e))?;
    let total: u64 = entries.iter().map(|e| e.size).sum();
    for entry in entries.iter().filter(|e| e.is_dir) {
        fs::create_dir_all(target.join(&entry.relative))
            .map_err(|e| io_error(&entry.relative, e))?;
    }
    let mut done = 0u64;
    for entry in entries.iter().filter(|e| !e.is_dir) {
        if cancel.is_cancelled() {
            return Err(TreeError::Cancelled);
        }
        let destination = target.join(&entry.relative);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(&entry.relative, e))?;
        }
        replace_file(&destination).map_err(|e| io_error(&entry.relative, e))?;
        match linker.link_or_copy(&source.join(&entry.relative), &destination, cancel) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted && cancel.is_cancelled() => {
                return Err(TreeError::Cancelled);
            }
            Err(error) => return Err(io_error(&entry.relative, error)),
        }
        done += entry.size;
        progress(if total == 0 {
            1.0
        } else {
            done as f64 / total as f64
        });
    }
    if total == 0 {
        progress(1.0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn links_a_tree_and_skips_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        write(&src.join("a.txt"), "aaa");
        write(&src.join("sub/b.txt"), "bb");
        write(&src.join(".hidden"), "h");
        write(&src.join(".AnyPS5/Games/x"), "store");
        fs::create_dir_all(src.join("emptydir")).unwrap();
        let dst = dir.path().join("dst");
        fs::create_dir_all(&dst).unwrap();
        let mut seen = Vec::new();
        let mut linker = Linker::default();
        link_tree(
            &src,
            &dst,
            &mut linker,
            &[".AnyPS5"],
            &CancelToken::new(),
            &mut |f| seen.push(f),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(dst.join("sub/b.txt")).unwrap(), "bb");
        assert_eq!(fs::read_to_string(dst.join(".hidden")).unwrap(), "h");
        assert!(dst.join("emptydir").is_dir());
        assert!(!dst.join(".AnyPS5").exists());
        assert!(seen.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(*seen.last().unwrap(), 1.0);
        assert_eq!(linker.counts().0 + linker.counts().1, 3);
    }

    #[test]
    fn overlay_replaces_and_never_writes_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let dump = dir.path().join("dump");
        write(&dump.join("m.prx"), "ORIGINAL");
        let app0 = dir.path().join("app0");
        fs::create_dir_all(&app0).unwrap();
        let mut linker = Linker::default();
        let cancel = CancelToken::new();
        link_tree(&dump, &app0, &mut linker, &[], &cancel, &mut |_| {}).unwrap();
        let before = fs::metadata(dump.join("m.prx"))
            .unwrap()
            .modified()
            .unwrap();

        // The converted module is laid over the linked file.
        let out = dir.path().join("out");
        write(&out.join("m.prx"), "CONVERTED");
        link_tree(&out, &app0, &mut linker, &[], &cancel, &mut |_| {}).unwrap();

        assert_eq!(fs::read_to_string(app0.join("m.prx")).unwrap(), "CONVERTED");
        assert_eq!(fs::read_to_string(dump.join("m.prx")).unwrap(), "ORIGINAL");
        assert_eq!(
            fs::metadata(dump.join("m.prx"))
                .unwrap()
                .modified()
                .unwrap(),
            before
        );
    }

    #[test]
    fn forced_copy_makes_independent_files_and_a_note() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        write(&src.join("a.txt"), "aaa");
        let dst = dir.path().join("dst");
        fs::create_dir_all(&dst).unwrap();
        let mut linker = Linker::new(LinkMode::ForceCopy);
        link_tree(
            &src,
            &dst,
            &mut linker,
            &[],
            &CancelToken::new(),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(linker.counts(), (0, 1));
        assert!(
            linker
                .take_note()
                .unwrap()
                .starts_with("files copied, not linked")
        );
        assert!(linker.take_note().is_none());
        fs::write(dst.join("a.txt"), "changed").unwrap();
        assert_eq!(fs::read_to_string(src.join("a.txt")).unwrap(), "aaa");
    }

    #[test]
    fn cancelled_tree_stops_with_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        write(&src.join("a.txt"), "aaa");
        let dst = dir.path().join("dst");
        fs::create_dir_all(&dst).unwrap();
        let cancel = CancelToken::new();
        cancel.cancel();
        let result = link_tree(
            &src,
            &dst,
            &mut Linker::default(),
            &[],
            &cancel,
            &mut |_| {},
        );
        assert!(matches!(result, Err(TreeError::Cancelled)));
    }

    #[test]
    fn sizes_and_removal() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a"), "12345");
        write(&dir.path().join("d/b"), "123");
        assert_eq!(tree_size(dir.path(), &CancelToken::new()), 8);
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(tree_size(dir.path(), &cancel), 0);

        let mut perms = fs::metadata(dir.path().join("d/b")).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(dir.path().join("d/b"), perms).unwrap();
        remove_tree(&dir.path().join("d")).unwrap();
        assert!(!dir.path().join("d").exists());
        remove_tree(&dir.path().join("never-existed")).unwrap();
    }

    #[test]
    fn walk_hides_dotfiles_unless_asked() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a"), "1");
        write(&dir.path().join(".b"), "2");
        assert_eq!(walk(dir.path(), false, &[]).unwrap().len(), 1);
        assert_eq!(walk(dir.path(), true, &[]).unwrap().len(), 2);
    }
}
