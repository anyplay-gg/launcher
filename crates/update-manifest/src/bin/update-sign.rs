//! The tool that makes the keys and the signed manifest of a launcher release.
//!
//! ```text
//! update-sign keygen --secret-out <file> [--public-out <file>] [--label <text>]
//! update-sign manifest --version <v> --exe <file> --out <latest.json>
//!                      [--appimage <file>] [--macos-arm64 <zip>] [--minimum <v>] [--supported-file <MIN_SUPPORTED_VERSION>] [--days 30] [--base-url <url>]
//! update-sign sign --manifest <latest.json> --out <latest.json.sig>
//!                  (--key-env <NAME> | --key-file <file>)
//! update-sign verify --manifest <latest.json> --sig <latest.json.sig> --keys <file>
//!                    [--current <v>]
//! ```
//!
//! The secret key is read from a file or an environment variable and written to a file; it is
//! never printed. `keygen` prints only the public line for `data/update-keys.txt`.
//!
//! On Unix `keygen` creates the secret file with mode 0600. **On Windows the file inherits the
//! ACL of its folder**, so write it into a private folder (not a shared or synced one) and move it
//! into the password manager at once.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyplay_update_manifest::{
    Artifact, LINUX_X64, MACOS_ARM64, MAX_VALIDITY_DAYS, Manifest, SCHEMA, TrustedKeys, Version,
    WINDOWS_X64, format_time, generate_signing_key, key_line, parse_supported_file, seed_hex, sign,
    signing_key_from_hex, to_hex, verify,
};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use zeroize::Zeroizing;

/// The longest a manifest may stay valid; `Manifest::validate` enforces the same line when signing.
const MAX_DAYS: i64 = MAX_VALIDITY_DAYS;
const DEFAULT_BASE: &str = "https://github.com/anyplaynet/launcher/releases/download";

