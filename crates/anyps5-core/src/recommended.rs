//! Recommended settings per game: a community-kept catalogue (`data/recommended-settings.v1.json`),
//! what the launcher accepts from it, and how an entry becomes a reviewable change.
//!
//! The data can only say things this module already allows. The allowlists below are the trust
//! boundary: a file that asks for anything else is refused as a whole. Entries come in three kinds:
//!
//! * `required`: the game does not start without it.
//! * `recommended`: a better experience. A value the player has changed themselves is kept.
//! * `avoid`: a known bad value. It only warns when the game would run with it.
//!
//! Nothing here applies itself. [`plan`] lists the changes, [`Plan::apply`] writes them.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde::Deserialize;

use crate::compat;
use crate::feed::{self, Request};
use crate::paths::AppPaths;
use crate::settings::{GameSettings, Overlay};
use crate::toolkit::ToolkitError;

/// The catalogue as shipped in the exe: the fallback when the network has nothing newer.
pub const EMBEDDED: &str = include_str!("../../../data/recommended-settings.v1.json");

/// Where the current catalogue lives (this repository, not GitHub Pages).
pub const URL: &str =
    "https://raw.githubusercontent.com/anyplay-gg/launcher/main/data/recommended-settings.v1.json";

const MAX_BYTES: u64 = 512 * 1024;

