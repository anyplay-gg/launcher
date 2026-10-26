//! Whole-bundle macOS updates. The signed zip is copied and hashed on the destination volume,
//! extracted without links, checked with macOS's signature tools, then exchanged atomically.

use super::*;
use std::ffi::CString;
use std::io::{Read as _, Seek as _};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;

const BUNDLE: &str = "AnyPlay Launcher.app";
const EXECUTABLE: &str = "Contents/MacOS/anyplay-launcher";
const IDENTIFIER: &str = "gg.anyplay.launcher";
const MAX_UNPACKED: u64 = 2 * 1024 * 1024 * 1024;

/// A real app bundle containing this executable; bare binaries stay manual. Canonical paths
/// also prevent an alias or a translocated app from selecting another installation to replace.
pub(super) fn install_path(running: &Path) -> Option<PathBuf> {
    let app = running.parent()?.parent()?.parent()?;
    if app.extension()? != "app" || running != app.join(EXECUTABLE) {
        return None;
    }
    if app
        .components()
        .any(|part| part.as_os_str() == "AppTranslocation")
    {
        return None;
    }
    if fs::canonicalize(running).ok()? != running || fs::canonicalize(app).ok()? != app {
        return None;
    }
    (plist(app, "CFBundleIdentifier").ok()? == IDENTIFIER).then(|| app.to_path_buf())
}

fn plist(app: &Path, key: &str) -> Result<String, SelfUpdateError> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()?;
    if !output.status.success() {
        return Err(SelfUpdateError::NotAnExecutable);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn verify_bundle(app: &Path, version: &str) -> Result<(), SelfUpdateError> {
    if plist(app, "CFBundleIdentifier")? != IDENTIFIER
        || plist(app, "CFBundleExecutable")? != "anyplay-launcher"
        || plist(app, "CFBundleShortVersionString")? != version
        || plist(app, "CFBundleVersion")? != version
    {
        return Err(SelfUpdateError::NotAnExecutable);
    }
    let executable = app.join(EXECUTABLE);
    if !fs::symlink_metadata(&executable)?.file_type().is_file() {
        return Err(SelfUpdateError::NotAnExecutable);
    }
    let archs = Command::new("/usr/bin/lipo")
        .arg("-archs")
        .arg(&executable)
        .output()?;
    if !archs.status.success()
        || !String::from_utf8_lossy(&archs.stdout)
            .split_whitespace()
            .any(|arch| arch == "arm64")
    {
        return Err(SelfUpdateError::NotAnExecutable);
    }
    // This is integrity of the entire bundle, including nested code. Authenticity comes from
    // the Ed25519-signed archive hash, not from its ad hoc code signature.
    if !Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .status()?
        .success()
    {
        return Err(SelfUpdateError::NotAnExecutable);
    }
    Ok(())
}

/// ZipArchive keeps only the last central entry for each name. Count the original records,
/// including duplicates, before accepting its indexed view. Entry bodies are still decoded
/// and CRC-checked by zip; this walk reads only fixed headers and skips their variable fields.
fn central_entries(archive: &Path, start: u64) -> io::Result<usize> {
    let mut file = fs::File::open(archive)?;
    file.seek(io::SeekFrom::Start(start))?;
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid zip directory");
    let mut count = 0;
    loop {
        let mut magic = [0; 4];
        file.read_exact(&mut magic)?;
        match &magic {
            b"PK\x05\x06" | b"PK\x06\x06" => return Ok(count),
            b"PK\x01\x02" => {}
            _ => return Err(invalid()),
        }
        count += 1;
        if count > 4096 {
            return Err(invalid());
        }
        let mut fields = [0; 42];
        file.read_exact(&mut fields)?;
        let variable: i64 = [24, 26, 28]
            .into_iter()
            .map(|offset| i64::from(u16::from_le_bytes([fields[offset], fields[offset + 1]])))
            .sum();
        file.seek(io::SeekFrom::Current(variable))?;
    }
}

fn unpack(archive: &Path, destination: &Path) -> Result<PathBuf, SelfUpdateError> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid macOS app archive");
    let mut zip = zip::ZipArchive::new(fs::File::open(archive)?).map_err(|_| invalid())?;
    if zip.len() > 4096 || central_entries(archive, zip.central_directory_start())? != zip.len() {
        return Err(invalid().into());
    }
    let mut total = 0u64;
    let mut seen = std::collections::HashSet::new();
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|_| invalid())?;
        let name = entry.enclosed_name().ok_or_else(invalid)?;
        if !name.starts_with(BUNDLE)
            || name
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            || !seen.insert(name.clone())
        {
            return Err(invalid().into());
        }
        let kind = entry.unix_mode().unwrap_or(0) & 0o170000;
        if (entry.is_dir() && kind != 0 && kind != 0o040000)
            || (!entry.is_dir() && kind != 0 && kind != 0o100000)
        {
            return Err(invalid().into());
        }
        total = total.checked_add(entry.size()).ok_or_else(invalid)?;
        if total > MAX_UNPACKED {
            return Err(invalid().into());
        }
        let path = destination.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&path)?;
        } else {
            fs::create_dir_all(path.parent().ok_or_else(invalid)?)?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            let size = entry.size();
            let copied = io::copy(&mut (&mut entry).take(size + 1), &mut file)?;
            if copied != size {
                return Err(invalid().into());
            }
            file.sync_all()?;
            let executable = entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0);
            fs::set_permissions(
                &path,
                fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }),
            )?;
        }
    }
    Ok(destination.join(BUNDLE))
}

