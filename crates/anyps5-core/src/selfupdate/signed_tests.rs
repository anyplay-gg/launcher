//! The signed update flow. Every key is made by the test and dropped with it; nothing here knows
//! the real key.

use super::*;
use anyplay_update_manifest::{generate_signing_key, sign};
use ed25519_dalek::SigningKey;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::Duration;
use time::Duration as Span;

const PROGRAM: &[u8] = b"MZ-new-signed-launcher";

fn sha_hex(data: &[u8]) -> String {
    toolkit::hex(&Sha256::digest(data))
}

/// Serves fixed bodies on localhost until the returned flag is set.
fn serve(routes: Vec<(&'static str, Vec<u8>)>) -> (String, Arc<AtomicBool>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        while !flag.load(AtomicOrdering::SeqCst) {
            let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            let path = request.url().to_string();
            let _ = match routes.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => request.respond(tiny_http::Response::from_data(body.clone())),
                None => request.respond(tiny_http::Response::empty(404)),
            };
        }
    });
    (base, stop)
}

fn manifest_for(version: &str, program: &[u8], base: &str) -> Manifest {
    let now = OffsetDateTime::now_utc();
    Manifest {
        schema: anyplay_update_manifest::SCHEMA,
        version: version.into(),
        published: anyplay_update_manifest::format_time(now - Span::days(1)),
        expires: anyplay_update_manifest::format_time(now + Span::days(29)),
        minimum_version: "0.1.0".into(),
        platforms: BTreeMap::from([(
            WINDOWS_X64.into(),
            Artifact {
                name: exe_name(version),
                url: format!("{base}/exe"),
                sha256: sha_hex(program),
                size: program.len() as u64,
            },
        )]),
    }
}

/// The JSON of `releases/latest`. `tag` is what the release claims, which a signed flow ignores.
fn release_json(tag: &str, base: &str, with_manifest: bool, with_sig: bool) -> String {
    let version = tag.trim_start_matches('v');
    let mut assets = vec![
        format!(
            r#"{{"name": "AnyPlay-Launcher-{version}-x64.exe", "size": 4, "browser_download_url": "{base}/exe"}}"#
        ),
        format!(
            r#"{{"name": "SHA256SUMS.txt", "size": 9, "browser_download_url": "{base}/sums"}}"#
        ),
    ];
    if with_manifest {
        assets.push(format!(
            r#"{{"name": "latest.json", "size": 9, "browser_download_url": "{base}/latest.json"}}"#
        ));
    }
    if with_sig {
        assets.push(format!(
            r#"{{"name": "latest.json.sig", "size": 9, "browser_download_url": "{base}/latest.json.sig"}}"#
        ));
    }
    format!(
        r#"{{"tag_name": "{tag}", "assets": [{}]}}"#,
        assets.join(",")
    )
}

/// A server with a signed release 0.2.0 whose manifest `edit` may change before it is signed
/// with `signer`. Returns the base address, the stop flag and the release JSON.
fn serve_signed(
    signer: &SigningKey,
    program: &[u8],
    edit: impl FnOnce(&mut Manifest),
) -> (String, Arc<AtomicBool>, String) {
    // The address is needed inside the manifest, so bind first.
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let mut manifest = manifest_for("0.2.0", program, &base);
    edit(&mut manifest);
    let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    let signature = sign(&bytes, signer);
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let routes: Vec<(&str, Vec<u8>)> = vec![
        ("/latest.json", bytes),
        ("/latest.json.sig", signature.into_bytes()),
        ("/exe", program.to_vec()),
    ];
    std::thread::spawn(move || {
        while !flag.load(AtomicOrdering::SeqCst) {
            let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            let path = request.url().to_string();
            let _ = match routes.iter().find(|(p, _)| *p == path) {
                Some((_, body)) => request.respond(tiny_http::Response::from_data(body.clone())),
                None => request.respond(tiny_http::Response::empty(404)),
            };
        }
    });
    // The release itself claims a tag that is not the manifest's: it must not matter.
    let json = release_json("v9.9.9", &base, true, true);
    (base, stop, json)
}

