//! The unlock pipeline against the fake relinker: no game data, no real toolkit.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyps5_core::fsx::LinkMode;
use anyps5_core::paths::GameId;
use anyps5_core::toolkit::Toolkit;
use anyps5_core::unlock::{
    self, EXECUTABLE, Step, SwapFault, UnlockControl, UnlockError, UnlockEvent, UnlockQueue,
    UnlockRequest, inject_swap_fault, run_unlock,
};
use support::*;

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    toolkit: Toolkit,
}

fn world(relinker_settings: &[&str]) -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("games");
    fs::create_dir_all(&root).unwrap();
    let toolkit = make_toolkit(&dir.path().join("toolkit"), relinker_settings);
    World {
        root,
        toolkit,
        _dir: dir,
    }
}

impl World {
    fn dump(&self, name: &str, title_id: &str, eboot: &[u8]) -> UnlockRequest {
        let folder = self.root.join(name);
        make_dump(&folder, title_id, eboot);
        UnlockRequest::new(
            folder,
            title_id.into(),
            self.root.clone(),
            self.toolkit.clone(),
        )
    }
}

fn run(request: &UnlockRequest) -> (Result<unlock::UnlockRecord, UnlockError>, Vec<UnlockEvent>) {
    let events = Events::new();
    let sink = events.clone();
    let result = run_unlock(request, &UnlockControl::new(), &move |e| sink.push(e));
    let events = Arc::try_unwrap(events.0)
        .ok()
        .unwrap()
        .into_inner()
        .unwrap();
    (result, events)
}

fn logs(events: &[UnlockEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            UnlockEvent::Log { line, .. } => Some(line.clone()),
            _ => None,
        })
        .collect()
}

fn args_of(exe: &Path) -> Vec<String> {
    fs::read_to_string(exe)
        .unwrap()
        .lines()
        .filter_map(|l| l.strip_prefix("arg=").map(String::from))
        .collect()
}

fn no_leftovers(root: &Path) {
    let store = root.join(".AnyPS5");
    if let Ok(entries) = fs::read_dir(store.join("Work")) {
        assert_eq!(entries.count(), 0, "work folder left behind");
    }
    if let Ok(entries) = fs::read_dir(store.join("Games")) {
        for e in entries.flatten() {
            assert!(
                !e.file_name().to_string_lossy().ends_with(".partial"),
                "partial copy left behind"
            );
            assert!(
                !e.file_name().to_string_lossy().ends_with(".previous"),
                "set-aside copy left behind"
            );
        }
    }
}