pub(super) fn swap(staged: &Staged, app: &Path) -> Result<(), SelfUpdateError> {
    let parent = app.parent().ok_or(SelfUpdateError::NotAnExecutable)?;
    if !fs::symlink_metadata(app)?.file_type().is_dir()
        || app.extension().is_none_or(|ext| ext != "app")
        || plist(app, "CFBundleIdentifier")? != IDENTIFIER
    {
        return Err(SelfUpdateError::NotAnExecutable);
    }
    let work = tempfile::Builder::new()
        .prefix(".anyplay-update-")
        .tempdir_in(parent)
        .map_err(|error| writable_or(error, parent))?;
    let archive = work.path().join("update.zip");
    fs::copy(&staged.path, &archive)?;
    staged.check_copy(&archive)?;
    let incoming = unpack(&archive, work.path())?;
    verify_bundle(&incoming, &staged.version)?;
    let from = CString::new(incoming.as_os_str().as_bytes())
        .map_err(|_| SelfUpdateError::NotAnExecutable)?;
    let to =
        CString::new(app.as_os_str().as_bytes()).map_err(|_| SelfUpdateError::NotAnExecutable)?;
    // Both directories exist on the same volume. A failed exchange leaves the installed app
    // untouched; a successful exchange puts the old bundle in the disposable work directory.
    let result = unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) };
    if result != 0 {
        return Err(writable_or(io::Error::last_os_error(), parent));
    }
    let _ = fs::remove_file(&staged.path);
    remove_proof(&staged.path);
    Ok(())
}