fn fixture() -> (tempfile::TempDir, AppPaths, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(root.path());
    let exe = root.path().join("app").join("AnyPlay-Launcher.exe");
    fs::create_dir_all(exe.parent().unwrap()).unwrap();
    fs::write(&exe, b"MZ-old-launcher").unwrap();
    (root, paths, exe)
}

fn signed_check(
    signer: &SigningKey,
    trusted: &TrustedKeys,
    current: &str,
    edit: impl FnOnce(&mut Manifest),
) -> (Result<Option<Release>, SelfUpdateError>, Arc<AtomicBool>) {
    let (base, stop, json) = serve_signed(signer, PROGRAM, edit);
    let result = parse_signed(
        &json,
        current,
        trusted,
        OffsetDateTime::now_utc(),
        &mut |url, max| {
            assert!(url.starts_with(&base));
            get_small(url, max)
        },
    );
    (result, stop)
}

#[test]
fn a_valid_manifest_is_accepted_and_the_tag_of_the_release_is_ignored() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (result, stop) = signed_check(&key, &keys, "0.1.3", |_| {});
    stop.store(true, AtomicOrdering::SeqCst);
    let release = result.unwrap().expect("an update");
    assert_eq!(release.tag, "v0.2.0", "the version is the manifest's");
    assert_eq!(release.version(), "0.2.0");
    assert_eq!(release.exe.sha256, Some(sha_hex(PROGRAM)));
    assert_eq!(release.exe.size, PROGRAM.len() as u64);
    assert!(
        matches!(&release.vouch, Vouch::Manifest { signature, .. } if signature.trim().len() == 128)
    );
}

#[test]
fn a_bad_signature_and_a_wrong_key_are_refused() {
    let key = generate_signing_key();
    let other = generate_signing_key();
    // Signed by a key the launcher does not trust.
    let (result, stop) = signed_check(
        &other,
        &TrustedKeys::of(key.verifying_key()),
        "0.1.3",
        |_| {},
    );
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(
            result,
            Err(SelfUpdateError::Manifest(ManifestError::Signature))
        ),
        "{result:?}"
    );

    // A signature that is garbage.
    let (base, stop, json) = serve_signed(&key, PROGRAM, |_| {});
    let manifest = get_small(&format!("{base}/latest.json"), 1 << 16).unwrap();
    let result = parse_signed(
        &json,
        "0.1.3",
        &TrustedKeys::of(key.verifying_key()),
        OffsetDateTime::now_utc(),
        &mut |url, _| {
            Ok(if url.ends_with(".sig") {
                format!("{}\n", "ab".repeat(64)).into_bytes()
            } else {
                manifest.clone()
            })
        },
    );
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(
            result,
            Err(SelfUpdateError::Manifest(ManifestError::Signature))
        ),
        "{result:?}"
    );
}

#[test]
fn an_expired_manifest_is_refused() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (result, stop) = signed_check(&key, &keys, "0.1.3", |m| {
        let now = OffsetDateTime::now_utc();
        m.published = anyplay_update_manifest::format_time(now - Span::days(40));
        m.expires = anyplay_update_manifest::format_time(now - Span::days(10));
    });
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(
            result,
            Err(SelfUpdateError::Manifest(ManifestError::Expired(_)))
        ),
        "{result:?}"
    );
}

#[test]
fn a_rollback_and_an_equal_version_offer_nothing() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    for running in ["0.2.0", "0.3.0", "1.0.0"] {
        let (result, stop) = signed_check(&key, &keys, running, |_| {});
        stop.store(true, AtomicOrdering::SeqCst);
        assert!(matches!(result, Ok(None)), "{running}: {result:?}");
    }
}

#[test]
fn a_running_version_below_the_minimum_is_refused() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (result, stop) = signed_check(&key, &keys, "0.1.3", |m| m.minimum_version = "0.1.5".into());
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(
            result,
            Err(SelfUpdateError::Manifest(
                ManifestError::RunningTooOld { .. }
            ))
        ),
        "{result:?}"
    );
}

