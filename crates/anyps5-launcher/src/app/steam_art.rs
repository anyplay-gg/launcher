//! The pictures of the Steam entries (docs/DESIGN.md 9.9): the launcher's own, drawn here from its
//! disc and name, and one set per game, made from the game's own `sce_sys` art on this PC.
//!
//! Game art is only read and only written as new files into the player's Steam folder; it never
//! goes anywhere else. A picture that cannot be read is left out, and Steam shows its own blank.

use std::io::Cursor;
use std::path::Path;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use anyps5_core::library::Game;
use anyps5_core::steam_shortcuts::Art;
use image::imageops::{self, FilterType};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

/// The name drawn in the launcher's capsules.
const NAME: &str = "AnyPlay";
const BOLD: &[u8] = include_bytes!("../../assets/fonts/noto/NotoSans-Bold.ttf");

const PORTRAIT: (u32, u32) = (600, 900);
const WIDE: (u32, u32) = (920, 430);
const HERO: (u32, u32) = (1920, 620);

/// The launcher's artwork.
pub fn launcher_art() -> Art {
    let font = FontRef::try_from_slice(BOLD).ok();
    let font = font.as_ref();

    let mut portrait = backdrop(PORTRAIT.0, PORTRAIT.1);
    glow(&mut portrait, 300.0, 400.0, 330.0);
    disc(&mut portrait, 300.0, 400.0, 150.0);
    name(&mut portrait, font, 300.0, 700.0, 112.0);

    let mut wide = backdrop(WIDE.0, WIDE.1);
    glow(&mut wide, 200.0, 215.0, 260.0);
    disc(&mut wide, 200.0, 215.0, 105.0);
    name_left(&mut wide, font, 350.0, 215.0, 120.0);

    let mut hero = backdrop(HERO.0, HERO.1);
    glow(&mut hero, 960.0, 310.0, 560.0);

    // The logo lies over the hero, so it is transparent.
    let mut logo = RgbaImage::new(1000, 300);
    disc(&mut logo, 150.0, 150.0, 130.0);
    name_left(&mut logo, font, 320.0, 150.0, 190.0);

    let mut icon = RgbaImage::new(256, 256);
    disc(&mut icon, 128.0, 128.0, 124.0);

    Art {
        portrait: png(portrait),
        wide: png(wide),
        hero: png(hero),
        logo: png(logo),
        icon: png(icon),
    }
}

/// A game's artwork from its `sce_sys` folder: the icon as it is, the background picture cut to
/// the shapes Steam shows, and a tall capsule with the icon on the darkened background.
pub fn game_art(game: &Game) -> Art {
    let sys = game.folder.join("sce_sys");
    let icon = game.icon.as_deref().and_then(read_image);
    // `pic1` is the second background of a title; `pic0` the one the launcher shows.
    let background = game
        .background
        .as_deref()
        .and_then(read_image)
        .or_else(|| read_image(&sys.join("pic1.png")));

    let mut art = Art {
        icon: icon.as_ref().and_then(|i| png(i.to_rgba8())),
        ..Art::default()
    };
    if let Some(background) = &background {
        art.wide = png(cover(background, WIDE.0, WIDE.1));
        art.hero = png(cover(background, HERO.0, HERO.1));
    }
    if icon.is_some() || background.is_some() {
        let mut tall = match &background {
            Some(b) => {
                let mut tall = cover(b, PORTRAIT.0, PORTRAIT.1);
                for pixel in tall.pixels_mut() {
                    for channel in &mut pixel.0[..3] {
                        *channel = (u16::from(*channel) * 45 / 100) as u8;
                    }
                }
                tall
            }
            None => backdrop(PORTRAIT.0, PORTRAIT.1),
        };
        if let Some(icon) = &icon {
            let size = 420;
            let small = imageops::resize(&icon.to_rgba8(), size, size, FilterType::Lanczos3);
            imageops::overlay(
                &mut tall,
                &small,
                i64::from((PORTRAIT.0 - size) / 2),
                i64::from((PORTRAIT.1 - size) / 2),
            );
        }
        art.portrait = png(tall);
    }
    art
}

fn read_image(path: &Path) -> Option<DynamicImage> {
    // Game art is a few megabytes at most; a file far beyond that is no game art.
    if std::fs::metadata(path).ok()?.len() > 64 * 1024 * 1024 {
        return None;
    }
    image::open(path).ok()
}