pub(super) fn restart(
    app: &Path,
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> io::Result<()> {
    Command::new("/usr/bin/open")
        .arg("-n")
        .arg(app)
        .arg("--args")
        .args(args)
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use zip::write::SimpleFileOptions;

    fn archive(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn archive_paths_duplicates_and_links_cannot_escape_the_bundle() {
        let work = tempfile::tempdir().unwrap();
        let zip = work.path().join("incoming.zip");
        for name in [
            "../outside",
            "other.app/Contents/Info.plist",
            "AnyPlay Launcher.app/../outside",
        ] {
            archive(&zip, &[(name, b"bad")]);
            let destination = tempfile::tempdir().unwrap();
            assert!(unpack(&zip, destination.path()).is_err(), "{name}");
            assert!(!work.path().join("outside").exists());
        }
        // ZipWriter refuses duplicate names itself. Make two same-length names, then change
        // the second in both headers so this reaches our reader as a valid duplicate archive.
        let first = "AnyPlay Launcher.app/Contents/one";
        let second = "AnyPlay Launcher.app/Contents/two";
        archive(&zip, &[(first, b"one"), (second, b"two")]);
        let mut bytes = fs::read(&zip).unwrap();
        for offset in 0..=bytes.len() - second.len() {
            if &bytes[offset..offset + second.len()] == second.as_bytes() {
                bytes[offset..offset + first.len()].copy_from_slice(first.as_bytes());
            }
        }
        fs::write(&zip, bytes).unwrap();
        let mut parsed = zip::ZipArchive::new(fs::File::open(&zip).unwrap()).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed.by_index(0).unwrap().name(), first);
        assert!(unpack(&zip, tempfile::tempdir().unwrap().path()).is_err());
        let mut writer = zip::ZipWriter::new(fs::File::create(&zip).unwrap());
        writer
            .add_symlink(
                "AnyPlay Launcher.app/Contents/link",
                "/outside",
                SimpleFileOptions::default(),
            )
            .unwrap();
        writer.finish().unwrap();
        assert!(unpack(&zip, tempfile::tempdir().unwrap().path()).is_err());
        archive(
            &zip,
            &[("AnyPlay Launcher.app/Contents/Info.plist", b"one")],
        );
        assert!(unpack(&zip, tempfile::tempdir().unwrap().path()).is_ok());
    }

    #[test]
    fn unsigned_or_modified_archives_leave_the_installed_bundle_untouched() {
        let work = tempfile::tempdir().unwrap();
        let app = work.path().join(BUNDLE);
        fs::create_dir_all(app.join("Contents")).unwrap();
        fs::write(app.join("Contents/Info.plist"), format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>{IDENTIFIER}</string></dict></plist>"#)).unwrap();
        fs::write(app.join("keep"), b"original").unwrap();
        let zip = work.path().join("incoming.zip");
        archive(
            &zip,
            &[("AnyPlay Launcher.app/Contents/Info.plist", b"invalid")],
        );
        let mut staged = Staged {
            version: "0.3.0".into(),
            path: zip.clone(),
            sha256: None,
            platform: Platform::MacosArm64,
        };
        assert!(matches!(staged.swap_in(&app), Err(SelfUpdateError::Digest)));
        staged.sha256 = Some(hash_file(&zip).unwrap());
        assert!(staged.swap_in(&app).is_err());
        assert_eq!(fs::read(app.join("keep")).unwrap(), b"original");
        assert!(zip.exists());
        assert_eq!(fs::read_dir(work.path()).unwrap().count(), 2);
    }
}

#[cfg(all(test, target_arch = "aarch64"))]
mod integration {
    use super::*;
    use anyplay_update_manifest::{SCHEMA, format_time, generate_signing_key, sign};
    use std::collections::BTreeMap;

    // Real native test programs and signed app bundles; only GitHub and its signing key are
    // substituted. The final boundary is executing the installed replacement and reading its
    // resource. No installed launcher, player settings or credentials are touched.
    fn app(parent: &Path, version: &str) -> PathBuf {
        let app = parent.join(BUNDLE);
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(app.join("Contents/Resources")).unwrap();
        let source = parent.join("fixture.c");
        fs::write(
            &source,
            format!("#include <stdio.h>\nint main(void) {{ puts(\"{version}\"); return 0; }}\n"),
        )
        .unwrap();
        assert!(
            Command::new("/usr/bin/clang")
                .arg(&source)
                .arg("-o")
                .arg(app.join(EXECUTABLE))
                .status()
                .unwrap()
                .success()
        );
        fs::write(app.join("Contents/Resources/version.txt"), version).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            format!(
                r#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{IDENTIFIER}</string>
<key>CFBundleExecutable</key><string>anyplay-launcher</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleVersion</key><string>{version}</string></dict></plist>"#
            ),
        )
        .unwrap();
        assert!(
            Command::new("/usr/bin/codesign")
                .args(["--force", "--sign", "-"])
                .arg(&app)
                .status()
                .unwrap()
                .success()
        );
        app
    }

    #[test]
    fn a_signed_zip_download_resumes_and_installs_the_whole_native_app() {
        let root = tempfile::tempdir().unwrap();
        let installed = app(&root.path().join("installed"), "0.2.6");
        let new_app = app(&root.path().join("build"), "0.3.0");
        let archive = root.path().join("release.zip");
        assert!(
            Command::new("/usr/bin/ditto")
                .args(["-c", "-k", "--norsrc", "--noextattr", "--keepParent"])
                .arg(&new_app)
                .arg(&archive)
                .status()
                .unwrap()
                .success()
        );
        let bytes = fs::read(&archive).unwrap();
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}", server.server_addr().to_ip().unwrap());
        let signer = generate_signing_key();
        let keys = TrustedKeys::of(signer.verifying_key());
        let now = OffsetDateTime::now_utc();
        let manifest = Manifest {
            schema: SCHEMA,
            version: "0.3.0".into(),
            published: format_time(now),
            expires: format_time(now + time::Duration::days(30)),
            minimum_version: "0.1.0".into(),
            min_supported_version: None,
            platforms: BTreeMap::from([(
                MACOS_ARM64.into(),
                Artifact {
                    name: "AnyPlay-Launcher-0.3.0-macos-arm64.zip".into(),
                    url: format!("{base}/app.zip"),
                    size: bytes.len() as u64,
                    sha256: hash_file(&archive).unwrap(),
                },
            )]),
        };
        let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
        let signature = sign(&manifest_bytes, &signer);
        let release_json = format!(
            r#"{{"assets":[{{"name":"latest.json","browser_download_url":"{base}/manifest"}},{{"name":"latest.json.sig","browser_download_url":"{base}/signature"}}]}}"#
        );
        let release = parse_signed(
            &release_json,
            "0.2.6",
            Platform::MacosArm64,
            &keys,
            now,
            &mut |url, _| {
                Ok(if url.ends_with("/manifest") {
                    manifest_bytes.clone()
                } else {
                    signature.as_bytes().to_vec()
                })
            },
        )
        .unwrap()
        .unwrap();
        let download = std::thread::spawn(move || {
            let request = server
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap()
                .unwrap();
            assert_eq!(request.url(), "/app.zip");
            request
                .respond(tiny_http::Response::from_data(bytes))
                .unwrap();
        });
        let paths = AppPaths::under(&root.path().join("data"));
        let staged = stage(
            &release,
            &installed,
            &paths,
            &mut |_| {},
            &CancelToken::new(),
        )
        .unwrap();
        download.join().unwrap();
        assert!(pending_with(&paths, "0.2.6", Platform::MacosArm64, None, now).is_none());
        let resumed =
            pending_with(&paths, "0.2.6", Platform::MacosArm64, Some(&keys), now).unwrap();
        assert_eq!(resumed.path, staged.path);
        // Tampering between staging and installation cannot replace the old app.
        let original = fs::read(&staged.path).unwrap();
        fs::write(&staged.path, b"tampered").unwrap();
        assert!(matches!(
            staged.swap_in(&installed),
            Err(SelfUpdateError::Digest)
        ));
        assert_eq!(plist(&installed, "CFBundleVersion").unwrap(), "0.2.6");
        fs::write(&staged.path, original).unwrap();
        resumed.swap_in(&installed).unwrap();
        assert_eq!(plist(&installed, "CFBundleVersion").unwrap(), "0.3.0");
        assert_eq!(
            fs::read_to_string(installed.join("Contents/Resources/version.txt")).unwrap(),
            "0.3.0"
        );
        let output = Command::new(installed.join(EXECUTABLE)).output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), "0.3.0");
        verify_bundle(&installed, "0.3.0").unwrap();
        let canonical = fs::canonicalize(installed.join(EXECUTABLE)).unwrap();
        assert_eq!(
            install_path(&canonical),
            Some(fs::canonicalize(&installed).unwrap())
        );
        assert!(install_path(&archive).is_none());
        assert!(!staged.path.exists());
        assert!(pending_with(&paths, "0.2.6", Platform::MacosArm64, Some(&keys), now).is_none());
    }
}