#[test]
fn a_release_without_a_manifest_or_signature_is_refused_with_no_fallback() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    // Everything the unsigned flow needs is there: the exe and SHA256SUMS.txt.
    for (manifest, sig) in [(false, false), (true, false), (false, true)] {
        let json = release_json("v0.2.0", "http://127.0.0.1:9", manifest, sig);
        assert!(
            parse_latest(&json, "0.1.3").unwrap().is_some(),
            "the unsigned flow would take this release"
        );
        let result = parse_signed(
            &json,
            "0.1.3",
            &keys,
            OffsetDateTime::now_utc(),
            &mut |_, _| panic!("nothing is fetched without both files"),
        );
        assert!(
            matches!(result, Err(SelfUpdateError::NoManifest)),
            "{result:?}"
        );
    }
}

#[test]
fn check_with_keys_requires_the_manifest_and_without_keys_runs_the_old_path() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    // A release that only the unsigned flow understands.
    let json = release_json("v0.2.0", "http://127.0.0.1:9", false, false);
    let (base, stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let none = TrustedKeys::default();
    let old = check_with(&base, "o/r", "0.1.3", &none).unwrap().unwrap();
    assert!(matches!(old.vouch, Vouch::Checksums(_)));
    assert_eq!(old.tag, "v0.2.0");
    let signed = check_with(&base, "o/r", "0.1.3", &keys);
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(signed, Err(SelfUpdateError::NoManifest)),
        "{signed:?}"
    );
}

#[test]
fn a_signed_release_is_found_through_the_api_staged_and_found_again() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (_base, stop, json) = serve_signed(&key, PROGRAM, |_| {});
    // The API answers on a second server; the assets come from the first.
    let (api, api_stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let release = check_with(&api, "o/r", "0.1.3", &keys).unwrap().unwrap();
    let (_root, paths, exe) = fixture();
    let staged = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new()).unwrap();
    assert_eq!(staged.version, "0.2.0");
    assert_eq!(fs::read(&staged.path).unwrap(), PROGRAM);
    assert!(sidecar(&staged.path, MANIFEST_SUFFIX).is_file());
    assert!(sidecar(&staged.path, SIGNATURE_SUFFIX).is_file());
    assert_eq!(
        pending_with(&paths, "0.1.3", Some(&keys), OffsetDateTime::now_utc()).map(|s| s.version),
        Some("0.2.0".into())
    );
    // A second stage reuses the file without the network.
    stop.store(true, AtomicOrdering::SeqCst);
    api_stop.store(true, AtomicOrdering::SeqCst);
    std::thread::sleep(Duration::from_millis(150));
    stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new()).unwrap();

    // A staged file that changed afterwards is no longer offered, and the swap removes the proof.
    fs::write(&staged.path, b"MZ-something-else!!").unwrap();
    assert!(pending_with(&paths, "0.1.3", Some(&keys), OffsetDateTime::now_utc()).is_none());
    fs::write(&staged.path, PROGRAM).unwrap();
    let again = pending_with(&paths, "0.1.3", Some(&keys), OffsetDateTime::now_utc())
        .expect("restored file is vouched for again");
    again.swap_in(&exe).unwrap();
    assert!(!sidecar(&staged.path, MANIFEST_SUFFIX).exists());
    assert!(!sidecar(&staged.path, SIGNATURE_SUFFIX).exists());
    assert_eq!(fs::read(&exe).unwrap(), PROGRAM);
}

#[test]
fn a_download_with_another_hash_is_refused_and_leaves_nothing() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    // The manifest vouches for one file, the server serves another of the same length.
    let (base, stop, json) = serve_signed(&key, b"MZ-tampered-launcher", |m| {
        let artifact = m.platforms.get_mut(WINDOWS_X64).unwrap();
        artifact.sha256 = sha_hex(PROGRAM);
        artifact.size = b"MZ-tampered-launcher".len() as u64;
    });
    let _ = base;
    let (api, api_stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let release = check_with(&api, "o/r", "0.1.3", &keys).unwrap().unwrap();
    let (_root, paths, exe) = fixture();
    let result = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new());
    stop.store(true, AtomicOrdering::SeqCst);
    api_stop.store(true, AtomicOrdering::SeqCst);
    assert!(matches!(result, Err(SelfUpdateError::Digest)), "{result:?}");
    assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
    assert!(pending_with(&paths, "0.1.3", Some(&keys), OffsetDateTime::now_utc()).is_none());
}

