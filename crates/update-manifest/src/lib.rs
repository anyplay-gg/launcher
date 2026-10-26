//! The signed update manifest of the AnyPlay Launcher.
//!
//! A GitHub release alone is not trusted: whoever can publish one (a stolen token, a compromised
//! workflow) could otherwise run code on every player's PC. Each release instead carries
//! `latest.json`, the manifest, and `latest.json.sig`, an Ed25519 signature over its exact bytes.
//! The launcher embeds the public keys it trusts (`data/update-keys.txt`) and accepts an update
//! only through a manifest that
//!
//! - is signed by one of those keys ([`ManifestError::Signature`]),
//! - has not expired ([`ManifestError::Expired`]; this stops a stale answer served forever), is
//!   valid for at most [`MAX_VALIDITY_DAYS`] days and was not made in the future,
//! - names a version newer than the running one ([`ManifestError::NotNewer`]; this stops a
//!   rollback to an old, vulnerable release),
//! - allows the running version to update from it ([`ManifestError::RunningTooOld`]),
//!
//! and then a download only when its SHA-256 and size are the manifest's (the caller checks that;
//! [`Artifact`] carries both).
//!
//! The signature covers [`DOMAIN`] followed by the manifest bytes, so a signature made for this
//! purpose is good for nothing else. The bytes are verified before they are parsed, and nothing
//! is canonicalised: what was signed is exactly what is read.

pub mod version;

use std::collections::BTreeMap;

use ed25519_dalek::{Signature, Signer as _, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub use version::{Version, is_newer};

/// The manifest format this code reads and writes.
pub const SCHEMA: u32 = 1;
/// The platform key of the portable Windows exe.
pub const WINDOWS_X64: &str = "windows-x86_64";
/// The platform key of the Linux AppImage. A manifest from before it existed has no such entry,
/// which means "nothing for Linux", not an error.
pub const LINUX_X64: &str = "linux-x86_64";
/// Prefixed to the manifest bytes before signing.
pub const DOMAIN: &[u8] = b"anyplay-launcher-update-manifest-v1\n";
/// A manifest is a few hundred bytes; the client reads at most this much.
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
/// A signature file is 128 hex digits and a line break.
pub const MAX_SIGNATURE_BYTES: u64 = 1024;
/// The longest a manifest may stay valid (`expires` minus `published`). A longer one only widens
/// the window in which a stale answer is believed. [`Manifest::validate`] enforces it, so the
/// signing tool and the client hold the same line.
pub const MAX_VALIDITY_DAYS: i64 = 90;
/// How far ahead of the clock `published` may be (skew between the signer and the player).
pub const MAX_FUTURE_SKEW_DAYS: i64 = 1;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("the list of trusted update keys is unreadable: {0}")]
    KeyList(String),
    #[error("the signature is not valid for any trusted key")]
    Signature,
    #[error("the signature file is not an Ed25519 signature in hex")]
    SignatureFormat,
    #[error("the manifest is not usable: {0}")]
    Malformed(String),
    #[error("the manifest expired on {0}")]
    Expired(String),
    #[error("the manifest offers {offered}, which is not newer than {current}")]
    NotNewer { offered: String, current: String },
    #[error("this manifest updates only from {minimum} or newer, this is {current}")]
    RunningTooOld { minimum: String, current: String },
}

/// One downloadable file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// The file name; the launcher stages the download under it.
    pub name: String,
    pub url: String,
    /// Lower-case hex.
    pub sha256: String,
    pub size: u64,
}

