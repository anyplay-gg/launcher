//! The browser client's certificate and TLS setup. The Gamepad API needs a secure context, so the
//! page is served over HTTPS with a certificate this PC signs itself; a phone accepts it once.
//! Every new certificate is one more warning on every phone, so it is kept as long as it covers
//! this PC's addresses and is far from its end.

use std::{
    fs, io,
    net::{IpAddr, Ipv4Addr},
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use rcgen::{
    CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
    PKCS_ECDSA_P256_SHA256, SanType, SerialNumber,
};
use rustls::{
    ServerConfig,
    crypto::ring::{default_provider, sign::any_ecdsa_type},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    sign::CertifiedKey,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::remote::security::random_bytes;

/// Apple's ceiling for a TLS server certificate.
pub const VALID_DAYS: u64 = 825;
/// A certificate this close to its end is replaced.
pub const RENEW_BEFORE: Duration = Duration::from_secs(30 * 86_400);
/// The most addresses one certificate keeps.
pub const MAX_ADDRESSES: usize = 32;
const DAY: u64 = 86_400;

/// What happened to the certificate at a start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Certificate {
    /// The stored one covers every address: a phone that accepted it does not warn again.
    Kept,
    /// The first one of this PC (or the file was unreadable): every phone warns once.
    New,
    /// An address was not covered or it was near its end: phones warn once more.
    Reissued,
}

#[derive(Serialize, Deserialize)]
struct File {
    cert: String,
    key: String,
    addresses: Vec<String>,
    /// Unix seconds.
    expires: u64,
}

/// A certificate with its key and the addresses it covers.
#[derive(Clone)]
pub struct Identity {
    pub cert: CertificateDer<'static>,
    key: Vec<u8>,
    pub addresses: Vec<Ipv4Addr>,
    pub expires: u64,
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Identity {
    pub fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.clone()))
    }

    /// SHA-256 of the certificate as "AB:CD:...".
    pub fn fingerprint(&self) -> String {
        Sha256::digest(self.cert.as_ref())
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":")
    }

    fn certified(&self) -> Option<CertifiedKey> {
        let signer = any_ecdsa_type(&self.key()).ok()?;
        Some(CertifiedKey::new(vec![self.cert.clone()], signer))
    }

    pub(super) fn read(file: &Path) -> Option<Identity> {
        let file: File = serde_json::from_slice(&fs::read(file).ok()?).ok()?;
        let identity = Identity {
            cert: CertificateDer::from(STANDARD.decode(file.cert).ok()?),
            key: STANDARD.decode(file.key).ok()?,
            addresses: file
                .addresses
                .iter()
                .map(|a| a.parse().ok())
                .collect::<Option<_>>()?,
            expires: file.expires,
        };
        // The key must be the certificate's.
        identity.certified()?.keys_match().ok()?;
        Some(identity)
    }

    fn save(&self, file: &Path) -> io::Result<()> {
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = File {
            cert: STANDARD.encode(self.cert.as_ref()),
            key: STANDARD.encode(&self.key),
            addresses: self.addresses.iter().map(ToString::to_string).collect(),
            expires: self.expires,
        };
        let text = serde_json::to_string_pretty(&body).map_err(io::Error::other)?;
        let temp = file.with_extension("json.tmp");
        fs::write(&temp, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
        }
        // Something (an indexer, a virus scanner) may hold the old file for a moment.
        let mut last = Ok(());
        for _ in 0..5 {
            last = fs::rename(&temp, file);
            if last.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        last
    }

    /// Whether this certificate can stay: it covers `current` and is far from its end.
    pub(super) fn fits(&self, current: &[Ipv4Addr], now: u64) -> bool {
        self.expires > now + RENEW_BEFORE.as_secs()
            && current.iter().all(|ip| self.addresses.contains(ip))
    }
}

/// A new certificate for `addresses` (plus 127.0.0.1 and localhost), with `key` when given.
pub fn mint(
    addresses: &[Ipv4Addr],
    key: Option<&[u8]>,
    now: u64,
) -> Result<Identity, rcgen::Error> {
    let pair = match key {
        Some(der) => KeyPair::from_pkcs8_der_and_sign_algo(
            &PrivatePkcs8KeyDer::from(der),
            &PKCS_ECDSA_P256_SHA256,
        )?,
        None => KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?,
    };
    let mut params = CertificateParams::default();
    params.distinguished_name = rcgen::DistinguishedName::new();
    params
        .distinguished_name
        .push(DnType::CommonName, "AnyPlay Launcher");
    params.subject_alt_names = addresses
        .iter()
        .map(|ip| SanType::IpAddress(IpAddr::V4(*ip)))
        .chain([
            SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            SanType::DnsName("localhost".try_into()?),
        ])
        .collect();
    // CA:false: iOS takes a leaf that says CA:true for a root to install.
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let mut serial = random_bytes::<16>();
    serial[0] &= 0x7F; // a positive INTEGER
    params.serial_number = Some(SerialNumber::from_slice(&serial));
    let start = now.saturating_sub(DAY);
    let expires = now + VALID_DAYS * DAY;
    params.not_before = time::OffsetDateTime::from_unix_timestamp(start as i64)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    params.not_after = time::OffsetDateTime::from_unix_timestamp(expires as i64)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let cert = params.self_signed(&pair)?;
    Ok(Identity {
        cert: cert.der().clone(),
        key: pair.serialize_der(),
        addresses: addresses.to_vec(),
        expires,
    })
}

/// The stored certificate if it fits `current`, otherwise a new one (the old key kept, the
/// addresses the union of the current and the old ones, the current first). A new one is saved;
/// a folder that cannot be written only costs a warning on the phone at every start.
pub fn load_or_mint(
    file: &Path,
    current: &[Ipv4Addr],
    now: u64,
) -> Result<(Identity, Certificate), rcgen::Error> {
    let stored = Identity::read(file);
    if let Some(identity) = &stored
        && identity.fits(current, now)
    {
        return Ok((identity.clone(), Certificate::Kept));
    }
    let mut addresses: Vec<Ipv4Addr> = Vec::new();
    for ip in current
        .iter()
        .chain(stored.iter().flat_map(|s| s.addresses.iter()))
    {
        if !addresses.contains(ip) && addresses.len() < MAX_ADDRESSES {
            addresses.push(*ip);
        }
    }
    let identity = mint(&addresses, stored.as_ref().map(|s| s.key.as_slice()), now)?;
    if let Err(error) = identity.save(file) {
        tracing::warn!("ludusctrl: could not save the browser certificate: {error}");
    }
    let state = if stored.is_some() {
        Certificate::Reissued
    } else {
        Certificate::New
    };
    Ok((identity, state))
}

/// TLS 1.2 and 1.3 on ring, no client certificates, no ALPN (always HTTP/1.1).
pub fn server_config(identity: &Identity) -> Result<Arc<ServerConfig>, rustls::Error> {
    let config = ServerConfig::builder_with_provider(Arc::new(default_provider()))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?
        .with_no_client_auth()
        .with_single_cert(vec![identity.cert.clone()], identity.key())?;
    Ok(Arc::new(config))
}