#[test]
fn a_download_with_another_size_is_refused_and_leaves_nothing() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    // The hash is right, the size the manifest states is not.
    let (_base, stop, json) = serve_signed(&key, PROGRAM, |m| {
        m.platforms.get_mut(WINDOWS_X64).unwrap().size += 1;
    });
    let (api, api_stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let release = check_with(&api, "o/r", "0.1.3", &keys).unwrap().unwrap();
    let (_root, paths, exe) = fixture();
    let result = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new());
    stop.store(true, AtomicOrdering::SeqCst);
    api_stop.store(true, AtomicOrdering::SeqCst);
    assert!(matches!(result, Err(SelfUpdateError::Size)), "{result:?}");
    assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
}

#[test]
fn a_manifest_that_points_somewhere_unsafe_is_refused_even_when_signed() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    type Edit = Box<dyn FnOnce(&mut Manifest)>;
    let edits: Vec<Edit> = vec![
        Box::new(|m| m.platforms.get_mut(WINDOWS_X64).unwrap().name = "..\\evil.exe".into()),
        Box::new(|m| m.platforms.get_mut(WINDOWS_X64).unwrap().name = "other.exe".into()),
        Box::new(|m| {
            m.platforms.get_mut(WINDOWS_X64).unwrap().url = "http://example.com/exe".into()
        }),
        Box::new(|m| {
            m.platforms.get_mut(WINDOWS_X64).unwrap().url = "http://127.0.0.1.evil.test/exe".into()
        }),
        // The host here is evil.example: everything before the @ is a user name.
        Box::new(|m| {
            m.platforms.get_mut(WINDOWS_X64).unwrap().url =
                "http://127.0.0.1:80@evil.example/x.exe".into()
        }),
        Box::new(|m| {
            m.platforms.get_mut(WINDOWS_X64).unwrap().url =
                "https://github.com@evil.example/x.exe".into()
        }),
        Box::new(|m| {
            m.platforms.get_mut(WINDOWS_X64).unwrap().url =
                "https://user:pw@example.com/x.exe".into()
        }),
        Box::new(|m| {
            m.platforms.get_mut(WINDOWS_X64).unwrap().url = "ftp://example.com/x.exe".into()
        }),
        Box::new(|m| m.platforms.get_mut(WINDOWS_X64).unwrap().url = "not a url".into()),
        Box::new(|m| {
            m.platforms.remove(WINDOWS_X64);
        }),
    ];
    for edit in edits {
        let (result, stop) = signed_check(&key, &keys, "0.1.3", edit);
        stop.store(true, AtomicOrdering::SeqCst);
        assert!(result.is_err(), "{result:?}");
    }
}

fn later() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