/// `image` scaled to fill `w` x `h` and cut to it, from the middle.
fn cover(image: &DynamicImage, w: u32, h: u32) -> RgbaImage {
    let (iw, ih) = (image.width().max(1), image.height().max(1));
    let scale = f64::max(f64::from(w) / f64::from(iw), f64::from(h) / f64::from(ih));
    let (sw, sh) = (
        ((f64::from(iw) * scale).ceil() as u32).max(w),
        ((f64::from(ih) * scale).ceil() as u32).max(h),
    );
    let scaled = imageops::resize(&image.to_rgba8(), sw, sh, FilterType::Triangle);
    imageops::crop_imm(&scaled, (sw - w) / 2, (sh - h) / 2, w, h).to_image()
}

fn png(image: RgbaImage) -> Option<Vec<u8>> {
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut out, ImageFormat::Png)
        .ok()?;
    Some(out.into_inner())
}

/// The night-blue ground of the launcher's capsules.
fn backdrop(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |_, y| {
        let t = y as f32 / h.max(1) as f32;
        let mix = |a: f32, b: f32| (a + (b - a) * t) as u8;
        Rgba([mix(13.0, 22.0), mix(16.0, 27.0), mix(27.0, 46.0), 255])
    })
}

/// The disc's gradient at a point of its box (0..1 each way): the icon's own colours.
fn brand(u: f32, v: f32) -> [f32; 3] {
    [40.0 + 64.0 * u, 70.0 + 64.0 * v, 200.0]
}

fn blend(pixel: &mut Rgba<u8>, color: [f32; 3], alpha: f32) {
    let alpha = alpha.clamp(0.0, 1.0);
    let under = f32::from(pixel.0[3]) / 255.0;
    let out = alpha + under * (1.0 - alpha);
    if out <= 0.0 {
        return;
    }
    for (i, c) in color.iter().enumerate() {
        let below = f32::from(pixel.0[i]);
        pixel.0[i] = ((c * alpha + below * under * (1.0 - alpha)) / out).round() as u8;
    }
    pixel.0[3] = (out * 255.0).round() as u8;
}

/// A soft light of the disc's blue behind it.
fn glow(image: &mut RgbaImage, cx: f32, cy: f32, radius: f32) {
    let (w, h) = image.dimensions();
    let x0 = (cx - radius).max(0.0) as u32;
    let x1 = ((cx + radius) as u32).min(w);
    let y0 = (cy - radius).max(0.0) as u32;
    let y1 = ((cy + radius) as u32).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / radius;
            if d < 1.0 {
                let alpha = 0.32 * (1.0 - d).powi(2);
                blend(image.get_pixel_mut(x, y), brand(0.5, 0.5), alpha);
            }
        }
    }
}

/// The launcher's disc: a gradient with a one-pixel soft edge.
fn disc(image: &mut RgbaImage, cx: f32, cy: f32, r: f32) {
    let (w, h) = image.dimensions();
    let x0 = (cx - r - 1.0).max(0.0) as u32;
    let x1 = ((cx + r + 1.0) as u32).min(w);
    let y0 = (cy - r - 1.0).max(0.0) as u32;
    let y1 = ((cy + r + 1.0) as u32).min(h);
    for y in y0..y1 {
        for x in x0..x1 {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let coverage = r + 0.5 - (dx * dx + dy * dy).sqrt();
            if coverage > 0.0 {
                let u = (dx / (2.0 * r) + 0.5).clamp(0.0, 1.0);
                let v = (dy / (2.0 * r) + 0.5).clamp(0.0, 1.0);
                blend(image.get_pixel_mut(x, y), brand(u, v), coverage);
            }
        }
    }
}

/// The name, centred on `cx` with its middle at `cy`.
fn name(image: &mut RgbaImage, font: Option<&FontRef<'_>>, cx: f32, cy: f32, px: f32) {
    if let Some(font) = font {
        let width = text_width(font, px);
        text(image, font, cx - width / 2.0, cy, px);
    }
}

/// The name, starting at `x`, with its middle at `cy`.
fn name_left(image: &mut RgbaImage, font: Option<&FontRef<'_>>, x: f32, cy: f32, px: f32) {
    if let Some(font) = font {
        text(image, font, x, cy, px);
    }
}

fn text_width(font: &FontRef<'_>, px: f32) -> f32 {
    let scaled = font.as_scaled(PxScale::from(px));
    NAME.chars()
        .map(|c| scaled.h_advance(scaled.glyph_id(c)))
        .sum()
}

