//! Starting and watching a game, with the fake relinker standing in as `eboot.exe`.

mod support;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyps5_core::diagnosis::{Diagnosis, ExitInfo};
use anyps5_core::launch::{LaunchEvent, LaunchRequest, Launcher, RunHandle};
use anyps5_core::paths::GameId;
use anyps5_core::unlock::EXECUTABLE;
use support::*;

struct Setup {
    _dir: tempfile::TempDir,
    copy: PathBuf,
    log: PathBuf,
    events: Events<LaunchEvent>,
}

fn setup(settings: &[&str]) -> Setup {
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("PPSA00001-abc");
    fs::create_dir_all(&copy).unwrap();
    install_fake_binary(&copy.join(EXECUTABLE));
    configure_fake(&copy, settings);
    Setup {
        log: dir.path().join("logs/PPSA00001.log"),
        copy,
        _dir: dir,
        events: Events::new(),
    }
}

impl Setup {
    fn request(&self) -> LaunchRequest {
        let mut request = LaunchRequest::new(
            GameId::from_path(std::path::Path::new("c:/games/stray")),
            "PPSA00001".into(),
            self.copy.clone(),
            self.log.clone(),
        );
        request.poll = Duration::from_millis(30);
        request
    }

    fn start(&self, request: LaunchRequest) -> RunHandle {
        let sink = self.events.clone();
        Launcher::start(request, Arc::new(move |e| sink.push(e))).expect("game starts")
    }

    fn wait_for_end(&self) -> (ExitInfo, Option<Diagnosis>, bool) {
        self.events
            .wait_until("diagnosis", Duration::from_secs(30), |e| {
                e.iter()
                    .any(|ev| matches!(ev, LaunchEvent::Diagnosed { .. }))
            });
        let all = self.events.0.lock().unwrap();
        let info = all
            .iter()
            .find_map(|e| match e {
                LaunchEvent::Exited { info, .. } => Some(info.clone()),
                _ => None,
            })
            .expect("Exited");
        let (diagnosis, without_window) = all
            .iter()
            .find_map(|e| match e {
                LaunchEvent::Diagnosed {
                    diagnosis,
                    without_window,
                    ..
                } => Some((diagnosis.clone(), *without_window)),
                _ => None,
            })
            .unwrap();
        (info, diagnosis, without_window)
    }

    fn log_text(&self) -> String {
        fs::read_to_string(&self.log).unwrap()
    }
}

