//! The avatars of other players as the profile page and the feed need them: fetched once from the
//! cloud by a worker thread (`anyps5_core::online::avatar`), kept on disk under their hash, and
//! read once for the colour of the profile banner.
//!
//! The texture itself comes from [`Textures`](super::textures::Textures), which decodes the file
//! off the UI thread like a game's icon.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyps5_core::online::{self, avatar};

/// A failed fetch is tried again after this long.
const RETRY: Duration = Duration::from_secs(60);

/// An avatar on disk.
#[derive(Clone, Debug, PartialEq)]
pub struct Ready {
    pub path: PathBuf,
    /// The banner colour of the picture, `None` for one without colour.
    pub banner: Option<[u8; 3]>,
}

enum Slot {
    Fetching,
    Ready(Ready),
    Failed(Instant),
}

static SLOTS: Mutex<BTreeMap<String, Slot>> = Mutex::new(BTreeMap::new());

/// The avatar with `hash` when it is on this PC; starts fetching it from `cloud` into `dir`
/// otherwise and answers `None` until it is there. Asking again is free.
pub fn get(ctx: &egui::Context, cloud: Option<&str>, dir: &Path, hash: &str) -> Option<Ready> {
    let mut slots = SLOTS.lock().ok()?;
    match slots.get(hash) {
        Some(Slot::Ready(ready)) => return Some(ready.clone()),
        Some(Slot::Fetching) => return None,
        Some(Slot::Failed(at)) if at.elapsed() < RETRY => return None,
        _ => {}
    }
    let cloud = cloud?.to_owned();
    if !avatar::is_hash(hash) {
        return None;
    }
    slots.insert(hash.to_owned(), Slot::Fetching);
    let (ctx, dir, key) = (ctx.clone(), dir.to_owned(), hash.to_owned());
    let spawned = std::thread::Builder::new()
        .name("avatar".into())
        .spawn(move || {
            let result = avatar::fetch(&cloud, &key, &dir).and_then(|path| {
                let banner = banner_of(&path)?;
                Ok(Ready { path, banner })
            });
            let slot = match result {
                Ok(ready) => Slot::Ready(ready),
                Err(error) => {
                    tracing::info!("avatar {key}: {error}");
                    Slot::Failed(Instant::now())
                }
            };
            if let Ok(mut slots) = SLOTS.lock() {
                slots.insert(key, slot);
            }
            ctx.request_repaint();
        });
    if spawned.is_err() {
        slots.insert(hash.to_owned(), Slot::Failed(Instant::now()));
    }
    None
}

/// The banner colour of the picture in `path`.
fn banner_of(path: &Path) -> Result<Option<[u8; 3]>, String> {
    let image = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    Ok(online::banner_colour(image.as_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_banner_comes_from_the_picture_on_disk() {
        let dir = std::env::temp_dir().join(format!("anyplay-avatars-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.png");
        image::RgbaImage::from_pixel(16, 16, image::Rgba([20, 170, 160, 255]))
            .save(&file)
            .unwrap();
        let banner = banner_of(&file).unwrap().expect("teal has a colour");
        assert!(banner[1] > banner[0] && online::contrast_with_white(banner) >= 7.0);
        let grey = dir.join("g.png");
        image::RgbaImage::from_pixel(16, 16, image::Rgba([128, 128, 128, 255]))
            .save(&grey)
            .unwrap();
        assert_eq!(banner_of(&grey).unwrap(), None);
        assert!(banner_of(&dir.join("missing.png")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