// ---------------------------------------------------------------------------------------------
// The data

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Required,
    Recommended,
    Avoid,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub schema: u32,
    pub updated: String,
    pub titles: BTreeMap<String, Title>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Title {
    pub name: String,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub kind: Kind,
    #[serde(default)]
    pub applies_to: AppliesTo,
    pub set: Set,
    pub why: String,
    pub evidence: Vec<String>,
    pub verified: Verified,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AppliesTo {
    /// Exact `contentVersion` strings, or `*`.
    #[serde(default = "any_version")]
    pub content_versions: Vec<String>,
    #[serde(default)]
    pub toolkit: ToolkitRange,
    #[serde(default)]
    pub when: When,
}

fn any_version() -> Vec<String> {
    vec!["*".to_owned()]
}

impl Default for AppliesTo {
    fn default() -> Self {
        AppliesTo {
            content_versions: any_version(),
            toolkit: ToolkitRange::default(),
            when: When::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolkitRange {
    pub min: Option<String>,
    pub max: Option<String>,
}

/// Conditions on the dump, from a fixed vocabulary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct When {
    /// Files that must all be absent from the dump (relative paths, see [`DUMP_FILES`]).
    #[serde(default)]
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Set {
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub relink: Vec<String>,
    #[serde(default)]
    pub launcher: LauncherSet,
    /// Reserved for a later layout gallery; the launcher ignores it.
    pub layout_id: Option<String>,
    /// Reserved for a later patch catalogue; the launcher ignores it.
    pub patch_set_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LauncherSet {
    pub vsync: Option<bool>,
    /// `off`, `tl`, `tr`, `bl` or `br`.
    pub overlay: Option<String>,
    pub shader_cache: Option<bool>,
    /// A console language number.
    pub language: Option<i32>,
    /// The toolkit release the game works best on (`v0.1.2`). Not a setting: the game sheet
    /// offers it as "Recommended for this game" in its toolkit choice (see [`toolkit_advice`]).
    pub toolkit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Verified {
    pub toolkit: String,
    pub os: Option<String>,
    pub gpu_vendor: Option<String>,
    pub date: String,
    /// `docs`, `maintainer` or `community`.
    pub by: String,
}

// ---------------------------------------------------------------------------------------------
// The allowlists

/// Environment variables a catalogue may set. The launcher's own variables (language, present
/// mode, overlay, shader cache, every path) are not here: those have a setting with one owner.
const ENV_SET: &[&str] = &["APS5_PLAYGO_UNLISTED_CHUNKS"];
/// Variables that may only appear in an `avoid` entry, never be set.
const ENV_AVOID_ONLY: &[&str] = &["APS5_TEXTURE_CACHE_MIB"];
/// Files a `when.missing` may name.
pub const DUMP_FILES: &[&str] = &["sce_sys/playgo-chunk.dat", "sce_sys/playgo-scenario.json"];

fn pattern(cell: &'static OnceLock<Regex>, source: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(source).expect("pattern"))
}

fn title_id_ok(id: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    pattern(&RE, r"^(PPSA|CUSA)\d{5}$").is_match(id)
}

fn entry_id_ok(id: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    pattern(&RE, r"^[a-z0-9][a-z0-9-]{0,63}$").is_match(id)
}

fn tag_ok(tag: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    pattern(&RE, r"^v\d+\.\d+\.\d+$").is_match(tag)
}

fn relative_path_ok(path: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    pattern(&RE, r"^[A-Za-z0-9._ -]+(/[A-Za-z0-9._ -]+)*$").is_match(path)
        && path.len() <= 200
        && path.split('/').all(|part| part != ".." && part != ".")
}

fn env_value_ok(key: &str, value: &str) -> bool {
    match key {
        "APS5_PLAYGO_UNLISTED_CHUNKS" => matches!(value, "installed" | "absent"),
        "APS5_TEXTURE_CACHE_MIB" => value
            .parse::<u32>()
            .is_ok_and(|n| (1..=65_536).contains(&n)),
        _ => false,
    }
}

/// Whether the launcher may pass `key=value` to a game on a catalogue's say-so.
pub fn env_allowed(key: &str, value: &str) -> bool {
    ENV_SET.contains(&key) && env_value_ok(key, value)
}

/// Relink flags as (flag, value) pairs; `None` when the list holds anything outside the allowlist.
fn relink_pairs(flags: &[String]) -> Option<Vec<(&str, &str)>> {
    static HEX: OnceLock<Regex> = OnceLock::new();
    if !flags.len().is_multiple_of(2) {
        return None;
    }
    flags
        .chunks(2)
        .map(|pair| {
            let (flag, value) = (pair[0].as_str(), pair[1].as_str());
            let ok = match flag {
                "--windows-fixed-base" => pattern(&HEX, r"^0x[0-9a-fA-F]{1,16}$").is_match(value),
                "--module-dir" | "--executable" => relative_path_ok(value),
                _ => false,
            };
            ok.then_some((flag, value))
        })
        .collect()
}

/// Whether the launcher may pass these flags to the relinker on a catalogue's say-so.
pub fn relink_allowed(flags: &[String]) -> bool {
    relink_pairs(flags).is_some()
}

fn overlay_of(name: &str) -> Option<Overlay> {
    Overlay::ALL.iter().copied().find(|o| match o.position() {
        Some(position) => position == name,
        None => name == "off",
    })
}

// ---------------------------------------------------------------------------------------------
// Reading and checking

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("not a recommended-settings file: {0}")]
    Format(String),
    #[error("{title} / {entry}: {problem}")]
    Entry {
        title: String,
        entry: String,
        problem: String,
    },
}

impl Catalog {
    /// Reads and validates a catalogue. Any problem refuses the whole file.
    pub fn parse(json: &str) -> Result<Catalog, CatalogError> {
        let catalog: Catalog =
            serde_json::from_str(json).map_err(|e| CatalogError::Format(e.to_string()))?;
        if catalog.schema != 1 {
            return Err(CatalogError::Format(format!("schema {}", catalog.schema)));
        }
        for (id, title) in &catalog.titles {
            if !title_id_ok(id) {
                return Err(CatalogError::Format(format!("title id {id}")));
            }
            let mut seen = Vec::new();
            for entry in &title.entries {
                let wrong = |problem: &str| CatalogError::Entry {
                    title: id.clone(),
                    entry: entry.id.clone(),
                    problem: problem.to_owned(),
                };
                if seen.contains(&entry.id) {
                    return Err(wrong("the id is used twice"));
                }
                seen.push(entry.id.clone());
                check_entry(entry).map_err(|problem| wrong(&problem))?;
            }
        }
        Ok(catalog)
    }

    /// The shipped catalogue. It is part of the build, and a test checks it.
    pub fn embedded() -> Catalog {
        Catalog::parse(EMBEDDED).expect("the shipped catalogue is valid")
    }

    /// The entries of a title.
    pub fn entries(&self, title_id: &str) -> &[Entry] {
        self.titles
            .get(title_id)
            .map_or(&[], |title| title.entries.as_slice())
    }
}

fn check_entry(entry: &Entry) -> Result<(), String> {
    if !entry_id_ok(&entry.id) {
        return Err("the id must be lowercase letters, digits and dashes".into());
    }
    if entry.why.trim().is_empty() || entry.why.len() > 400 {
        return Err("`why` must be one to four hundred characters".into());
    }
    if entry.evidence.is_empty() || !entry.evidence.iter().all(|u| u.starts_with("https://")) {
        return Err("`evidence` needs at least one https link".into());
    }
    if !["docs", "maintainer", "community"].contains(&entry.verified.by.as_str()) {
        return Err("`verified.by` must be docs, maintainer or community".into());
    }
    let applies = &entry.applies_to;
    if applies.content_versions.is_empty() {
        return Err("`contentVersions` must not be empty".into());
    }
    for tag in [&applies.toolkit.min, &applies.toolkit.max]
        .into_iter()
        .flatten()
    {
        if !tag_ok(tag) {
            return Err(format!("toolkit version {tag} must look like v0.1.3"));
        }
    }
    if let Some(file) = applies
        .when
        .missing
        .iter()
        .find(|f| !DUMP_FILES.contains(&f.as_str()))
    {
        return Err(format!("`when.missing` cannot name {file}"));
    }
    let set = &entry.set;
    for (key, value) in &set.env {
        let allowed = if entry.kind == Kind::Avoid {
            ENV_SET.contains(&key.as_str()) || ENV_AVOID_ONLY.contains(&key.as_str())
        } else {
            ENV_SET.contains(&key.as_str())
        };
        if !allowed {
            return Err(if ENV_AVOID_ONLY.contains(&key.as_str()) {
                format!("{key} can only be avoided, not set")
            } else {
                format!("{key} is not a variable a catalogue may set")
            });
        }
        if !env_value_ok(key, value) {
            return Err(format!("{value} is not a valid value for {key}"));
        }
    }
    if relink_pairs(&set.relink).is_none() {
        return Err("`relink` holds a flag or value outside the allowlist".into());
    }
    let launcher = &set.launcher;
    if launcher
        .overlay
        .as_deref()
        .is_some_and(|o| overlay_of(o).is_none())
    {
        return Err("`overlay` must be off, tl, tr, bl or br".into());
    }
    if launcher.language.is_some_and(|n| !(0..=30).contains(&n)) {
        return Err("`language` must be 0 to 30".into());
    }
    if let Some(tag) = &launcher.toolkit {
        if !tag_ok(tag) {
            return Err(format!("toolkit version {tag} must look like v0.1.3"));
        }
        if entry.kind == Kind::Avoid {
            return Err("an `avoid` entry cannot recommend a toolkit".into());
        }
    }
    if setting_count(set) == 0 {
        return Err("the entry sets nothing".into());
    }
    if entry.kind == Kind::Avoid && !set.relink.is_empty() {
        return Err("an `avoid` entry cannot name relink flags".into());
    }
    Ok(())
}

fn setting_count(set: &Set) -> usize {
    let l = &set.launcher;
    set.env.len()
        + set.relink.len() / 2
        + [
            l.vsync.is_some(),
            l.overlay.is_some(),
            l.shader_cache.is_some(),
            l.language.is_some(),
            l.toolkit.is_some(),
        ]
        .into_iter()
        .filter(|x| *x)
        .count()
}

// ---------------------------------------------------------------------------------------------
// The newest catalogue

/// The catalogue to use: the stored or downloaded copy when it is valid and not older than the
/// shipped one, else the shipped one. Blocking when it asks the network: call from a worker.
pub fn load(paths: &AppPaths) -> Catalog {
    load_from(paths, URL)
}

pub(crate) fn load_from(paths: &AppPaths, url: &str) -> Catalog {
    let embedded = Catalog::embedded();
    let fetched = feed::fetch(
        &paths.cache_dir().join("Feeds"),
        &Request {
            url,
            name: "recommended-settings.v1.json",
            accept: "application/json",
            max_age: Duration::from_secs(24 * 3600),
            max_bytes: MAX_BYTES,
        },
    );
    match fetched.map(|d| Catalog::parse(&d.body)) {
        Ok(Ok(remote)) if remote.updated >= embedded.updated => remote,
        Ok(Err(error)) => {
            tracing::warn!("recommended settings: {error}");
            embedded
        }
        _ => embedded,
    }
}

impl From<CatalogError> for ToolkitError {
    fn from(error: CatalogError) -> Self {
        ToolkitError::Network(error.to_string())
    }
}

// ---------------------------------------------------------------------------------------------
// From entries to changes

/// What an entry is checked against.
pub struct Context<'a> {
    pub title_id: &'a str,
    /// `contentVersion` of the game.
    pub content_version: &'a str,
    /// The toolkit version as the launcher stores it (`0.1.3-abc`, `local-abc`).
    pub toolkit: &'a str,
    /// The dump folder.
    pub dump: &'a Path,
    /// Environment variables the game would inherit from this PC.
    pub inherited: &'a dyn Fn(&str) -> Option<String>,
}

/// One setting an entry touches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Setting {
    Env(String),
    /// A relinker flag, such as `--windows-fixed-base`.
    Relink(String),
    Vsync,
    Overlay,
    ShaderCache,
    Language,
}

/// One line of the diff: a setting, what it is now and what the entry wants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub entry: String,
    pub kind: Kind,
    pub setting: Setting,
    /// `None` when the setting is not set (the default).
    pub current: Option<String>,
    pub proposed: String,
    pub why: String,
}

/// A known bad value the game would run with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caution {
    pub entry: String,
    pub setting: Setting,
    pub value: String,
    pub why: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub changes: Vec<Change>,
    pub cautions: Vec<Caution>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.cautions.is_empty()
    }

    /// Whether a relinker flag changes: the game has to be unlocked again for it to count.
    pub fn needs_unlock(&self) -> bool {
        self.changes
            .iter()
            .any(|c| matches!(c.setting, Setting::Relink(_)))
    }

    /// Writes every change into the settings.
    pub fn apply(&self, settings: &mut GameSettings) {
        for change in &self.changes {
            let value = change.proposed.as_str();
            match &change.setting {
                Setting::Env(key) => {
                    settings.env.insert(key.clone(), value.to_owned());
                }
                Setting::Relink(flag) => set_flag(&mut settings.relink, flag, value),
                Setting::Vsync => settings.vsync = value == "on",
                Setting::Overlay => {
                    settings.overlay = overlay_of(value).unwrap_or(Overlay::Off);
                }
                Setting::ShaderCache => settings.shader_cache = Some(value == "on"),
                Setting::Language => settings.language = value.parse().ok(),
            }
        }
    }
}

