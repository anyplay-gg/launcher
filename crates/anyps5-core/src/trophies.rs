//! Trophy data read from game dumps and the records the engine writes while a game runs
//! (PORT.md 6.8, Trophies.swift; the logic is unchanged from the macOS launcher).
//!
//! The list and texts come from the trophy packages (`.ucp` archives) in the dump's
//! `sce_sys/trophy2`; what the player earned is in `<unlocked copy>\_trophy`.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use serde_json::Value;

use crate::{
    events::{CoreEvent, Notifier},
    library::Game,
};

/// The console's trophy grades.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Grade {
    Platinum,
    Gold,
    Silver,
    Bronze,
}

impl Grade {
    pub const ALL: [Grade; 4] = [Grade::Platinum, Grade::Gold, Grade::Silver, Grade::Bronze];

    /// The letter used in `tropconf.json`.
    pub fn letter(self) -> char {
        match self {
            Grade::Platinum => 'P',
            Grade::Gold => 'G',
            Grade::Silver => 'S',
            Grade::Bronze => 'B',
        }
    }

    pub fn from_letter(letter: &str) -> Option<Grade> {
        Grade::ALL
            .into_iter()
            .find(|g| letter.len() == 1 && letter.starts_with(g.letter()))
    }

    /// The console's weights: a gold trophy counts as six bronze ones.
    pub fn points(self) -> u32 {
        match self {
            Grade::Platinum => 300,
            Grade::Gold => 90,
            Grade::Silver => 30,
            Grade::Bronze => 15,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Trophy {
    pub service_label: u32,
    pub number: u32,
    pub grade: Grade,
    pub hidden: bool,
    pub name: String,
    pub detail: String,
    /// PNG bytes.
    pub icon: Option<Vec<u8>>,
    /// When it was earned, in microseconds since the Unix epoch (the engine's resolution).
    pub unlocked: Option<i64>,
    /// `(value, target)` of a progressive trophy.
    pub progress: Option<(u64, u64)>,
}

impl Trophy {
    /// `"<service label>/<number>"`, the key of [`Trophies::earned`].
    pub fn id(&self) -> String {
        format!("{}/{}", self.service_label, self.number)
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked.is_some()
    }

    /// The unlock time in whole Unix seconds.
    pub fn unlocked_secs(&self) -> Option<i64> {
        self.unlocked.map(|micros| micros.div_euclid(1_000_000))
    }
}

/// A trophy the player earned, as far as the network may know it: no name, text or picture
/// (network ADR 0009).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Earned {
    pub number: u32,
    pub grade: Grade,
    /// Microseconds since the Unix epoch.
    pub unlocked: i64,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TrophySet {
    pub title: String,
    /// PNG bytes.
    pub icon: Option<Vec<u8>>,
    pub trophies: Vec<Trophy>,
}

impl TrophySet {
    pub fn unlocked_count(&self) -> usize {
        self.trophies.iter().filter(|t| t.is_unlocked()).count()
    }

    /// `(unlocked, total)` of one grade.
    pub fn count(&self, grade: Grade) -> (usize, usize) {
        let graded = self.trophies.iter().filter(|t| t.grade == grade);
        (
            graded.clone().filter(|t| t.is_unlocked()).count(),
            graded.count(),
        )
    }

    /// The earned trophies of the first set (service label 0), the one the network records.
    pub fn earned(&self) -> Vec<Earned> {
        self.trophies
            .iter()
            .filter(|t| t.service_label == 0)
            .filter_map(|t| {
                Some(Earned {
                    number: t.number,
                    grade: t.grade,
                    unlocked: t.unlocked?,
                })
            })
            .collect()
    }

    /// The numbers of the first set's hidden trophies, the ones the network is told are secret.
    pub fn hidden(&self) -> Vec<u32> {
        self.trophies
            .iter()
            .filter(|t| t.service_label == 0 && t.hidden)
            .map(|t| t.number)
            .collect()
    }

    /// Unlocked points over all points, 0 to 100 (integer division).
    pub fn percent(&self) -> u32 {
        let total: u32 = self.trophies.iter().map(|t| t.grade.points()).sum();
        if total == 0 {
            return 0;
        }
        let unlocked: u32 = self
            .trophies
            .iter()
            .filter(|t| t.is_unlocked())
            .map(|t| t.grade.points())
            .sum();
        unlocked * 100 / total
    }
}

/// Microseconds from 1 January of year 1 to the Unix epoch: the engine's tick base.
const UNIX_EPOCH_TICK: u64 = 62_135_596_800_000_000;
const RECORD_SUFFIX: &str = ".txt";
const STATS_SUFFIX: &str = ".stats.txt";

pub struct Trophies;

impl Trophies {
    /// The game's trophies in the first of `languages` its packages have; `None` when the dump has
    /// none. `copy` is the unlocked copy, which holds the unlocks; without one every trophy is locked.
    pub fn load(game: &Game, copy: Option<&Path>, languages: &[String]) -> Option<TrophySet> {
        let folder = game.folder.join("sce_sys").join("trophy2");
        let records = copy.map(|c| c.join("_trophy"));
        let mut title = game.name.clone();
        let mut set_icon = None;
        let mut trophies = Vec::new();
        for label in 0..100u32 {
            let Ok(data) = fs::read(folder.join(format!("trophy{label:02}.ucp"))) else {
                if label == 0 {
                    continue;
                }
                break;
            };
            let Some(files) = read_ucp(&data) else {
                continue;
            };
            let Some(loaded) = parse(&files, label, languages, records.as_deref()) else {
                continue;
            };
            if label == 0 || trophies.is_empty() {
                title = loaded.title;
                set_icon = loaded.icon;
            }
            trophies.extend(loaded.trophies);
        }
        (!trophies.is_empty()).then_some(TrophySet {
            title,
            icon: set_icon,
            trophies,
        })
    }

    /// What [`TrophySet::earned`] gives, read without the texts and pictures: only the grades from
    /// the first package and the unlocked copy's records. Empty when nothing was earned.
    pub fn earned_records(game: &Game, copy: &Path) -> Vec<Earned> {
        let unlocks = unlocked(&copy.join("_trophy"), 0);
        if unlocks.is_empty() {
            return Vec::new();
        }
        let package = game
            .folder
            .join("sce_sys")
            .join("trophy2")
            .join("trophy00.ucp");
        let Some(conf) = fs::read(package)
            .ok()
            .and_then(|data| read_ucp(&data))
            .and_then(|files| json(files.get("tropconf.json")))
        else {
            return Vec::new();
        };
        let mut earned: Vec<Earned> = conf
            .get("trophies")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|definition| {
                let number = identifier(definition.get("id"))?;
                Some(Earned {
                    number,
                    grade: Grade::from_letter(definition.get("grade")?.as_str()?)?,
                    unlocked: *unlocks.get(&number)?,
                })
            })
            .collect();
        earned.sort_by_key(|e| e.number);
        earned
    }

    /// The ids (`"<label>/<number>"`) of every trophy the engine recorded as earned in the unlocked
    /// copy. Cheap enough to call every second.
    pub fn earned(copy: &Path) -> HashSet<String> {
        let records = copy.join("_trophy");
        let Ok(names) = fs::read_dir(&records) else {
            return HashSet::new();
        };
        let mut labels = HashSet::new();
        for name in names
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
        {
            if let Some(stem) = name.strip_suffix(RECORD_SUFFIX)
                && !name.ends_with(STATS_SUFFIX)
                && let Some(label) = stem.rsplit('_').next().and_then(|l| l.parse::<u32>().ok())
            {
                labels.insert(label);
            }
        }
        labels
            .into_iter()
            .flat_map(|label| {
                unlocked(&records, label)
                    .into_keys()
                    .map(move |id| format!("{label}/{id}"))
            })
            .collect()
    }
}

/// The package's files: a big-endian table of names, offsets and sizes.
fn read_ucp(data: &[u8]) -> Option<HashMap<String, Vec<u8>>> {
    let number = |offset: usize, size: usize| -> Option<u64> {
        let bytes = data.get(offset..offset.checked_add(size)?)?;
        Some(bytes.iter().fold(0u64, |acc, b| acc << 8 | u64::from(*b)))
    };
    if number(0, 4)? != 0xB228_C60A || number(4, 4)? != 1 {
        return None;
    }
    let count = number(0x10, 4)?;
    let toc = usize::try_from(number(0x14, 4)?).ok()?;
    if count > 4096 {
        return None;
    }
    let mut files = HashMap::new();
    for index in 0..count as usize {
        let entry = toc
            .checked_add(0x20)?
            .checked_add(index.checked_mul(0x40)?)?;
        let offset = number(entry + 0x20, 8)?;
        let length = number(entry + 0x28, 8)?;
        if offset.checked_add(length)? > data.len() as u64 {
            return None;
        }
        let field = data.get(entry..entry + 0x20)?;
        let name_len = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        let name = String::from_utf8_lossy(&field[..name_len]).into_owned();
        files.insert(
            name,
            data[offset as usize..(offset + length) as usize].to_vec(),
        );
    }
    Some(files)
}

fn parse(
    files: &HashMap<String, Vec<u8>>,
    label: u32,
    languages: &[String],
    records: Option<&Path>,
) -> Option<TrophySet> {
    let conf = json(files.get("tropconf.json"))?;
    let definitions = conf.get("trophies")?.as_array()?;
    let fallback = conf
        .get("defaultLanguage")
        .and_then(Value::as_str)
        .unwrap_or("en-US")
        .to_string();
    let available: HashSet<&str> = files
        .keys()
        .filter_map(|n| n.strip_prefix("tropmeta_")?.strip_suffix(".json"))
        .collect();
    let candidates =
        languages
            .iter()
            .cloned()
            .chain(["en-US".to_string(), "en-GB".to_string(), fallback]);
    let language = candidates
        .into_iter()
        .find(|c| available.contains(c.as_str()))
        .or_else(|| available.iter().min().map(|s| s.to_string()))?;
    let meta = json(files.get(&format!("tropmeta_{language}.json")))?;
    let metadata = meta.get("metadata")?.as_object()?;

    let mut texts: HashMap<u32, (String, String)> = HashMap::new();
    for entry in metadata
        .get("trophyMetadata")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(id) = identifier(entry.get("id")) else {
            continue;
        };
        let text = |key: &str| {
            entry
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        texts.insert(id, (text("name"), text("detail")));
    }
    let unlocks = records.map(|r| unlocked(r, label)).unwrap_or_default();
    let stats = records.map(|r| stat_values(r, label)).unwrap_or_default();

    let trophies = definitions
        .iter()
        .filter_map(|definition| {
            let id = identifier(definition.get("id"))?;
            let grade = Grade::from_letter(definition.get("grade")?.as_str()?)?;
            let mut progress = None;
            if let Some(condition) = definition.get("unlockCondition")
                && condition.get("progressive").and_then(Value::as_bool) == Some(true)
                && let Some(stat) = unsigned(condition.get("udsStatId"))
                && let Some(target) = unsigned(condition.get("targetValue"))
                && target > 1
            {
                progress = Some((stats.get(&stat).copied().unwrap_or(0).min(target), target));
            }
            let (name, detail) = texts.get(&id).cloned().unwrap_or_default();
            Some(Trophy {
                service_label: label,
                number: id,
                grade,
                hidden: definition.get("hidden").and_then(Value::as_bool) == Some(true),
                name,
                detail,
                icon: files.get(&format!("trop{id:04}.png")).cloned(),
                unlocked: unlocks.get(&id).copied(),
                progress,
            })
        })
        .collect();

    let title = metadata
        .get("titleMetadata")
        .and_then(|t| t.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let icon = files
        .get(&format!("icon0_{language}.png"))
        .or_else(|| files.get("icon0.png"))
        .or_else(|| {
            let mut names: Vec<&String> = files.keys().filter(|n| n.starts_with("icon0")).collect();
            names.sort();
            names.first().and_then(|n| files.get(*n))
        })
        .cloned();
    Some(TrophySet {
        title,
        icon,
        trophies,
    })
}

/// Every user's unlocks of the package (`<user>_<label>.txt`, lines of `<trophy id> <tick>`):
/// trophy id to the earliest time, in Unix microseconds.
fn unlocked(records: &Path, label: u32) -> HashMap<u32, i64> {
    let mut result: HashMap<u32, i64> = HashMap::new();
    for (key, tick) in pairs(records, label, RECORD_SUFFIX) {
        let Ok(id) = u32::try_from(key) else { continue };
        if tick < UNIX_EPOCH_TICK {
            continue;
        }
        let time = i64::try_from(tick - UNIX_EPOCH_TICK).unwrap_or(i64::MAX);
        result
            .entry(id)
            .and_modify(|t| *t = (*t).min(time))
            .or_insert(time);
    }
    result
}

/// The stats the engine counted (`<user>_<label>.stats.txt`, lines of `<stat id> <value>`): the
/// maximum across users.
fn stat_values(records: &Path, label: u32) -> HashMap<u64, u64> {
    let mut result: HashMap<u64, u64> = HashMap::new();
    for (key, value) in pairs(records, label, STATS_SUFFIX) {
        result
            .entry(key)
            .and_modify(|v| *v = (*v).max(value))
            .or_insert(value);
    }
    result
}

fn pairs(records: &Path, label: u32, suffix: &str) -> Vec<(u64, u64)> {
    let Ok(names) = fs::read_dir(records) else {
        return Vec::new();
    };
    let ending = format!("_{label}{suffix}");
    let mut result = Vec::new();
    for entry in names.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(user) = name.strip_suffix(&ending) else {
            continue;
        };
        if user.is_empty() || !user.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        // A file the engine is writing right now may be unreadable or not UTF-8: skip it.
        let Ok(text) = fs::read(entry.path())
            .map_err(|_| ())
            .and_then(|b| String::from_utf8(b).map_err(|_| ()))
        else {
            continue;
        };
        for line in text.split(['\n', '\r']) {
            let parts: Vec<&str> = line.split(' ').filter(|p| !p.is_empty()).collect();
            if let [key, value] = parts[..]
                && let (Ok(key), Ok(value)) = (key.parse::<u64>(), value.parse::<u64>())
            {
                result.push((key, value));
            }
        }
    }
    result
}

fn json(data: Option<&Vec<u8>>) -> Option<serde_json::Map<String, Value>> {
    let data = data?;
    let data = data.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(data);
    match serde_json::from_slice(data).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Ids and stat numbers come as `"0007"` strings or plain numbers.
fn unsigned(value: Option<&Value>) -> Option<u64> {
    match value? {
        Value::String(text) => text.parse().ok(),
        Value::Number(number) => number.as_u64(),
        _ => None,
    }
}

fn identifier(value: Option<&Value>) -> Option<u32> {
    unsigned(value)
        .filter(|id| *id <= i32::MAX as u64)
        .map(|id| id as u32)
}

/// Polls the running game's records once a second and reports earned trophies.
///
/// Sends [`CoreEvent::Trophies`] once at the start, then [`CoreEvent::TrophyUnlocked`] for each new
/// trophy followed by a fresh [`CoreEvent::Trophies`]. Dropping it stops the thread.
pub struct TrophyWatcher {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TrophyWatcher {
    pub fn start(
        game: Game,
        copy: PathBuf,
        languages: Vec<String>,
        tx: Sender<CoreEvent>,
        notify: Notifier,
    ) -> Self {
        Self::start_with_interval(game, copy, languages, tx, notify, Duration::from_secs(1))
    }

    pub fn start_with_interval(
        game: Game,
        copy: PathBuf,
        languages: Vec<String>,
        tx: Sender<CoreEvent>,
        notify: Notifier,
        interval: Duration,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = thread::Builder::new()
            .name("trophy-watch".into())
            .spawn(move || {
                let send = |event: CoreEvent| {
                    let _ = tx.send(event);
                    notify();
                };
                let mut set = Trophies::load(&game, Some(&copy), &languages);
                let mut known = Trophies::earned(&copy);
                send(CoreEvent::Trophies {
                    game: game.id.clone(),
                    set: set.clone(),
                });
                while !flag.load(Ordering::Relaxed) {
                    thread::sleep(interval);
                    let now = Trophies::earned(&copy);
                    if now.len() <= known.len() && now.is_subset(&known) {
                        continue;
                    }
                    let fresh = Trophies::load(&game, Some(&copy), &languages);
                    for id in now.difference(&known) {
                        let trophy = fresh
                            .as_ref()
                            .and_then(|s| s.trophies.iter().find(|t| &t.id() == id));
                        if let Some(trophy) = trophy {
                            send(CoreEvent::TrophyUnlocked {
                                game: game.id.clone(),
                                trophy: trophy.clone(),
                            });
                        }
                    }
                    if fresh != set {
                        send(CoreEvent::Trophies {
                            game: game.id.clone(),
                            set: fresh.clone(),
                        });
                        set = fresh;
                    }
                    known = now;
                }
            })
            .expect("spawn trophy-watch thread");
        TrophyWatcher {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for TrophyWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{library::ExecutableKind, paths::GameId};
    use serde_json::json;
    use std::{collections::BTreeMap, sync::mpsc};

    /// Writes the big-endian archive `read_ucp` reads.
    fn ucp(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let toc = 0x40usize;
        let table_end = toc + 0x20 + 0x40 * files.len();
        let mut out = vec![0u8; table_end];
        out[0..4].copy_from_slice(&0xB228_C60Au32.to_be_bytes());
        out[4..8].copy_from_slice(&1u32.to_be_bytes());
        out[0x10..0x14].copy_from_slice(&(files.len() as u32).to_be_bytes());
        out[0x14..0x18].copy_from_slice(&(toc as u32).to_be_bytes());
        let mut body = Vec::new();
        for (i, (name, data)) in files.iter().enumerate() {
            let entry = toc + 0x20 + 0x40 * i;
            out[entry..entry + name.len()].copy_from_slice(name.as_bytes());
            out[entry + 0x20..entry + 0x28]
                .copy_from_slice(&((table_end + body.len()) as u64).to_be_bytes());
            out[entry + 0x28..entry + 0x30].copy_from_slice(&(data.len() as u64).to_be_bytes());
            body.extend_from_slice(data);
        }
        out.extend(body);
        out
    }

    fn game(folder: &Path) -> Game {
        Game {
            id: GameId::from_path(folder),
            folder: folder.to_path_buf(),
            title_id: "PPSA1".into(),
            name: "Fallback Name".into(),
            names: BTreeMap::new(),
            version: "1.0".into(),
            content_id: None,
            icon: None,
            background: None,
            music: None,
            executable: ExecutableKind::Elf,
            size: None,
            unlocked_with: None,
        }
    }

    fn conf(defs: Value) -> Vec<u8> {
        json!({"defaultLanguage": "en-US", "trophies": defs})
            .to_string()
            .into_bytes()
    }

    fn meta(title: &str, texts: Value) -> Vec<u8> {
        json!({"metadata": {"titleMetadata": {"name": title}, "trophyMetadata": texts}})
            .to_string()
            .into_bytes()
    }

    fn package(defs: Value, texts: Value) -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("tropconf.json", conf(defs)),
            ("tropmeta_en-US.json", meta("Set Title", texts)),
            ("icon0.png", b"set-icon".to_vec()),
            ("trop0001.png", b"icon-1".to_vec()),
        ]
    }

    fn put_package(dump: &Path, label: u32, files: &[(&str, Vec<u8>)]) {
        let dir = dump.join("sce_sys/trophy2");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("trophy{label:02}.ucp")), ucp(files)).unwrap();
    }

    fn record(copy: &Path, name: &str, text: &str) {
        let dir = copy.join("_trophy");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(name), text).unwrap();
    }

    const TICK: u64 = UNIX_EPOCH_TICK;

    #[test]
    fn archive_rejects_bad_headers() {
        let good = ucp(&[("a", vec![1, 2, 3])]);
        assert_eq!(read_ucp(&good).unwrap()["a"], [1, 2, 3]);
        let mut bad = good.clone();
        bad[0] ^= 1;
        assert!(read_ucp(&bad).is_none(), "magic");
        let mut v2 = good.clone();
        v2[7] = 2;
        assert!(read_ucp(&v2).is_none(), "version");
        let mut many = good.clone();
        many[0x10..0x14].copy_from_slice(&5000u32.to_be_bytes());
        assert!(read_ucp(&many).is_none(), "count");
        let mut beyond = good.clone();
        let entry = 0x40 + 0x20;
        beyond[entry + 0x28..entry + 0x30].copy_from_slice(&9999u64.to_be_bytes());
        assert!(read_ucp(&beyond).is_none(), "entry beyond file");
        assert!(read_ucp(&good[..10]).is_none(), "truncated");
        assert!(read_ucp(&[]).is_none());
    }

    #[test]
    fn archive_later_entry_replaces_earlier() {
        let files = read_ucp(&ucp(&[("a", vec![1]), ("a", vec![2])])).unwrap();
        assert_eq!(files["a"], [2]);
    }

    #[test]
    fn loads_definitions_texts_icons_and_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let defs = json!([
            {"id": 0, "grade": "P"},
            {"id": 1, "grade": "B", "hidden": true},
            {"id": "0007", "grade": "G"},
            {"id": 8, "grade": "X"},
            {"grade": "S"}
        ]);
        let texts = json!([{"id": 0, "name": "All", "detail": "Get all"}, {"id": "0007", "name": "Seven"}, {"id": 1, "name": "One", "detail": "d"}]);
        put_package(dir.path(), 0, &package(defs, texts));
        let set = Trophies::load(&game(dir.path()), None, &[]).unwrap();
        assert_eq!(set.title, "Set Title");
        assert_eq!(set.icon.as_deref(), Some(&b"set-icon"[..]));
        assert_eq!(set.trophies.len(), 3);
        assert_eq!(set.trophies[0].name, "All");
        assert!(set.trophies[1].hidden && set.trophies[1].icon.as_deref() == Some(&b"icon-1"[..]));
        assert_eq!(
            (
                set.trophies[2].number,
                set.trophies[2].name.as_str(),
                set.trophies[2].detail.as_str()
            ),
            (7, "Seven", "")
        );
        assert!(set.trophies[2].icon.is_none());
        assert!(set.trophies.iter().all(|t| !t.is_unlocked()));
        assert_eq!(set.trophies[1].id(), "0/1");
    }

