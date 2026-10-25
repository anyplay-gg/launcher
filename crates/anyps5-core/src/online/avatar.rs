//! The pictures of other players: fetched once from the cloud, kept on disk under their hash.
//!
//! An avatar is public by its address: `/v1/avatars/{hash}`, where the hash is the SHA-256 of the
//! picture and only given out to those who may see it (`spec/cloud-api.md`). So the picture needs
//! no sign-in and the launcher can check it against the name it asked for. Blocking: call from a
//! worker thread.

use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::settings::write_atomic;
use crate::toolkit::{agent, hex, user_agent};

/// The largest picture the launcher takes (the cloud makes them 256 by 256 pixels, a few KB).
pub const MAX_BYTES: u64 = 1024 * 1024;

const TIMEOUT: Duration = Duration::from_secs(20);

/// True for what the cloud calls an avatar: 64 lowercase hex digits.
pub fn is_hash(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Where the picture with `hash` lives in `dir`.
pub fn file(dir: &Path, hash: &str) -> PathBuf {
    dir.join(format!("{hash}.png"))
}

/// The picture with `hash` from the cloud at `cloud`, in `dir`. A picture already there is not
/// fetched again; one that does not hash to its name is thrown away.
pub fn fetch(cloud: &str, hash: &str, dir: &Path) -> Result<PathBuf, String> {
    if !is_hash(hash) {
        return Err("not an avatar".into());
    }
    let path = file(dir, hash);
    if path.is_file() {
        return Ok(path);
    }
    let url = format!("{}/v1/avatars/{hash}", cloud.trim_end_matches('/'));
    let mut response = agent(Some(TIMEOUT))
        .get(&url)
        .header("User-Agent", user_agent())
        .call()
        .map_err(|e| e.to_string())?;
    if response.status().as_u16() != 200 {
        return Err(format!("the cloud answered {}", response.status()));
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    if hex(&Sha256::digest(&bytes)) != hash {
        return Err("the picture is not the one asked for".into());
    }
    write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn serve(body: Vec<u8>, status: u16) -> String {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", server.server_addr());
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let _ = request
                    .respond(tiny_http::Response::from_data(body.clone()).with_status_code(status));
            }
        });
        base
    }

    #[test]
    fn only_a_hash_is_an_avatar() {
        assert!(is_hash(&"a1".repeat(32)));
        assert!(!is_hash(&"A1".repeat(32)), "lower case only");
        assert!(!is_hash(&"a1".repeat(31)));
        assert!(!is_hash("../../etc/passwd"));
        assert!(!is_hash(&format!("{}/x", "a".repeat(62))));
    }

    #[test]
    fn a_picture_is_fetched_checked_and_kept() {
        let picture = b"not really a png".to_vec();
        let hash = hex(&Sha256::digest(&picture));
        let base = serve(picture.clone(), 200);
        let dir = tempfile::tempdir().unwrap();
        let path = fetch(&base, &hash, dir.path()).unwrap();
        assert_eq!(fs::read(&path).unwrap(), picture);
        // The second look needs no cloud.
        assert_eq!(
            fetch("http://127.0.0.1:1", &hash, dir.path()).unwrap(),
            path
        );
    }

    #[test]
    fn a_picture_that_is_not_the_one_asked_for_is_refused() {
        let base = serve(b"something else".to_vec(), 200);
        let dir = tempfile::tempdir().unwrap();
        let hash = "b".repeat(64);
        assert!(fetch(&base, &hash, dir.path()).is_err());
        assert!(!file(dir.path(), &hash).exists());
    }

    #[test]
    fn a_missing_picture_is_an_error() {
        let base = serve(Vec::new(), 404);
        let dir = tempfile::tempdir().unwrap();
        assert!(fetch(&base, &"c".repeat(64), dir.path()).is_err());
        assert!(fetch(&base, "nope", dir.path()).is_err());
    }
}
