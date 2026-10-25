//! What the launcher keeps of the profiles it has looked at, and the two small computations behind
//! the profile page: the colour of the banner (the cloud has no banner picture, so the page
//! derives one from the avatar) and the playtime highlights.

use super::api::{AccountId, ProfilePage, TitlePlaytime};

/// A profile the cloud answered, or a failed attempt to read it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProfileEntry {
    /// The last answer. Kept while a newer one is on its way.
    pub page: Option<ProfilePage>,
    /// The last attempt failed (and there is no answer to show).
    pub failed: bool,
}

/// The profile shown for `account`, if it has an answer.
pub fn page_of(
    profiles: &std::collections::BTreeMap<AccountId, ProfileEntry>,
    account: AccountId,
) -> Option<&ProfilePage> {
    profiles.get(&account).and_then(|e| e.page.as_ref())
}

/// The titles a profile shows as highlights: the most played, at most `most`, never one without
/// time. The cloud sends them most played first; this does not rely on it.
pub fn most_played(playtime: &[TitlePlaytime], most: usize) -> Vec<&TitlePlaytime> {
    let mut played: Vec<&TitlePlaytime> = playtime.iter().filter(|p| p.minutes > 0).collect();
    played.sort_by(|a, b| {
        b.minutes
            .cmp(&a.minutes)
            .then_with(|| a.title_id.cmp(&b.title_id))
    });
    played.truncate(most);
    played
}

/// The least contrast with white ink the banner colour keeps (WCAG 2: 4.5 is the floor for small
/// text; the text on a banner is not always full white, so there is room above it).
pub const BANNER_CONTRAST: f32 = 7.0;

/// The most vivid colour that dominates a picture, `None` for a picture without colour (grey,
/// black, white, transparent). `rgba` is 8-bit RGBA, straight alpha.
///
/// Pixels vote for one of 12 hue sectors, each with the weight saturation times brightness, so a
/// small bright logo does not lose to a large dull background; the sector with the most weight
/// wins (the lowest hue on a tie), and the answer is the weighted mean of its pixels. Nothing
/// random, nothing that depends on the order of the pixels beyond that tie.
pub fn dominant_colour(rgba: &[u8]) -> Option<[u8; 3]> {
    const SECTORS: usize = 12;
    let mut weight = [0.0f64; SECTORS];
    let mut sum = [[0.0f64; 3]; SECTORS];
    let mut seen = 0usize;
    for pixel in rgba.as_chunks::<4>().0 {
        seen += 1;
        if pixel[3] < 128 {
            continue;
        }
        let (r, g, b) = (
            f64::from(pixel[0]) / 255.0,
            f64::from(pixel[1]) / 255.0,
            f64::from(pixel[2]) / 255.0,
        );
        let top = r.max(g).max(b);
        let chroma = top - r.min(g).min(b);
        if top < 0.2 || chroma / top < 0.2 {
            continue;
        }
        let hue = if chroma == 0.0 {
            0.0
        } else if top == r {
            ((g - b) / chroma).rem_euclid(6.0)
        } else if top == g {
            (b - r) / chroma + 2.0
        } else {
            (r - g) / chroma + 4.0
        };
        let sector = ((hue * 2.0) as usize).min(SECTORS - 1);
        // Saturation times brightness: chroma.
        let vote = chroma;
        weight[sector] += vote;
        for (channel, value) in [r, g, b].into_iter().enumerate() {
            sum[sector][channel] += value * vote;
        }
    }
    let mut best = 0;
    for sector in 1..SECTORS {
        if weight[sector] > weight[best] {
            best = sector;
        }
    }
    // A picture with a handful of coloured pixels is a grey picture.
    if weight[best] <= 0.0 || weight[best] < seen as f64 * 0.01 {
        return None;
    }
    let mean = |channel: usize| (sum[best][channel] / weight[best] * 255.0).round() as u8;
    Some([mean(0), mean(1), mean(2)])
}

/// How opaque the white of the text under the handle is (the text of the profile).
pub const BANNER_TEXT_ALPHA: f32 = 0.88;

/// The least contrast that text of [`BANNER_TEXT_ALPHA`] keeps on the banner.
pub const BANNER_TEXT_CONTRAST: f32 = 5.5;

