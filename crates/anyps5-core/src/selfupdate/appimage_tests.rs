//! The Linux AppImage in the signed flow: check, download, verify, stage and swap. Every key is
//! made by the test; the server is a local one. The checks run on any system, the swap only where
//! an AppImage can be installed (Unix).

use super::*;
use anyplay_update_manifest::{SCHEMA, format_time, generate_signing_key, sign};
use ed25519_dalek::SigningKey;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::time::Duration;
use time::Duration as Span;

/// A 64-bit ELF header with the AppImage type 2 magic, then a payload.
const IMAGE: &[u8] = b"\x7fELF\x02\x01\x01\x00AI\x02-new-appimage";
const OLD_IMAGE: &[u8] = b"\x7fELF\x02\x01\x01\x00AI\x02-old-appimage";

fn sha_hex(data: &[u8]) -> String {
    toolkit::hex(&Sha256::digest(data))
}

fn artifact(name: String, url: String, data: &[u8]) -> Artifact {
    Artifact {
        name,
        url,
        sha256: sha_hex(data),
        size: data.len() as u64,
    }
}

/// A manifest for 0.2.0 that offers `image` for Linux and, when given, an exe for Windows.
fn manifest(base: &str, image: &[u8], windows: bool) -> Manifest {
    let now = OffsetDateTime::now_utc();
    let mut platforms = BTreeMap::new();
    if windows {
        platforms.insert(
            WINDOWS_X64.to_owned(),
            artifact(exe_name("0.2.0"), format!("{base}/exe"), b"MZ-exe"),
        );
    }
    platforms.insert(
        LINUX_X64.to_owned(),
        artifact(appimage_name("0.2.0"), format!("{base}/image"), image),
    );
    Manifest {
        schema: SCHEMA,
        version: "0.2.0".into(),
        published: format_time(now - Span::days(1)),
        expires: format_time(now + Span::days(29)),
        minimum_version: "0.1.0".into(),
        min_supported_version: None,
        platforms,
    }
}

/// One server for the API, the manifest, its signature, the AppImage (`served`) and the exe.
/// `edit` changes the manifest before it is signed with `signer`.
fn serve_release(
    signer: &SigningKey,
    served: &[u8],
    edit: impl FnOnce(&mut Manifest),
) -> (String, Arc<AtomicBool>) {
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let base = format!("http://{}", server.server_addr().to_ip().unwrap());
    let mut manifest = manifest(&base, IMAGE, true);
    edit(&mut manifest);
    let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    let signature = sign(&bytes, signer);
    let release = format!(
        r#"{{"tag_name": "v9.9.9", "assets": [
            {{"name": "latest.json", "size": 9, "browser_download_url": "{base}/latest.json"}},
            {{"name": "latest.json.sig", "size": 9, "browser_download_url": "{base}/latest.json.sig"}}
        ]}}"#
    );
    let routes: Vec<(&str, Vec<u8>)> = vec![
        ("/repos/o/r/releases/latest", release.into_bytes()),
        ("/latest.json", bytes),
        ("/latest.json.sig", signature.into_bytes()),
        ("/image", served.to_vec()),
        ("/exe", b"MZ-exe".to_vec()),
    ];
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

fn keys_of(key: &SigningKey) -> TrustedKeys {
    TrustedKeys::of(key.verifying_key())
}

fn check_linux(base: &str, keys: &TrustedKeys) -> Result<Option<Release>, SelfUpdateError> {
    check_with(base, "o/r", "0.1.3", keys, Platform::Linux)
}

/// A folder with a running "AppImage" in it.
fn fixture() -> (tempfile::TempDir, AppPaths, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let paths = AppPaths::under(root.path());
    let image = root.path().join("apps").join("AnyPlay-Launcher.AppImage");
    fs::create_dir_all(image.parent().unwrap()).unwrap();
    fs::write(&image, OLD_IMAGE).unwrap();
    (root, paths, image)
}

fn pending_linux(paths: &AppPaths, keys: &TrustedKeys) -> Option<Staged> {
    pending_with(
        paths,
        "0.1.3",
        Platform::Linux,
        Some(keys),
        OffsetDateTime::now_utc(),
    )
}