/// The signed statement about one release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    /// The version this manifest offers, without a leading `v`.
    pub version: String,
    /// When it was made (RFC 3339, UTC).
    pub published: String,
    /// After this instant the launcher ignores the manifest (RFC 3339, UTC).
    pub expires: String,
    /// The oldest running version that may update from this manifest.
    pub minimum_version: String,
    /// The oldest running version that may keep running: a launcher below it must update before
    /// it goes online (docs/UPDATES.md, "Retiring a version"). Absent means every version may.
    /// It is a different thing from `minimum_version` above (which versions can *take* this
    /// update), and it has its own name because launchers up to v0.2.x read `minimum_version`
    /// as "may not update from this manifest", so reusing it would have locked them out.
    /// Launchers that do not know the field ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_supported_version: Option<String>,
    /// Per platform ([`WINDOWS_X64`], [`LINUX_X64`]).
    pub platforms: BTreeMap<String, Artifact>,
}

impl Manifest {
    /// The file for `platform`, if the manifest has one.
    pub fn artifact(&self, platform: &str) -> Option<&Artifact> {
        self.platforms.get(platform)
    }

    /// Whether a launcher running `current` is below [`Manifest::min_supported_version`], so the
    /// update this manifest offers is required and not optional. `false` without the field and
    /// for a running version that is not a version at all (that one is never updated anyway).
    pub fn requires_update_from(&self, current: &str) -> bool {
        let Some(supported) = self
            .min_supported_version
            .as_deref()
            .and_then(Version::parse)
        else {
            return false;
        };
        Version::parse(current).is_some_and(|running| running < supported)
    }

    /// The instant after which the manifest is void.
    pub fn expires_at(&self) -> Result<OffsetDateTime, ManifestError> {
        parse_time("expires", &self.expires)
    }

    /// Whether a launcher running `current` may take the update this manifest offers: the offered
    /// version must be newer, and `current` must not be older than `minimum_version`. A running
    /// version that is not a version at all is never updated.
    pub fn allows_update_from(&self, current: &str) -> Result<(), ManifestError> {
        if !is_newer(&self.version, current) {
            return Err(ManifestError::NotNewer {
                offered: self.version.clone(),
                current: current.to_owned(),
            });
        }
        let minimum = Version::parse(&self.minimum_version)
            .ok_or_else(|| ManifestError::Malformed("minimum_version".into()))?;
        match Version::parse(current) {
            Some(running) if running >= minimum => Ok(()),
            _ => Err(ManifestError::RunningTooOld {
                minimum: self.minimum_version.clone(),
                current: current.to_owned(),
            }),
        }
    }

    /// The checks against the clock: `published` may lie at most [`MAX_FUTURE_SKEW_DAYS`] ahead
    /// of `now`, and the manifest must not have expired. Call it on a validated manifest.
    pub fn check_time(&self, now: OffsetDateTime) -> Result<(), ManifestError> {
        let published = parse_time("published", &self.published)?;
        if published > now + time::Duration::days(MAX_FUTURE_SKEW_DAYS) {
            return Err(ManifestError::Malformed(
                "published is in the future".to_owned(),
            ));
        }
        if self.expires_at()? <= now {
            return Err(ManifestError::Expired(self.expires.clone()));
        }
        Ok(())
    }

    /// The rules that hold for any manifest, signed or about to be (no clock involved).
    pub fn validate(&self) -> Result<(), ManifestError> {
        let bad = |what: &str| Err(ManifestError::Malformed(what.to_owned()));
        if self.schema != SCHEMA {
            return bad("unknown schema");
        }
        let (Some(version), Some(minimum)) = (
            Version::parse(&self.version),
            Version::parse(&self.minimum_version),
        ) else {
            return bad("version or minimum_version is not a version");
        };
        if self.version.starts_with('v') || self.version.contains('+') {
            return bad("version must be plain semver");
        }
        if minimum > version {
            return bad("minimum_version is above version");
        }
        if let Some(supported) = &self.min_supported_version {
            if supported.starts_with('v') || supported.contains('+') {
                return bad("min_supported_version must be plain semver");
            }
            match Version::parse(supported) {
                None => return bad("min_supported_version is not a version"),
                // A release cannot demand a version above itself: nobody could ever satisfy it.
                Some(supported) if supported > version => {
                    return bad("min_supported_version is above version");
                }
                Some(_) => {}
            }
        }
        let published = parse_time("published", &self.published)?;
        let expires = self.expires_at()?;
        if expires <= published {
            return bad("expires is not after published");
        }
        if expires - published > time::Duration::days(MAX_VALIDITY_DAYS) {
            return bad("valid for longer than 90 days");
        }
        for (platform, artifact) in &self.platforms {
            if platform.is_empty() || artifact.name.is_empty() || artifact.url.is_empty() {
                return bad("an empty platform, name or url");
            }
            let hash = &artifact.sha256;
            let lower_hex = hash.len() == 64
                && hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !lower_hex {
                return bad("sha256 must be 64 lower-case hex digits");
            }
            if artifact.size == 0 {
                return bad("size is zero");
            }
        }
        Ok(())
    }
}