#[test]
fn output_order_is_kept_the_log_has_the_marker_and_the_exit_code_arrives() {
    let s = setup(&["FAKE_GAME_LINES=50", "FAKE_GAME_EXIT=7"]);
    let handle = s.start(s.request());
    let (info, diagnosis, without_window) = s.wait_for_end();
    assert_eq!(info.code, Some(7));
    assert!(!info.had_window);
    assert_eq!(diagnosis, None);
    assert!(
        without_window,
        "ended without a window and nothing explains it"
    );

    let events = s.events.0.lock().unwrap();
    assert!(
        matches!(events.first(), Some(LaunchEvent::Spawned { pid, .. }) if *pid == handle.pid())
    );
    assert!(matches!(
        events[events.len() - 2],
        LaunchEvent::Exited { .. }
    ));
    drop(events);

    let log = s.log_text();
    assert!(log.starts_with("\n[anyps5-launcher] start "), "{log:?}");
    let session = Diagnosis::last_session(&log);
    // stdout and stderr share one pipe, so the order of writes is the order of lines.
    let lines: Vec<&str> = session
        .lines()
        .filter(|l| l.starts_with("out ") || l.starts_with("err "))
        .collect();
    let expected: Vec<String> = (1..=50)
        .flat_map(|i| [format!("out {i}"), format!("err {i}")])
        .collect();
    assert_eq!(
        lines,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    // The game ran in its copy.
    let cwd = session
        .lines()
        .find_map(|line| line.strip_prefix("game cwd="))
        .expect("reported working directory");
    assert_eq!(
        fs::canonicalize(cwd).unwrap(),
        fs::canonicalize(&s.copy).unwrap()
    );
    // The tail in memory holds the same.
    assert!(handle.tail().contains("err 50"));
}

#[test]
fn the_settings_environment_reaches_the_game() {
    let s = setup(&["FAKE_GAME_DUMP_ENV=ANYPS5_PRESENT_MODE,ANYPS5_INPUT_CONFIG,ANYPS5_OVERLAY"]);
    let mut request = s.request();
    request
        .env
        .insert("ANYPS5_PRESENT_MODE".into(), "immediate".into());
    request.env.insert(
        "ANYPS5_INPUT_CONFIG".into(),
        "C:\\Users\\Zoë\\input file.ini".into(),
    );
    let _handle = s.start(request);
    s.wait_for_end();
    let log = s.log_text();
    assert!(log.contains("ANYPS5_PRESENT_MODE=immediate"));
    assert!(
        log.contains("ANYPS5_INPUT_CONFIG=C:\\Users\\Zoë\\input file.ini"),
        "{log}"
    );
    assert!(log.contains("ANYPS5_OVERLAY=<unset>"));
}

#[test]
fn folders_are_created_and_a_second_start_adds_a_second_marker() {
    let s = setup(&[]);
    let mut request = s.request();
    let shots = s.copy.join("shots/deep");
    request.create_dirs = vec![shots.clone()];
    let _handle = s.start(request);
    s.wait_for_end();
    assert!(shots.is_dir());
    let again = Events::new();
    let sink = again.clone();
    let _second = Launcher::start(s.request(), Arc::new(move |e| sink.push(e))).unwrap();
    again.wait_until("second end", Duration::from_secs(30), |e| {
        e.iter()
            .any(|ev| matches!(ev, LaunchEvent::Diagnosed { .. }))
    });
    assert_eq!(s.log_text().matches("[anyps5-launcher] start").count(), 2);
}

// A Unix exit status has eight bits, so a Windows status code only survives on Windows.
#[cfg(windows)]
#[test]
fn a_crash_status_is_diagnosed() {
    let s = setup(&["FAKE_GAME_EXIT=3221225477"]); // 0xC0000005
    let _handle = s.start(s.request());
    let (info, diagnosis, without_window) = s.wait_for_end();
    assert_eq!(info.code, Some(0xC000_0005));
    assert_eq!(
        diagnosis,
        Some(Diagnosis::Crashed("EXCEPTION_ACCESS_VIOLATION".into()))
    );
    assert!(!without_window, "there is an explanation");
}

#[test]
fn the_tail_is_capped_but_the_log_is_complete() {
    let s = setup(&["FAKE_GAME_BIG_KB=600", "FAKE_GAME_LINES=1"]);
    let handle = s.start(s.request());
    s.wait_for_end();
    let tail = handle.tail();
    assert!(tail.len() <= 400 * 1024, "{}", tail.len());
    assert!(tail.len() > 390 * 1024);
    assert!(tail.contains("err 1"));
    let log = s.log_text();
    assert!(log.contains("filler 000000"));
    assert!(log.contains("filler 000599"));
    assert!(log.len() > 600 * 1000);
}

#[test]
fn kill_ends_the_game_and_the_children_it_started() {
    let s = setup(&[
        "FAKE_GAME_SLEEP_MS=60000",
        "FAKE_GAME_CHILD=1",
        "FAKE_CHILD_MS=60000",
    ]);
    let handle = s.start(s.request());
    s.events
        .wait_until("a moment of play", Duration::from_secs(10), |_| {
            handle.tail().contains("err 3")
        });
    assert!(handle.is_alive());
    let started = std::time::Instant::now();
    handle.kill();
    let (info, diagnosis, without_window) = s.wait_for_end();
    assert!(started.elapsed() < Duration::from_secs(15));
    assert_eq!(info.code, None, "killed by the launcher");
    assert_eq!(diagnosis, None);
    assert!(!without_window, "a cancelled start is not a failed one");
    assert!(!handle.is_alive(), "the job is empty: the child died too");
}

#[test]
fn a_game_that_outlives_the_deadline_without_a_window_is_reported_as_forced() {
    let s = setup(&["FAKE_GAME_SLEEP_MS=60000"]);
    let mut request = s.request();
    request.window_deadline = Duration::from_millis(300);
    let handle = s.start(request);
    s.events
        .wait_until("forced window event", Duration::from_secs(10), |e| {
            e.iter()
                .any(|ev| matches!(ev, LaunchEvent::WindowShown { forced: true, .. }))
        });
    handle.kill();
    let (info, ..) = s.wait_for_end();
    assert!(!info.had_window);
}

#[cfg(windows)]
#[test]
fn the_first_window_is_found_placed_and_reported() {
    use anyps5_core::launch::WindowPrefs;
    let s = setup(&["FAKE_GAME_SLEEP_MS=60000", "FAKE_GAME_WINDOW=800x600"]);
    let mut request = s.request();
    request.window = WindowPrefs {
        fullscreen: false,
        size_percent: 50,
    };
    let handle = s.start(request);
    s.events
        .wait_until("window or no desktop", Duration::from_secs(15), |e| {
            e.iter()
                .any(|ev| matches!(ev, LaunchEvent::WindowShown { .. }))
                || handle.tail().contains("NOWINDOW")
        });
    if handle.tail().contains("NOWINDOW") {
        eprintln!("no desktop available, skipping");
        handle.kill();
        return;
    }
    assert!(
        s.events
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|ev| matches!(ev, LaunchEvent::WindowShown { forced: false, .. }))
    );
    handle.kill();
    let (info, ..) = s.wait_for_end();
    assert!(info.had_window);
}
