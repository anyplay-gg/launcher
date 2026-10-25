//! The phones the player allowed: `RemoteDevices.json` under the config folder.
//!
//! Same shape as the macOS launcher's file (array of `added`, `id`, `key`, `name`) so it can be copied
//! between machines. The key is 32 random bytes in base64 and is compared in constant time.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::security::{constant_time_eq, random_bytes};
use crate::paths::AppPaths;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedDevice {
    pub added: String,
    pub id: String,
    pub key: String,
    pub name: String,
}

#[derive(Debug)]
pub struct PairedDevices {
    file: PathBuf,
    devices: Vec<PairedDevice>,
}

impl PairedDevices {
    pub fn file_in(paths: &AppPaths) -> PathBuf {
        paths.config.join("RemoteDevices.json")
    }

    /// Loads the file; a missing or unreadable file is an empty list.
    pub fn load(file: &Path) -> PairedDevices {
        let devices = fs::read(file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        PairedDevices {
            file: file.to_path_buf(),
            devices,
        }
    }

    pub fn list(&self) -> &[PairedDevice] {
        &self.devices
    }

    /// Whether `key` is the stored key of device `id`.
    pub fn is_known(&self, id: &str, key: &str) -> bool {
        // Look at every entry and compare every key so timing says nothing about which matched.
        let mut found = false;
        for device in &self.devices {
            let same_key = constant_time_eq(device.key.as_bytes(), key.as_bytes());
            found |= same_key && device.id == id;
        }
        found
    }

    pub fn has_id(&self, id: &str) -> bool {
        self.devices.iter().any(|d| d.id == id)
    }

    /// Stores a phone under a fresh key (replacing an older entry of the same id) and returns it.
    pub fn add(&mut self, id: &str, name: &str) -> io::Result<PairedDevice> {
        let device = PairedDevice {
            added: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .unwrap_or_default(),
            id: id.to_string(),
            key: STANDARD.encode(random_bytes::<32>()),
            name: name.to_string(),
        };
        self.devices.retain(|d| d.id != id);
        self.devices.push(device.clone());
        self.save()?;
        Ok(device)
    }

    pub fn remove(&mut self, id: &str) -> io::Result<()> {
        self.devices.retain(|d| d.id != id);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.devices).map_err(io::Error::other)?;
        let temp = self.file.with_extension("json.tmp");
        fs::write(&temp, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))?;
        }
        fs::rename(&temp, &self.file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_find_remove_and_persist() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub").join("RemoteDevices.json");
        let mut devices = PairedDevices::load(&file);
        assert!(devices.list().is_empty());
        let first = devices.add("aa", "Pixel").unwrap();
        assert_eq!(STANDARD.decode(&first.key).unwrap().len(), 32);
        assert!(devices.is_known("aa", &first.key));
        assert!(!devices.is_known("bb", &first.key));
        assert!(!devices.is_known("aa", "nope"));

        let again = devices.add("aa", "Pixel 8").unwrap();
        assert_ne!(again.key, first.key);
        assert_eq!(devices.list().len(), 1);
        assert!(!devices.is_known("aa", &first.key));

        let reloaded = PairedDevices::load(&file);
        assert!(reloaded.is_known("aa", &again.key));
        assert_eq!(reloaded.list()[0].name, "Pixel 8");

        let mut reloaded = reloaded;
        reloaded.remove("aa").unwrap();
        assert!(PairedDevices::load(&file).list().is_empty());
    }

    #[test]
    fn reads_the_macos_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("RemoteDevices.json");
        fs::write(
            &file,
            "[\n  {\n    \"added\" : \"2026-10-22T18:14:03Z\",\n    \"id\" : \"9f3a\",\n    \"key\" : \"q3J=\",\n    \"name\" : \"Pixel 8\"\n  }\n]",
        )
        .unwrap();
        assert!(PairedDevices::load(&file).is_known("9f3a", "q3J="));
    }
}