/// Reads the `MIN_SUPPORTED_VERSION` file of the repository: the lowest launcher version that may keep
/// running, as plain semver on one line. Blank lines and lines starting with `#` are comments;
/// a file with nothing else (or no file) switches the mechanism off. **Anything else is an error,
/// never "off"**: a typo must not silently let a retired version keep going.
pub fn parse_supported_file(text: &str) -> Result<Option<String>, ManifestError> {
    let text = text.strip_prefix('﻿').unwrap_or(text);
    let mut lines = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let Some(line) = lines.next() else {
        return Ok(None);
    };
    let bad = |why: &str| ManifestError::Malformed(format!("MIN_SUPPORTED_VERSION: {why}"));
    if lines.next().is_some() {
        return Err(bad("more than one version"));
    }
    if line.starts_with('v') || line.contains('+') || Version::parse(line).is_none() {
        return Err(bad("expected plain semver such as 0.3.0"));
    }
    Ok(Some(line.to_owned()))
}

fn parse_time(field: &str, text: &str) -> Result<OffsetDateTime, ManifestError> {
    OffsetDateTime::parse(text, &Rfc3339)
        .map_err(|_| ManifestError::Malformed(format!("{field} is not an RFC 3339 time")))
}

/// A time as the manifest spells it: RFC 3339, UTC, whole seconds.
pub fn format_time(time: OffsetDateTime) -> String {
    let time = time.to_offset(time::UtcOffset::UTC);
    time.replace_nanosecond(0)
        .unwrap_or(time)
        .format(&Rfc3339)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Keys

/// The public keys an update may be signed by.
#[derive(Debug, Clone, Default)]
pub struct TrustedKeys {
    keys: Vec<VerifyingKey>,
}

impl TrustedKeys {
    /// Reads a key list: one key per line as `ed25519 <64 hex digits> [label]`; blank lines and
    /// lines starting with `#` are comments. **Anything else is an error, never skipped**: a key
    /// that silently went missing would switch signing off and the launcher back to the unsigned
    /// path.
    pub fn parse(text: &str) -> Result<TrustedKeys, ManifestError> {
        // A byte order mark from an editor must not turn the first comment into an error.
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut keys = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fail = |why: &str| ManifestError::KeyList(format!("line {}: {why}", number + 1));
            let mut words = line.split_whitespace();
            if words.next() != Some("ed25519") {
                return Err(fail("expected `ed25519 <public key in hex> [label]`"));
            }
            let bytes: [u8; 32] = words
                .next()
                .and_then(from_hex)
                .and_then(|b| b.try_into().ok())
                .ok_or_else(|| fail("the key is not 64 hex digits"))?;
            let key =
                VerifyingKey::from_bytes(&bytes).map_err(|_| fail("not a valid public key"))?;
            keys.push(key);
        }
        Ok(TrustedKeys { keys })
    }

    /// No key: signing is not switched on yet and the launcher keeps its unsigned update path.
    /// With one key or more, a signed manifest is required and the unsigned path is gone.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// The list for a single key (tests, the tool).
    pub fn of(key: VerifyingKey) -> TrustedKeys {
        TrustedKeys { keys: vec![key] }
    }

    /// The list for several keys (tests, the tool).
    pub fn of_many(keys: impl IntoIterator<Item = VerifyingKey>) -> TrustedKeys {
        TrustedKeys {
            keys: keys.into_iter().collect(),
        }
    }
}

