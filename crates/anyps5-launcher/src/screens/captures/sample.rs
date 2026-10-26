//! Invented captures for the gallery pages (`--gallery=captures-*`): file names, times and
//! pictures are made up here and nothing of a real game or a real folder is used.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyps5_core::captures::{Capture, Kind};
use anyps5_core::library::Game;
use egui::{ColorImage, Context, TextureHandle, TextureOptions};

use super::{Load, Media, Thumb};
use crate::ui::chrome::TileImage;

/// "Now" of the pages: 26 Oct 2026, 20:00 UTC.
pub const NOW: i64 = 1_793_044_800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturesLook {
    /// A folder with nothing in it.
    Empty,
    /// Nine captures of three games.
    Few,
    /// A library of hundreds.
    Many,
    /// One screenshot, full screen.
    Screenshot,
    /// One clip, full screen, with its picture.
    Clip,
    /// A clip the system gives no picture for: the plain tile.
    ClipPlain,
    /// The question before a delete.
    Confirm,
    /// The folder is gone.
    Missing,
    Loading,
    /// Only the clips.
    Clips,
}

impl CapturesLook {
    pub fn load(self) -> Load {
        match self {
            CapturesLook::Missing => Load::Missing,
            CapturesLook::Loading => Load::Loading,
            _ => Load::Ready,
        }
    }
}

/// The invented folder.
pub struct Sample {
    pub captures: Vec<Capture>,
    media: SampleMedia,
}

impl Sample {
    pub fn new(ctx: &Context, look: CapturesLook, games: &[Game]) -> Sample {
        Sample {
            captures: captures(look, games),
            media: SampleMedia {
                ctx: ctx.clone(),
                plain_clips: look == CapturesLook::ClipPlain,
                some_plain: matches!(
                    look,
                    CapturesLook::Few | CapturesLook::Many | CapturesLook::Clips
                ),
                pictures: HashMap::new(),
            },
        }
    }

    pub fn media(&mut self) -> &mut dyn Media {
        &mut self.media
    }

    /// The place of the first capture of a kind, in the order the grid shows (for the pages that
    /// open on one).
    pub fn first(&self, kind: Kind, sections: &[super::Section]) -> usize {
        sections
            .iter()
            .flat_map(|s| s.items.iter())
            .position(|&i| self.captures[i].kind == kind)
            .unwrap_or(0)
    }
}

fn pseudo(seed: u64) -> u64 {
    let mut x = seed
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(0x1234_5678);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^ (x >> 32)
}

fn stamp(modified: i64) -> (String, String) {
    let days = modified.div_euclid(86_400);
    let secs = modified.rem_euclid(86_400);
    // Civil from days (1970-01-01 = 0).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        format!(
            "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
            secs / 3600,
            secs % 3600 / 60,
            secs % 60
        ),
        format!(
            "{y:04}-{m:02}-{d:02} {:02}-{:02}-{:02}",
            secs / 3600,
            secs % 3600 / 60,
            secs % 60
        ),
    )
}

fn captures(look: CapturesLook, games: &[Game]) -> Vec<Capture> {
    let count = match look {
        CapturesLook::Empty | CapturesLook::Missing | CapturesLook::Loading => 0,
        CapturesLook::Many => 360,
        _ => 9,
    };
    let in_games = if look == CapturesLook::Many {
        games.len()
    } else {
        3
    };
    let mut list = Vec::new();
    for n in 0..count {
        let game = &games[(pseudo(n as u64) % in_games as u64) as usize % games.len()];
        let clip = n % 4 == 2;
        let age = 600 + n as i64 * (3_000 + (pseudo(n as u64 + 7) % 9_000) as i64);
        let modified = NOW - age;
        let (shot, clip_stamp) = stamp(modified);
        let name = if clip {
            format!(
                "{} {clip_stamp}.mp4",
                anyps5_core::clips::safe_name(&game.name)
            )
        } else {
            format!("{}-{shot}.png", game.title_id)
        };
        list.push(Capture {
            path: PathBuf::from("/Pictures/AnyPS5").join(name),
            kind: if clip { Kind::Clip } else { Kind::Screenshot },
            owner: if clip {
                anyps5_core::captures::Owner::Name(anyps5_core::clips::safe_name(&game.name))
            } else {
                anyps5_core::captures::Owner::Title(game.title_id.clone())
            },
            modified,
            size: 1_000_000 + n as u64 * 1_000,
        });
    }
    list.sort_by_key(|c| std::cmp::Reverse(c.modified));
    list
}