#[test]
fn a_signed_appimage_is_found_with_its_own_name_and_hash() {
    let key = generate_signing_key();
    let (base, stop) = serve_release(&key, IMAGE, |_| {});
    let release = check_linux(&base, &keys_of(&key)).unwrap().unwrap();
    assert_eq!(release.version(), "0.2.0");
    assert_eq!(release.exe.name, "AnyPlay-Launcher-0.2.0-x86_64.AppImage");
    assert_eq!(release.exe.sha256.as_deref(), Some(sha_hex(IMAGE).as_str()));
    assert_eq!(release.platform, Platform::Linux);
    // The same manifest still gives Windows its exe.
    let windows = check_with(&base, "o/r", "0.1.3", &keys_of(&key), Platform::Windows)
        .unwrap()
        .unwrap();
    assert_eq!(windows.exe.name, "AnyPlay-Launcher-0.2.0-x64.exe");
    stop.store(true, AtomicOrdering::SeqCst);
}

#[test]
fn a_manifest_without_a_linux_entry_offers_nothing_and_is_no_error() {
    let key = generate_signing_key();
    // What every release up to v0.2.4 has: a Windows entry only.
    let (base, stop) = serve_release(&key, IMAGE, |m| {
        m.platforms.remove(LINUX_X64);
    });
    assert!(matches!(check_linux(&base, &keys_of(&key)), Ok(None)));
    // Windows is served as before.
    assert!(
        check_with(&base, "o/r", "0.1.3", &keys_of(&key), Platform::Windows)
            .unwrap()
            .is_some()
    );
    stop.store(true, AtomicOrdering::SeqCst);
}

#[test]
fn a_manifest_with_no_windows_entry_is_still_an_error_for_windows_only() {
    let key = generate_signing_key();
    let (base, stop) = serve_release(&key, IMAGE, |m| {
        m.platforms.remove(WINDOWS_X64);
    });
    let windows = check_with(&base, "o/r", "0.1.3", &keys_of(&key), Platform::Windows);
    assert!(
        matches!(windows, Err(SelfUpdateError::NoRelease)),
        "{windows:?}"
    );
    assert!(check_linux(&base, &keys_of(&key)).unwrap().is_some());
    stop.store(true, AtomicOrdering::SeqCst);
}

#[test]
fn linux_never_updates_without_a_trusted_key() {
    let key = generate_signing_key();
    let (base, stop) = serve_release(&key, IMAGE, |_| {});
    let none = TrustedKeys::parse("").unwrap();
    assert!(matches!(check_linux(&base, &none), Ok(None)));
    stop.store(true, AtomicOrdering::SeqCst);
    // Nor is a file in the updates folder offered when nothing is signed.
    let (_root, paths, _) = fixture();
    let dir = updates_dir(&paths);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(appimage_name("0.2.0")), IMAGE).unwrap();
    assert!(
        pending_with(
            &paths,
            "0.1.3",
            Platform::Linux,
            None,
            OffsetDateTime::now_utc()
        )
        .is_none()
    );
}

#[test]
fn the_signature_expiry_and_rollback_rules_hold_for_linux() {
    let key = generate_signing_key();
    let other = generate_signing_key();
    // Signed by a key nobody trusts.
    let (base, stop) = serve_release(&other, IMAGE, |_| {});
    assert!(matches!(
        check_linux(&base, &keys_of(&key)),
        Err(SelfUpdateError::Manifest(ManifestError::Signature))
    ));
    stop.store(true, AtomicOrdering::SeqCst);
    // Expired.
    let (base, stop) = serve_release(&key, IMAGE, |m| {
        let now = OffsetDateTime::now_utc();
        m.published = format_time(now - Span::days(40));
        m.expires = format_time(now - Span::days(10));
    });
    assert!(matches!(
        check_linux(&base, &keys_of(&key)),
        Err(SelfUpdateError::Manifest(ManifestError::Expired(_)))
    ));
    stop.store(true, AtomicOrdering::SeqCst);
    // Not newer than what runs: nothing, and no error.
    let (base, stop) = serve_release(&key, IMAGE, |_| {});
    assert!(matches!(
        check_with(&base, "o/r", "0.2.0", &keys_of(&key), Platform::Linux),
        Ok(None)
    ));
    assert!(matches!(
        check_with(&base, "o/r", "0.3.0", &keys_of(&key), Platform::Linux),
        Ok(None)
    ));
    stop.store(true, AtomicOrdering::SeqCst);
}