/// A planted exe beside whatever proof an attacker can make without the signing key.
#[test]
fn a_planted_exe_with_a_forged_marker_or_manifest_is_not_offered() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (_root, paths, _) = fixture();
    let path = updates_dir(&paths).join(exe_name("0.2.0"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, PROGRAM).unwrap();
    let offered = || pending_with(&paths, "0.1.3", Some(&keys), later());

    // What an unsigned launcher left: offered while signing is off, never once it is on.
    assert!(pending_with(&paths, "0.1.3", None, later()).is_some());
    assert!(offered().is_none());
    // The old marker with the right hash, which was all that vouched for a file before.
    fs::write(sidecar(&path, LEGACY_MARKER_SUFFIX), sha_hex(PROGRAM)).unwrap();
    assert!(offered().is_none());
    // A manifest and signature made with a key the launcher does not trust.
    let attacker = generate_signing_key();
    let forged =
        serde_json::to_vec_pretty(&manifest_for("0.2.0", PROGRAM, "http://127.0.0.1:1")).unwrap();
    fs::write(sidecar(&path, MANIFEST_SUFFIX), &forged).unwrap();
    fs::write(sidecar(&path, SIGNATURE_SUFFIX), sign(&forged, &attacker)).unwrap();
    assert!(offered().is_none());
    // The real manifest, signed by the real key, replayed beside another exe: wrong hash.
    let genuine = serde_json::to_vec_pretty(&manifest_for(
        "0.2.0",
        b"MZ-the-real-one",
        "http://127.0.0.1:1",
    ))
    .unwrap();
    fs::write(sidecar(&path, MANIFEST_SUFFIX), &genuine).unwrap();
    fs::write(sidecar(&path, SIGNATURE_SUFFIX), sign(&genuine, &key)).unwrap();
    assert!(offered().is_none());
    // And the real proof for this very file is the only thing that is offered.
    let own =
        serde_json::to_vec_pretty(&manifest_for("0.2.0", PROGRAM, "http://127.0.0.1:1")).unwrap();
    fs::write(sidecar(&path, MANIFEST_SUFFIX), &own).unwrap();
    fs::write(sidecar(&path, SIGNATURE_SUFFIX), sign(&own, &key)).unwrap();
    assert!(offered().is_some());
    // Proof for another version does not carry over to this file name.
    fs::rename(&path, updates_dir(&paths).join(exe_name("0.2.1"))).unwrap();
    assert!(offered().is_none());
}

#[test]
fn a_staged_update_is_checked_again_for_expiry_and_for_a_newer_running_version() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (_base, stop, json) = serve_signed(&key, PROGRAM, |_| {});
    let (api, api_stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let release = check_with(&api, "o/r", "0.1.3", &keys).unwrap().unwrap();
    let (_root, paths, exe) = fixture();
    stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new()).unwrap();
    stop.store(true, AtomicOrdering::SeqCst);
    api_stop.store(true, AtomicOrdering::SeqCst);
    assert!(pending_with(&paths, "0.1.3", Some(&keys), later()).is_some());
    // The manifest was made for 29 days from now: 40 days on it has lapsed.
    assert!(pending_with(&paths, "0.1.3", Some(&keys), later() + Span::days(40)).is_none());
    // Already running that version: nothing to offer.
    assert!(pending_with(&paths, "0.2.0", Some(&keys), later()).is_none());
    // The key was rotated out of the list.
    let other = TrustedKeys::of(generate_signing_key().verifying_key());
    assert!(pending_with(&paths, "0.1.3", Some(&other), later()).is_none());
}

#[test]
fn a_staged_file_swapped_after_the_check_is_not_installed() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (_base, stop, json) = serve_signed(&key, PROGRAM, |_| {});
    let (api, api_stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let release = check_with(&api, "o/r", "0.1.3", &keys).unwrap().unwrap();
    let (_root, paths, exe) = fixture();
    stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new()).unwrap();
    stop.store(true, AtomicOrdering::SeqCst);
    api_stop.store(true, AtomicOrdering::SeqCst);

    // Checked and offered ...
    let offered = pending_with(&paths, "0.1.3", Some(&keys), later()).expect("offered");
    // ... and replaced by something else before the swap happens (even keeping the length).
    let evil = vec![b'M'; PROGRAM.len()];
    fs::write(&offered.path, &evil).unwrap();
    let result = offered.swap_in(&exe);
    assert!(matches!(result, Err(SelfUpdateError::Digest)), "{result:?}");
    assert_eq!(
        fs::read(&exe).unwrap(),
        b"MZ-old-launcher",
        "the exe is untouched"
    );
    let beside: Vec<_> = fs::read_dir(exe.parent().unwrap())
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(beside.len(), 1, "no .new or .old is left beside the exe");

    // The same holds for a Staged that stage() itself returned.
    fs::write(&offered.path, PROGRAM).unwrap();
    let staged = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new()).unwrap();
    fs::write(&staged.path, &evil).unwrap();
    assert!(matches!(staged.swap_in(&exe), Err(SelfUpdateError::Digest)));
    assert_eq!(fs::read(&exe).unwrap(), b"MZ-old-launcher");
}

