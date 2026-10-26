//! The pictures of the Captures area: thumbnails of screenshots and clips and the big picture of
//! one screenshot, decoded on worker threads and kept in a cache with a size cap.
//!
//! What it promises:
//!
//! * Nothing is decoded on the interface thread. A frame asks for the pictures it draws; the asks
//!   of the *last* frame are the queue, newest ask first, so a tile that was scrolled past while
//!   it waited is dropped instead of decoded.
//! * Three workers at most, whatever the library holds.
//! * Textures stay below [`CACHE_BYTES`]; when more comes in, the ones not drawn for the longest
//!   time go first. One that is drawn right now is never taken.
//! * A file that does not decode is asked for once. The tile shows without a picture.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use egui::{ColorImage, Context, TextureHandle, TextureOptions};

use crate::screens::captures::Thumb;
use crate::ui::chrome::TileImage;

/// What the textures may take together.
pub const CACHE_BYTES: usize = 160 * 1024 * 1024;
/// The longer side of a thumbnail, in pixels.
const THUMB_SIDE: u32 = 512;
/// The longer side of the big picture.
const FULL_SIDE: u32 = 2560;
/// The longer side of a clip's picture in the full view.
const FULL_CLIP_SIDE: u32 = 1024;
/// Workers for pictures, and one more for the system's clip thumbnails, which can hang on a bad
/// file or a slow drive and must not hold up the screenshots.
const IMAGE_WORKERS: usize = 2;
/// Pictures bigger than this many pixels are not opened (a decompression bomb, not a screenshot).
const MAX_PIXELS: u64 = 64_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A screenshot, small.
    Thumb,
    /// A screenshot, big.
    Full,
    /// A clip: the picture the system has for it, and its length.
    Clip,
    /// A clip, big, for the full view.
    ClipFull,
}