#[test]
fn a_linux_entry_must_carry_the_appimage_name_and_a_safe_address() {
    let key = generate_signing_key();
    let refused = |edit: fn(&mut Manifest)| {
        let (base, stop) = serve_release(&key, IMAGE, edit);
        let result = check_linux(&base, &keys_of(&key));
        stop.store(true, AtomicOrdering::SeqCst);
        result
    };
    for name in [
        "AnyPlay-Launcher-0.2.0-x64.exe",
        "AnyPlay-Launcher-0.2.1-x86_64.AppImage",
        "../AnyPlay-Launcher-0.2.0-x86_64.AppImage",
        "evil.AppImage",
    ] {
        // Signed and served for real: the name alone is what is refused.
        let (base, stop) = serve_release(&key, IMAGE, move |m| {
            m.platforms.get_mut(LINUX_X64).unwrap().name = name.to_owned();
        });
        let result = check_linux(&base, &keys_of(&key));
        stop.store(true, AtomicOrdering::SeqCst);
        assert!(
            matches!(
                result,
                Err(SelfUpdateError::Manifest(ManifestError::Malformed(_)))
            ),
            "{name}: {result:?}"
        );
    }
    let http = refused(|m| {
        // Plain http to somewhere else than this machine.
        m.platforms.get_mut(LINUX_X64).unwrap().url = "http://example.com/a.AppImage".into();
    });
    assert!(matches!(
        http,
        Err(SelfUpdateError::Manifest(ManifestError::Malformed(_)))
    ));
}

#[test]
fn a_download_that_is_no_appimage_is_refused_and_leaves_nothing() {
    let key = generate_signing_key();
    let page = b"<html>rate limited</html>--------";
    let (base, stop) = serve_release(&key, page, |m| {
        *m.platforms.get_mut(LINUX_X64).unwrap() = artifact(
            appimage_name("0.2.0"),
            m.platforms[LINUX_X64].url.clone(),
            page,
        );
    });
    let release = check_linux(&base, &keys_of(&key)).unwrap().unwrap();
    let (_root, paths, image) = fixture();
    let result = stage(&release, &image, &paths, &mut |_| {}, &CancelToken::new());
    stop.store(true, AtomicOrdering::SeqCst);
    assert!(
        matches!(result, Err(SelfUpdateError::NotAnExecutable)),
        "{result:?}"
    );
    assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
}

#[test]
fn another_hash_or_size_than_signed_is_refused_and_leaves_nothing() {
    let key = generate_signing_key();
    let other = b"\x7fELF\x02\x01\x01\x00AI\x02-tampered!!";
    for served in [&other[..], &IMAGE[..IMAGE.len() - 1]] {
        let (base, stop) = serve_release(&key, served, |_| {});
        let release = check_linux(&base, &keys_of(&key)).unwrap().unwrap();
        let (_root, paths, image) = fixture();
        let result = stage(&release, &image, &paths, &mut |_| {}, &CancelToken::new());
        stop.store(true, AtomicOrdering::SeqCst);
        assert!(
            matches!(result, Err(SelfUpdateError::Digest | SelfUpdateError::Size)),
            "{result:?}"
        );
        assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
    }
}