#[test]
fn cleanup_removes_proofs_with_their_exes() {
    let (_root, paths, exe) = fixture();
    let dir = updates_dir(&paths);
    fs::create_dir_all(&dir).unwrap();
    let old = dir.join(exe_name("0.1.1"));
    let newer = dir.join(exe_name("0.3.0"));
    for path in [&old, &newer] {
        fs::write(path, b"MZ").unwrap();
        for suffix in [MANIFEST_SUFFIX, SIGNATURE_SUFFIX, LEGACY_MARKER_SUFFIX] {
            fs::write(sidecar(path, suffix), "x").unwrap();
        }
    }
    for suffix in [MANIFEST_SUFFIX, SIGNATURE_SUFFIX, LEGACY_MARKER_SUFFIX] {
        fs::write(
            dir.join(format!("AnyPlay-Launcher-0.4.0-x64.exe{suffix}")),
            "x",
        )
        .unwrap();
    }
    cleanup(&exe, &paths, "0.2.0");
    for suffix in [MANIFEST_SUFFIX, SIGNATURE_SUFFIX, LEGACY_MARKER_SUFFIX] {
        assert!(!sidecar(&old, suffix).exists());
        assert!(
            !dir.join(format!("AnyPlay-Launcher-0.4.0-x64.exe{suffix}"))
                .exists()
        );
    }
    assert!(!old.exists());
    assert!(newer.exists());
    assert!(
        sidecar(&newer, MANIFEST_SUFFIX).exists() && sidecar(&newer, SIGNATURE_SUFFIX).exists()
    );
}

#[test]
fn an_up_to_date_launcher_does_not_call_a_stale_manifest_an_error() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let (base, stop, json) = serve_signed(&key, PROGRAM, |_| {});
    let long_after = OffsetDateTime::now_utc() + Span::days(400);
    let check = |current: &str| {
        parse_signed(&json, current, &keys, long_after, &mut |url, max| {
            assert!(url.starts_with(&base));
            get_small(url, max)
        })
    };
    // Running the version the manifest offers (or a later one): nothing to do, and no error.
    assert!(matches!(check("0.2.0"), Ok(None)));
    assert!(matches!(check("0.3.0"), Ok(None)));
    // A manifest that would offer an update is still refused when it has lapsed.
    assert!(matches!(
        check("0.1.3"),
        Err(SelfUpdateError::Manifest(ManifestError::Expired(_)))
    ));
    stop.store(true, AtomicOrdering::SeqCst);
}

#[test]
fn a_manifest_published_in_the_future_or_valid_for_too_long_is_refused() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    let now = OffsetDateTime::now_utc();
    let (result, stop) = signed_check(&key, &keys, "0.1.3", |m| {
        m.published = anyplay_update_manifest::format_time(now + Span::days(5));
        m.expires = anyplay_update_manifest::format_time(now + Span::days(35));
    });
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(
            result,
            Err(SelfUpdateError::Manifest(ManifestError::Malformed(_)))
        ),
        "{result:?}"
    );
    let (result, stop) = signed_check(&key, &keys, "0.1.3", |m| {
        m.expires = anyplay_update_manifest::format_time(now + Span::days(365 * 7000));
    });
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(
            result,
            Err(SelfUpdateError::Manifest(ManifestError::Malformed(_)))
        ),
        "{result:?}"
    );
}