fn text(image: &mut RgbaImage, font: &FontRef<'_>, x: f32, cy: f32, px: f32) {
    let scaled = font.as_scaled(PxScale::from(px));
    // The capital height sits on the baseline; centring that on `cy` centres what the eye sees.
    let baseline = cy + scaled.ascent() * 0.36;
    let mut pen = x;
    for c in NAME.chars() {
        let id = scaled.glyph_id(c);
        let glyph = id.with_scale_and_position(PxScale::from(px), point(pen, baseline));
        pen += scaled.h_advance(id);
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            let (w, h) = image.dimensions();
            outline.draw(|gx, gy, coverage| {
                let (ix, iy) = (
                    bounds.min.x as i64 + i64::from(gx),
                    bounds.min.y as i64 + i64::from(gy),
                );
                if ix >= 0 && iy >= 0 && (ix as u32) < w && (iy as u32) < h {
                    blend(
                        image.get_pixel_mut(ix as u32, iy as u32),
                        [255.0, 255.0, 255.0],
                        coverage,
                    );
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyps5_core::library;

    fn size_of(bytes: &Option<Vec<u8>>) -> (u32, u32) {
        let image = image::load_from_memory(bytes.as_ref().expect("a picture")).unwrap();
        (image.width(), image.height())
    }

    #[test]
    fn the_launchers_art_has_the_sizes_steam_shows() {
        let art = launcher_art();
        assert_eq!(size_of(&art.portrait), (600, 900));
        assert_eq!(size_of(&art.wide), (920, 430));
        assert_eq!(size_of(&art.hero), (1920, 620));
        assert_eq!(size_of(&art.logo), (1000, 300));
        assert_eq!(size_of(&art.icon), (256, 256));
    }

    #[test]
    fn the_disc_and_the_name_are_drawn_and_the_logo_stays_clear_around_them() {
        let art = launcher_art();
        let portrait = image::load_from_memory(art.portrait.as_ref().unwrap())
            .unwrap()
            .to_rgba8();
        // The middle of the disc is its gradient, not the night-blue ground.
        let middle = portrait.get_pixel(300, 400).0;
        assert!(middle[2] > 150 && middle[0] > 60, "{middle:?}");
        // Some pixel of the name's band is white.
        assert!(
            (0..600)
                .any(|x| (640..760).any(|y| portrait.get_pixel(x, y).0[..3] == [255, 255, 255]))
        );
        let logo = image::load_from_memory(art.logo.as_ref().unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(logo.get_pixel(5, 5).0[3], 0, "the corner is transparent");
    }

    fn dump(root: &Path, with_art: bool) -> Game {
        let folder = root.join("dump");
        let sys = folder.join("sce_sys");
        std::fs::create_dir_all(&sys).unwrap();
        std::fs::write(folder.join("eboot.bin"), b"\x7fELF").unwrap();
        std::fs::write(
            sys.join("param.json"),
            r#"{"titleId":"PPSA09999","localizedParameters":{"defaultLanguage":"en-US","en-US":{"titleName":"Test Game"}}}"#,
        )
        .unwrap();
        if with_art {
            RgbaImage::from_pixel(512, 512, Rgba([200, 30, 30, 255]))
                .save(sys.join("icon0.png"))
                .unwrap();
            RgbaImage::from_pixel(1920, 1080, Rgba([20, 120, 30, 255]))
                .save(sys.join("pic0.png"))
                .unwrap();
        }
        library::inspect(&folder).expect("a game")
    }

    #[test]
    fn a_game_gets_art_from_its_own_pictures_in_the_shapes_steam_shows() {
        let root = tempfile::tempdir().unwrap();
        let game = dump(root.path(), true);
        let art = game_art(&game);
        assert_eq!(size_of(&art.portrait), (600, 900));
        assert_eq!(size_of(&art.wide), (920, 430));
        assert_eq!(size_of(&art.hero), (1920, 620));
        assert_eq!(size_of(&art.icon), (512, 512));
        assert!(art.logo.is_none(), "Steam writes the title itself");
        // The tall capsule is the background, darkened, with the icon in the middle.
        let tall = image::load_from_memory(art.portrait.as_ref().unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(tall.get_pixel(300, 450).0[..3], [200, 30, 30]);
        assert_eq!(tall.get_pixel(5, 5).0[..3], [9, 54, 13]);
    }

    #[test]
    fn a_game_without_pictures_gets_none() {
        let root = tempfile::tempdir().unwrap();
        let art = game_art(&dump(root.path(), false));
        assert!(art.portrait.is_none() && art.wide.is_none() && art.hero.is_none());
        assert!(art.icon.is_none());
    }

    #[test]
    fn the_dump_is_not_changed() {
        let root = tempfile::tempdir().unwrap();
        let game = dump(root.path(), true);
        let before = std::fs::read(game.folder.join("sce_sys/pic0.png")).unwrap();
        let _ = game_art(&game);
        assert_eq!(
            std::fs::read(game.folder.join("sce_sys/pic0.png")).unwrap(),
            before
        );
    }
}