#[test]
fn a_staged_appimage_is_checked_again_and_planted_files_are_not_offered() {
    let key = generate_signing_key();
    let keys = keys_of(&key);
    let (base, stop) = serve_release(&key, IMAGE, |_| {});
    let release = check_linux(&base, &keys).unwrap().unwrap();
    let (_root, paths, image) = fixture();
    let staged = stage(&release, &image, &paths, &mut |_| {}, &CancelToken::new()).unwrap();
    stop.store(true, AtomicOrdering::SeqCst);
    assert_eq!(staged.platform, Platform::Linux);
    assert_eq!(fs::read(&staged.path).unwrap(), IMAGE);
    assert_eq!(
        pending_linux(&paths, &keys).map(|s| s.version),
        Some("0.2.0".into())
    );
    // Changed afterwards: not offered. Another key: not offered. A Windows exe: not offered.
    fs::write(&staged.path, b"\x7fELF\x02\x01\x01\x00AI\x02-changed!!!!").unwrap();
    assert!(pending_linux(&paths, &keys).is_none());
    fs::write(&staged.path, IMAGE).unwrap();
    assert!(pending_linux(&paths, &keys).is_some());
    assert!(pending_linux(&paths, &keys_of(&generate_signing_key())).is_none());
    assert!(
        pending_with(
            &paths,
            "0.1.3",
            Platform::Windows,
            Some(&keys),
            OffsetDateTime::now_utc()
        )
        .is_none()
    );
    // A later cleanup keeps it while it is newer, and drops it with its proof once it is not.
    cleanup_for(Platform::Linux, &image, &paths, "0.1.3");
    assert!(staged.path.exists());
    cleanup_for(Platform::Linux, &image, &paths, "0.2.0");
    assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
}

#[cfg(unix)]
mod swap {
    use super::*;
    use std::io::Read as _;
    use std::os::unix::fs::PermissionsExt as _;

    /// A signed AppImage staged next to a running one.
    fn staged() -> (tempfile::TempDir, AppPaths, PathBuf, Staged, TrustedKeys) {
        let key = generate_signing_key();
        let keys = keys_of(&key);
        let (base, stop) = serve_release(&key, IMAGE, |_| {});
        let release = check_linux(&base, &keys).unwrap().unwrap();
        let (root, paths, image) = fixture();
        let staged = stage(&release, &image, &paths, &mut |_| {}, &CancelToken::new()).unwrap();
        stop.store(true, AtomicOrdering::SeqCst);
        (root, paths, image, staged, keys)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_whole_flow_replaces_the_running_appimage_atomically() {
        let (_root, paths, image, staged, keys) = staged();
        // The running image holds its file open, as the AppImage runtime does.
        let mut running = fs::File::open(&image).unwrap();
        fs::set_permissions(&image, fs::Permissions::from_mode(0o700)).unwrap();

        let offered = pending_linux(&paths, &keys).expect("offered again after a restart");
        assert_eq!(offered.sha256, staged.sha256);
        offered.swap_in(&image).unwrap();

        assert_eq!(fs::read(&image).unwrap(), IMAGE);
        assert_eq!(
            fs::metadata(&image).unwrap().permissions().mode() & 0o777,
            0o755
        );
        // The old image keeps running from its open file; nothing else is left beside it.
        let mut old = Vec::new();
        running.read_to_end(&mut old).unwrap();
        assert_eq!(old, OLD_IMAGE);
        assert_eq!(
            names(image.parent().unwrap()),
            ["AnyPlay-Launcher.AppImage"]
        );
        // The staged file and its proof are consumed.
        assert_eq!(fs::read_dir(updates_dir(&paths)).unwrap().count(), 0);
    }

    #[test]
    fn a_staged_file_changed_after_the_check_is_not_installed() {
        let (_root, _paths, image, staged, _keys) = staged();
        fs::write(&staged.path, b"\x7fELF\x02\x01\x01\x00AI\x02-swapped!!!!").unwrap();
        let result = staged.swap_in(&image);
        assert!(matches!(result, Err(SelfUpdateError::Digest)), "{result:?}");
        assert_eq!(fs::read(&image).unwrap(), OLD_IMAGE);
        assert_eq!(
            names(image.parent().unwrap()),
            ["AnyPlay-Launcher.AppImage"]
        );
    }

    #[test]
    fn an_appimage_without_a_signed_hash_is_never_installed() {
        let (_root, _paths, image, staged, _keys) = staged();
        let unsigned = Staged {
            sha256: None,
            ..staged
        };
        assert!(matches!(
            unsigned.swap_in(&image),
            Err(SelfUpdateError::Digest)
        ));
        assert_eq!(fs::read(&image).unwrap(), OLD_IMAGE);
        assert_eq!(
            names(image.parent().unwrap()),
            ["AnyPlay-Launcher.AppImage"]
        );
    }

    #[test]
    fn a_link_is_not_replaced_and_a_missing_staged_file_changes_nothing() {
        let (root, _paths, image, staged, _keys) = staged();
        let link = root.path().join("apps").join("link.AppImage");
        std::os::unix::fs::symlink(&image, &link).unwrap();
        assert!(staged.swap_in(&link).is_err());
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(&image).unwrap(), OLD_IMAGE);

        let gone = Staged {
            path: root.path().join("no-such-file"),
            ..staged
        };
        assert!(gone.swap_in(&image).is_err());
        assert_eq!(fs::read(&image).unwrap(), OLD_IMAGE);
        assert_eq!(
            names(image.parent().unwrap()),
            ["AnyPlay-Launcher.AppImage", "link.AppImage"]
        );
    }