struct SampleMedia {
    ctx: Context,
    plain_clips: bool,
    /// A third of the clips have no picture, as with a shell that cannot read some files.
    some_plain: bool,
    pictures: HashMap<(PathBuf, bool), (TextureHandle, TileImage)>,
}

impl SampleMedia {
    fn picture(&mut self, path: &Path, big: bool) -> TileImage {
        let ctx = self.ctx.clone();
        self.pictures
            .entry((path.to_path_buf(), big))
            .or_insert_with(|| {
                let seed = pseudo(path.to_string_lossy().bytes().map(u64::from).sum());
                let (w, h) = if big { (1280, 720) } else { (320, 180) };
                let handle = ctx.load_texture(
                    format!("sample-capture-{}-{big}", path.display()),
                    scene(seed, w, h),
                    TextureOptions::LINEAR,
                );
                let image = TileImage {
                    texture: handle.id(),
                    aspect: w as f32 / h as f32,
                };
                (handle, image)
            })
            .1
    }
}

impl Media for SampleMedia {
    fn thumb(&mut self, capture: &Capture) -> Thumb {
        let plain = self.plain_clips
            || (self.some_plain && pseudo(capture.modified as u64).is_multiple_of(3));
        if capture.kind == Kind::Clip && plain {
            return Thumb::Unavailable;
        }
        Thumb::Ready(self.picture(&capture.path, false))
    }

    fn full(&mut self, capture: &Capture) -> Thumb {
        if capture.kind == Kind::Clip && self.plain_clips {
            return Thumb::Unavailable;
        }
        Thumb::Ready(self.picture(&capture.path, true))
    }

    fn duration(&mut self, capture: &Capture) -> Option<Duration> {
        (capture.kind == Kind::Clip)
            .then(|| Duration::from_secs(15 + pseudo(capture.modified as u64) % 105))
    }
}

/// A made-up game scene: a sky, a low sun, two ranges of hills and a dark foreground.
fn scene(seed: u64, w: usize, h: usize) -> ColorImage {
    let hue = (seed % 360) as f32 / 360.0;
    let sun_x = 0.25 + (seed >> 8) as f32 % 50.0 / 100.0;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let far = 0.62 + 0.06 * (fx * 7.0 + hue * 6.0).sin();
            let near = 0.78 + 0.05 * (fx * 4.0 + 1.0 + hue * 3.0).sin();
            let (r, g, b) = if fy > near {
                hsv(hue + 0.02, 0.45, 0.12 + 0.05 * fy)
            } else if fy > far {
                hsv(hue + 0.05, 0.5, 0.26 + 0.12 * (fy - far))
            } else {
                let sun = (1.0 - (((fx - sun_x).powi(2) + (fy - 0.5).powi(2)).sqrt() / 0.32))
                    .clamp(0.0, 1.0);
                hsv(
                    hue + 0.1 * fy,
                    0.55 - 0.2 * sun,
                    0.35 + 0.45 * (1.0 - fy) + 0.4 * sun.powf(1.5),
                )
            };
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ColorImage::from_rgba_unmultiplied([w, h], &rgba)
}

fn hsv(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let v = v.clamp(0.0, 1.0);
    let h = h.rem_euclid(1.0) * 6.0;
    let (i, f) = (h.floor() as i32, h.fract());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}
