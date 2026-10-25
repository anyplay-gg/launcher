//! The launcher's side against the real net service: the process starts, answers the hello over
//! its standard streams, reports its state, and ends when its input closes.
//!
//! The net service is built in anyplaynet/network. The test uses `ANYPLAY_NET`, or the debug
//! build of a clone next to this repository; without either it has nothing to run and says so.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyps5_core::online::{Cloud, Command, Line, NetProcess, Notice, Online, Output, Purpose};

fn binary() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("ANYPLAY_NET").map(PathBuf::from) {
        return Some(path);
    }
    let name = anyps5_core::online::BINARY;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../network/target/debug")
        .join(name);
    path.is_file().then_some(path)
}

#[test]
fn the_net_service_answers_over_its_standard_streams() {
    let Some(binary) = binary() else {
        eprintln!("no anyplay-net built: build it in ../network to run this test");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let mut process = NetProcess::spawn(
        &binary,
        &dir.path().join("Network"),
        false,
        &dir.path().join("network.log"),
        Arc::new(|| {}),
    )
    .expect("the net service starts");
    let mut online = Online::default();
    // A cloud nobody listens on: the service must still answer and report itself offline.
    online.send(
        Purpose::Hello,
        Command::Hello {
            launcher: "test".into(),
            cloud: "http://127.0.0.1:9".into(),
        },
    );
    process.write(&online.take_outbox()).unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut hello = false;
    while Instant::now() < deadline && !(hello && online.cloud.is_some()) {
        for line in process.drain() {
            for notice in online.apply(line) {
                if let Notice::Answered(Purpose::Hello, result) = notice {
                    let value = result.expect("hello is answered");
                    assert_eq!(value["protocol"], 0);
                    hello = true;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(hello, "no answer to the hello");
    assert_eq!(online.cloud, Some(Cloud::Offline));
    assert!(online.account.is_none());

    // A command it does not know is answered, not ignored.
    process
        .write(&["{\"id\":99,\"do\":\"teleport\"}\n".to_owned()])
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut refused = false;
    while Instant::now() < deadline && !refused {
        for line in process.drain() {
            if let Line::Output(Output::Answer(answer)) = line {
                refused = answer.id == 99
                    && answer
                        .into_result::<serde_json::Value>()
                        .is_err_and(|f| f.code == anyps5_core::online::Failure::UNKNOWN_COMMAND);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(refused, "the unknown command was not refused");

    // Dropping the process closes its input; the service ends, and its log has what it did.
    drop(process);
    let log = dir.path().join("network.log");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        if text.contains("cloud http://127.0.0.1:9") {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the net service log does not show the hello");
}