#[test]
fn addresses_are_judged_by_their_parsed_host() {
    // Fine: https, no user name, GitHub's hosts.
    for ok in [
        "https://github.com/anyplaynet/launcher/releases/download/v1/latest.json",
        "https://objects.githubusercontent.com/x?y=1",
        "https://release-assets.githubusercontent.com/x",
    ] {
        assert!(vet_url(ok, true).is_ok(), "{ok}");
    }
    // Not GitHub, not https, a user name in front of the real host, or a lookalike.
    for bad in [
        "https://evil.example/latest.json",
        "http://github.com/latest.json",
        "https://github.com@evil.example/latest.json",
        "https://github.com:8443/latest.json",
        "https://github.com.evil.example/latest.json",
        "https://evilgithub.com/latest.json",
        "https://evil.example/github.com",
        "https://githubusercontent.com.evil.example/x",
        "http://127.0.0.1:80@evil.example/x",
        "javascript:alert(1)",
        "",
    ] {
        assert!(vet_url(bad, true).is_err(), "{bad}");
    }
    // The exe may be on any https host (its hash is signed), but still not behind a user name.
    assert!(vet_url("https://downloads.example/x.exe", false).is_ok());
    assert!(vet_url("https://github.com@downloads.example/x.exe", false).is_err());
}

#[test]
fn the_urls_of_the_manifest_and_signature_must_be_github_https() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    for (manifest_url, sig_url) in [
        (
            "https://evil.example/latest.json",
            "https://github.com/x/latest.json.sig",
        ),
        (
            "https://github.com/x/latest.json",
            "http://github.com/x/latest.json.sig",
        ),
        (
            "https://github.com/x/latest.json",
            "https://github.com@evil.example/x.sig",
        ),
    ] {
        let json = format!(
            r#"{{"tag_name": "v0.2.0", "assets": [
                {{"name": "latest.json", "size": 9, "browser_download_url": "{manifest_url}"}},
                {{"name": "latest.json.sig", "size": 9, "browser_download_url": "{sig_url}"}}]}}"#
        );
        let result = parse_signed(
            &json,
            "0.1.3",
            &keys,
            OffsetDateTime::now_utc(),
            &mut |_, _| panic!("nothing is fetched from an address that fails the check"),
        );
        assert!(
            matches!(
                result,
                Err(SelfUpdateError::Manifest(ManifestError::Malformed(_)))
            ),
            "{manifest_url} {sig_url}: {result:?}"
        );
    }
}

/// Serves `respond` on localhost, one thread per request, until the flag is set.
fn serve_with(
    respond: impl Fn(tiny_http::Request) + Send + Sync + 'static,
) -> (String, Arc<AtomicBool>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let respond = Arc::new(respond);
    std::thread::spawn(move || {
        while !flag.load(AtomicOrdering::SeqCst) {
            let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                continue;
            };
            let respond = respond.clone();
            std::thread::spawn(move || respond(request));
        }
    });
    (base, stop)
}

/// A body with no end.
struct Endless;

impl std::io::Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        buf.fill(b'M');
        Ok(buf.len())
    }
}

/// A body that sends two bytes and then says nothing for `pause`.
struct Stall {
    sent: bool,
    pause: Duration,
}

impl std::io::Read for Stall {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.sent {
            std::thread::sleep(self.pause);
            return Ok(0);
        }
        self.sent = true;
        buf[..2].copy_from_slice(b"MZ");
        Ok(2)
    }
}

fn chunked(body: impl std::io::Read + Send + 'static) -> tiny_http::ResponseBox {
    tiny_http::Response::new(tiny_http::StatusCode(200), vec![], body, None, None).boxed()
}

fn asset_at(url: String, size: u64) -> toolkit::Asset {
    toolkit::Asset {
        name: exe_name("0.2.0"),
        url,
        size,
        sha256: Some(sha_hex(PROGRAM)),
    }
}