#[test]
fn unlocks_a_dump_end_to_end() {
    let w = world(&[]);
    let mut request = w.dump("Stray", "PPSA02100", &self_v1());
    request.to_intel = true;
    let (result, events) = run(&request);
    let record = result.expect("unlock succeeds");

    // The record.
    let copy = unlock::unlocked_copy("PPSA02100", &request.game, &w.root);
    assert_eq!(unlock::read_record(&copy), Some(record.clone()));
    assert_eq!(record.title_id, "PPSA02100");
    assert_eq!(record.toolkit, "0.1.3-aaaaaaaaaaaa");
    assert_eq!(PathBuf::from(&record.source), request.folder);
    assert!(
        record.date.ends_with('Z') && record.date.len() == 20,
        "{}",
        record.date
    );

    // The layout.
    assert!(copy.join(EXECUTABLE).is_file());
    assert_eq!(
        fs::read(copy.join("app0/data/level1.pak")).unwrap(),
        b"LEVEL-ONE-DATA"
    );
    assert_eq!(
        fs::read(copy.join("app0/eboot.bin")).unwrap(),
        self_v1(),
        "original executable stays in app0"
    );
    assert!(copy.join("libs/libSceLibc.prx").is_file());
    for dll in MINGW {
        assert!(copy.join("libs").join(dll).is_file());
    }
    // Converted modules are laid over the dump's files, which stay as they were.
    assert_eq!(
        fs::read(copy.join("app0/sce_module/libFoo.prx.guest.prx")).unwrap(),
        b"FAKE-GUEST-MODULE"
    );
    assert_eq!(
        fs::read(copy.join("app0/Media/Plugins/libPlugin.prx.guest.prx")).unwrap(),
        b"FAKE-GUEST-MODULE"
    );
    assert_eq!(
        fs::read(copy.join("app0/sce_module/libFoo.prx")).unwrap(),
        elf_bytes()
    );
    assert_eq!(
        fs::read(request.folder.join("sce_module/libFoo.prx")).unwrap(),
        elf_bytes()
    );
    assert!(
        !request
            .folder
            .join("sce_module/libFoo.prx.guest.prx")
            .exists(),
        "nothing written into the dump"
    );
    no_leftovers(&w.root);

    // What the relinker was asked.
    let args = args_of(&copy.join(EXECUTABLE));
    let format_args = anyps5_core::platform::Os::current().relinker_args();
    let (format, args) = args.split_at(format_args.len());
    let cpu_flag = if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "--to-rosetta"
    } else {
        "--to-intel"
    };
    assert_eq!(format, format_args);
    assert_eq!(
        &args[..5],
        [
            cpu_flag,
            "--module-dir",
            ".",
            "--module-dir",
            "Media/Plugins"
        ]
    );
    assert_eq!(args[5], "eboot.bin");
    assert!(args[6].ends_with(EXECUTABLE));
    let exe_text = fs::read_to_string(copy.join(EXECUTABLE)).unwrap();
    assert!(
        exe_text.contains("icon=true"),
        "the title icon is staged beside the executable"
    );

    // The events.
    assert!(matches!(events.first(), Some(UnlockEvent::Started { .. })));
    let log = logs(&events);
    assert!(
        log.iter().any(|l| l == "eboot.bin: unwrapped, 4352 bytes"),
        "{log:?}"
    );
    assert!(
        log.iter().any(|l| l.contains("Write output")),
        "relinker lines are logged: {log:?}"
    );
    let progress: Vec<(Step, f64)> = events
        .iter()
        .filter_map(|e| match e {
            UnlockEvent::Progress { step, fraction, .. } => Some((*step, *fraction)),
            _ => None,
        })
        .collect();
    let order = |s: Step| match s {
        Step::Unwrap => 0,
        Step::Relink => 1,
        Step::Copy => 2,
        Step::Assemble => 3,
        Step::Finish => 4,
    };
    assert_eq!(progress.first(), Some(&(Step::Unwrap, 0.0)));
    assert_eq!(progress.last(), Some(&(Step::Finish, 1.0)));
    for pair in progress.windows(2) {
        assert!(
            order(pair[0].0) <= order(pair[1].0),
            "steps never go back: {pair:?}"
        );
        if pair[0].0 == pair[1].0 {
            assert!(pair[1].1 > pair[0].1, "coalesced and rising: {pair:?}");
        }
    }
    let relink: Vec<f64> = progress
        .iter()
        .filter(|p| p.0 == Step::Relink)
        .map(|p| p.1)
        .collect();
    assert!(
        relink.len() > 3 && relink.windows(2).all(|w| w[0] < w[1]),
        "{relink:?}"
    );
}

#[test]
fn the_dumps_files_are_linked_not_copied_when_the_volume_allows() {
    let w = world(&[]);
    let request = w.dump("Linked", "PPSA00001", &elf_bytes());
    run(&request).0.unwrap();
    let copy = unlock::unlocked_copy("PPSA00001", &request.game, &w.root);
    // Append through the dump's path: a link shows it, a copy does not.
    let pak = request.folder.join("data/level1.pak");
    let before = fs::read(&pak).unwrap();
    let mut longer = before.clone();
    longer.extend(b"+");
    fs::write(&pak, &longer).unwrap();
    let seen = fs::read(copy.join("app0/data/level1.pak")).unwrap();
    assert_eq!(
        seen,
        if cfg!(target_os = "macos") {
            before
        } else {
            longer
        },
        "APFS clones isolate writes; Windows/Linux hard links share them"
    );
}

#[test]
fn forced_copy_mode_makes_independent_files_and_says_so() {
    let w = world(&[]);
    let mut request = w.dump("Copied", "PPSA00002", &elf_bytes());
    request.link_mode = LinkMode::ForceCopy;
    let (result, events) = run(&request);
    result.unwrap();
    assert!(
        logs(&events)
            .iter()
            .any(|l| l.starts_with("files copied, not linked"))
    );
    let copy = unlock::unlocked_copy("PPSA00002", &request.game, &w.root);
    fs::write(copy.join("app0/data/level1.pak"), b"changed").unwrap();
    assert_eq!(
        fs::read(request.folder.join("data/level1.pak")).unwrap(),
        b"LEVEL-ONE-DATA"
    );
}

#[test]
fn reconversion_ignores_old_host_guest_modules_but_converts_the_original() {
    let w = world(&[]);
    let request = w.dump("PreviouslyConverted", "PPSA00009", &elf_bytes());
    write(
        &request.folder.join("sce_module/libFoo.prx.guest.prx"),
        b"old host executable",
    );
    let (result, _) = run(&request);
    result.expect("the original module is converted again");
    let copy = unlock::unlocked_copy("PPSA00009", &request.game, &w.root);
    assert_eq!(
        fs::read(copy.join("app0/sce_module/libFoo.prx.guest.prx")).unwrap(),
        b"FAKE-GUEST-MODULE"
    );
    assert_eq!(
        fs::read(request.folder.join("sce_module/libFoo.prx.guest.prx")).unwrap(),
        b"old host executable"
    );
}