fn set_flag(flags: &mut Vec<String>, flag: &str, value: &str) {
    match flags.chunks(2).position(|pair| pair[0] == flag) {
        Some(index) => flags[index * 2 + 1] = value.to_owned(),
        None => flags.extend([flag.to_owned(), value.to_owned()]),
    }
}

fn flag_value<'a>(flags: &'a [String], flag: &str) -> Option<&'a str> {
    flags
        .chunks(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn on_off(on: bool) -> String {
    if on { "on" } else { "off" }.to_owned()
}

fn overlay_name(overlay: Overlay) -> String {
    overlay.position().unwrap_or("off").to_owned()
}

/// A setting, what the entry wants for it, what it is now and what counts as untouched.
struct Wanted {
    setting: Setting,
    proposed: String,
    current: Option<String>,
    /// The player has chosen this value themselves (it differs from the default).
    own_choice: bool,
}

fn wanted(set: &Set, settings: &GameSettings) -> Vec<Wanted> {
    let mut out = Vec::new();
    for (key, value) in &set.env {
        let current = settings.env.get(key).cloned();
        out.push(Wanted {
            setting: Setting::Env(key.clone()),
            proposed: value.clone(),
            own_choice: current.as_ref().is_some_and(|c| c != value),
            current,
        });
    }
    for (flag, value) in relink_pairs(&set.relink).unwrap_or_default() {
        let current = flag_value(&settings.relink, flag).map(str::to_owned);
        out.push(Wanted {
            setting: Setting::Relink(flag.to_owned()),
            proposed: value.to_owned(),
            own_choice: current.as_ref().is_some_and(|c| c != value),
            current,
        });
    }
    let l = &set.launcher;
    if let Some(vsync) = l.vsync {
        out.push(Wanted {
            setting: Setting::Vsync,
            proposed: on_off(vsync),
            current: Some(on_off(settings.vsync)),
            own_choice: !settings.vsync && vsync,
        });
    }
    if let Some(name) = &l.overlay {
        out.push(Wanted {
            setting: Setting::Overlay,
            proposed: name.clone(),
            current: Some(overlay_name(settings.overlay)),
            own_choice: settings.overlay != Overlay::Off
                && overlay_of(name) != Some(settings.overlay),
        });
    }
    if let Some(cache) = l.shader_cache {
        out.push(Wanted {
            setting: Setting::ShaderCache,
            proposed: on_off(cache),
            current: Some(on_off(settings.shader_cache.unwrap_or(true))),
            own_choice: settings.shader_cache.is_some_and(|c| c != cache),
        });
    }
    if let Some(language) = l.language {
        out.push(Wanted {
            setting: Setting::Language,
            proposed: language.to_string(),
            current: settings.language.map(|n| n.to_string()),
            own_choice: settings.language.is_some_and(|n| n != language),
        });
    }
    out
}

fn applies(entry: &Entry, ctx: &Context<'_>) -> bool {
    let a = &entry.applies_to;
    let version_ok = a
        .content_versions
        .iter()
        .any(|v| v == "*" || v == ctx.content_version);
    let toolkit_ok = {
        let at = |bound: &Option<String>| bound.as_deref().and_then(compat::release_tag);
        let tag = compat::release_tag(ctx.toolkit);
        match tag {
            // A local build is assumed to be in range.
            None => true,
            Some(tag) => {
                let order =
                    |other: Option<String>| other.and_then(|o| compat::compare_versions(&tag, &o));
                order(at(&a.toolkit.min)).is_none_or(|o| o.is_ge())
                    && order(at(&a.toolkit.max)).is_none_or(|o| o.is_le())
            }
        }
    };
    let dump_ok = a
        .when
        .missing
        .iter()
        .all(|file| !ctx.dump.join(file).exists());
    version_ok && toolkit_ok && dump_ok
}

/// What the catalogue's entries for this game would change, and what they warn about. Settings
/// that already have the wanted value are not listed. A `recommended` value the player has set
/// to something else is left alone; a `required` one is not.
pub fn plan(entries: &[Entry], ctx: &Context<'_>, settings: &GameSettings) -> Plan {
    let mut plan = Plan::default();
    for entry in entries.iter().filter(|e| applies(e, ctx)) {
        let wants = wanted(&entry.set, settings);
        match entry.kind {
            Kind::Avoid => {
                // It warns when every named setting has the bad value, counting this PC's own
                // environment for variables.
                let all_bad = !wants.is_empty()
                    && wants.iter().all(|w| {
                        let effective = match &w.setting {
                            Setting::Env(key) => (ctx.inherited)(key).or(w.current.clone()),
                            _ => w.current.clone(),
                        };
                        effective.as_deref() == Some(w.proposed.as_str())
                    });
                if all_bad {
                    for w in wants {
                        plan.cautions.push(Caution {
                            entry: entry.id.clone(),
                            setting: w.setting,
                            value: w.proposed,
                            why: entry.why.clone(),
                        });
                    }
                }
            }
            Kind::Required | Kind::Recommended => {
                for w in wants {
                    let unchanged = w.current.as_deref() == Some(w.proposed.as_str());
                    let kept = entry.kind == Kind::Recommended && w.own_choice;
                    if unchanged || kept {
                        continue;
                    }
                    plan.changes.push(Change {
                        entry: entry.id.clone(),
                        kind: entry.kind,
                        setting: w.setting,
                        current: w.current,
                        proposed: w.proposed,
                        why: entry.why.clone(),
                    });
                }
            }
        }
    }
    plan
}

/// The toolkit release an entry of this game recommends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advice {
    /// `v0.1.2`.
    pub tag: String,
    pub why: String,
}