    #[test]
    fn no_package_or_empty_package_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(Trophies::load(&game(dir.path()), None, &[]).is_none());
        put_package(dir.path(), 0, &package(json!([]), json!([])));
        assert!(Trophies::load(&game(dir.path()), None, &[]).is_none());
    }

    #[test]
    fn two_packages_concatenate_and_labels_may_start_at_one() {
        let dir = tempfile::tempdir().unwrap();
        put_package(
            dir.path(),
            0,
            &package(json!([{"id": 0, "grade": "P"}]), json!([])),
        );
        put_package(
            dir.path(),
            1,
            &package(
                json!([{"id": 0, "grade": "B"}, {"id": 1, "grade": "B"}]),
                json!([]),
            ),
        );
        let set = Trophies::load(&game(dir.path()), None, &[]).unwrap();
        assert_eq!(
            set.trophies.iter().map(Trophy::id).collect::<Vec<_>>(),
            ["0/0", "1/0", "1/1"]
        );

        let from_one = tempfile::tempdir().unwrap();
        put_package(
            from_one.path(),
            1,
            &package(json!([{"id": 3, "grade": "S"}]), json!([])),
        );
        let set = Trophies::load(&game(from_one.path()), None, &[]).unwrap();
        assert_eq!(set.title, "Set Title");
        assert_eq!(set.trophies[0].id(), "1/3");
    }

    #[test]
    fn broken_package_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path().join("sce_sys/trophy2");
        fs::create_dir_all(&sys).unwrap();
        fs::write(sys.join("trophy00.ucp"), b"garbage").unwrap();
        put_package(
            dir.path(),
            1,
            &package(json!([{"id": 0, "grade": "G"}]), json!([])),
        );
        assert_eq!(
            Trophies::load(&game(dir.path()), None, &[])
                .unwrap()
                .trophies
                .len(),
            1
        );
    }

    #[test]
    fn tropconf_with_bom_is_read() {
        let mut files = package(json!([{"id": 2, "grade": "B"}]), json!([]));
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend(&files[0].1);
        files[0].1 = bom;
        let dir = tempfile::tempdir().unwrap();
        put_package(dir.path(), 0, &files);
        assert_eq!(
            Trophies::load(&game(dir.path()), None, &[])
                .unwrap()
                .trophies
                .len(),
            1
        );
    }

    #[test]
    fn language_selection_order() {
        let defs = json!([{"id": 0, "grade": "B"}]);
        let name_in = |lang: &str| {
            (
                format!("tropmeta_{lang}.json"),
                meta(lang, json!([{"id": 0, "name": lang}])),
            )
        };
        let build = |langs: &[&str], default: &str, wanted: &[&str]| {
            let dir = tempfile::tempdir().unwrap();
            let mut owned: Vec<(String, Vec<u8>)> = langs.iter().map(|l| name_in(l)).collect();
            owned.push((
                "tropconf.json".into(),
                json!({"defaultLanguage": default, "trophies": defs})
                    .to_string()
                    .into_bytes(),
            ));
            let files: Vec<(&str, Vec<u8>)> =
                owned.iter().map(|(n, d)| (n.as_str(), d.clone())).collect();
            put_package(dir.path(), 0, &files);
            let wanted: Vec<String> = wanted.iter().map(|s| s.to_string()).collect();
            Trophies::load(&game(dir.path()), None, &wanted)
                .unwrap()
                .trophies[0]
                .name
                .clone()
        };
        assert_eq!(
            build(&["de-DE", "en-US"], "en-US", &["de-DE"]),
            "de-DE",
            "requested first"
        );
        assert_eq!(
            build(&["de-DE", "en-US", "en-GB"], "de-DE", &["fr-FR"]),
            "en-US",
            "en-US next"
        );
        assert_eq!(
            build(&["de-DE", "en-GB"], "de-DE", &[]),
            "en-GB",
            "en-GB next"
        );
        assert_eq!(
            build(&["de-DE", "tr-TR"], "tr-TR", &[]),
            "tr-TR",
            "package default next"
        );
        assert_eq!(
            build(&["ru-RU", "ja-JP"], "xx-XX", &[]),
            "ja-JP",
            "alphabetical last"
        );
    }

    #[test]
    fn progress_is_clamped_and_needs_target_above_one() {
        let dir = tempfile::tempdir().unwrap();
        let defs = json!([
            {"id": 0, "grade": "B", "unlockCondition": {"progressive": true, "udsStatId": 5, "targetValue": 10}},
            {"id": 1, "grade": "B", "unlockCondition": {"progressive": true, "udsStatId": "5", "targetValue": "1"}},
            {"id": 2, "grade": "B", "unlockCondition": {"progressive": false, "udsStatId": 5, "targetValue": 10}},
            {"id": 3, "grade": "B", "unlockCondition": {"progressive": true, "udsStatId": 6, "targetValue": "20"}}
        ]);
        put_package(dir.path(), 0, &package(defs, json!([])));
        let copy = dir.path().join("copy");
        record(&copy, "100_0.stats.txt", "5 14\n6 3\n");
        record(&copy, "200_0.stats.txt", "5 9");
        let set = Trophies::load(&game(dir.path()), Some(&copy), &[]).unwrap();
        assert_eq!(set.trophies[0].progress, Some((10, 10)));
        assert_eq!(set.trophies[1].progress, None);
        assert_eq!(set.trophies[2].progress, None);
        assert_eq!(set.trophies[3].progress, Some((3, 20)));
    }

    #[test]
    fn unlocks_earliest_user_wins_and_bad_lines_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let copy = dir.path();
        let t = |secs: u64| TICK + secs * 1_000_000;
        record(
            copy,
            "100_0.txt",
            &format!(
                "1 {}\r\n2 {}\n\nbad line here\n3\n4 {}\n",
                t(2000),
                t(50),
                TICK - 1
            ),
        );
        record(copy, "200_0.txt", &format!("1 {}\n", t(1000) + 5));
        record(copy, "300_0.stats.txt", &format!("1 {}\n", t(1)));
        record(copy, "abc_0.txt", &format!("9 {}\n", t(1)));
        record(copy, "100_1.txt", &format!("1 {}\n", t(7)));
        let u = unlocked(&copy.join("_trophy"), 0);
        assert_eq!(u.len(), 2);
        assert_eq!(u[&1], 1_000_000_000 + 5);
        assert_eq!(u[&2], 50_000_000);
        assert!(!u.contains_key(&4), "tick below the epoch");
        assert!(!u.contains_key(&9), "non-numeric user");
        assert_eq!(unlocked(&copy.join("_trophy"), 1)[&1], 7_000_000);
    }

    #[test]
    fn unreadable_record_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("_trophy");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("1_0.txt"), [0xFF, 0xFE, 0xFD]).unwrap(); // not UTF-8
        fs::create_dir(folder.join("2_0.txt")).unwrap(); // cannot be read as a file
        fs::write(folder.join("3_0.txt"), format!("5 {}", TICK + 1)).unwrap();
        assert_eq!(unlocked(&folder, 0).len(), 1);
    }

    #[test]
    fn load_marks_unlocked_and_earned_returns_ids() {
        let dir = tempfile::tempdir().unwrap();
        put_package(
            dir.path(),
            0,
            &package(
                json!([{"id": 0, "grade": "P"}, {"id": 1, "grade": "B"}, {"id": 7, "grade": "S"}]),
                json!([]),
            ),
        );
        put_package(
            dir.path(),
            1,
            &package(json!([{"id": 4, "grade": "G"}]), json!([])),
        );
        let copy = dir.path().join("copy");
        record(&copy, "100_0.txt", &format!("7 {}\n", TICK + 3_000_000));
        record(&copy, "100_1.txt", &format!("4 {}\n", TICK + 9_000_000));
        record(&copy, "100_0.stats.txt", "1 2\n");
        let earned = Trophies::earned(&copy);
        assert_eq!(
            earned,
            HashSet::from(["0/7".to_string(), "1/4".to_string()])
        );
        let set = Trophies::load(&game(dir.path()), Some(&copy), &[]).unwrap();
        assert_eq!(set.unlocked_count(), 2);
        let seven = set.trophies.iter().find(|t| t.id() == "0/7").unwrap();
        assert_eq!(seven.unlocked, Some(3_000_000));
        assert_eq!(seven.unlocked_secs(), Some(3));
        assert!(Trophies::earned(&dir.path().join("nothing")).is_empty());
    }

    #[test]
    fn earned_records_hold_the_first_set_without_texts() {
        let dir = tempfile::tempdir().unwrap();
        put_package(
            dir.path(),
            0,
            &package(
                json!([{"id": 0, "grade": "P"}, {"id": 1, "grade": "B"}, {"id": 7, "grade": "S"}]),
                json!([{"id": 7, "name": "Seven", "detail": "Earn it"}]),
            ),
        );
        put_package(
            dir.path(),
            1,
            &package(json!([{"id": 4, "grade": "G"}]), json!([])),
        );
        let copy = dir.path().join("copy");
        record(
            &copy,
            "100_0.txt",
            &format!("7 {}\n0 {}\n", TICK + 3_000_000, TICK + 1),
        );
        record(&copy, "100_1.txt", &format!("4 {}\n", TICK + 9_000_000));
        let records = Trophies::earned_records(&game(dir.path()), &copy);
        assert_eq!(
            records,
            vec![
                Earned {
                    number: 0,
                    grade: Grade::Platinum,
                    unlocked: 1,
                },
                Earned {
                    number: 7,
                    grade: Grade::Silver,
                    unlocked: 3_000_000,
                },
            ]
        );
        let set = Trophies::load(&game(dir.path()), Some(&copy), &[]).unwrap();
        assert_eq!(set.earned(), records);
        let nothing = dir.path().join("nothing");
        assert!(Trophies::earned_records(&game(dir.path()), &nothing).is_empty());
    }

    fn trophy(grade: Grade, unlocked: bool) -> Trophy {
        Trophy {
            service_label: 0,
            number: 0,
            grade,
            hidden: false,
            name: String::new(),
            detail: String::new(),
            icon: None,
            unlocked: unlocked.then_some(1),
            progress: None,
        }
    }

    fn set_of(trophies: Vec<Trophy>) -> TrophySet {
        TrophySet {
            title: String::new(),
            icon: None,
            trophies,
        }
    }

    #[test]
    fn percent_and_counts() {
        let bronze = set_of(vec![
            trophy(Grade::Bronze, true),
            trophy(Grade::Bronze, false),
            trophy(Grade::Bronze, false),
            trophy(Grade::Bronze, false),
        ]);
        assert_eq!(bronze.percent(), 25);
        let mixed = set_of(vec![
            trophy(Grade::Platinum, false),
            trophy(Grade::Gold, true),
            trophy(Grade::Silver, false),
            trophy(Grade::Bronze, true),
        ]);
        assert_eq!(mixed.percent(), 24);
        assert_eq!(mixed.count(Grade::Gold), (1, 1));
        assert_eq!(mixed.count(Grade::Silver), (0, 1));
        assert_eq!(set_of(vec![]).percent(), 0);
        assert_eq!(Grade::from_letter("G"), Some(Grade::Gold));
        assert_eq!(Grade::from_letter("GG"), None);
        assert_eq!(Grade::from_letter(""), None);
    }

    #[test]
    fn unicode_names_pass_through() {
        let dir = tempfile::tempdir().unwrap();
        put_package(
            dir.path(),
            0,
            &package(
                json!([{"id": 0, "grade": "B"}]),
                json!([{"id": 0, "name": "Größe 日本", "detail": "Ünïcode ✓"}]),
            ),
        );
        let set = Trophies::load(&game(dir.path()), None, &[]).unwrap();
        assert_eq!(
            (
                set.trophies[0].name.as_str(),
                set.trophies[0].detail.as_str()
            ),
            ("Größe 日本", "Ünïcode ✓")
        );
    }

    #[test]
    fn watcher_reports_new_trophies() {
        let dir = tempfile::tempdir().unwrap();
        put_package(
            dir.path(),
            0,
            &package(
                json!([{"id": 0, "grade": "P"}, {"id": 1, "grade": "B"}]),
                json!([{"id": 1, "name": "One"}]),
            ),
        );
        let copy = dir.path().join("copy");
        fs::create_dir_all(&copy).unwrap();
        let (tx, rx) = mpsc::channel();
        let _watcher = TrophyWatcher::start_with_interval(
            game(dir.path()),
            copy.clone(),
            vec![],
            tx,
            Arc::new(|| {}),
            Duration::from_millis(30),
        );
        let first = rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert!(
            matches!(first, CoreEvent::Trophies { set: Some(ref s), .. } if s.unlocked_count() == 0)
        );
        record(&copy, "100_0.txt", &format!("1 {}\n", TICK + 5));
        let mut got_unlock = false;
        let mut got_set = false;
        while !(got_unlock && got_set) {
            match rx.recv_timeout(Duration::from_secs(3)).unwrap() {
                CoreEvent::TrophyUnlocked { trophy, .. } => {
                    assert_eq!((trophy.id().as_str(), trophy.name.as_str()), ("0/1", "One"));
                    got_unlock = true;
                }
                CoreEvent::Trophies { set: Some(s), .. } => {
                    assert_eq!(s.unlocked_count(), 1);
                    got_set = true;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    #[test]
    fn watcher_does_not_replay_old_unlocks() {
        let dir = tempfile::tempdir().unwrap();
        put_package(
            dir.path(),
            0,
            &package(json!([{"id": 1, "grade": "B"}]), json!([])),
        );
        let copy = dir.path().join("copy");
        record(&copy, "100_0.txt", &format!("1 {}\n", TICK + 5));
        let (tx, rx) = mpsc::channel();
        let _watcher = TrophyWatcher::start_with_interval(
            game(dir.path()),
            copy,
            vec![],
            tx,
            Arc::new(|| {}),
            Duration::from_millis(30),
        );
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            CoreEvent::Trophies { .. }
        ));
        assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    }
}