#[test]
fn an_elf_is_copied_and_an_esbak_backup_is_preferred_over_a_container() {
    let w = world(&[]);
    let request = w.dump("Plain", "PPSA00003", &elf_bytes());
    let (result, events) = run(&request);
    result.unwrap();
    assert!(!logs(&events).iter().any(|l| l.contains("unwrapped")));

    let request = w.dump("Backup", "PPSA00004", &self_v1());
    write(&request.folder.join("eboot.bin.esbak"), &elf_bytes());
    let (result, events) = run(&request);
    result.unwrap();
    assert!(logs(&events).iter().any(|l| l == "eboot.bin: from .esbak"));
}

#[test]
fn an_encrypted_dump_is_refused_before_the_relinker_runs() {
    let w = world(&["FAKE_RELINKER=sleep"]);
    let request = w.dump("Encrypted", "PPSA00005", &self_encrypted());
    let started = std::time::Instant::now();
    let (result, events) = run(&request);
    let error = result.unwrap_err();
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the sleeping relinker was never started"
    );
    match &error {
        UnlockError::Unwrap { file, source } => {
            assert_eq!(file, "eboot.bin");
            assert!(
                source
                    .reason
                    .starts_with("every load segment looks encrypted")
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(
        error
            .to_string()
            .starts_with("eboot.bin: every load segment looks encrypted")
    );
    assert!(!events.iter().any(|e| matches!(
        e,
        UnlockEvent::Progress {
            step: Step::Relink,
            ..
        }
    )));
    no_leftovers(&w.root);
}

#[test]
fn garbage_is_not_an_executable() {
    let w = world(&[]);
    let request = w.dump("Garbage", "PPSA00006", b"this is not an executable");
    let error = run(&request).0.unwrap_err();
    assert!(matches!(&error, UnlockError::NotAnExecutable { file } if file == "eboot.bin"));
    assert_eq!(
        error.to_string(),
        "eboot.bin is neither an ELF nor a decrypted SELF container (an encrypted dump cannot be used)"
    );
}

#[test]
fn a_failing_relinker_reports_code_and_last_line() {
    let w = world(&["FAKE_RELINKER=fail"]);
    let request = w.dump("Fails", "PPSA00007", &elf_bytes());
    let (result, events) = run(&request);
    match result.unwrap_err() {
        UnlockError::Relinker { code, last_line } => {
            assert_eq!(code, Some(2));
            assert!(last_line.contains("conversion failed"), "{last_line}");
        }
        other => panic!("{other:?}"),
    }
    assert!(
        logs(&events)
            .iter()
            .any(|l| l.contains("conversion failed"))
    );
    assert!(
        unlock::read_record(&unlock::unlocked_copy("PPSA00007", &request.game, &w.root)).is_none()
    );
    no_leftovers(&w.root);
}

#[test]
fn a_relinker_without_output_is_an_error() {
    let w = world(&["FAKE_RELINKER=nothing"]);
    let request = w.dump("Nothing", "PPSA00008", &elf_bytes());
    assert!(matches!(
        run(&request).0.unwrap_err(),
        UnlockError::NoOutput
    ));
    no_leftovers(&w.root);
}

#[test]
fn patches_are_passed_to_the_relinker() {
    let w = world(&[]);
    let mut request = w.dump("Patched", "PPSA00009", &elf_bytes());
    request.patches_toml = Some("format = 1\n".into());
    run(&request).0.unwrap();
    let copy = unlock::unlocked_copy("PPSA00009", &request.game, &w.root);
    let args = args_of(&copy.join(EXECUTABLE));
    let at = args
        .iter()
        .position(|a| a == "--patches")
        .expect("--patches");
    assert!(args[at + 1].ends_with("patches.toml"));
}

#[test]
fn the_store_inside_the_dump_is_not_copied_into_app0() {
    let w = world(&[]);
    // The games folder is the dump itself.
    make_dump(&w.root, "PPSA00010", &elf_bytes());
    let request = UnlockRequest::new(
        w.root.clone(),
        "PPSA00010".into(),
        w.root.clone(),
        w.toolkit.clone(),
    );
    run(&request).0.unwrap();
    let copy = unlock::unlocked_copy("PPSA00010", &request.game, &w.root);
    assert!(copy.join("app0/data/level1.pak").is_file());
    assert!(!copy.join("app0/.AnyPS5").exists());
    // Unlock again from the same place: still no recursion.
    run(&request).0.unwrap();
    assert!(!copy.join("app0/.AnyPS5").exists());
}

#[test]
fn unlocking_again_keeps_saves_and_trophies() {
    let w = world(&[]);
    let request = w.dump("Again", "PPSA00011", &elf_bytes());
    run(&request).0.unwrap();
    let copy = unlock::unlocked_copy("PPSA00011", &request.game, &w.root);
    write(&copy.join("_sd/slot0.sav"), b"SAVE-1");
    write(&copy.join("_trophy/1_0.txt"), b"1 63000000000000000\n");
    write(&copy.join("data/cache.bin"), b"C");
    write(&copy.join("_sd_mem/u1/slot0.bin"), b"MEM");
    write(
        &copy.join("anyps5-entitlements.ini"),
        b"[owned]
",
    );
    write(&copy.join("anyps5-screenshots/a.png"), b"PNG");
    write(&copy.join("shader_cache/v1/a.bin"), b"S");

    // A new toolkit version makes the copy outdated; the player unlocks again.
    write(
        &w.toolkit.root().join("version.txt"),
        b"0.1.4-bbbbbbbbbbbb\n",
    );
    let toolkit = Toolkit::open(w.toolkit.root()).unwrap();
    let mut again = request.clone();
    again.toolkit = toolkit;
    let record = run(&again).0.unwrap();
    assert_eq!(record.toolkit, "0.1.4-bbbbbbbbbbbb");
    assert_eq!(
        unlock::read_record(&copy).unwrap().toolkit,
        "0.1.4-bbbbbbbbbbbb"
    );
    assert_eq!(fs::read(copy.join("_sd/slot0.sav")).unwrap(), b"SAVE-1");
    assert!(copy.join("_trophy/1_0.txt").is_file());
    assert!(copy.join("data/cache.bin").is_file());
    // Everything else the game and the toolkit wrote beside the executable stays too.
    assert_eq!(fs::read(copy.join("_sd_mem/u1/slot0.bin")).unwrap(), b"MEM");
    assert!(copy.join("anyps5-entitlements.ini").is_file());
    assert!(copy.join("anyps5-screenshots/a.png").is_file());
    assert!(copy.join("shader_cache/v1/a.bin").is_file());
    no_leftovers(&w.root);
}

#[test]
fn unlocking_again_backs_up_the_saves_first_when_asked() {
    let w = world(&[]);
    let mut request = w.dump("Backup", "PPSA00015", &elf_bytes());
    let backups = w.root.join("backups");
    request.backup_dir = Some(backups.clone());
    // The first unlock has no copy to protect.
    run(&request).0.unwrap();
    assert!(!backups.exists());
    let copy = unlock::unlocked_copy("PPSA00015", &request.game, &w.root);
    write(&copy.join("_sd/slot0.sav"), b"SAVE-1");
    run(&request).0.unwrap();
    let kept = anyps5_core::saves::SaveData::new(copy.clone(), backups).list();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].reason, anyps5_core::saves::Reason::BeforeUnlock);
    // The saves are also still where the game wants them.
    assert_eq!(fs::read(copy.join("_sd/slot0.sav")).unwrap(), b"SAVE-1");
    // Without a backup folder nothing extra is written.
    let mut quiet = request.clone();
    quiet.backup_dir = None;
    run(&quiet).0.unwrap();
    assert_eq!(
        anyps5_core::saves::SaveData::new(copy, w.root.join("backups"))
            .list()
            .len(),
        1
    );
    no_leftovers(&w.root);
}

