//! One real end-to-end check with the upstream toolkit and a real dump. Ignored by default: it
//! needs a dump, a lot of disk and a machine that may run AnyPS5 games (hold the lab lock).
//!
//! ```text
//! ANYPS5_E2E_DUMP=<dump folder> ANYPS5_E2E_STORE=<empty folder on the dump's volume> \
//!   [ANYPS5_E2E_TOOLKIT=<toolkit folder>] [ANYPS5_E2E_WAIT=240] [ANYPS5_E2E_KEEP=1] \
//!   [ANYPS5_E2E_NOLAUNCH=1: unlock only] \
//!   cargo test -p anyps5-core --test real_dump -- --ignored --nocapture
//! ```
//!
//! Without `ANYPS5_E2E_TOOLKIT` the latest upstream release is downloaded into the store folder.
//! The dump is only read; nothing of it is kept in the repository.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyps5_core::diagnosis::Diagnosis;
use anyps5_core::launch::{LaunchEvent, LaunchRequest, Launcher};
use anyps5_core::paths::{AppPaths, GameId};
use anyps5_core::toolkit::{self, DEFAULT_REPOSITORY, Toolkit};
use anyps5_core::unlock::{self, UnlockControl, UnlockEvent, UnlockRequest};

mod support;
use support::Events;

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

#[test]
#[ignore = "needs a real dump, the upstream toolkit and a machine that may run games"]
fn unlock_and_start_a_real_dump() {
    let dump = env_path("ANYPS5_E2E_DUMP").expect("ANYPS5_E2E_DUMP");
    let store = env_path("ANYPS5_E2E_STORE").expect("ANYPS5_E2E_STORE");
    let wait: u64 = std::env::var("ANYPS5_E2E_WAIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(240);
    std::fs::create_dir_all(&store).unwrap();
    let paths = AppPaths::under(&store.join("appdata"));

    // The toolkit: given, or the newest upstream release.
    let toolkit = match env_path("ANYPS5_E2E_TOOLKIT") {
        Some(folder) => Toolkit::open(&folder)
            .or_else(|_| toolkit::adopt_folder(&folder))
            .expect("toolkit folder"),
        None => match Toolkit::locate_with(&paths, None, None) {
            Some(t) => t,
            None => {
                let info = toolkit::check_latest(DEFAULT_REPOSITORY).expect("release lookup");
                println!(
                    "downloading toolkit {} ({} + {} bytes)",
                    info.tag,
                    info.relinker.as_ref().map_or(0, |a| a.size),
                    info.libs.size
                );
                let mut last = -1.0;
                toolkit::download_and_install(
                    &info,
                    &paths,
                    None,
                    &mut |stage, fraction| {
                        if fraction - last >= 0.1 || fraction >= 1.0 {
                            println!("  {stage:?} {:.0}%", fraction * 100.0);
                            last = fraction;
                        }
                    },
                    &anyps5_core::error::CancelToken::new(),
                )
                .expect("toolkit download")
            }
        },
    };
    println!(
        "toolkit {} at {}",
        toolkit.version(),
        toolkit.root().display()
    );

    let title_id = {
        let text = std::fs::read_to_string(dump.join("sce_sys/param.json")).expect("param.json");
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();
        json["titleId"].as_str().expect("titleId").to_string()
    };
    println!("dump {} ({title_id})", dump.display());

    // Unlock.
    let request = UnlockRequest::new(
        dump.clone(),
        title_id.clone(),
        store.clone(),
        toolkit.clone(),
    );
    let started = Instant::now();
    let last_step = std::cell::Cell::new(None);
    let result = unlock::run_unlock(&request, &UnlockControl::new(), &|event| match event {
        UnlockEvent::Progress { step, fraction, .. }
            if last_step.get() != Some(step) || fraction >= 1.0 =>
        {
            println!(
                "  [{:>5.1}s] {step:?} {:.0}%",
                started.elapsed().as_secs_f32(),
                fraction * 100.0
            );
            last_step.set(Some(step));
        }
        UnlockEvent::Log { line, .. } => println!("  relinker: {line}"),
        _ => {}
    });
    let record = result.unwrap_or_else(|e| panic!("unlock failed: {e}"));
    println!(
        "unlocked in {:.1}s: {record:?}",
        started.elapsed().as_secs_f32()
    );
    let game = GameId::from_path(&dump);
    let copy = unlock::unlocked_copy(&title_id, &game, &store);
    assert!(unlock::read_record(&copy).is_some());

    if std::env::var_os("ANYPS5_E2E_NOLAUNCH").is_some() {
        println!("ANYPS5_E2E_NOLAUNCH set: not starting the game");
        if std::env::var_os("ANYPS5_E2E_KEEP").is_none() {
            unlock::remove_unlocked(&title_id, &request.game, &store).expect("remove the copy");
        }
        return;
    }

    // Play.
    let log = paths.log_file(&title_id);
    let mut launch = LaunchRequest::new(game, title_id.clone(), copy.clone(), log.clone());
    launch
        .env
        .insert("ANYPS5_PRESENT_MODE".into(), "fifo".into());
    launch.env.insert("ANYPS5_LANGUAGE".into(), "1".into());
    launch.env.insert(
        "ANYPS5_SHADER_CACHE_DIR".into(),
        store.join("shader_cache").to_string_lossy().into_owned(),
    );
    launch.create_dirs = vec![store.join("shader_cache")];
    let events: Events<LaunchEvent> = Events::new();
    let sink = events.clone();
    let handle = Launcher::start(launch, Arc::new(move |e| sink.push(e))).expect("start");
    println!("started pid {}", handle.pid());
    let begin = Instant::now();
    let mut window = None;
    let mut exited = false;
    while begin.elapsed() < Duration::from_secs(wait) {
        std::thread::sleep(Duration::from_millis(500));
        let all = events.0.lock().unwrap();
        for event in all.iter() {
            match event {
                LaunchEvent::WindowShown { forced, .. } => window = Some(*forced),
                LaunchEvent::Exited { .. } => exited = true,
                _ => {}
            }
        }
        if window.is_some() || exited {
            break;
        }
    }
    match window {
        Some(false) => println!("first window after {:.1}s", begin.elapsed().as_secs_f32()),
        Some(true) => println!("no window within the deadline"),
        None if exited => println!(
            "the game ended after {:.1}s without a window",
            begin.elapsed().as_secs_f32()
        ),
        None => println!("no window after {wait}s"),
    }
    if window == Some(false) {
        // Let it draw for a few seconds, then end it.
        std::thread::sleep(Duration::from_secs(8));
    }
    handle.kill();
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline
        && !events
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, LaunchEvent::Diagnosed { .. }))
    {
        std::thread::sleep(Duration::from_millis(200));
    }
    for event in events.0.lock().unwrap().iter() {
        println!("event: {event:?}");
    }
    let tail = handle.tail();
    let lines: Vec<&str> = tail.lines().collect();
    println!("--- last log lines ---");
    for line in &lines[lines.len().saturating_sub(40)..] {
        println!("{line}");
    }
    println!(
        "session read back: {:?}",
        Diagnosis::read(&anyps5_core::diagnosis::read_last_session(&log), None)
    );

    if std::env::var_os("ANYPS5_E2E_KEEP").is_none() {
        unlock::remove_unlocked(&title_id, &request.game, &store).expect("remove the copy");
    }
    assert!(
        window.is_some() || exited,
        "the game neither showed a window nor ended"
    );
}