impl Kind {
    fn is_clip(self) -> bool {
        matches!(self, Kind::Clip | Kind::ClipFull)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lane {
    Pictures,
    Clips,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub path: PathBuf,
    pub kind: Kind,
}

enum Entry {
    /// Asked for, in the queue.
    Queued,
    /// A worker has it.
    Working,
    Ready {
        texture: TextureHandle,
        aspect: f32,
        bytes: usize,
        used: u64,
    },
    /// No picture, and none to come.
    Failed,
}

struct Done {
    key: Key,
    image: Option<ColorImage>,
    duration: Option<Duration>,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Key>,
    clip_jobs: VecDeque<Key>,
    /// Taken by a worker and not yet delivered.
    in_flight: HashSet<Key>,
    closed: bool,
}

struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
    /// The longer side of the big picture: [`FULL_SIDE`], or what the graphics card takes.
    full_side: AtomicU32,
}

pub struct Images {
    ctx: Context,
    shared: Arc<Shared>,
    rx: Receiver<Done>,
    entries: HashMap<Key, Entry>,
    durations: HashMap<PathBuf, Option<Duration>>,
    /// The asks of this frame, in order.
    asked: Vec<Key>,
    frame: u64,
    bytes: usize,
    /// What the textures may take together.
    cap: usize,
    copy_rx: Receiver<Option<ColorImage>>,
    copy_tx: Sender<Option<ColorImage>>,
}

impl Images {
    pub fn new(ctx: &Context) -> Images {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            full_side: AtomicU32::new(FULL_SIDE.min(2048)),
        });
        let (tx, rx) = channel();
        for n in 0..=IMAGE_WORKERS {
            let lane = if n == IMAGE_WORKERS {
                Lane::Clips
            } else {
                Lane::Pictures
            };
            let (shared, tx, ctx) = (shared.clone(), tx.clone(), ctx.clone());
            let spawned = std::thread::Builder::new()
                .name(format!("capture-images-{n}"))
                .spawn(move || work(&shared, &tx, &ctx, lane));
            if let Err(error) = spawned {
                tracing::warn!("captures: a decoder could not start: {error}");
            }
        }
        let (copy_tx, copy_rx) = channel();
        Images {
            ctx: ctx.clone(),
            shared,
            rx,
            entries: HashMap::new(),
            durations: HashMap::new(),
            asked: Vec::new(),
            frame: 1,
            bytes: 0,
            cap: CACHE_BYTES,
            copy_rx,
            copy_tx,
        }
    }

    /// Uploads what the workers finished. Call once per frame, before drawing.
    pub fn poll(&mut self) {
        let side = self.ctx.input(|i| i.max_texture_side) as u32;
        self.shared
            .full_side
            .store(side.clamp(1024, FULL_SIDE), Ordering::Relaxed);
        while let Ok(done) = self.rx.try_recv() {
            self.shared
                .queue
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .in_flight
                .remove(&done.key);
            if done.key.kind.is_clip() {
                self.durations.insert(done.key.path.clone(), done.duration);
            }
            let entry = match done.image {
                Some(image) => {
                    let aspect = image.size[0] as f32 / image.size[1].max(1) as f32;
                    let bytes = image.pixels.len() * 4;
                    self.bytes += bytes;
                    let name = format!("capture/{:?}/{}", done.key.kind, done.key.path.display());
                    Entry::Ready {
                        texture: self.ctx.load_texture(name, image, TextureOptions::LINEAR),
                        aspect,
                        bytes,
                        used: self.frame,
                    }
                }
                None => Entry::Failed,
            };
            self.entries.insert(done.key, entry);
            self.ctx.request_repaint();
        }
    }

    /// Ends the frame: what was asked for becomes the queue, what nobody asks for any more leaves
    /// it, and the cache is brought under its cap.
    pub fn end_frame(&mut self) {
        let asked = std::mem::take(&mut self.asked);
        {
            let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            let mut jobs = VecDeque::new();
            let mut clip_jobs = VecDeque::new();
            for key in &asked {
                if matches!(self.entries.get(key), Some(Entry::Queued)) {
                    let lane = if key.kind.is_clip() {
                        &mut clip_jobs
                    } else {
                        &mut jobs
                    };
                    if queue.in_flight.contains(key) {
                        self.entries.insert(key.clone(), Entry::Working);
                    } else if !lane.contains(key) {
                        lane.push_back(key.clone());
                    }
                }
            }
            queue.jobs = jobs;
            queue.clip_jobs = clip_jobs;
        }
        // Queued entries nobody asked for this frame were scrolled past: forget them.
        self.entries
            .retain(|key, entry| !matches!(entry, Entry::Queued) || asked.contains(key));
        self.shared.wake.notify_all();
        self.evict();
        self.frame += 1;
    }

    fn evict(&mut self) {
        if self.bytes <= self.cap {
            return;
        }
        let mut idle: Vec<(u64, Key)> = self
            .entries
            .iter()
            .filter_map(|(key, entry)| match entry {
                Entry::Ready { used, .. } if *used + 1 < self.frame => Some((*used, key.clone())),
                _ => None,
            })
            .collect();
        idle.sort_by_key(|(used, _)| *used);
        for (_, key) in idle {
            if self.bytes <= self.cap {
                break;
            }
            if let Some(Entry::Ready { bytes, .. }) = self.entries.remove(&key) {
                self.bytes -= bytes;
            }
        }
    }

    /// The picture for `key`, asking for it when it is not there.
    pub fn get(&mut self, key: Key) -> Thumb {
        match self.entries.get_mut(&key) {
            Some(Entry::Ready {
                texture,
                aspect,
                used,
                ..
            }) => {
                *used = self.frame;
                Thumb::Ready(TileImage {
                    texture: texture.id(),
                    aspect: *aspect,
                })
            }
            Some(Entry::Failed) => Thumb::Unavailable,
            Some(Entry::Queued | Entry::Working) => {
                self.asked.push(key);
                Thumb::Pending
            }
            None => {
                self.entries.insert(key.clone(), Entry::Queued);
                self.asked.push(key);
                Thumb::Pending
            }
        }
    }

    /// The length of a clip, once a worker has read it.
    pub fn duration(&self, path: &Path) -> Option<Duration> {
        self.durations.get(path).copied().flatten()
    }

    /// Forgets everything about a file that is gone.
    pub fn forget(&mut self, path: &Path) {
        for kind in [Kind::Thumb, Kind::Full, Kind::Clip, Kind::ClipFull] {
            let key = Key {
                path: path.to_path_buf(),
                kind,
            };
            if let Some(Entry::Ready { bytes, .. }) = self.entries.remove(&key) {
                self.bytes -= bytes;
            }
        }
        self.durations.remove(path);
    }

    /// Lets go of every texture (the area was closed).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.asked.clear();
        self.bytes = 0;
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.jobs.clear();
        queue.clip_jobs.clear();
    }

    /// How much the textures hold, in bytes.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Decodes a screenshot for the clipboard on a thread of its own; [`Images::copied`] has it.
    pub fn start_copy(&self, path: PathBuf) {
        let (tx, ctx) = (self.copy_tx.clone(), self.ctx.clone());
        let spawned = std::thread::Builder::new()
            .name("capture-copy".into())
            .spawn(move || {
                let _ = tx.send(decode(&path, 8192));
                ctx.request_repaint();
            });
        if spawned.is_err() {
            let _ = self.copy_tx.send(None);
        }
    }

    /// The answer of a copy: the picture, or `None` when it could not be read. One per call.
    pub fn copied(&self) -> Option<Option<ColorImage>> {
        self.copy_rx.try_recv().ok()
    }

    /// Texture of a game's icon is the library's business; this only exposes ids for tests.
    #[cfg(test)]
    pub fn ready_ids(&self) -> Vec<egui::TextureId> {
        self.entries
            .values()
            .filter_map(|e| match e {
                Entry::Ready { texture, .. } => Some(texture.id()),
                _ => None,
            })
            .collect()
    }
}