#[test]
fn the_record_names_the_release_checksums() {
    let w = world(&[]);
    let request = w.dump("Sums", "PPSA00014", &elf_bytes());
    let record = run(&request).0.unwrap();
    assert_eq!(record.relinker_sha256, None, "a folder toolkit has none");
    write(
        &w.toolkit.root().join("toolkit.json"),
        br#"{"tag":"0.1.3","relinkerSha256":"aa","libsSha256":"bb"}"#,
    );
    let mut released = request.clone();
    released.toolkit = Toolkit::open(w.toolkit.root()).unwrap();
    let record = run(&released).0.unwrap();
    assert_eq!(record.relinker_sha256.as_deref(), Some("aa"));
    assert_eq!(record.libs_sha256.as_deref(), Some("bb"));
    let copy = unlock::unlocked_copy("PPSA00014", &request.game, &w.root);
    assert_eq!(unlock::read_record(&copy), Some(record));
}

#[test]
fn only_allowlisted_relink_flags_are_passed() {
    let w = world(&[]);
    let mut request = w.dump("Flags", "PPSA00015", &elf_bytes());
    request.relink_flags = vec!["--windows-fixed-base".into(), "0x400000".into()];
    run(&request).0.unwrap();
    let copy = unlock::unlocked_copy("PPSA00015", &request.game, &w.root);
    let args = args_of(&copy.join(EXECUTABLE));
    let at = args.iter().position(|a| a == "--windows-fixed-base");
    if cfg!(windows) {
        assert_eq!(args[at.expect("flag") + 1], "0x400000");
    } else {
        assert_eq!(
            at, None,
            "a Windows flag is not passed to the Linux relinker"
        );
    }
    request.relink_flags = vec!["--autorun".into(), "x".into()];
    run(&request).0.unwrap();
    assert!(
        !args_of(&copy.join(EXECUTABLE))
            .iter()
            .any(|a| a == "--autorun")
    );
}

