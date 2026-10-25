//! The self-update swap on a real program that is running: Windows refuses to overwrite or delete
//! a running exe but allows renaming it, and the swap relies on exactly that.
//!
//! The "launcher" is the fake relinker copied to the exe name an install has: the old
//! `AnyPS5-Launcher.exe` in the first test (an install from v0.1.x keeps it), the new
//! `AnyPlay-Launcher.exe` in the second. It is kept running with its `--fake-sleeper` mode; the
//! "update" is a second copy with a marker appended.
#![cfg(windows)]

mod support;

use std::fs;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use anyps5_core::paths::AppPaths;
use anyps5_core::selfupdate::{Platform, Staged, cleanup, pending, updates_dir};
use support::*;

const MARKER: &[u8] = b"\0\0SELF-UPDATE-MARKER-0.2.0";

fn start(exe: &std::path::Path) -> Child {
    Command::new(exe)
        .arg("--fake-sleeper")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the running launcher")
}

#[test]
fn a_running_exe_is_replaced_and_its_old_file_removed_after_it_stops() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(root.path());
    let folder = root.path().join("Programs").join("AnyPS5 Launcher");
    fs::create_dir_all(&folder).unwrap();
    let exe = folder.join("AnyPS5-Launcher.exe");
    fs::copy(fake_binary(), &exe).unwrap();
    configure_fake(&folder, &["FAKE_CHILD_MS=60000"]);
    let original = fs::read(&exe).unwrap();

    // The update: the same program with a marker, staged where the downloader puts it.
    let mut updated = original.clone();
    updated.extend_from_slice(MARKER);
    // An install of the old name takes an update that arrives under the new name.
    let path = updates_dir(&paths).join("AnyPlay-Launcher-0.2.0-x64.exe");
    write(&path, &updated);
    let staged = Staged {
        version: "0.2.0".into(),
        path,
        sha256: None,
        platform: Platform::Windows,
    };

    let mut running = start(&exe);
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        running.try_wait().unwrap().is_none(),
        "the launcher is running"
    );
    assert!(
        fs::OpenOptions::new().write(true).open(&exe).is_err(),
        "a running exe cannot be overwritten (this is why the swap renames)"
    );

    staged.swap_in(&exe).expect("swap while the exe runs");

    let old = folder.join("AnyPS5-Launcher.exe.old");
    println!(
        "running pid {} after the swap: {:?}",
        running.id(),
        running.try_wait()
    );
    println!(
        "{} is now {} bytes (was {})",
        exe.display(),
        fs::metadata(&exe).unwrap().len(),
        original.len()
    );
    println!(
        "{} is {} bytes",
        old.display(),
        fs::metadata(&old).unwrap().len()
    );
    assert!(
        running.try_wait().unwrap().is_none(),
        "the old process was not disturbed"
    );
    assert_eq!(fs::read(&exe).unwrap(), updated, "the new exe is in place");
    assert_eq!(
        fs::read(&old).unwrap(),
        original,
        "the running one moved to .old"
    );

    // While the old process runs its file cannot be deleted; the cleanup leaves it, no error.
    cleanup(&exe, &paths, "0.2.0");
    assert!(old.exists(), "a running .old stays until the next start");

    running.kill().unwrap();
    running.wait().unwrap();
    cleanup(&exe, &paths, "0.2.0");
    assert!(!old.exists(), "the next start removes .old");
    assert_eq!(fs::read(&exe).unwrap(), updated);
    assert!(pending(&paths, "0.2.0").is_none());
}

#[test]
fn restart_swaps_and_starts_the_new_exe() {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(root.path());
    let exe = root.path().join("AnyPlay-Launcher.exe");
    fs::copy(fake_binary(), &exe).unwrap();
    configure_fake(root.path(), &["FAKE_CHILD_MS=1200"]);
    let mut updated = fs::read(&exe).unwrap();
    updated.extend_from_slice(MARKER);
    let path = updates_dir(&paths).join("AnyPlay-Launcher-0.2.0-x64.exe");
    write(&path, &updated);

    let mut running = start(&exe);
    std::thread::sleep(Duration::from_millis(300));
    let staged = Staged {
        version: "0.2.0".into(),
        path,
        sha256: None,
        platform: Platform::Windows,
    };
    staged
        .restart(&exe, ["--fake-sleeper".into()])
        .expect("swap and start the new exe");
    assert_eq!(fs::read(&exe).unwrap(), updated);
    assert!(running.try_wait().unwrap().is_none());
    running.kill().unwrap();
    running.wait().unwrap();
    // The restarted process runs from the new file; let it finish before the folder is removed.
    std::thread::sleep(Duration::from_millis(1800));
}
