//! The Captures area's data (docs/DESIGN.md 9.10): the list of the files in the screenshots
//! folder, read on a thread of its own, and the pictures of them ([`images`]).
//!
//! The list is read when the area opens and when the player asks again; the area never writes to
//! the folder, only [`delete`](anyps5_core::captures::delete) after the player confirmed it.

pub mod images;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use anyps5_core::captures::{self as core, Capture, Owner, ScanError};
use anyps5_core::clips::safe_name;
use anyps5_core::library::Game;
use egui::Context;

use crate::screens::captures::{Load, Section};
use crate::ui::chrome::TileImage;
pub use images::Images;

pub struct CaptureService {
    pub images: Images,
    ctx: Context,
    folder: PathBuf,
    load: Load,
    captures: Vec<Capture>,
    tx: Sender<(u64, Result<Vec<Capture>, ScanError>)>,
    rx: Receiver<(u64, Result<Vec<Capture>, ScanError>)>,
    /// The scan whose answer counts; an older one that arrives late is dropped.
    generation: u64,
    /// Counts every change of the list, so what is made from it can be kept until it changes.
    revision: u64,
    grouped: Option<((u64, core::Filter, u64), Grouped)>,
}

impl CaptureService {
    pub fn new(ctx: &Context) -> CaptureService {
        let (tx, rx) = channel();
        CaptureService {
            images: Images::new(ctx),
            ctx: ctx.clone(),
            folder: PathBuf::new(),
            load: Load::Loading,
            captures: Vec::new(),
            tx,
            rx,
            generation: 0,
            revision: 0,
            grouped: None,
        }
    }

    /// The area opens: forget the old list and read `folder`.
    pub fn begin(&mut self, folder: &Path) {
        self.captures.clear();
        self.load = Load::Loading;
        self.scan(folder);
    }

    /// Read again; what is on screen stays until the answer comes (the retry of a failure shows
    /// the loading state).
    pub fn refresh(&mut self, folder: &Path) {
        if self.load != Load::Ready {
            self.load = Load::Loading;
        }
        self.scan(folder);
    }

    fn scan(&mut self, folder: &Path) {
        self.folder = folder.to_path_buf();
        self.generation += 1;
        let (generation, folder, tx, ctx) = (
            self.generation,
            folder.to_path_buf(),
            self.tx.clone(),
            self.ctx.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("captures-scan".into())
            .spawn(move || {
                let _ = tx.send((generation, core::scan(&folder)));
                ctx.request_repaint();
            });
        if spawned.is_err() {
            self.load = Load::Failed;
        }
    }

    /// Takes in a finished scan. Call once per frame.
    pub fn poll(&mut self) {
        while let Ok((generation, result)) = self.rx.try_recv() {
            if generation != self.generation {
                continue;
            }
            match result {
                Ok(list) => {
                    self.captures = list;
                    self.load = Load::Ready;
                    self.revision += 1;
                }
                Err(ScanError::Missing) => {
                    self.captures.clear();
                    self.load = Load::Missing;
                }
                Err(ScanError::Unreadable) => {
                    self.captures.clear();
                    self.load = Load::Failed;
                }
            }
        }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn load(&self) -> Load {
        self.load
    }

    pub fn captures(&self) -> &[Capture] {
        &self.captures
    }

    /// Everything one frame of the area needs from the service at once: the list, how it is
    /// grouped for `filter`, the pictures and where the reading stands. The grouping is made
    /// again only when the list, the filter or the library changed, not on every frame.
    pub fn frame(
        &mut self,
        games: &[Game],
        filter: core::Filter,
    ) -> (&[Capture], &Grouped, &mut Images, Load) {
        let key = (self.revision, filter, fingerprint(games));
        if self.grouped.as_ref().is_none_or(|(have, _)| *have != key) {
            let grouped = group_by_game(games, &self.captures, filter);
            self.grouped = Some((key, grouped));
        }
        let grouped = &self.grouped.as_ref().expect("made above").1;
        (&self.captures, grouped, &mut self.images, self.load)
    }

    /// A capture was deleted: it leaves the list now, without reading the folder again.
    pub fn remove(&mut self, path: &Path) {
        self.captures.retain(|c| c.path != path);
        self.revision += 1;
        self.images.forget(path);
    }

    /// The area is closed: the textures go, the list is read again next time.
    pub fn release(&mut self) {
        self.captures = Vec::new();
        self.revision += 1;
        self.grouped = None;
        self.images.clear();
    }

    /// Previews and tests: a list that is already read.
    pub fn show(&mut self, folder: &Path, captures: Vec<Capture>, load: Load) {
        self.folder = folder.to_path_buf();
        self.captures = captures;
        self.load = load;
        self.generation += 1;
        self.revision += 1;
    }
}

/// Who a capture belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Who {
    /// A game of the library, by its place there.
    Game(usize),
    /// A game the library does not have, by the name the file gives it.
    Named(String),
    Other,
}

/// The games' title IDs and file-safe names, looked up in one step each. Matches a capture to the
/// library: a screenshot by the title ID in its name, a clip by the game name in its name. Nothing
/// here knows one title from another.
struct Owners {
    by_title: HashMap<String, usize>,
    by_name: HashMap<String, usize>,
}

impl Owners {
    fn new(games: &[Game]) -> Owners {
        let mut by_title = HashMap::new();
        let mut by_name = HashMap::new();
        for (i, game) in games.iter().enumerate() {
            by_title
                .entry(game.title_id.to_ascii_lowercase())
                .or_insert(i);
            by_name
                .entry(safe_name(&game.name).to_lowercase())
                .or_insert(i);
        }
        Owners { by_title, by_name }
    }