#[test]
fn a_failed_unlock_again_leaves_the_old_copy_untouched() {
    let w = world(&[]);
    let request = w.dump("Keeps", "PPSA00012", &elf_bytes());
    run(&request).0.unwrap();
    let copy = unlock::unlocked_copy("PPSA00012", &request.game, &w.root);
    write(&copy.join("_sd/slot0.sav"), b"SAVE-1");
    let broken = make_toolkit(&w._dir.path().join("broken"), &["FAKE_RELINKER=fail"]);
    let mut again = request.clone();
    again.toolkit = broken;
    // The fake relinker behaves differently through a sidecar file only, so the kept output
    // would be taken for a match; a real different relinker changes the key.
    again.fresh = true;
    assert!(run(&again).0.is_err());
    assert!(unlock::read_record(&copy).is_some());
    assert_eq!(fs::read(copy.join("_sd/slot0.sav")).unwrap(), b"SAVE-1");
    no_leftovers(&w.root);
}

/// Unlocks again with the swap failing at `fault`: the call fails, and the old copy is still the
/// one the player had, saves and trophies byte for byte, with the old executable.
fn assert_failed_swap_loses_nothing(fault: SwapFault, title_id: &str) {
    let w = world(&[]);
    let request = w.dump("Swap", title_id, &elf_bytes());
    run(&request).0.unwrap();
    let copy = unlock::unlocked_copy(title_id, &request.game, &w.root);
    write(&copy.join("_sd/slot0.sav"), b"SAVE-1");
    write(
        &copy.join("_trophy/1_0.txt"),
        b"1 63000000000000000
",
    );
    write(&copy.join("data/cache.bin"), b"C");
    let old_executable = fs::read(copy.join(EXECUTABLE)).unwrap();
    let old_record = unlock::read_record(&copy).unwrap();

    write(
        &w.toolkit.root().join("version.txt"),
        b"0.1.4-bbbbbbbbbbbb
",
    );
    let mut again = request.clone();
    again.toolkit = Toolkit::open(w.toolkit.root()).unwrap();
    inject_swap_fault(Some(fault));
    let (result, _) = run(&again);
    inject_swap_fault(None);
    assert!(result.is_err(), "{fault:?}");

    assert_eq!(fs::read(copy.join("_sd/slot0.sav")).unwrap(), b"SAVE-1");
    assert_eq!(
        fs::read(copy.join("_trophy/1_0.txt")).unwrap(),
        b"1 63000000000000000
"
    );
    assert!(copy.join("data/cache.bin").is_file());
    assert_eq!(fs::read(copy.join(EXECUTABLE)).unwrap(), old_executable);
    assert_eq!(unlock::read_record(&copy), Some(old_record), "{fault:?}");
    no_leftovers(&w.root);
}

#[test]
fn a_swap_that_cannot_set_the_old_copy_aside_changes_nothing() {
    assert_failed_swap_loses_nothing(SwapFault::SetAside, "PPSA00020");
}

#[test]
fn a_swap_that_cannot_place_the_new_copy_brings_the_old_one_back() {
    assert_failed_swap_loses_nothing(SwapFault::PutInPlace, "PPSA00021");
}

#[test]
fn a_swap_that_cannot_keep_the_saves_brings_the_old_copy_back() {
    assert_failed_swap_loses_nothing(SwapFault::KeepSaves, "PPSA00022");
}

#[test]
fn a_missing_toolkit_is_reported_without_other_events() {
    let w = world(&[]);
    let request = w.dump("NoToolkit", "PPSA00013", &elf_bytes());
    fs::remove_file(w.toolkit.relinker()).unwrap();
    let (result, events) = run(&request);
    assert!(matches!(result.unwrap_err(), UnlockError::ToolkitMissing));
    assert!(events.is_empty(), "no Started, no Progress");
}

// ---------------------------------------------------------------------------------------------
// The queue

fn queue() -> (UnlockQueue, Events<UnlockEvent>) {
    let events = Events::new();
    let sink = events.clone();
    (UnlockQueue::new(Arc::new(move |e| sink.push(e))), events)
}

fn finished(events: &[UnlockEvent], game: &GameId) -> Option<bool> {
    events.iter().find_map(|e| match e {
        UnlockEvent::Finished { game: g, result } if g == game => Some(result.is_ok()),
        _ => None,
    })
}