impl Drop for Images {
    fn drop(&mut self) {
        let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.closed = true;
        queue.jobs.clear();
        queue.clip_jobs.clear();
        drop(queue);
        self.shared.wake.notify_all();
    }
}

fn work(shared: &Shared, tx: &Sender<Done>, ctx: &Context, lane: Lane) {
    loop {
        let key = {
            let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if queue.closed {
                    return;
                }
                let jobs = match lane {
                    Lane::Pictures => &mut queue.jobs,
                    Lane::Clips => &mut queue.clip_jobs,
                };
                if let Some(key) = jobs.pop_front() {
                    queue.in_flight.insert(key.clone());
                    break key;
                }
                queue = shared.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
            }
        };
        let (image, duration) = match key.kind {
            Kind::Thumb => (decode(&key.path, THUMB_SIDE), None),
            Kind::Full => (
                decode(&key.path, shared.full_side.load(Ordering::Relaxed)),
                None,
            ),
            Kind::Clip | Kind::ClipFull => (
                anyps5_core::captures::clip_thumbnail(
                    &key.path,
                    if key.kind == Kind::ClipFull {
                        FULL_CLIP_SIDE
                    } else {
                        THUMB_SIDE
                    },
                )
                .and_then(|p| {
                    (p.width > 0 && p.height > 0).then(|| {
                        ColorImage::from_rgba_unmultiplied(
                            [p.width as usize, p.height as usize],
                            &p.rgba,
                        )
                    })
                }),
                anyps5_core::captures::clip_duration(&key.path),
            ),
        };
        if tx
            .send(Done {
                key,
                image,
                duration,
            })
            .is_err()
        {
            return;
        }
        ctx.request_repaint();
    }
}

