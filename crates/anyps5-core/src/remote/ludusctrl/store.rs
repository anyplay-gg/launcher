//! The phones paired through LudusCtrl and this PC's id: `LudusCtrl.json` under the config folder.
//!
//! The launcher's own file, written like `RemoteDevices.json` (a temp file, mode 0600 on Linux,
//! then a rename). It also holds each phone's highest session, which is written before a new
//! session is accepted: after a restart a recording of an older one must not pass as new.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    crypto::{Kid, LinkKey, kid},
    wire::Id,
};
use crate::{paths::AppPaths, remote::security::random_bytes};

/// Names longer than this are cut when the file is read.
const NAME_CHARS: usize = 64;

/// A paired phone as the settings list it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LudusPhone {
    /// The 8-byte phone id in lowercase hex.
    pub id: String,
    pub name: String,
    /// When it was paired, RFC 3339 in UTC.
    pub added: String,
}

#[derive(Clone, Debug)]
pub(super) struct Phone {
    pub id: Id,
    pub name: String,
    pub key: LinkKey,
    pub kid: Kid,
    pub added: String,
    pub highest_session: u32,
}

#[derive(Serialize, Deserialize)]
struct FilePhone {
    added: String,
    id: String,
    key: String,
    name: String,
    highest_session: u32,
}

#[derive(Serialize, Deserialize)]
struct FileBody {
    pc_id: String,
    phones: Vec<FilePhone>,
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn id_from_hex(text: &str) -> Option<Id> {
    if text.len() != 16 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut id = [0u8; 8];
    for (at, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * at..2 * at + 2], 16).ok()?;
    }
    Some(id)
}

pub struct LudusStore {
    file: PathBuf,
    pc_id: Id,
    phones: Vec<Phone>,
    /// False when the file exists but could not be read: nothing is written over it.
    writable: bool,
}

impl LudusStore {
    pub fn file_in(paths: &AppPaths) -> PathBuf {
        paths.config.join("LudusCtrl.json")
    }

    /// Reads the file. A missing file starts a new one with a fresh PC id (the beacon advertises
    /// it, so it is saved at once). A file that does not parse is moved aside and the PC starts
    /// clean: every phone pairs again. A file that cannot be read is left alone and this run
    /// saves nothing.
    pub fn load(file: &Path) -> LudusStore {
        let fresh = |writable| LudusStore {
            file: file.to_path_buf(),
            pc_id: random_bytes::<8>(),
            phones: Vec::new(),
            writable,
        };
        let bytes = match fs::read(file) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let store = fresh(true);
                if let Err(error) = store.save() {
                    tracing::warn!("ludusctrl: could not save the pairing file: {error}");
                }
                return store;
            }
            Err(error) => {
                tracing::warn!(
                    "ludusctrl: the pairing file cannot be read, nothing is saved: {error}"
                );
                return fresh(false);
            }
        };
        match parse(&bytes) {
            Some((pc_id, phones)) => LudusStore {
                file: file.to_path_buf(),
                pc_id,
                phones,
                writable: true,
            },
            None => {
                let seconds = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                let aside = file.with_file_name(format!("LudusCtrl.corrupt-{seconds}.json"));
                tracing::warn!(
                    "ludusctrl: the pairing file is damaged; moved to {}, every phone pairs again",
                    aside.display()
                );
                let _ = fs::rename(file, &aside);
                let store = fresh(true);
                if let Err(error) = store.save() {
                    tracing::warn!("ludusctrl: could not save the pairing file: {error}");
                }
                store
            }
        }
    }

    pub fn pc_id(&self) -> Id {
        self.pc_id
    }

    pub fn list(&self) -> Vec<LudusPhone> {
        self.phones
            .iter()
            .map(|p| LudusPhone {
                id: to_hex(&p.id),
                name: p.name.clone(),
                added: p.added.clone(),
            })
            .collect()
    }

    pub(super) fn phones(&self) -> &[Phone] {
        &self.phones
    }

    pub fn len(&self) -> usize {
        self.phones.len()
    }

    pub fn is_empty(&self) -> bool {
        self.phones.is_empty()
    }

    pub fn has(&self, id: &Id) -> bool {
        self.phones.iter().any(|p| p.id == *id)
    }

    /// Stores a phone with its new key, replacing an older pairing of the same phone (whose
    /// session count starts again at 0). Written before it takes effect.
    pub fn add(&mut self, id: Id, name: &str, key: LinkKey) -> io::Result<()> {
        let phone = Phone {
            id,
            name: name.to_string(),
            key,
            kid: kid(&key),
            added: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_default(),
            highest_session: 0,
        };
        let before = self.phones.clone();
        self.phones.retain(|p| p.id != id);
        self.phones.push(phone);
        self.save().inspect_err(|_| self.phones = before)
    }

    /// Forgets a phone by its hex id. True if it was paired.
    pub fn remove(&mut self, id: &str) -> io::Result<bool> {
        let before = self.phones.len();
        self.phones.retain(|p| to_hex(&p.id) != id);
        if self.phones.len() == before {
            return Ok(false);
        }
        self.save().map(|()| true)
    }

    /// Raises a phone's highest session (only ever upwards) and writes it.
    pub(super) fn raise_session(&mut self, id: &Id, session: u32) -> io::Result<()> {
        if let Some(phone) = self.phones.iter_mut().find(|p| p.id == *id)
            && session > phone.highest_session
        {
            phone.highest_session = session;
            return self.save();
        }
        Ok(())
    }

    /// Gives a phone another key id, to test a collision of the 32-bit ids.
    #[cfg(test)]
    pub(super) fn set_kid(&mut self, id: &Id, kid: Kid) {
        if let Some(phone) = self.phones.iter_mut().find(|p| p.id == *id) {
            phone.kid = kid;
        }
    }

    fn save(&self) -> io::Result<()> {
        if !self.writable {
            return Ok(());
        }
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = FileBody {
            pc_id: to_hex(&self.pc_id),
            phones: self
                .phones
                .iter()
                .map(|p| FilePhone {
                    added: p.added.clone(),
                    id: to_hex(&p.id),
                    key: STANDARD.encode(p.key),
                    name: p.name.clone(),
                    highest_session: p.highest_session,
                })
                .collect(),
        };
        let text = serde_json::to_string_pretty(&body).map_err(io::Error::other)?;
        let temp = self.file.with_extension("json.tmp");
        fs::write(&temp, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
        }
        // Something (an indexer, a virus scanner) may hold the old file for a moment.
        let mut last = Ok(());
        for _ in 0..5 {
            last = fs::rename(&temp, &self.file);
            if last.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        last
    }
}