fn tag(e: &UnlockEvent) -> (&'static str, GameId) {
    match e {
        UnlockEvent::Queued { game } => ("queued", game.clone()),
        UnlockEvent::Started { game } => ("started", game.clone()),
        UnlockEvent::Progress { game, .. } => ("progress", game.clone()),
        UnlockEvent::Log { game, .. } => ("log", game.clone()),
        UnlockEvent::Paused { game, .. } => ("paused", game.clone()),
        UnlockEvent::Timings { game, .. } => ("timings", game.clone()),
        UnlockEvent::Finished { game, .. } => ("finished", game.clone()),
    }
}

#[test]
fn two_queued_games_run_strictly_one_after_the_other() {
    let w = world(&["FAKE_DELAY_MS=15"]);
    let first = w.dump("One", "PPSA10001", &elf_bytes());
    let second = w.dump("Two", "PPSA10002", &elf_bytes());
    let (queue, events) = queue();
    assert!(queue.enqueue(first.clone()));
    assert!(!queue.enqueue(first.clone()), "already queued or running");
    assert!(queue.enqueue(second.clone()));
    events.wait_until("both finished", Duration::from_secs(60), |e| {
        finished(e, &first.game).is_some() && finished(e, &second.game).is_some()
    });
    let all = events.0.lock().unwrap();
    assert_eq!(finished(&all, &first.game), Some(true));
    assert_eq!(finished(&all, &second.game), Some(true));
    // Queued events first, then every event of One, then every event of Two.
    let mut current: Option<GameId> = None;
    let mut done = Vec::new();
    for event in all.iter() {
        let (kind, game) = tag(event);
        match kind {
            "queued" => {}
            "started" => {
                assert!(
                    current.is_none(),
                    "a second unlock started while one was running"
                );
                current = Some(game);
            }
            "finished" => {
                assert_eq!(current.take().as_ref(), Some(&game));
                done.push(game);
            }
            _ => assert_eq!(
                current.as_ref(),
                Some(&game),
                "event of {game} outside its run"
            ),
        }
    }
    assert_eq!(done, [first.game.clone(), second.game.clone()]);
    drop(all);
    assert!(queue.is_idle() || queue.position(&first.game).is_none());
    // After finishing, the same game can be queued again (retry).
    assert!(queue.enqueue(first));
}

#[test]
fn cancel_removes_a_queued_game_and_stops_the_running_one() {
    let w = world(&["FAKE_RELINKER=sleep", "FAKE_SLEEP_MS=60000"]);
    let first = w.dump("Slow", "PPSA10003", &elf_bytes());
    let second = w.dump("Waiting", "PPSA10004", &elf_bytes());
    let (queue, events) = queue();
    assert!(queue.enqueue(first.clone()));
    assert!(queue.enqueue(second.clone()));
    events.wait_until("first is in the relinker", Duration::from_secs(30), |e| {
        e.iter().any(|ev| matches!(ev, UnlockEvent::Progress { step: Step::Relink, fraction, .. } if *fraction > 0.0))
    });
    assert_eq!(queue.position(&second.game), Some(0));

    // Queued: removed, finished with Cancelled, never started.
    queue.cancel(&second.game);
    events.wait_until("second cancelled", Duration::from_secs(5), |e| {
        finished(e, &second.game).is_some()
    });
    assert_eq!(queue.position(&second.game), None);

    // Running: the relinker is killed.
    let started = std::time::Instant::now();
    queue.cancel(&first.game);
    events.wait_until("first cancelled", Duration::from_secs(20), |e| {
        finished(e, &first.game).is_some()
    });
    assert!(started.elapsed() < Duration::from_secs(20));
    let all = events.0.lock().unwrap();
    for game in [&first.game, &second.game] {
        let result = all.iter().find_map(|e| match e {
            UnlockEvent::Finished { game: g, result } if g == game => Some(result),
            _ => None,
        });
        assert!(
            matches!(result, Some(Err(UnlockError::Cancelled))),
            "{game}"
        );
    }
    assert!(
        !all.iter()
            .any(|e| matches!(e, UnlockEvent::Started { game } if *game == second.game))
    );
    drop(all);
    events.wait_until("idle", Duration::from_secs(5), |_| queue.is_idle());
    no_leftovers(&w.root);
    assert!(
        unlock::read_record(&unlock::unlocked_copy("PPSA10003", &first.game, &w.root)).is_none()
    );
}