fn luminance(rgb: [f32; 3]) -> f32 {
    let linear = |c: f32| {
        let c = c / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
}

/// The contrast ratio of white over `rgb` (WCAG 2).
pub fn contrast_with_white(rgb: [u8; 3]) -> f32 {
    1.05 / (luminance(rgb.map(f32::from)) + 0.05)
}

/// The contrast of white at `alpha` over `rgb`, against `rgb` itself.
pub fn contrast_of_text(rgb: [u8; 3], alpha: f32) -> f32 {
    let ground = rgb.map(f32::from);
    let ink = ground.map(|c| c + alpha * (255.0 - c));
    (luminance(ink) + 0.05) / (luminance(ground) + 0.05)
}

/// `colour` darkened, hue kept, until white ink on it has [`BANNER_CONTRAST`] and the text of
/// [`BANNER_TEXT_ALPHA`] has [`BANNER_TEXT_CONTRAST`].
pub fn readable_banner(colour: [u8; 3]) -> [u8; 3] {
    let mut c = colour;
    // Every step multiplies by 0.92, so this ends: pure black has the contrast of 21.
    while contrast_with_white(c) < BANNER_CONTRAST
        || contrast_of_text(c, BANNER_TEXT_ALPHA) < BANNER_TEXT_CONTRAST
    {
        c = c.map(|v| (f32::from(v) * 0.92) as u8);
    }
    c
}

/// The banner colour for an avatar's pixels, `None` when the picture has no colour to take (the
/// page then uses a calm colour of its theme).
pub fn banner_colour(rgba: &[u8]) -> Option<[u8; 3]> {
    dominant_colour(rgba).map(readable_banner)
}

/// Minutes as hours and the minutes beyond: `(0, 35)`, `(4, 20)`.
pub fn hours_minutes(minutes: u64) -> (u64, u64) {
    (minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(pixels: &[([u8; 3], usize)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (rgb, count) in pixels {
            for _ in 0..*count {
                out.extend_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
            }
        }
        out
    }

    #[test]
    fn the_banner_takes_the_colour_of_the_picture() {
        // Mostly teal with some orange and a grey frame.
        let rgba = picture(&[
            ([20, 170, 160], 600),
            ([240, 140, 30], 100),
            ([120, 120, 120], 900),
        ]);
        let [r, g, b] = dominant_colour(&rgba).unwrap();
        assert!(g > r && b > r, "teal wins: {r} {g} {b}");
    }

    #[test]
    fn a_small_vivid_logo_beats_a_large_dull_background() {
        let rgba = picture(&[([90, 80, 80], 5000), ([230, 30, 40], 700)]);
        let [r, g, b] = dominant_colour(&rgba).unwrap();
        assert!(r > 200 && g < 60 && b < 60, "{r} {g} {b}");
    }

    #[test]
    fn the_colour_is_the_same_every_time() {
        let rgba = picture(&[([20, 170, 160], 300), ([30, 40, 200], 300)]);
        let first = dominant_colour(&rgba);
        assert_eq!(first, dominant_colour(&rgba));
        // Blue is the more vivid of the two, so it wins.
        let [r, _, b] = first.unwrap();
        assert!(b > 150 && r < 60);
    }

    #[test]
    fn a_picture_without_colour_gives_none() {
        assert_eq!(dominant_colour(&picture(&[([128, 128, 128], 500)])), None);
        assert_eq!(dominant_colour(&picture(&[([255, 255, 255], 500)])), None);
        assert_eq!(dominant_colour(&picture(&[([0, 0, 0], 500)])), None);
        assert_eq!(dominant_colour(&[]), None);
        // Transparent pixels vote for nothing.
        let mut clear = Vec::new();
        for _ in 0..500 {
            clear.extend_from_slice(&[255, 0, 0, 0]);
        }
        assert_eq!(dominant_colour(&clear), None);
        // A few coloured pixels in a grey picture are still a grey picture.
        assert_eq!(
            dominant_colour(&picture(&[([128, 128, 128], 2000), ([255, 0, 0], 5)])),
            None
        );
    }

    #[test]
    fn white_ink_stays_readable_on_every_banner() {
        for colour in [
            [255, 255, 0],
            [255, 0, 0],
            [0, 255, 255],
            [250, 250, 120],
            [20, 170, 160],
            [10, 10, 60],
        ] {
            let banner = readable_banner(colour);
            assert!(
                contrast_with_white(banner) >= BANNER_CONTRAST,
                "{colour:?} -> {banner:?}"
            );
            assert!(
                contrast_of_text(banner, BANNER_TEXT_ALPHA) >= BANNER_TEXT_CONTRAST,
                "the text at 88 %: {colour:?} -> {banner:?}"
            );
        }
        // A colour that reads already is left alone.
        assert_eq!(readable_banner([10, 10, 60]), [10, 10, 60]);
        // Darkening keeps the hue: red stays red.
        let [r, g, b] = readable_banner([255, 0, 0]);
        assert!(r > g && r > b);
    }

    #[test]
    fn the_contrast_of_black_and_white_is_the_known_one() {
        assert!((contrast_with_white([0, 0, 0]) - 21.0).abs() < 0.01);
        assert!((contrast_with_white([255, 255, 255]) - 1.0).abs() < 0.01);
    }

    #[test]
    fn the_highlights_are_the_most_played_with_time() {
        let t = |id: &str, minutes| TitlePlaytime {
            title_id: id.into(),
            minutes,
        };
        let playtime = [
            t("GAME00003", 30),
            t("GAME00001", 600),
            t("GAME00002", 0),
            t("GAME00004", 600),
        ];
        let top = most_played(&playtime, 3);
        assert_eq!(
            top.iter().map(|p| p.title_id.as_str()).collect::<Vec<_>>(),
            ["GAME00001", "GAME00004", "GAME00003"],
            "ties by title, none without time"
        );
        assert!(most_played(&[], 3).is_empty());
    }

    #[test]
    fn minutes_split_into_hours() {
        assert_eq!(hours_minutes(35), (0, 35));
        assert_eq!(hours_minutes(260), (4, 20));
    }
}
