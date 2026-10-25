//! The pad layouts the phone saved per game: `RemoteLayouts.json` under the config folder, a map
//! from game id to the phone's own JSON text. The launcher keeps them so a new phone finds them.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use crate::paths::AppPaths;

/// A layout is a few hundred buttons at most; the phone caps its own message at half a megabyte.
const MAX_LAYOUT: usize = 256 * 1024;
const MAX_GAMES: usize = 500;
const MAX_ID: usize = 256;

#[derive(Debug)]
pub struct Layouts {
    file: PathBuf,
    map: BTreeMap<String, String>,
}

impl Layouts {
    pub fn file_in(paths: &AppPaths) -> PathBuf {
        paths.config.join("RemoteLayouts.json")
    }

    /// A missing or unreadable file is an empty map.
    pub fn load(file: &Path) -> Layouts {
        let map = fs::read(file)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Layouts {
            file: file.to_path_buf(),
            map,
        }
    }

    pub fn get(&self, game: &str) -> Option<&str> {
        self.map.get(game).map(String::as_str)
    }

    /// Stores (or with `None` forgets) a layout. Oversized or excessive input is refused with
    /// `InvalidInput`: the phone is paired, but it is not trusted with the disk.
    pub fn set(&mut self, game: &str, layout: Option<&str>) -> io::Result<()> {
        if game.is_empty() || game.len() > MAX_ID {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        match layout.filter(|text| !text.is_empty()) {
            Some(text) => {
                if text.len() > MAX_LAYOUT
                    || (!self.map.contains_key(game) && self.map.len() >= MAX_GAMES)
                {
                    return Err(io::ErrorKind::InvalidInput.into());
                }
                self.map.insert(game.to_string(), text.to_string());
            }
            None => {
                if self.map.remove(game).is_none() {
                    return Ok(());
                }
            }
        }
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string(&self.map).map_err(io::Error::other)?;
        let temp = self.file.with_extension("json.tmp");
        fs::write(&temp, text)?;
        fs::rename(&temp, &self.file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_persist_and_can_be_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config").join("RemoteLayouts.json");
        let mut layouts = Layouts::load(&file);
        assert_eq!(layouts.get("g"), None);
        layouts.set("g", Some(r#"{"buttons":[]}"#)).unwrap();
        assert_eq!(Layouts::load(&file).get("g"), Some(r#"{"buttons":[]}"#));
        layouts.set("g", None).unwrap();
        assert_eq!(Layouts::load(&file).get("g"), None);
        layouts.set("g", None).unwrap();
    }

    #[test]
    fn oversized_and_excessive_input_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut layouts = Layouts::load(&dir.path().join("l.json"));
        let big = "x".repeat(MAX_LAYOUT + 1);
        assert!(layouts.set("g", Some(&big)).is_err());
        assert!(layouts.set("", Some("{}")).is_err());
        assert!(layouts.set(&"i".repeat(MAX_ID + 1), Some("{}")).is_err());
        for n in 0..MAX_GAMES {
            layouts.set(&format!("g{n}"), Some("{}")).unwrap();
        }
        assert!(layouts.set("one-more", Some("{}")).is_err());
        layouts.set("g0", Some("{\"a\":1}")).unwrap();
    }
}