#[test]
fn a_cancel_before_the_relinker_starts_still_ends_the_unlock() {
    let w = world(&["FAKE_RELINKER=sleep", "FAKE_SLEEP_MS=60000"]);
    let request = w.dump("Early", "PPSA10005", &elf_bytes());
    let control = UnlockControl::new();
    control.cancel();
    let result = run_unlock(&request, &control, &|_| {});
    assert!(matches!(result, Err(UnlockError::Cancelled)));
    no_leftovers(&w.root);
}

#[test]
fn dropping_the_queue_stops_a_running_unlock() {
    let w = world(&["FAKE_RELINKER=sleep", "FAKE_SLEEP_MS=60000"]);
    let request = w.dump("Dropped", "PPSA10006", &elf_bytes());
    let (queue, events) = queue();
    assert!(queue.enqueue(request));
    events.wait_until("relinking", Duration::from_secs(30), |e| {
        e.iter().any(|ev| matches!(ev, UnlockEvent::Progress { step: Step::Relink, fraction, .. } if *fraction > 0.0))
    });
    let started = std::time::Instant::now();
    drop(queue);
    assert!(started.elapsed() < Duration::from_secs(20));
    no_leftovers(&w.root);
}

// ---------------------------------------------------------------------------------------------
// Unlock speed: parallel layout, kept relinker output, background queue

fn counting(events: &[UnlockEvent]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, UnlockEvent::Started { .. }))
        .count()
}

#[test]
fn a_big_tree_is_laid_out_in_parallel_and_matches_the_dump() {
    let w = world(&[]);
    let request = w.dump("Many", "PPSA20001", &elf_bytes());
    for i in 0..400 {
        let file = request.folder.join(format!("data/pack{}/f{i}.bin", i % 7));
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, format!("file {i}")).unwrap();
    }
    let (result, events) = run(&request);
    result.unwrap();
    let copy = unlock::unlocked_copy("PPSA20001", &request.game, &w.root);
    for i in 0..400 {
        let name = format!("data/pack{}/f{i}.bin", i % 7);
        assert_eq!(
            fs::read_to_string(copy.join("app0").join(&name)).unwrap(),
            format!("file {i}")
        );
    }
    assert!(logs(&events).iter().any(|l| l.starts_with("layout: ")));
    assert!(logs(&events).iter().any(|l| l.starts_with("timing: ")));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, UnlockEvent::Timings { .. }))
    );
    no_leftovers(&w.root);
}

#[test]
fn unlocking_again_keeps_the_relinker_output_until_something_it_reads_changes() {
    let w = world(&[]);
    let request = w.dump("Again", "PPSA20002", &elf_bytes());
    let (first, _) = run(&request);
    let first = first.unwrap();
    assert!(first.relink_key.is_some());

    let (second, events) = run(&request);
    let second = second.unwrap();
    assert!(
        logs(&events)
            .iter()
            .any(|l| l.starts_with("relinker output kept")),
        "{:?}",
        logs(&events)
    );
    assert_eq!(second.relink_key, first.relink_key);
    let copy = unlock::unlocked_copy("PPSA20002", &request.game, &w.root);
    assert!(copy.join(EXECUTABLE).is_file());
    assert!(unlock::read_record(&copy).is_some());

    // A changed input (the executable grows) relinks again.
    let mut bigger = elf_bytes();
    bigger.extend_from_slice(b"more");
    fs::write(request.folder.join("eboot.bin"), bigger).unwrap();
    let (third, events) = run(&request);
    third.unwrap();
    assert!(
        !logs(&events)
            .iter()
            .any(|l| l.starts_with("relinker output kept"))
    );

    // `fresh` always relinks.
    let mut fresh = request.clone();
    fresh.fresh = true;
    let (fourth, events) = run(&fresh);
    fourth.unwrap();
    assert!(
        !logs(&events)
            .iter()
            .any(|l| l.starts_with("relinker output kept"))
    );
}

#[test]
fn an_asked_for_game_runs_before_prepared_ones_and_prepared_ones_wait() {
    let w = world(&["FAKE_DELAY_MS=10"]);
    let mut prepared = w.dump("Prep", "PPSA20003", &elf_bytes());
    prepared.background = true;
    let asked = w.dump("Asked", "PPSA20004", &elf_bytes());
    let (queue, events) = queue();
    queue.hold_background(true);
    assert!(queue.enqueue(prepared.clone()));
    // Held: the prepared game does not start by itself.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(counting(&events.0.lock().unwrap()), 0);
    assert!(queue.enqueue(asked.clone()));
    events.wait_until("asked finished", Duration::from_secs(60), |e| {
        finished(e, &asked.game).is_some()
    });
    assert!(finished(&events.0.lock().unwrap(), &prepared.game).is_none());
    // Free again: it goes on.
    queue.hold_background(false);
    events.wait_until("prepared finished", Duration::from_secs(60), |e| {
        finished(e, &prepared.game).is_some()
    });
    assert_eq!(
        finished(&events.0.lock().unwrap(), &prepared.game),
        Some(true)
    );
}