    #[test]
    fn restart_starts_the_new_image_with_the_arguments() {
        // A staged "image" that is a script: the swap makes it executable and restart runs it.
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("out.txt");
        let script = format!("#!/bin/sh\necho \"$@\" > {}\n", out.display());
        let image = root.path().join("AnyPlay-Launcher.AppImage");
        fs::write(&image, OLD_IMAGE).unwrap();
        let path = root.path().join("staged");
        fs::write(&path, &script).unwrap();
        let staged = Staged {
            version: "0.2.0".into(),
            path,
            sha256: Some(sha_hex(script.as_bytes())),
            platform: Platform::Linux,
        };
        staged
            .restart(&image, ["--after-update=1".into(), "x".into()])
            .unwrap();
        for _ in 0..100 {
            if out.exists() && !fs::read_to_string(&out).unwrap_or_default().is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(fs::read_to_string(&out).unwrap(), "--after-update=1 x\n");
    }

    #[test]
    fn the_appimage_is_found_from_the_runtime_variables_only_when_it_is_the_running_one() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().canonicalize().unwrap();
        let image = dir.join("AnyPlay-Launcher.AppImage");
        fs::write(&image, OLD_IMAGE).unwrap();
        let mount = dir.join(".mount_abc");
        let running = mount.join("usr/bin/anyplay-launcher");
        let find = |image: &Path, appdir: &Path, running: &Path| {
            appimage_path(Some(image.as_os_str()), Some(appdir.as_os_str()), running)
        };
        assert_eq!(find(&image, &mount, &running), Some(image.clone()));
        // No variables: a .tar.gz run, which stays manual.
        assert_eq!(appimage_path(None, None, &running), None);
        assert_eq!(appimage_path(Some(image.as_os_str()), None, &running), None);
        // Variables inherited from another AppImage: this program does not run from that mount.
        let tarball = dir.join("AnyPlay-Launcher-0.2.0/anyplay-launcher");
        assert_eq!(find(&image, &mount, &tarball), None);
        // Not absolute, with `..`, the filesystem root as the mount, a wrong name, a folder,
        // a missing file and a link are all refused.
        assert_eq!(find(Path::new("a.AppImage"), &mount, &running), None);
        let dotted = dir.join("x/../AnyPlay-Launcher.AppImage");
        assert_eq!(find(&dotted, &mount, &running), None);
        assert_eq!(find(&image, Path::new("/"), &running), None);
        let plain = dir.join("AnyPlay-Launcher");
        fs::write(&plain, b"x").unwrap();
        assert_eq!(find(&plain, &mount, &running), None);
        let bare = dir.join(".AppImage");
        fs::write(&bare, b"x").unwrap();
        assert_eq!(find(&bare, &mount, &running), None);
        let folder = dir.join("folder.AppImage");
        fs::create_dir(&folder).unwrap();
        assert_eq!(find(&folder, &mount, &running), None);
        assert_eq!(find(&dir.join("gone.AppImage"), &mount, &running), None);
        let link = dir.join("link.AppImage");
        std::os::unix::fs::symlink(&image, &link).unwrap();
        assert_eq!(find(&link, &mount, &running), None);
    }
}
