//! Which theme song plays (docs/DESIGN.md section 7.3): the selected game's own, once the
//! selection has rested on it for a moment, as a console does. Songs are read on a thread of
//! their own and the last two are kept, so moving back and forth between two games never reads
//! a file twice.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use anyps5_core::theme_song::{self, ThemeError, ThemeSong};

/// How long the selection rests on a game before its song starts.
pub const REST: Duration = Duration::from_millis(1_000);
/// Songs kept after they played.
const KEPT: usize = 2;

type Loaded = Receiver<Result<ThemeSong, ThemeError>>;

/// What the mixer should do now.
#[derive(Debug)]
pub enum Action {
    Play(Arc<ThemeSong>),
    Stop,
}

pub struct Picker {
    wanted: Option<PathBuf>,
    since: Instant,
    playing: Option<PathBuf>,
    loading: Option<(PathBuf, Loaded)>,
    kept: VecDeque<(PathBuf, Arc<ThemeSong>)>,
    /// Songs that would not read or decode; they are not tried again.
    failed: HashSet<PathBuf>,
    load: fn(PathBuf) -> Loaded,
}

impl Default for Picker {
    fn default() -> Self {
        Self::new(load_on_thread)
    }
}

/// Reads a song on a short-lived thread.
fn load_on_thread(path: PathBuf) -> Loaded {
    let (tx, rx) = channel();
    let spawned = std::thread::Builder::new()
        .name("theme song".into())
        .spawn(move || {
            let _ = tx.send(theme_song::load(&path));
        });
    if let Err(err) = spawned {
        tracing::warn!("cannot start the theme song thread: {err}");
    }
    rx
}

impl Picker {
    fn new(load: fn(PathBuf) -> Loaded) -> Self {
        Self {
            wanted: None,
            since: Instant::now(),
            playing: None,
            loading: None,
            kept: VecDeque::new(),
            failed: HashSet::new(),
            load,
        }
    }

    /// Called every frame with the song of the selected game, or `None` when no theme should
    /// play (sounds off, a game running, another screen in front).
    pub fn update(&mut self, wanted: Option<&Path>, now: Instant) -> Option<Action> {
        if self.wanted.as_deref() != wanted {
            self.wanted = wanted.map(Path::to_path_buf);
            self.since = now;
            if self.playing.take().is_some() {
                return Some(Action::Stop);
            }
        }
        self.receive();
        let path = self.wanted.clone()?;
        if self.playing.as_ref() == Some(&path)
            || self.failed.contains(&path)
            || now.duration_since(self.since) < REST
        {
            return None;
        }
        if let Some((_, song)) = self.kept.iter().find(|(kept, _)| *kept == path) {
            let song = song.clone();
            self.playing = Some(path);
            return Some(Action::Play(song));
        }
        // A song still loading for another game is left to finish and kept; this one starts now.
        if self
            .loading
            .as_ref()
            .is_none_or(|(loading, _)| *loading != path)
        {
            self.loading = Some((path.clone(), (self.load)(path)));
        }
        None
    }

    fn receive(&mut self) {
        let Some((path, rx)) = &self.loading else {
            return;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(ThemeError::Format("the reader stopped")),
        };
        let path = path.clone();
        self.loading = None;
        match result {
            Ok(song) => {
                if self.kept.len() == KEPT {
                    self.kept.pop_front();
                }
                self.kept.push_back((path, Arc::new(song)));
            }
            Err(err) => {
                tracing::info!("no theme song from {}: {err}", path.display());
                self.failed.insert(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_song() -> ThemeSong {
        theme_song::tests_support::silent_song()
    }

    /// Answers at once: a song for paths ending in `.at9`, an error for others.
    fn instant(path: PathBuf) -> Loaded {
        let (tx, rx) = channel();
        let result = if path.extension().is_some_and(|e| e == "at9") {
            Ok(fake_song())
        } else {
            Err(ThemeError::Format("not a song"))
        };
        tx.send(result).unwrap();
        rx
    }

    fn picker() -> Picker {
        Picker::new(instant)
    }

    #[test]
    fn a_song_starts_only_after_the_selection_rests() {
        let mut p = picker();
        let t0 = Instant::now();
        let a = Path::new("a/snd0.at9");
        assert!(p.update(Some(a), t0).is_none());
        assert!(p.update(Some(a), t0 + REST / 2).is_none());
        // Loading starts once rested; the next frame has it.
        assert!(p.update(Some(a), t0 + REST).is_none());
        assert!(matches!(
            p.update(Some(a), t0 + REST + Duration::from_millis(16)),
            Some(Action::Play(_))
        ));
        // Playing: nothing more to do.
        assert!(p.update(Some(a), t0 + REST * 3).is_none());
    }

    #[test]
    fn moving_on_stops_at_once_and_waits_again() {
        let mut p = picker();
        let t0 = Instant::now();
        let (a, b) = (Path::new("a/snd0.at9"), Path::new("b/snd0.at9"));
        p.update(Some(a), t0);
        p.update(Some(a), t0 + REST);
        assert!(matches!(
            p.update(Some(a), t0 + REST * 2),
            Some(Action::Play(_))
        ));
        let t1 = t0 + REST * 3;
        assert!(matches!(p.update(Some(b), t1), Some(Action::Stop)));
        assert!(p.update(Some(b), t1 + REST / 2).is_none());
        p.update(Some(b), t1 + REST);
        assert!(matches!(
            p.update(Some(b), t1 + REST * 2),
            Some(Action::Play(_))
        ));
        // Nothing selected: the theme stops.
        assert!(matches!(p.update(None, t1 + REST * 3), Some(Action::Stop)));
        assert!(p.update(None, t1 + REST * 9).is_none());
    }

    #[test]
    fn a_kept_song_plays_again_without_reading_the_file() {
        fn never(_: PathBuf) -> Loaded {
            panic!("read twice")
        }
        let mut p = picker();
        let t0 = Instant::now();
        let a = Path::new("a/snd0.at9");
        p.update(Some(a), t0);
        p.update(Some(a), t0 + REST);
        p.update(Some(a), t0 + REST * 2);
        p.update(None, t0 + REST * 3);
        p.load = never;
        p.update(Some(a), t0 + REST * 4);
        assert!(matches!(
            p.update(Some(a), t0 + REST * 5),
            Some(Action::Play(_))
        ));
    }

    #[test]
    fn a_broken_song_is_tried_once() {
        let mut p = picker();
        let t0 = Instant::now();
        let bad = Path::new("a/snd0.wav");
        p.update(Some(bad), t0);
        p.update(Some(bad), t0 + REST);
        assert!(p.update(Some(bad), t0 + REST * 2).is_none());
        assert!(p.failed.contains(bad));
        assert!(p.loading.is_none());
        assert!(p.update(Some(bad), t0 + REST * 3).is_none());
        assert!(p.loading.is_none());
    }

    #[test]
    fn only_two_songs_are_kept() {
        let mut p = picker();
        let mut t = Instant::now();
        for name in ["a", "b", "c"] {
            let path = PathBuf::from(format!("{name}/snd0.at9"));
            for _ in 0..3 {
                p.update(Some(&path), t);
                t += REST;
            }
        }
        assert_eq!(p.kept.len(), KEPT);
        assert!(p.kept.iter().all(|(path, _)| !path.starts_with("a")));
    }
}