/// The `ed25519 <hex>` line that goes into the key list.
pub fn key_line(key: &VerifyingKey, label: &str) -> String {
    let line = format!("ed25519 {}", to_hex(key.as_bytes()));
    if label.is_empty() {
        line
    } else {
        format!("{line} {label}")
    }
}

/// A signing key from its 32-byte seed in hex.
pub fn signing_key_from_hex(text: &str) -> Option<SigningKey> {
    // The decoded bytes are wiped when they go out of scope.
    let bytes = zeroize::Zeroizing::new(from_hex(text.trim())?);
    let seed = zeroize::Zeroizing::new(<[u8; 32]>::try_from(bytes.as_slice()).ok()?);
    Some(SigningKey::from_bytes(&seed))
}

/// A fresh random signing key.
pub fn generate_signing_key() -> SigningKey {
    SigningKey::from_bytes(&rand::random::<[u8; 32]>())
}

/// The seed of a signing key in hex: the form of the secret. Never log it.
pub fn seed_hex(key: &SigningKey) -> zeroize::Zeroizing<String> {
    zeroize::Zeroizing::new(to_hex(&zeroize::Zeroizing::new(key.to_bytes())[..]))
}

// ---------------------------------------------------------------------------------------------
// Signing and verifying

fn signed_message(manifest: &[u8]) -> Vec<u8> {
    let mut message = DOMAIN.to_vec();
    message.extend_from_slice(manifest);
    message
}

/// The signature file for `manifest`: 128 hex digits and a line break.
pub fn sign(manifest: &[u8], key: &SigningKey) -> String {
    let signature = key.sign(&signed_message(manifest));
    format!("{}\n", to_hex(&signature.to_bytes()))
}

/// Checks `signature` over `manifest` against every trusted key, then reads and validates the
/// manifest (the rules that need no clock). The bytes are parsed only after the signature holds.
/// Whether the manifest is current is [`Manifest::check_time`] and whether its version is newer
/// is [`Manifest::allows_update_from`]; a client asks the second first, so one that is already
/// up to date does not report an expired manifest as an error.
pub fn verify_signed(
    manifest: &[u8],
    signature: &str,
    keys: &TrustedKeys,
) -> Result<Manifest, ManifestError> {
    let bytes: [u8; 64] = from_hex(signature.trim())
        .and_then(|b| b.try_into().ok())
        .ok_or(ManifestError::SignatureFormat)?;
    let signature = Signature::from_bytes(&bytes);
    let message = signed_message(manifest);
    // Strict verification also refuses the malleable encodings that plain verification accepts.
    if !keys
        .keys
        .iter()
        .any(|key| key.verify_strict(&message, &signature).is_ok())
    {
        return Err(ManifestError::Signature);
    }
    let manifest: Manifest =
        serde_json::from_slice(manifest).map_err(|e| ManifestError::Malformed(e.to_string()))?;
    manifest.validate()?;
    Ok(manifest)
}

/// [`verify_signed`], then [`Manifest::check_time`] at `now`.
pub fn verify(
    manifest: &[u8],
    signature: &str,
    keys: &TrustedKeys,
    now: OffsetDateTime,
) -> Result<Manifest, ManifestError> {
    let manifest = verify_signed(manifest, signature, keys)?;
    manifest.check_time(now)?;
    Ok(manifest)
}

// ---------------------------------------------------------------------------------------------
// Hex

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads hex of either case; `None` for odd length or any other character (no sign, no space).
pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests;