/// A picture file, shrunk to at most `side` pixels on its longer side (never enlarged).
pub fn decode(path: &Path, side: u32) -> Option<ColorImage> {
    let reader = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?;
    let (w, h) = reader.into_dimensions().ok()?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return None;
    }
    let decoded = image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let decoded = if decoded.width().max(decoded.height()) > side {
        decoded.thumbnail(side, side)
    } else {
        decoded
    };
    let rgba = decoded.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_png(path: &Path, w: u32, h: u32) {
        image::RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]))
            .save(path)
            .unwrap();
    }

    fn settle(images: &mut Images, key: &Key) -> Thumb {
        for _ in 0..400 {
            images.poll();
            let thumb = images.get(key.clone());
            images.end_frame();
            if !matches!(thumb, Thumb::Pending) {
                return thumb;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("never decoded");
    }

    #[test]
    fn a_big_picture_comes_back_small_and_keeps_its_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        write_png(&path, 4000, 2000);
        let image = decode(&path, 512).unwrap();
        assert_eq!(image.size, [512, 256]);
        let small = dir.path().join("b.png");
        write_png(&small, 100, 50);
        assert_eq!(
            decode(&small, 512).unwrap().size,
            [100, 50],
            "never enlarged"
        );
        assert!(decode(&dir.path().join("none.png"), 512).is_none());
        let broken = dir.path().join("c.png");
        std::fs::write(&broken, b"not a picture").unwrap();
        assert!(decode(&broken, 512).is_none());
    }

    #[test]
    fn pictures_arrive_off_thread_and_a_broken_file_is_asked_for_once() {
        let ctx = Context::default();
        let mut images = Images::new(&ctx);
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("a.png");
        write_png(&good, 320, 180);
        let bad = dir.path().join("b.png");
        std::fs::write(&bad, b"nope").unwrap();
        let good_key = Key {
            path: good,
            kind: Kind::Thumb,
        };
        let bad_key = Key {
            path: bad,
            kind: Kind::Thumb,
        };
        assert!(matches!(images.get(good_key.clone()), Thumb::Pending));
        images.end_frame();
        assert!(
            matches!(settle(&mut images, &good_key), Thumb::Ready(i) if (i.aspect - 16.0 / 9.0).abs() < 0.01)
        );
        assert!(matches!(settle(&mut images, &bad_key), Thumb::Unavailable));
        // Asking again does not queue it again.
        assert!(matches!(images.get(bad_key), Thumb::Unavailable));
        assert!(images.shared.queue.lock().unwrap().jobs.is_empty());
    }

    #[test]
    fn tiles_scrolled_past_leave_the_queue() {
        let ctx = Context::default();
        let mut images = Images::new(&ctx);
        // Park the workers' input: the queue is replaced every frame from the asks.
        for n in 0..50 {
            images.get(Key {
                path: PathBuf::from(format!("/nowhere/{n}.png")),
                kind: Kind::Thumb,
            });
        }
        images.end_frame();
        // Next frame asks for only one of them.
        images.get(Key {
            path: PathBuf::from("/nowhere/7.png"),
            kind: Kind::Thumb,
        });
        images.end_frame();
        assert!(images.shared.queue.lock().unwrap().jobs.len() <= 1);
        let waiting = images
            .entries
            .values()
            .filter(|e| matches!(e, Entry::Queued))
            .count();
        assert!(waiting <= 1, "{waiting} still wait");
    }

    #[test]
    fn the_cache_stays_under_its_cap_and_keeps_what_is_drawn() {
        let ctx = Context::default();
        let mut images = Images::new(&ctx);
        // Fill past the cap with images nobody draws, then one that is drawn.
        let mut keys = Vec::new();
        images.cap = 20 * 1024 * 1024;
        for n in 0..4 {
            let key = Key {
                path: PathBuf::from(format!("/fake/{n}.png")),
                kind: Kind::Full,
            };
            let image = ColorImage::filled([1600, 1200], egui::Color32::BLACK);
            let bytes = image.pixels.len() * 4;
            images.bytes += bytes;
            images.entries.insert(
                key.clone(),
                Entry::Ready {
                    texture: ctx.load_texture(format!("t{n}"), image, TextureOptions::LINEAR),
                    aspect: 16.0 / 9.0,
                    bytes,
                    used: n as u64,
                },
            );
            keys.push(key);
        }
        images.frame = 10;
        assert!(images.bytes() > images.cap);
        // The newest is drawn this frame.
        assert!(matches!(images.get(keys[3].clone()), Thumb::Ready(_)));
        images.end_frame();
        assert!(images.bytes() <= images.cap, "{}", images.bytes());
        assert!(
            matches!(images.get(keys[3].clone()), Thumb::Ready(_)),
            "the drawn one stays"
        );
        assert!(!images.ready_ids().is_empty());
    }
}