fn parse(bytes: &[u8]) -> Option<(Id, Vec<Phone>)> {
    let body: FileBody = serde_json::from_slice(bytes).ok()?;
    let pc_id = id_from_hex(&body.pc_id)?;
    let phones = body
        .phones
        .into_iter()
        .map(|p| {
            let key: LinkKey = STANDARD.decode(&p.key).ok()?.try_into().ok()?;
            Some(Phone {
                id: id_from_hex(&p.id)?,
                name: p.name.chars().take(NAME_CHARS).collect(),
                kid: kid(&key),
                key,
                added: p.added,
                highest_session: p.highest_session,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some((pc_id, phones))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: Id = [1, 2, 3, 4, 5, 6, 7, 8];

    #[test]
    fn a_new_file_keeps_its_pc_id() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub").join("LudusCtrl.json");
        let first = LudusStore::load(&file);
        assert!(file.exists(), "the id is saved at once");
        assert_eq!(LudusStore::load(&file).pc_id(), first.pc_id());
        assert!(first.is_empty());
    }

    #[test]
    fn add_raise_remove_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("LudusCtrl.json");
        let mut store = LudusStore::load(&file);
        store.add(PHONE, "Pixel 8", [7; 32]).unwrap();
        store.raise_session(&PHONE, 5).unwrap();
        store.raise_session(&PHONE, 3).unwrap();
        let reloaded = LudusStore::load(&file);
        assert_eq!(reloaded.phones()[0].highest_session, 5, "only upwards");
        assert_eq!(reloaded.phones()[0].key, [7; 32]);
        assert_eq!(reloaded.phones()[0].kid, kid(&[7; 32]));
        let listed = reloaded.list();
        assert_eq!(listed[0].id, "0102030405060708");
        assert_eq!(listed[0].name, "Pixel 8");
        assert!(listed[0].added.ends_with('Z'));

        // Pairing again replaces the key and starts the sessions over.
        let mut store = reloaded;
        store.add(PHONE, "Pixel 8", [9; 32]).unwrap();
        assert_eq!(store.len(), 1);
        assert_eq!(LudusStore::load(&file).phones()[0].highest_session, 0);

        assert!(store.remove("0102030405060708").unwrap());
        assert!(!store.remove("0102030405060708").unwrap());
        assert!(LudusStore::load(&file).is_empty());
    }

    #[test]
    fn a_damaged_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("LudusCtrl.json");
        for damaged in [
            "not json",
            r#"{"pc_id":"00","phones":[]}"#,
            r#"{"pc_id":"0102030405060708","phones":[{"added":"","id":"0102030405060708","key":"AAAA","name":"x","highest_session":1}]}"#,
            r#"{"pc_id":"0102030405060708","phones":[{"added":"","id":"0102030405060708","key":"BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=","name":"x","highest_session":4294967296}]}"#,
        ] {
            fs::write(&file, damaged).unwrap();
            let store = LudusStore::load(&file);
            assert!(store.is_empty(), "{damaged}");
            let aside = fs::read_dir(dir.path())
                .unwrap()
                .filter_map(Result::ok)
                .any(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with("LudusCtrl.corrupt-")
                });
            assert!(aside, "{damaged}");
            assert!(LudusStore::load(&file).pc_id() == store.pc_id());
        }
    }

    #[test]
    fn long_names_are_cut_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("LudusCtrl.json");
        let mut store = LudusStore::load(&file);
        store.add(PHONE, &"x".repeat(100), [1; 32]).unwrap();
        assert_eq!(LudusStore::load(&file).phones()[0].name.len(), 64);
    }
}
