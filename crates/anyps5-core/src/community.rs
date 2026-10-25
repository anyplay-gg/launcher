//! The community layouts of a title (docs/DESIGN.md 5.18): key layouts that players share in the
//! launcher's repository, `community/layouts/index.json` and the `.ini` files beside it, read
//! from GitHub raw when the Controls panel opens. Offline, or with nothing listed, there are none.
//!
//! Nothing is sent but the GET of the files, and nothing is run: a layout is a text file in the
//! form of upstream's `INPUT_MAPPING.md`, checked line by line by [`Controls::parse`].

use std::time::Duration;

use serde::Deserialize;

use crate::controls::{Changes, Controls, InputTuning};

/// Where the files live; `index.json` and the layouts' paths are relative to it.
pub const BASE_URL: &str =
    "https://raw.githubusercontent.com/anyplaynet/launcher/main/community/layouts";

/// The index understood by this launcher. A higher number is a format it does not know.
pub const INDEX_VERSION: u32 = 1;

/// Layouts shown per title.
pub const MAX_PER_TITLE: usize = 8;

/// Larger files are not layouts.
const MAX_FILE_BYTES: u64 = 16 * 1024;
const MAX_INDEX_BYTES: u64 = 256 * 1024;

/// A layout of the gallery, ready to apply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommunityLayout {
    pub name: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub changes: Changes,
    /// `Some` when the file carries tuning lines.
    pub tuning: Option<InputTuning>,
}

/// One entry of `index.json`.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    pub title_id: String,
    pub name: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Path below [`BASE_URL`], `<TITLEID>/<name>.ini`.
    pub file: String,
}

impl IndexEntry {
    /// Entries that would point anywhere but at a layout of the repository are refused.
    fn is_valid(&self) -> bool {
        let id_ok = self.title_id.len() == 9
            && self.title_id[..4].chars().all(|c| c.is_ascii_uppercase())
            && self.title_id[4..].chars().all(|c| c.is_ascii_digit());
        let name_ok = !self.name.trim().is_empty()
            && self.name.chars().count() <= 40
            && !self.name.chars().any(char::is_control);
        let file_ok = self.file.len() <= 80
            && self.file.ends_with(".ini")
            && self.file.split('/').count() == 2
            && self.file.split('/').all(|part| {
                !part.is_empty()
                    && !part.starts_with('.')
                    && part
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            });
        id_ok && name_ok && file_ok
    }
}

/// The valid entries of an `index.json`; none for text that is not one or a newer version.
pub fn parse_index(text: &str) -> Vec<IndexEntry> {
    #[derive(Deserialize)]
    struct Index {
        version: u32,
        layouts: Vec<serde_json::Value>,
    }
    let Ok(index) = serde_json::from_str::<Index>(text) else {
        return Vec::new();
    };
    if index.version != INDEX_VERSION {
        return Vec::new();
    }
    index
        .layouts
        .into_iter()
        .filter_map(|value| serde_json::from_value::<IndexEntry>(value).ok())
        .filter(IndexEntry::is_valid)
        .collect()
}

/// The layouts for `title_id` from the repository. Blocking: call from a worker thread.
pub fn fetch(title_id: &str) -> Vec<CommunityLayout> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .build()
        .into();
    load(title_id, &|path, limit| get(&agent, path, limit))
}

fn get(agent: &ureq::Agent, path: &str, limit: u64) -> Option<String> {
    let mut response = agent
        .get(format!("{BASE_URL}/{path}"))
        .header("User-Agent", crate::toolkit::user_agent())
        .call()
        .ok()?;
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_string()
        .ok()
}

