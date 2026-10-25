//! The derivations of LudusCtrl's pairing and signatures: ECDH on P-256, HKDF-SHA256 for the link
//! key and the confirmation code, the nonce commitment, the key id and the truncated HMAC.

use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use p256::{
    FieldBytes, PublicKey, SecretKey, ecdh::diffie_hellman, elliptic_curve::sec1::ToSec1Point,
};
use sha2_v11::{Digest, Sha256};

use crate::remote::security::{constant_time_eq, random_bytes};

/// An uncompressed X9.62 point: `0x04 || X || Y`.
pub const POINT_SIZE: usize = 65;
pub const KEY_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 16;
pub const MAC_SIZE: usize = 8;
pub const KID_SIZE: usize = 4;

const LINK_INFO: &[u8] = b"LudusCtrl v2 link key";
const CONFIRM_INFO: &[u8] = b"LudusCtrl v2 confirm";
const COMMIT_LABEL: &[u8] = b"LudusCtrl v2 commit";

pub type Point = [u8; POINT_SIZE];
pub type LinkKey = [u8; KEY_SIZE];
pub type Kid = [u8; KID_SIZE];

/// One side's ephemeral key pair for one pairing attempt.
pub struct Ephemeral {
    secret: SecretKey,
    pub public: Point,
}

impl Ephemeral {
    pub fn generate() -> Ephemeral {
        loop {
            // A scalar of zero or past the group order is refused; try again.
            if let Ok(ephemeral) = Ephemeral::from_scalar(&random_bytes::<32>()) {
                return ephemeral;
            }
        }
    }

    /// A fixed scalar (big-endian), for the golden vectors.
    pub fn from_scalar(scalar: &[u8; 32]) -> Result<Ephemeral, ()> {
        let secret = SecretKey::from_bytes(&FieldBytes::from(*scalar)).map_err(|_| ())?;
        let public: Point = secret
            .public_key()
            .to_uncompressed_point()
            .as_slice()
            .try_into()
            .map_err(|_| ())?;
        Ok(Ephemeral { secret, public })
    }

    /// Z, the x-coordinate of the shared point.
    pub fn shared(&self, peer: &PublicKey) -> [u8; 32] {
        let shared = diffie_hellman(self.secret.to_nonzero_scalar(), peer.as_affine());
        let mut z = [0u8; 32];
        z.copy_from_slice(shared.raw_secret_bytes());
        z
    }
}

/// The peer's key if it is a 65-byte uncompressed point on the curve; anything else (compressed,
/// the identity, off the curve) is `None`.
pub fn parse_point(bytes: &[u8]) -> Option<PublicKey> {
    if bytes.len() != POINT_SIZE || bytes[0] != 0x04 {
        return None;
    }
    PublicKey::from_sec1_bytes(bytes).ok()
}

fn salt(phone_pub: &Point, pc_pub: &Point) -> Vec<u8> {
    [phone_pub.as_slice(), pc_pub.as_slice()].concat()
}

fn hkdf<const N: usize>(z: &[u8; 32], salt: &[u8], info: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    Hkdf::<Sha256>::new(Some(salt), z)
        .expand(info, &mut out)
        .expect("a short output");
    out
}

/// K, the per-phone key that signs every input packet and ack.
pub fn link_key(z: &[u8; 32], phone_pub: &Point, pc_pub: &Point) -> LinkKey {
    hkdf(z, &salt(phone_pub, pc_pub), LINK_INFO)
}

/// The six digits both screens show, 0 to 999 999.
pub fn confirmation_code(
    z: &[u8; 32],
    phone_pub: &Point,
    pc_pub: &Point,
    nonce: &[u8; NONCE_SIZE],
) -> u32 {
    let mut salt = salt(phone_pub, pc_pub);
    salt.extend_from_slice(nonce);
    u32::from_be_bytes(hkdf(z, &salt, CONFIRM_INFO)) % 1_000_000
}

/// "260 329": two groups of three digits.
pub fn format_code(code: u32) -> String {
    let code = code % 1_000_000;
    format!("{:03} {:03}", code / 1000, code % 1000)
}

pub fn commitment(nonce: &[u8; NONCE_SIZE]) -> [u8; 16] {
    let digest = Sha256::new()
        .chain_update(COMMIT_LABEL)
        .chain_update(nonce)
        .finalize();
    digest[..16].try_into().expect("16 bytes")
}

/// The key id: a lookup hint, not a secret.
pub fn kid(key: &LinkKey) -> Kid {
    Sha256::digest(key)[..KID_SIZE].try_into().expect("4 bytes")
}