#[test]
fn a_download_longer_than_the_manifest_size_is_cut_off() {
    let key = generate_signing_key();
    let keys = TrustedKeys::of(key.verifying_key());
    // The manifest vouches for PROGRAM; the server never stops sending.
    let (_base, stop, json) = serve_signed(&key, PROGRAM, |_| {});
    let (api, api_stop) = serve(vec![("/repos/o/r/releases/latest", json.into_bytes())]);
    let mut release = check_with(&api, "o/r", "0.1.3", &keys).unwrap().unwrap();
    stop.store(true, AtomicOrdering::SeqCst);
    let (endless, endless_stop) = serve_with(|request| {
        let _ = request.respond(chunked(Endless));
    });
    release.exe.url = format!("{endless}/exe");
    let (_root, paths, exe) = fixture();
    let started = std::time::Instant::now();
    let result = stage(&release, &exe, &paths, &mut |_| {}, &CancelToken::new());
    endless_stop.store(true, AtomicOrdering::SeqCst);
    api_stop.store(true, AtomicOrdering::SeqCst);
    assert!(matches!(result, Err(SelfUpdateError::Size)), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    // Neither the partial file nor a proof stays.
    assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
}

#[test]
fn a_body_larger_than_the_size_is_refused_without_writing_it() {
    let big = vec![b'M'; 64 * 1024];
    let (base, stop) = serve(vec![("/exe", big)]);
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    let result = toolkit::download_capped(
        &toolkit::update_agent(None),
        &asset_at(format!("{base}/exe"), 100),
        &part,
        Some(100),
        &mut |_| {},
        &CancelToken::new(),
    );
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(result, Err(toolkit::ToolkitError::TooLarge)),
        "{result:?}"
    );
    let written = fs::metadata(&part).map_or(0, |m| m.len());
    assert_eq!(written, 0, "not a byte of the body was written");
}

#[test]
fn the_cap_stops_a_body_without_a_length_after_the_first_byte_over() {
    let (base, stop) = serve_with(|request| {
        let _ = request.respond(chunked(Endless));
    });
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    let result = toolkit::download_capped(
        &toolkit::update_agent(None),
        &asset_at(format!("{base}/exe"), 1000),
        &part,
        Some(1000),
        &mut |_| {},
        &CancelToken::new(),
    );
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(result, Err(toolkit::ToolkitError::TooLarge)),
        "{result:?}"
    );
    assert!(
        fs::metadata(&part).unwrap().len() <= 1000,
        "never writes past the cap"
    );
    // Without a cap nothing changes for the other callers: the digest decides.
}

#[test]
fn a_stalled_download_times_out() {
    let (base, stop) = serve_with(|request| {
        let body = Stall {
            sent: false,
            pause: Duration::from_secs(8),
        };
        let _ = request.respond(chunked(body));
    });
    let dir = tempfile::tempdir().unwrap();
    let part = dir.path().join("x.part");
    let agent = toolkit::update_agent_with(None, false, Duration::from_millis(400));
    let started = std::time::Instant::now();
    let result = toolkit::download_capped(
        &agent,
        &asset_at(format!("{base}/exe"), 1000),
        &part,
        Some(1000),
        &mut |_| {},
        &CancelToken::new(),
    );
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(result, Err(toolkit::ToolkitError::Network(_))),
        "{result:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "gave up after {:?}",
        started.elapsed()
    );
}

#[test]
fn the_update_agent_follows_few_redirects_and_in_production_speaks_only_https() {
    // A server that always sends the client on to itself.
    let (base, stop) = serve_with(|request| {
        let location = tiny_http::Header::from_bytes(&b"Location"[..], &b"/again"[..]).unwrap();
        let _ = request.respond(tiny_http::Response::empty(302).with_header(location));
    });
    let result = toolkit::update_agent(None)
        .get(&format!("{base}/start"))
        .call();
    assert!(result.is_err(), "a redirect loop must end in an error");
    // The production policy refuses plain http outright (so also a redirect that leads to it).
    let strict = toolkit::update_agent_with(None, true, Duration::from_secs(5));
    let result = strict.get(&format!("{base}/start")).call();
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(result.is_err(), "{result:?}");
}

#[test]
fn the_key_list_in_the_repository_is_readable_and_decides_the_mode() {
    // A line that is not a comment or a key fails here, in CI, instead of reading as "no key".
    let keys = TrustedKeys::parse(EMBEDDED_KEYS).expect("data/update-keys.txt must parse");
    assert_eq!(signing_required(), !keys.is_empty());
    // And an unreadable list is never taken for an empty one.
    assert!(TrustedKeys::parse("ed25519 nonsense").is_err());
}