type Result<T> = std::result::Result<T, String>;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("update-sign: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<()> {
    let (command, rest) = args.split_first().ok_or_else(usage)?;
    let flags = Flags::parse(rest)?;
    match command.as_str() {
        "keygen" => keygen(&flags),
        "manifest" => manifest(&flags),
        "sign" => sign_command(&flags),
        "verify" => verify_command(&flags),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage: update-sign keygen|manifest|sign|verify (see the header of crates/update-manifest/src/bin/update-sign.rs)
     keygen: on Windows the secret file inherits the ACL of its folder; write it to a private folder"
        .into()
}

/// `--name value` pairs.
struct Flags(BTreeMap<String, String>);

impl Flags {
    fn parse(args: &[String]) -> Result<Flags> {
        let mut map = BTreeMap::new();
        let mut args = args.iter();
        while let Some(name) = args.next() {
            let name = name
                .strip_prefix("--")
                .ok_or_else(|| format!("unexpected argument {name}"))?;
            let value = args
                .next()
                .ok_or_else(|| format!("--{name} needs a value"))?;
            if map.insert(name.to_owned(), value.clone()).is_some() {
                return Err(format!("--{name} given twice"));
            }
        }
        Ok(Flags(map))
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }

    fn need(&self, name: &str) -> Result<&str> {
        self.get(name)
            .ok_or_else(|| format!("--{name} is required"))
    }
}

fn keygen(flags: &Flags) -> Result<()> {
    let secret_out = PathBuf::from(flags.need("secret-out")?);
    let label = flags.get("label").unwrap_or("");
    let key = generate_signing_key();
    // create_new: an existing secret is never overwritten.
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options
        .open(&secret_out)
        .map_err(|e| format!("cannot create {}: {e}", secret_out.display()))?;
    writeln!(file, "{}", seed_hex(&key).as_str()).map_err(|e| e.to_string())?;
    drop(file);
    let line = key_line(&key.verifying_key(), label);
    if let Some(public_out) = flags.get("public-out") {
        fs::write(public_out, format!("{line}\n")).map_err(|e| e.to_string())?;
    }
    eprintln!(
        "secret key written to {} (not shown). Keep it offline and in a password manager.",
        secret_out.display()
    );
    println!("{line}");
    Ok(())
}

fn manifest(flags: &Flags) -> Result<()> {
    let version = flags.need("version")?.trim_start_matches('v').to_owned();
    let exe = PathBuf::from(flags.need("exe")?);
    let out = PathBuf::from(flags.need("out")?);
    let minimum = flags.get("minimum").unwrap_or("0.1.0").to_owned();
    // The repository's MIN_SUPPORTED_VERSION file; one with no version in it leaves the field out.
    let supported = match flags.get("supported-file") {
        Some(file) => {
            let text = fs::read_to_string(file).map_err(|e| format!("cannot read {file}: {e}"))?;
            parse_supported_file(&text).map_err(|e| e.to_string())?
        }
        None => None,
    };
    let days: i64 = flags
        .get("days")
        .unwrap_or("30")
        .parse()
        .map_err(|_| "--days is not a number".to_owned())?;
    if !(1..=MAX_DAYS).contains(&days) {
        return Err(format!("--days must be 1 to {MAX_DAYS}"));
    }
    let name = exe
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("--exe has no file name")?
        .to_owned();
    let expected = format!("AnyPlay-Launcher-{version}-x64.exe");
    if name != expected {
        return Err(format!("the exe must be named {expected}, not {name}"));
    }
    let data = fs::read(&exe).map_err(|e| format!("cannot read {}: {e}", exe.display()))?;
    let base = flags.get("base-url").map_or_else(
        || format!("{DEFAULT_BASE}/v{version}"),
        |b| b.trim_end_matches('/').to_owned(),
    );
    let mut platforms = BTreeMap::from([(WINDOWS_X64.to_owned(), artifact(&base, name, &data))]);
    if let Some(path) = flags.get("appimage") {
        let path = Path::new(path);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("--appimage has no file name")?
            .to_owned();
        let expected = format!("AnyPlay-Launcher-{version}-x86_64.AppImage");
        if name != expected {
            return Err(format!("the AppImage must be named {expected}, not {name}"));
        }
        let data = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        platforms.insert(LINUX_X64.to_owned(), artifact(&base, name, &data));
    }
    if let Some(path) = flags.get("macos-arm64") {
        let path = Path::new(path);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("--macos-arm64 has no file name")?
            .to_owned();
        let expected = format!("AnyPlay-Launcher-{version}-macos-arm64.zip");
        if name != expected {
            return Err(format!(
                "the macOS archive must be named {expected}, not {name}"
            ));
        }
        let data = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        platforms.insert(MACOS_ARM64.to_owned(), artifact(&base, name, &data));
    }
    let now = OffsetDateTime::now_utc();
    let manifest = Manifest {
        schema: SCHEMA,
        version,
        published: format_time(now),
        expires: format_time(now + Duration::days(days)),
        minimum_version: minimum,
        min_supported_version: supported,
        platforms,
    };
    manifest.validate().map_err(|e| e.to_string())?;
    let mut text = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
    text.push('\n');
    fs::write(&out, text).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    eprintln!("wrote {} (expires {})", out.display(), manifest.expires);
    Ok(())
}

/// The manifest entry of a file that is served from `base`.
fn artifact(base: &str, name: String, data: &[u8]) -> Artifact {
    Artifact {
        url: format!("{base}/{name}"),
        name,
        sha256: to_hex(&Sha256::digest(data)),
        size: data.len() as u64,
    }
}

fn read_secret(flags: &Flags) -> Result<Zeroizing<String>> {
    match (flags.get("key-env"), flags.get("key-file")) {
        (Some(name), None) => std::env::var(name)
            .map(Zeroizing::new)
            .map_err(|_| format!("the environment variable {name} is not set")),
        (None, Some(path)) => fs::read_to_string(path)
            .map(Zeroizing::new)
            .map_err(|e| format!("cannot read the key file: {e}")),
        _ => Err("give exactly one of --key-env and --key-file".into()),
    }
}

fn sign_command(flags: &Flags) -> Result<()> {
    let manifest_path = Path::new(flags.need("manifest")?);
    let out = Path::new(flags.need("out")?);
    // The secret never appears in an error message.
    let key = signing_key_from_hex(&read_secret(flags)?)
        .ok_or("the secret is not a 32-byte key in hex")?;
    let bytes = fs::read(manifest_path).map_err(|e| format!("cannot read the manifest: {e}"))?;
    // Never sign what the launcher would refuse.
    let parsed: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    parsed.validate().map_err(|e| e.to_string())?;
    parsed
        .check_time(OffsetDateTime::now_utc())
        .map_err(|e| e.to_string())?;
    let signature = sign(&bytes, &key);
    // Check the signature with the public key before it leaves this machine.
    verify(
        &bytes,
        &signature,
        &TrustedKeys::of(key.verifying_key()),
        OffsetDateTime::now_utc(),
    )
    .map_err(|e| format!("the new signature does not verify: {e}"))?;
    fs::write(out, signature).map_err(|e| format!("cannot write the signature: {e}"))?;
    eprintln!(
        "signed {} with the key {}",
        manifest_path.display(),
        key_line(&key.verifying_key(), "")
    );
    Ok(())
}

fn verify_command(flags: &Flags) -> Result<()> {
    let bytes = fs::read(flags.need("manifest")?).map_err(|e| e.to_string())?;
    let signature = fs::read_to_string(flags.need("sig")?).map_err(|e| e.to_string())?;
    let keys =
        TrustedKeys::parse(&fs::read_to_string(flags.need("keys")?).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if keys.is_empty() {
        return Err("the key list holds no key".into());
    }
    let manifest =
        verify(&bytes, &signature, &keys, OffsetDateTime::now_utc()).map_err(|e| e.to_string())?;
    if let Some(current) = flags.get("current") {
        if Version::parse(current).is_none() {
            return Err("--current is not a version".into());
        }
        manifest
            .allows_update_from(current)
            .map_err(|e| e.to_string())?;
    }
    eprintln!(
        "ok: version {} signed by a trusted key, valid until {}",
        manifest.version, manifest.expires
    );
    Ok(())
}