pub fn mac(key: &LinkKey, data: &[u8]) -> [u8; MAC_SIZE] {
    let mut hmac = Hmac::<Sha256>::new_from_slice(key).expect("any key length");
    hmac.update(data);
    hmac.finalize().into_bytes()[..MAC_SIZE]
        .try_into()
        .expect("8 bytes")
}

pub fn mac_ok(key: &LinkKey, data: &[u8], tag: &[u8]) -> bool {
    constant_time_eq(&mac(key, data), tag)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn unhex(text: &str) -> Vec<u8> {
        let text: String = text.split_whitespace().collect();
        (0..text.len())
            .step_by(2)
            .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
            .collect()
    }

    pub fn phone() -> Ephemeral {
        Ephemeral::from_scalar(&[0x11; 32]).unwrap()
    }

    pub fn pc() -> Ephemeral {
        Ephemeral::from_scalar(&[0x22; 32]).unwrap()
    }

    pub const NONCE: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];

    pub fn golden_key() -> LinkKey {
        unhex("78fb82829c51a3cc9762af1ca02bd7749f797d182d18971b695bb78c28ab7a24")
            .try_into()
            .unwrap()
    }

    #[test]
    fn the_golden_keys_and_derivations() {
        let (phone, pc) = (phone(), pc());
        assert_eq!(
            hex(&phone.public),
            "040217e617f0b6443928278f96999e69a23a4f2c152bdf6d6cdf66e5b80282d4ed194a7debcb97712d2dda3ca85aa8765a56f45fc758599652f2897c65306e5794"
        );
        assert_eq!(
            hex(&pc.public),
            "04d65a93977caa3d1b081852ff57a79e465f1660577304baead505dd3a48589cf350185e895372df6221ea3a137557e473fddb6755f05bd507c3c533fce9c91285"
        );
        let z = pc.shared(&parse_point(&phone.public).unwrap());
        assert_eq!(
            hex(&z),
            "ccfc261f58193c98ca4ad4a53bbac6f0ee29bc4d48438090446908622ca79af6"
        );
        assert_eq!(z, phone.shared(&parse_point(&pc.public).unwrap()));
        let key = link_key(&z, &phone.public, &pc.public);
        assert_eq!(key, golden_key());
        assert_eq!(hex(&kid(&key)), "10253095");
        let code = confirmation_code(&z, &phone.public, &pc.public, &NONCE);
        assert_eq!(code, 260_329);
        assert_eq!(format_code(code), "260 329");
        assert_eq!(hex(&commitment(&NONCE)), "d06ad9ddcb46f53c44d669f7d08fd932");
    }

    #[test]
    fn codes_keep_their_leading_zeros() {
        assert_eq!(format_code(42), "000 042");
        assert_eq!(format_code(0), "000 000");
        assert_eq!(format_code(999_999), "999 999");
    }

    #[test]
    fn hkdf_is_rfc_5869_case_one() {
        let ikm = [0x0b; 22];
        let salt = unhex("000102030405060708090a0b0c");
        let info = unhex("f0f1f2f3f4f5f6f7f8f9");
        let mut okm = [0u8; 42];
        Hkdf::<Sha256>::new(Some(&salt), &ikm)
            .expand(&info, &mut okm)
            .unwrap();
        assert_eq!(
            hex(&okm),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );
    }

    #[test]
    fn only_uncompressed_points_on_the_curve_pass() {
        let good = pc().public;
        assert!(parse_point(&good).is_some());
        assert!(parse_point(&[0u8; 65]).is_none(), "all zero");
        assert!(parse_point(&good[..64]).is_none(), "short");
        let mut compressed = [0u8; 33];
        compressed[0] = 0x02 | (good[64] & 1);
        compressed[1..].copy_from_slice(&good[1..33]);
        assert!(parse_point(&compressed).is_none(), "compressed");
        let mut off = good;
        off[64] ^= 1;
        assert!(parse_point(&off).is_none(), "off the curve");
        let mut wrong_tag = good;
        wrong_tag[0] = 0x06;
        assert!(parse_point(&wrong_tag).is_none(), "hybrid form");
    }

    #[test]
    fn the_mac_is_eight_bytes_and_checks_the_key() {
        let key = golden_key();
        let tag = mac(&key, b"data");
        assert!(mac_ok(&key, b"data", &tag));
        assert!(!mac_ok(&key, b"datb", &tag));
        assert!(!mac_ok(&[0u8; 32], b"data", &tag));
        assert!(!mac_ok(&key, b"data", &tag[..7]));
    }

    #[test]
    fn fresh_keys_differ() {
        assert_ne!(Ephemeral::generate().public, Ephemeral::generate().public);
    }
}