/// The toolkit release the catalogue recommends for the game, from the first entry that applies
/// to the toolkit in `ctx` and names one. The entry's `appliesTo.toolkit` range says on which
/// toolkits the advice is given: `min` = `v0.1.3` recommends `v0.1.2` to a player on v0.1.3 or
/// newer.
pub fn toolkit_advice(entries: &[Entry], ctx: &Context<'_>) -> Option<Advice> {
    entries.iter().filter(|e| applies(e, ctx)).find_map(|e| {
        Some(Advice {
            tag: e.set.launcher.toolkit.clone()?,
            why: e.why.clone(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn entry_json(kind: &str, set: &str) -> String {
        format!(
            r#"{{"schema":1,"updated":"2026-10-22","titles":{{"PPSA15388":{{"name":"High on Life","entries":[
            {{"id":"e1","kind":"{kind}","set":{set},"why":"Because.",
              "evidence":["https://example.com/a"],
              "verified":{{"toolkit":"v0.1.3","date":"2026-10-22","by":"docs"}}}}]}}}}}}"#
        )
    }

    fn parse(kind: &str, set: &str) -> Result<Catalog, CatalogError> {
        Catalog::parse(&entry_json(kind, set))
    }

    #[test]
    fn the_shipped_catalogue_is_valid_and_has_evidence() {
        let catalog = Catalog::embedded();
        assert_eq!(catalog.schema, 1);
        assert!(!catalog.titles.is_empty());
        for title in catalog.titles.values() {
            for entry in &title.entries {
                assert!(!entry.evidence.is_empty(), "{}", entry.id);
            }
        }
    }

    #[test]
    fn allowed_settings_pass() {
        assert!(
            parse(
                "required",
                r#"{"env":{"APS5_PLAYGO_UNLISTED_CHUNKS":"installed"}}"#
            )
            .is_ok()
        );
        assert!(
            parse(
                "recommended",
                r#"{"relink":["--windows-fixed-base","0x400000"]}"#
            )
            .is_ok()
        );
        assert!(
            parse(
                "recommended",
                r#"{"relink":["--module-dir","Media/Modules"]}"#
            )
            .is_ok()
        );
        assert!(
            parse(
                "recommended",
                r#"{"launcher":{"vsync":false,"overlay":"tl","language":4}}"#
            )
            .is_ok()
        );
        assert!(parse("avoid", r#"{"env":{"APS5_TEXTURE_CACHE_MIB":"3072"}}"#).is_ok());
    }

    #[test]
    fn the_env_allowlist_is_the_trust_boundary() {
        for key in [
            "ANYPS5_INPUT_CONFIG",
            "ANYPS5_SHADER_CACHE_DIR",
            "ANYPS5_SCREENSHOTS",
            "APS5_EBOOT",
            "APS5_TRACE_DIR",
            "APS5_PAD_INPUT_FILE",
            "ANYPS5_LANGUAGE",
            "ANYPS5_PRESENT_MODE",
            "PATH",
        ] {
            let set = format!(r#"{{"env":{{"{key}":"1"}}}}"#);
            assert!(parse("required", &set).is_err(), "{key}");
            assert!(parse("avoid", &set).is_err(), "{key} (avoid)");
        }
        assert!(
            parse(
                "recommended",
                r#"{"env":{"APS5_TEXTURE_CACHE_MIB":"3072"}}"#
            )
            .is_err(),
            "the texture cache can only be avoided"
        );
        assert!(
            parse(
                "required",
                r#"{"env":{"APS5_PLAYGO_UNLISTED_CHUNKS":"maybe"}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn the_relink_allowlist_is_the_trust_boundary() {
        for flags in [
            r#"["--autorun","x"]"#,
            r#"["--patches","evil.toml"]"#,
            r#"["--rpath","x"]"#,
            r#"["--module-dir","../escape"]"#,
            r#"["--module-dir","/abs"]"#,
            r#"["--module-dir","C:/x"]"#,
            r#"["--module-dir","a\\b"]"#,
            r#"["--windows-fixed-base","400000"]"#,
            r#"["--windows-fixed-base"]"#,
        ] {
            let set = format!(r#"{{"relink":{flags}}}"#);
            assert!(parse("recommended", &set).is_err(), "{flags}");
        }
        assert!(relink_allowed(&[
            "--windows-fixed-base".into(),
            "0x400000".into()
        ]));
        assert!(!relink_allowed(&["--skip-checks".into(), "1".into()]));
    }

    #[test]
    fn a_toolkit_recommendation_is_a_valid_entry_that_changes_no_setting() {
        let dir = tempfile::tempdir().unwrap();
        let none = |_: &str| None;
        let e = entries("recommended", r#"{"launcher":{"toolkit":"v0.1.2"}}"#, "{}");
        let ctx = context(dir.path(), &none);
        assert!(plan(&e, &ctx, &GameSettings::default()).is_empty());
        let advice = toolkit_advice(&e, &ctx).unwrap();
        assert_eq!(advice.tag, "v0.1.2");
        assert_eq!(advice.why, "Because.");
        // Only for the toolkits the entry names: this one is for v0.1.4 and newer.
        let newer = entries(
            "required",
            r#"{"launcher":{"toolkit":"v0.1.2"}}"#,
            r#"{"toolkit":{"min":"v0.1.4","max":null}}"#,
        );
        assert_eq!(toolkit_advice(&newer, &ctx), None);
        // Entries that set something else give no advice.
        let other = entries("recommended", r#"{"launcher":{"vsync":false}}"#, "{}");
        assert_eq!(toolkit_advice(&other, &ctx), None);
        // It is a recommendation, never a warning, and the tag has to be a release tag.
        assert!(parse("avoid", r#"{"launcher":{"toolkit":"v0.1.2"}}"#).is_err());
        assert!(parse("recommended", r#"{"launcher":{"toolkit":"latest"}}"#).is_err());
        assert!(parse("recommended", r#"{"launcher":{"toolkit":"../v0.1.2"}}"#).is_err());
    }

    #[test]
    fn a_file_with_a_problem_is_refused_whole() {
        assert!(Catalog::parse("{").is_err());
        assert!(
            Catalog::parse(&entry_json("required", "{}")).is_err(),
            "sets nothing"
        );
        let unknown = entry_json(
            "required",
            r#"{"env":{"APS5_PLAYGO_UNLISTED_CHUNKS":"absent"},"cmd":"x"}"#,
        );
        assert!(
            Catalog::parse(&unknown).is_err(),
            "unknown keys are refused"
        );
        let no_proof = entry_json(
            "required",
            r#"{"env":{"APS5_PLAYGO_UNLISTED_CHUNKS":"absent"}}"#,
        )
        .replace(
            r#""evidence":["https://example.com/a"]"#,
            r#""evidence":[]"#,
        );
        assert!(Catalog::parse(&no_proof).is_err(), "evidence is required");
        let schema2 = entry_json("avoid", r#"{"env":{"APS5_TEXTURE_CACHE_MIB":"1"}}"#)
            .replace(r#""schema":1"#, r#""schema":2"#);
        assert!(Catalog::parse(&schema2).is_err());
        let bad_id = entry_json("avoid", r#"{"env":{"APS5_TEXTURE_CACHE_MIB":"1"}}"#)
            .replace("PPSA15388", "nope");
        assert!(Catalog::parse(&bad_id).is_err());
        let twice = r#"{"schema":1,"updated":"x","titles":{"PPSA00001":{"name":"n","entries":[
          {"id":"a","kind":"avoid","set":{"env":{"APS5_TEXTURE_CACHE_MIB":"5"}},"why":"w","evidence":["https://e"],"verified":{"toolkit":"v0.1.3","date":"d","by":"docs"}},
          {"id":"a","kind":"avoid","set":{"env":{"APS5_TEXTURE_CACHE_MIB":"6"}},"why":"w","evidence":["https://e"],"verified":{"toolkit":"v0.1.3","date":"d","by":"docs"}}]}}}"#;
        assert!(Catalog::parse(twice).is_err());
    }

    fn context<'a>(dump: &'a Path, inherited: &'a dyn Fn(&str) -> Option<String>) -> Context<'a> {
        Context {
            title_id: "PPSA15388",
            content_version: "01.000.000",
            toolkit: "0.1.3-abc",
            dump,
            inherited,
        }
    }

    fn entries(kind: &str, set: &str, applies: &str) -> Vec<Entry> {
        let json =
            entry_json(kind, set).replace(r#""set":"#, &format!(r#""appliesTo":{applies},"set":"#));
        Catalog::parse(&json).unwrap().titles["PPSA15388"]
            .entries
            .clone()
    }

    #[test]
    fn a_required_entry_is_planned_until_it_is_set() {
        let dir = tempfile::tempdir().unwrap();
        let none = |_: &str| None;
        let e = entries(
            "required",
            r#"{"env":{"APS5_PLAYGO_UNLISTED_CHUNKS":"installed"}}"#,
            "{}",
        );
        let mut settings = GameSettings::default();
        let plan_ = plan(&e, &context(dir.path(), &none), &settings);
        assert_eq!(plan_.changes.len(), 1);
        assert_eq!(plan_.changes[0].current, None);
        assert_eq!(plan_.changes[0].proposed, "installed");
        plan_.apply(&mut settings);
        assert_eq!(settings.env["APS5_PLAYGO_UNLISTED_CHUNKS"], "installed");
        assert!(plan(&e, &context(dir.path(), &none), &settings).is_empty());
        // The environment a game starts with now carries it.
        let env = settings.environment(&Default::default());
        assert_eq!(env["APS5_PLAYGO_UNLISTED_CHUNKS"], "installed");
    }

    #[test]
    fn required_overrides_and_recommended_keeps_the_players_choice() {
        let dir = tempfile::tempdir().unwrap();
        let none = |_: &str| None;
        let set = r#"{"env":{"APS5_PLAYGO_UNLISTED_CHUNKS":"installed"}}"#;
        let mut settings = GameSettings::default();
        settings
            .env
            .insert("APS5_PLAYGO_UNLISTED_CHUNKS".into(), "absent".into());
        let required = plan(
            &entries("required", set, "{}"),
            &context(dir.path(), &none),
            &settings,
        );
        assert_eq!(required.changes.len(), 1, "required wins");
        assert_eq!(required.changes[0].current.as_deref(), Some("absent"));
        let recommended = plan(
            &entries("recommended", set, "{}"),
            &context(dir.path(), &none),
            &settings,
        );
        assert!(recommended.is_empty(), "the player's own value wins");
        // Vsync off is a choice; a recommendation to turn it on is dropped, to keep it off is not.
        let vsync = entries("recommended", r#"{"launcher":{"vsync":true}}"#, "{}");
        let own = GameSettings {
            vsync: false,
            ..Default::default()
        };
        assert!(plan(&vsync, &context(dir.path(), &none), &own).is_empty());
        let off = entries("recommended", r#"{"launcher":{"vsync":false}}"#, "{}");
        assert_eq!(
            plan(&off, &context(dir.path(), &none), &GameSettings::default())
                .changes
                .len(),
            1
        );
    }

    #[test]
    fn avoid_only_warns_and_only_when_the_value_would_be_used() {
        let dir = tempfile::tempdir().unwrap();
        let e = entries(
            "avoid",
            r#"{"env":{"APS5_TEXTURE_CACHE_MIB":"3072"}}"#,
            "{}",
        );
        let settings = GameSettings::default();
        let none = |_: &str| None;
        assert!(plan(&e, &context(dir.path(), &none), &settings).is_empty());
        let other = |_: &str| Some("2048".to_owned());
        assert!(plan(&e, &context(dir.path(), &other), &settings).is_empty());
        let set = |key: &str| (key == "APS5_TEXTURE_CACHE_MIB").then(|| "3072".to_owned());
        let warned = plan(&e, &context(dir.path(), &set), &settings);
        assert!(warned.changes.is_empty(), "never a change");
        assert_eq!(warned.cautions.len(), 1);
        assert_eq!(warned.cautions[0].value, "3072");
    }

    #[test]
    fn conditions_decide_whether_an_entry_applies() {
        let dump = tempfile::tempdir().unwrap();
        let none = |_: &str| None;
        let set = r#"{"env":{"APS5_PLAYGO_UNLISTED_CHUNKS":"absent"}}"#;
        let missing = r#"{"when":{"missing":["sce_sys/playgo-chunk.dat"]}}"#;
        let e = entries("required", set, missing);
        let ctx = context(dump.path(), &none);
        let s = GameSettings::default();
        assert_eq!(
            plan(&e, &ctx, &s).changes.len(),
            1,
            "the dump lacks the file"
        );
        fs::create_dir_all(dump.path().join("sce_sys")).unwrap();
        fs::write(dump.path().join("sce_sys/playgo-chunk.dat"), b"x").unwrap();
        assert!(
            plan(&e, &ctx, &s).is_empty(),
            "metadata present: unaffected"
        );

        let version = entries("required", set, r#"{"contentVersions":["02.000.000"]}"#);
        assert!(
            plan(&version, &ctx, &s).is_empty(),
            "another content version"
        );

        let newer = entries(
            "required",
            set,
            r#"{"toolkit":{"min":"v0.1.4","max":null}}"#,
        );
        assert!(
            plan(&newer, &ctx, &s).is_empty(),
            "toolkit 0.1.3 is below the minimum"
        );
        let old = entries(
            "required",
            set,
            r#"{"toolkit":{"min":null,"max":"v0.1.2"}}"#,
        );
        assert!(plan(&old, &ctx, &s).is_empty(), "past the maximum");
        let ranged = entries(
            "required",
            set,
            r#"{"toolkit":{"min":"v0.1.3","max":"v0.1.3"}}"#,
        );
        assert_eq!(plan(&ranged, &ctx, &s).changes.len(), 1);
        let local = Context {
            toolkit: "local-abc",
            ..context(dump.path(), &none)
        };
        assert_eq!(
            plan(&newer, &local, &s).changes.len(),
            1,
            "a local build is in range"
        );
    }

    #[test]
    fn relink_flags_are_replaced_or_added_and_ask_for_an_unlock() {
        let dir = tempfile::tempdir().unwrap();
        let none = |_: &str| None;
        let e = entries(
            "recommended",
            r#"{"relink":["--windows-fixed-base","0x400000"]}"#,
            "{}",
        );
        let mut settings = GameSettings::default();
        let p = plan(&e, &context(dir.path(), &none), &settings);
        assert!(p.needs_unlock());
        p.apply(&mut settings);
        assert_eq!(settings.relink, ["--windows-fixed-base", "0x400000"]);
        assert!(plan(&e, &context(dir.path(), &none), &settings).is_empty());
    }

    #[test]
    fn launcher_settings_land_in_the_game_settings() {
        let dir = tempfile::tempdir().unwrap();
        let none = |_: &str| None;
        let set = r#"{"launcher":{"vsync":false,"overlay":"br","shaderCache":false,"language":4}}"#;
        let e = entries("recommended", set, "{}");
        let mut settings = GameSettings::default();
        plan(&e, &context(dir.path(), &none), &settings).apply(&mut settings);
        assert!(!settings.vsync);
        assert_eq!(settings.overlay, Overlay::BottomRight);
        assert_eq!(settings.shader_cache, Some(false));
        assert_eq!(settings.language, Some(4));
    }

    #[test]
    fn the_catalogue_is_fetched_and_the_shipped_one_is_the_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::under(dir.path());
        let embedded = Catalog::embedded();
        // Unreachable server: the shipped catalogue.
        assert_eq!(load_from(&paths, "http://127.0.0.1:9/x"), embedded);
        // A newer, valid catalogue wins; an invalid one does not.
        let newer = entry_json("avoid", r#"{"env":{"APS5_TEXTURE_CACHE_MIB":"3072"}}"#)
            .replace("2026-10-22", "2099-01-01");
        let leaked: &'static str = Box::leak(newer.into_boxed_str());
        let (base, _) = crate::feed::testing::serve(leaked);
        let got = load_from(&paths, &format!("{base}/c.json"));
        assert_eq!(got.updated, "2099-01-01");
        let bad_dir = tempfile::tempdir().unwrap();
        let (base, _) = crate::feed::testing::serve("{\"nope\":1}");
        let got = load_from(&AppPaths::under(bad_dir.path()), &format!("{base}/c.json"));
        assert_eq!(got, embedded);
    }
}