#[test]
fn promote_turns_a_prepared_game_into_an_asked_for_one() {
    let w = world(&["FAKE_DELAY_MS=10"]);
    let mut prepared = w.dump("Promo", "PPSA20005", &elf_bytes());
    prepared.background = true;
    let (queue, events) = queue();
    queue.hold_background(true);
    assert!(queue.enqueue(prepared.clone()));
    assert!(queue.promote(&prepared.game));
    assert!(!queue.promote(&prepared.game), "already asked for");
    events.wait_until("promoted game finished", Duration::from_secs(60), |e| {
        finished(e, &prepared.game).is_some()
    });
    assert_eq!(
        finished(&events.0.lock().unwrap(), &prepared.game),
        Some(true)
    );
}

#[test]
fn a_paused_unlock_stands_still_and_goes_on_without_losing_work() {
    let w = world(&["FAKE_DELAY_MS=40"]);
    let mut prepared = w.dump("Pause", "PPSA20006", &elf_bytes());
    prepared.background = true;
    let (queue, events) = queue();
    assert!(queue.enqueue(prepared.clone()));
    events.wait_until("relinking", Duration::from_secs(30), |e| {
        e.iter().any(|ev| {
            matches!(
                ev,
                UnlockEvent::Progress {
                    step: Step::Relink,
                    ..
                }
            )
        })
    });
    queue.hold_background(true);
    events.wait_until("paused event", Duration::from_secs(10), |e| {
        e.iter()
            .any(|ev| matches!(ev, UnlockEvent::Paused { paused: true, .. }))
    });
    assert!(queue.is_paused(&prepared.game));
    std::thread::sleep(Duration::from_millis(400));
    assert!(finished(&events.0.lock().unwrap(), &prepared.game).is_none());
    queue.hold_background(false);
    events.wait_until("finished", Duration::from_secs(60), |e| {
        finished(e, &prepared.game).is_some()
    });
    assert_eq!(
        finished(&events.0.lock().unwrap(), &prepared.game),
        Some(true)
    );
}

#[test]
fn cancelling_a_paused_unlock_ends_it() {
    let w = world(&["FAKE_DELAY_MS=40"]);
    let mut prepared = w.dump("PauseCancel", "PPSA20007", &elf_bytes());
    prepared.background = true;
    let (queue, events) = queue();
    assert!(queue.enqueue(prepared.clone()));
    events.wait_until("relinking", Duration::from_secs(30), |e| {
        e.iter().any(|ev| {
            matches!(
                ev,
                UnlockEvent::Progress {
                    step: Step::Relink,
                    ..
                }
            )
        })
    });
    queue.hold_background(true);
    events.wait_until("paused", Duration::from_secs(10), |e| {
        e.iter()
            .any(|ev| matches!(ev, UnlockEvent::Paused { paused: true, .. }))
    });
    queue.cancel(&prepared.game);
    events.wait_until("cancelled", Duration::from_secs(20), |e| {
        finished(e, &prepared.game).is_some()
    });
    no_leftovers(&w.root);
}

#[test]
fn promoting_a_paused_prepared_game_lets_it_go_on() {
    let w = world(&["FAKE_DELAY_MS=40"]);
    let mut prepared = w.dump("PromoPaused", "PPSA20008", &elf_bytes());
    prepared.background = true;
    let (queue, events) = queue();
    assert!(queue.enqueue(prepared.clone()));
    events.wait_until("relinking", Duration::from_secs(30), |e| {
        e.iter().any(|ev| {
            matches!(
                ev,
                UnlockEvent::Progress {
                    step: Step::Relink,
                    ..
                }
            )
        })
    });
    // The PC got busy: it stands still. The player then presses Play on it.
    queue.hold_background(true);
    events.wait_until("paused", Duration::from_secs(10), |e| {
        e.iter()
            .any(|ev| matches!(ev, UnlockEvent::Paused { paused: true, .. }))
    });
    assert!(queue.promote(&prepared.game));
    events.wait_until("it goes on and finishes", Duration::from_secs(60), |e| {
        finished(e, &prepared.game).is_some()
    });
    assert_eq!(
        finished(&events.0.lock().unwrap(), &prepared.game),
        Some(true)
    );
}

#[test]
fn a_kept_relinker_output_does_not_report_timings() {
    let w = world(&[]);
    let request = w.dump("NoTimings", "PPSA20009", &elf_bytes());
    run(&request).0.unwrap();
    let (again, events) = run(&request);
    again.unwrap();
    assert!(
        logs(&events)
            .iter()
            .any(|l| l.starts_with("relinker output kept"))
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, UnlockEvent::Timings { .. }))
    );
}