    fn who(&self, capture: &Capture) -> Who {
        match &capture.owner {
            Owner::Title(id) => self
                .by_title
                .get(&id.to_ascii_lowercase())
                .map_or_else(|| Who::Named(id.clone()), |i| Who::Game(*i)),
            Owner::Name(name) => self
                .by_name
                .get(&name.to_lowercase())
                .map_or_else(|| Who::Named(name.clone()), |i| Who::Game(*i)),
            Owner::Unknown => Who::Other,
        }
    }
}

/// The captures a filter lets through, grouped by game in the order of their newest capture. The
/// members are positions in the list.
pub type Grouped = Vec<(Who, Vec<usize>)>;

pub fn group_by_game(games: &[Game], list: &[Capture], filter: core::Filter) -> Grouped {
    let owners = Owners::new(games);
    core::group(list, filter, |c| owners.who(c))
}

/// The groups as the screen shows them: the game's own name and icon from the library.
pub fn sections(
    games: &[Game],
    grouped: &[(Who, Vec<usize>)],
    other: &str,
    mut icon_of: impl FnMut(&Game) -> Option<TileImage>,
) -> Vec<Section> {
    let mut icons: HashMap<usize, Option<TileImage>> = HashMap::new();
    grouped
        .iter()
        .map(|(who, items)| match who {
            Who::Game(i) => Section {
                title: games[*i].name.clone(),
                icon: *icons.entry(*i).or_insert_with(|| icon_of(&games[*i])),
                items: items.clone(),
            },
            Who::Named(name) => Section {
                title: name.clone(),
                icon: None,
                items: items.clone(),
            },
            Who::Other => Section {
                title: other.to_owned(),
                icon: None,
                items: items.clone(),
            },
        })
        .collect()
}

/// What changes the grouping besides the list and the filter: the library.
fn fingerprint(games: &[Game]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for game in games {
        game.title_id.hash(&mut hasher);
        game.name.hash(&mut hasher);
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(id: &str, name: &str) -> Game {
        Game {
            id: anyps5_core::paths::GameId::from_path(Path::new(&format!("/games/{id}"))),
            folder: PathBuf::from(format!("/games/{id}")),
            title_id: id.to_owned(),
            name: name.to_owned(),
            names: Default::default(),
            version: "01.00".into(),
            content_id: None,
            icon: None,
            background: None,
            music: None,
            executable: anyps5_core::library::ExecutableKind::Elf,
            size: None,
            unlocked_with: None,
        }
    }

    fn capture(name: &str, modified: i64) -> Capture {
        let (kind, owner) = core::classify(name).unwrap();
        Capture {
            path: PathBuf::from(name),
            kind,
            owner,
            modified,
            size: 1,
        }
    }

    #[test]
    fn screenshots_match_by_title_id_and_clips_by_name_for_every_game_alike() {
        let games = [
            game("PPSA00001", "First: Game"),
            game("PPSA00002", "Second"),
        ];
        let list = [
            capture("Second 2026-10-25 18-04-31.mp4", 50),
            capture("PPSA00001-20261025-180431.png", 40),
            // The colon went out of the file name, as the recorder writes it.
            capture("First Game 2026-10-25 18-04-31.mp4", 30),
            capture("PPSA77777-20261025-180431.png", 20),
            capture("holiday.png", 10),
        ];
        let grouped = group_by_game(&games, &list, core::Filter::All);
        let secs = sections(&games, &grouped, "Other", |_| None);
        let titles: Vec<_> = secs
            .iter()
            .map(|s| (s.title.as_str(), s.items.clone()))
            .collect();
        assert_eq!(
            titles,
            [
                ("Second", vec![0]),
                ("First: Game", vec![1, 2]),
                ("PPSA77777", vec![3]),
                ("Other", vec![4]),
            ]
        );
        let grouped = group_by_game(&games, &list, core::Filter::Clips);
        let clips = sections(&games, &grouped, "Other", |_| None);
        assert_eq!(clips.len(), 2);
    }
}