/// [`fetch`] with the transport injected: `get(path, byte limit)` is the text of a file below
/// the base, `None` when it cannot be had.
pub fn load(title_id: &str, get: &dyn Fn(&str, u64) -> Option<String>) -> Vec<CommunityLayout> {
    let Some(index) = get("index.json", MAX_INDEX_BYTES) else {
        return Vec::new();
    };
    let mut layouts: Vec<CommunityLayout> = Vec::new();
    for entry in parse_index(&index)
        .into_iter()
        .filter(|e| e.title_id == title_id)
    {
        if layouts.len() == MAX_PER_TITLE {
            break;
        }
        if layouts.iter().any(|l| l.name == entry.name) {
            continue;
        }
        let Some(text) = get(&entry.file, MAX_FILE_BYTES) else {
            continue;
        };
        match Controls::parse(&text) {
            Ok(parsed) => layouts.push(CommunityLayout {
                name: entry.name.trim().to_string(),
                author: entry.author.filter(|a| !a.trim().is_empty()),
                description: entry.description.filter(|d| !d.trim().is_empty()),
                changes: parsed.changes,
                tuning: parsed.tuning,
            }),
            Err(error) => tracing::debug!("community layout {}: {error}", entry.file),
        }
    }
    layouts
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = r#"{
        "version": 1,
        "layouts": [
            {"titleId": "PPSA16064", "name": "From the game", "author": "AlxcBTW", "file": "PPSA16064/from-the-game.ini"},
            {"titleId": "PPSA16064", "name": "Broken", "file": "PPSA16064/broken.ini"},
            {"titleId": "PPSA16064", "name": "Missing", "file": "PPSA16064/missing.ini"},
            {"titleId": "PPSA16064", "name": "From the game", "file": "PPSA16064/again.ini"},
            {"titleId": "PPSA16064", "name": "Escapes", "file": "../secret.ini"},
            {"titleId": "PPSA16064", "name": "Elsewhere", "file": "https://example.com/x.ini"},
            {"titleId": "ppsa1", "name": "Bad id", "file": "PPSA1/a.ini"},
            {"titleId": "PPSA00001", "name": "Other title", "file": "PPSA00001/a.ini"},
            {"name": "No id", "file": "a/b.ini"},
            "not an object"
        ]
    }"#;

    fn files(path: &str, _limit: u64) -> Option<String> {
        match path {
            "index.json" => Some(INDEX.into()),
            "PPSA16064/from-the-game.ini" => {
                Some("Cross = KEY:Space\nLeftStickDeadzone = 10\n".into())
            }
            "PPSA16064/broken.ini" => Some("Cross = KEY:Banana".into()),
            "PPSA00001/a.ini" => Some("Circle = KEY:Z".into()),
            _ => None,
        }
    }

    #[test]
    fn only_clean_entries_survive_the_index() {
        let entries = parse_index(INDEX);
        let files: Vec<_> = entries.iter().map(|e| e.file.as_str()).collect();
        assert_eq!(
            files,
            [
                "PPSA16064/from-the-game.ini",
                "PPSA16064/broken.ini",
                "PPSA16064/missing.ini",
                "PPSA16064/again.ini",
                "PPSA00001/a.ini"
            ]
        );
    }

    #[test]
    fn other_versions_and_other_text_give_nothing() {
        assert!(parse_index(r#"{"version": 2, "layouts": []}"#).is_empty());
        assert!(parse_index("<html>404</html>").is_empty());
        assert!(parse_index("").is_empty());
        assert!(parse_index(r#"{"version": 1}"#).is_empty());
    }

    #[test]
    fn a_title_gets_its_readable_layouts() {
        let layouts = load("PPSA16064", &files);
        assert_eq!(
            layouts.len(),
            1,
            "broken, missing and repeated ones are left out"
        );
        assert_eq!(layouts[0].name, "From the game");
        assert_eq!(layouts[0].author.as_deref(), Some("AlxcBTW"));
        assert_eq!(layouts[0].changes["Cross"], ["KEY:Space"]);
        assert_eq!(layouts[0].tuning.unwrap().left_deadzone, 10);
        assert_eq!(load("PPSA00001", &files)[0].changes["Circle"], ["KEY:Z"]);
        assert!(load("PPSA99999", &files).is_empty());
    }

    #[test]
    fn offline_is_empty() {
        assert!(load("PPSA16064", &|_, _| None).is_empty());
    }

    #[test]
    fn at_most_eight_per_title() {
        let many: String = (0..12)
            .map(|i| {
                format!(r#"{{"titleId":"PPSA00001","name":"L{i}","file":"PPSA00001/l{i}.ini"}}"#)
            })
            .collect::<Vec<_>>()
            .join(",");
        let index = format!(r#"{{"version":1,"layouts":[{many}]}}"#);
        let get = move |path: &str, _| {
            Some(if path == "index.json" {
                index.clone()
            } else {
                "Circle = KEY:Z".to_string()
            })
        };
        assert_eq!(load("PPSA00001", &get).len(), MAX_PER_TITLE);
    }

    /// The index and the layouts of this repository are what the launcher will fetch.
    #[test]
    fn the_shipped_index_and_layouts_are_valid() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../community/layouts");
        let text = std::fs::read_to_string(format!("{root}/index.json")).unwrap();
        let entries = parse_index(&text);
        let listed = text.matches("titleId").count();
        assert_eq!(entries.len(), listed, "every entry of index.json is valid");
        assert!(!entries.is_empty());
        let read = |path: &str, _| std::fs::read_to_string(format!("{root}/{path}")).ok();
        for entry in &entries {
            let layouts = load(&entry.title_id, &read);
            assert!(
                layouts.iter().any(|l| l.name == entry.name),
                "{} does not load",
                entry.file
            );
        }
    }
}
