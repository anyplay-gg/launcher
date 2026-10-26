//! WCAG contrast of the launcher's ink against the surfaces it is drawn on (docs/DESIGN.md 3.10).
//!
//! Every text token and the tokens of status, state and accent colour are composited over the
//! backdrops they meet and measured with the WCAG 2.x formula (relative luminance of sRGB, ratio
//! `(L1 + 0.05) / (L2 + 0.05)`). A pair must reach 4.5:1 for text, 3:1 for large text (24 dp, or 19
//! dp bold) and for glyphs and other parts of a control. Inactive and disabled ink is exempt by the
//! standard and listed as such.
//!
//! Backdrops come from the theme tokens, not from screenshots: per theme the deep tone, the middle
//! tone and a lit tone (the middle and the bright tone half and half, the lightest a shader paints
//! behind text), and a pure white picture under the artwork scrims, the worst a game can bring.
//! The home screen's text is measured where it sits, under the shades of `ui::shade`.
//!
//! `cargo test -p anyps5-launcher contrast -- --nocapture` prints the table. The same ratios of a
//! rendered screenshot come from `tools/measure-contrast.mjs` (the fixtures of a polish round).

use egui::{Color32, Pos2, Rect, pos2, vec2};

use crate::ui::artwork;
use crate::ui::shade;
use crate::ui::theme::{self, Accent, TextStyle, Theme, Weight, black, color, mix, ty, white};

/// sRGB channel (0..=255) to linear light.
fn linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance of an opaque colour.
pub fn luminance(c: Color32) -> f64 {
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

/// WCAG contrast ratio of two opaque colours.
pub fn ratio(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `fg` (straight alpha) laid over the opaque `bg`, blended in sRGB like egui does.
pub fn over(fg: Color32, bg: Color32) -> Color32 {
    let [r, g, b, a] = fg.to_srgba_unmultiplied();
    let a = f32::from(a) / 255.0;
    let k = |f: u8, b: u8| (f32::from(f) * a + f32::from(b) * (1.0 - a)).round() as u8;
    Color32::from_rgb(k(r, bg.r()), k(g, bg.g()), k(b, bg.b()))
}

/// What a piece of ink has to reach.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Need {
    /// Text below 24 dp (and below 19 dp bold).
    Text,
    /// Large text.
    Large,
    /// A glyph or another part of a control.
    Glyph,
}

impl Need {
    fn of(style: TextStyle) -> Need {
        let bold = matches!(style.weight, Weight::SemiBold | Weight::Bold);
        if style.size >= 24.0 || (style.size >= 18.66 && bold) {
            Need::Large
        } else {
            Need::Text
        }
    }
    fn min(self) -> f64 {
        match self {
            Need::Text => 4.5,
            Need::Large | Need::Glyph => 3.0,
        }
    }
}

/// The screen of the reference size, for the home positions below.
fn screen() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 900.0))
}

/// The lightest tone a theme's shader lays behind text: the middle tone lifted toward the bright
/// one. Cosmic and Aurora only brighten in soft glows (a third of the way); Sunset paints a lit
/// band across the lower part of the screen, right under the hero and the hint bar (half the way).
fn lit(theme: Theme) -> Color32 {
    let p = theme.palette();
    let k = match theme {
        Theme::Cosmic | Theme::Aurora => 0.35,
        Theme::Sunset => 0.5,
    };
    mix(p.middle_color(), p.bright_color(), k)
}

/// What a ground is made of.
#[derive(Clone, Copy)]
enum Base {
    Fixed(Color32),
    /// The lightest tone the theme's shader lays at that place (see [`lit_at`]).
    Lit(Theme),
}

/// A backdrop the home screen's text meets.
#[derive(Clone)]
struct Ground {
    name: String,
    /// The colour before the shades.
    base: Base,
    /// The artwork scrims lie over it (a picture), at the text's place.
    art: bool,
}

/// The lightest tone the shader lays at `at`. Under the top bar it is the middle tone with a soft
/// glow (a quarter of the way to the bright one). Lower down it is the middle tone, except Sunset,
/// which paints a lit band centred at 76 % of the height that falls off as `exp(-5 |y - 0.76|)`,
/// of the colour `mix(bright, (1, 0.5, 0.42), 0.35) * 0.92` (`ui/shaders/background.wgsl`, 5.2).
fn lit_at(theme: Theme, at: Pos2) -> Color32 {
    let p = theme.palette();
    if at.y < shade::TOP_HEIGHT {
        return mix(p.middle_color(), p.bright_color(), 0.25);
    }
    if theme != Theme::Sunset {
        return p.middle_color();
    }
    let [br, bg, bb] = p.bright;
    let warm = |b: f32, w: f32| (b * 0.65 + w * 0.35) * 0.92;
    let band = theme::rgb(warm(br, 1.0), warm(bg, 0.5), warm(bb, 0.42));
    let weight = (-5.0 * (at.y / screen().height() - 0.76).abs()).exp();
    mix(p.middle_color(), band, weight)
}

/// The deep tone, the lit tone and a pure white picture, which every theme can be given.
fn grounds(theme: Theme) -> Vec<Ground> {
    let p = theme.palette();
    let t = theme.title(crate::ui::i18n::Language::English);
    vec![
        Ground {
            name: format!("{t} deep"),
            base: Base::Fixed(p.deep_color()),
            art: false,
        },
        Ground {
            name: format!("{t} lit"),
            base: Base::Lit(theme),
            art: false,
        },
        Ground {
            name: format!("{t} deep, glass window"),
            base: Base::Fixed(glassed(p.deep_color())),
            art: false,
        },
        Ground {
            name: format!("{t} lit, glass window"),
            base: Base::Fixed(glassed(lit(theme))),
            art: false,
        },
        Ground {
            name: format!("{t} + white picture"),
            base: Base::Fixed(Color32::WHITE),
            art: true,
        },
    ]
}

/// The backdrop colour at a home screen position: the ground, its artwork scrims, the shades.
fn home_backdrop(ground: &Ground, at: Pos2) -> Color32 {
    let s = screen();
    let mut bg = match ground.base {
        Base::Fixed(c) => c,
        Base::Lit(theme) => lit_at(theme, at),
    };
    if ground.art {
        bg = over(
            black(artwork::scrim_alpha(at.x / s.width(), at.y / s.height())),
            bg,
        );
    }
    over(black(shade::alpha_at(s, at, true)), bg)
}

/// One measured pair.
struct Line {
    what: String,
    ground: String,
    ratio: f64,
    need: Need,
}

impl Line {
    fn ok(&self) -> bool {
        self.ratio >= self.need.min()
    }
}

struct Report(Vec<Line>);

impl Report {
    fn add(&mut self, what: &str, ground: &str, fg: Color32, bg: Color32, need: Need) {
        let seen = over(fg, bg);
        self.0.push(Line {
            what: what.to_owned(),
            ground: ground.to_owned(),
            ratio: ratio(seen, bg),
            need,
        });
    }
}

/// Home screen text at its places (reference size 1600 x 900).
fn home(report: &mut Report) {
    // (what, ink, style, where, on a keycap)
    let places: [(&str, Color32, Need, Pos2, bool); 14] = [
        (
            "clock",
            color::text_clock(),
            Need::of(ty::CLOCK),
            pos2(1512.0, 55.0),
            false,
        ),
        (
            "window glyphs",
            color::caption_glyph(),
            Need::Glyph,
            pos2(1530.0, 16.0),
            false,
        ),
        (
            "state, neutral",
            color::state_neutral(),
            Need::of(ty::STATE_LABEL),
            pos2(120.0, 522.0),
            false,
        ),
        (
            "state, active",
            color::text_active(),
            Need::of(ty::STATE_LABEL),
            pos2(120.0, 522.0),
            false,
        ),
        (
            "state, update",
            color::update_available(),
            Need::of(ty::STATE_LABEL),
            pos2(120.0, 522.0),
            false,
        ),
        (
            "state, error",
            color::error_state(),
            Need::of(ty::STATE_LABEL),
            pos2(120.0, 522.0),
            false,
        ),
        (
            "fact overline",
            color::text_overline(),
            Need::of(ty::INFO_OVERLINE),
            pos2(330.0, 735.0),
            false,
        ),
        // The facts and the glibc line run to the right when a value or a sentence is long.
        (
            "fact overline, far",
            color::text_overline(),
            Need::of(ty::INFO_OVERLINE),
            pos2(870.0, 735.0),
            false,
        ),
        (
            "fact value, far",
            color::text_value(),
            Need::of(ty::INFO_VALUE),
            pos2(870.0, 758.0),
            false,
        ),
        (
            "warning under Play, far",
            color::warning_text(),
            Need::of(ty::DETAIL_LARGE),
            pos2(870.0, 700.0),
            false,
        ),
        (
            "fact value",
            color::text_value(),
            Need::of(ty::INFO_VALUE),
            pos2(330.0, 758.0),
            false,
        ),
        (
            "warning under Play",
            color::warning_text(),
            Need::of(ty::DETAIL_LARGE),
            pos2(330.0, 700.0),
            false,
        ),
        (
            "hint label",
            color::text_hint(),
            Need::of(ty::HINT_LABEL),
            pos2(1400.0, 862.0),
            false,
        ),
        (
            "hint key",
            color::text_hint_key(),
            Need::of(ty::HINT_KEY),
            pos2(1250.0, 862.0),
            true,
        ),
    ];
    for theme in Theme::ALL {
        for ground in grounds(theme) {
            for (what, ink, need, at, keycap) in places {
                let mut bg = home_backdrop(&ground, at);
                if keycap {
                    bg = over(color::keycap(), bg);
                }
                report.add(what, &ground.name, ink, bg, need);
            }
        }
    }
}

/// The surfaces of a panel over a theme's lit tone.
/// The brightest the system's blur behind a see-through window can be: the dark material over a
/// white wallpaper. The macOS runners show 92 to 96 (sRGB, gray) for Liquid Glass and for the
/// sidebar material there (docs/DESIGN.md 19.2), so 105 leaves a margin.
const WORST_BLUR: u8 = 105;

/// A colour of the theme as seen in a see-through window at its worst: the theme over the plate
/// over the brightest blur.
fn glassed(theme_color: Color32) -> Color32 {
    let under = over(
        black(crate::ui::background::GLASS_PLATE),
        Color32::from_gray(WORST_BLUR),
    );
    over(
        theme_color.gamma_multiply(crate::ui::background::GLASS_VEIL),
        under,
    )
}

fn panel(report: &mut Report) {
    for (theme, glass) in Theme::ALL.into_iter().flat_map(|t| [(t, false), (t, true)]) {
        let name = format!(
            "{}{}",
            theme.title(crate::ui::i18n::Language::English),
            if glass { " glass" } else { "" }
        );
        let under = if glass {
            glassed(lit(theme))
        } else {
            lit(theme)
        };
        let panel = over(color::side_panel(), under);
        let row = over(color::surface_row(), panel);
        let hover = over(color::surface_hover(), panel);
        let focus = over(color::surface_focus(), panel);
        let pressed = over(color::surface_pressed(), panel);
        let surfaces = [
            ("panel", panel),
            ("row", row),
            ("row hover", hover),
            ("row focus", focus),
            ("row pressed", pressed),
        ];
        let inks: [(&str, Color32, Need); 21] = [
            ("row title", Color32::WHITE, Need::of(ty::ROW_TITLE)),
            (
                "row title, inert",
                color::text_inert(),
                Need::of(ty::ROW_TITLE),
            ),
            ("row detail", color::text_row_detail(), Need::of(ty::DETAIL)),
            ("label", color::text_label(), Need::Text),
            ("value", color::text_value(), Need::Text),
            ("hint", color::text_hint(), Need::Text),
            ("subtitle", color::text_subtitle(), Need::Text),
            ("empty state", color::text_empty(), Need::Text),
            ("detail", color::text_detail(), Need::of(ty::DETAIL)),
            ("note", color::text_note(), Need::of(ty::NOTE)),
            (
                "caption",
                color::text_caption(),
                Need::of(ty::SECTION_CAPTION),
            ),
            (
                "overline",
                color::text_overline(),
                Need::of(ty::INFO_OVERLINE),
            ),
            ("count", color::text_count(), Need::Text),
            ("path", color::text_path(), Need::Text),
            ("state, neutral", color::state_neutral(), Need::Text),
            (
                "destructive",
                color::destructive_text(),
                Need::of(ty::ROW_TITLE),
            ),
            ("destructive glyph", color::destructive_icon(), Need::Glyph),
            ("error", color::error_text(), Need::Text),
            ("error state", color::error_state(), Need::Text),
            ("warning", color::warning_text(), Need::Text),
            ("update available", color::update_available(), Need::Text),
        ];
        for (surface, bg) in surfaces {
            for (what, ink, need) in inks {
                // A row never carries a note, a caption or a count (they are bare on the panel).
                if surface != "panel" && matches!(what, "note" | "caption" | "count" | "path") {
                    continue;
                }
                report.add(what, &format!("{name} {surface}"), ink, bg, need);
            }
        }
        // Toasts and banners.
        let card = over(color::card_fill(), lit(theme));
        for (what, ink, need) in [
            ("toast text", Color32::WHITE, Need::Text),
            (
                "banner detail",
                color::text_subtitle(),
                Need::of(ty::BANNER_DETAIL),
            ),
            (
                "banner label",
                color::text_subtitle(),
                Need::of(ty::BANNER_LABEL),
            ),
        ] {
            report.add(what, &format!("{name} card"), ink, card, need);
        }
        // Buttons: the label on the primary in every state, the focus ring on the panel.
        for (state, fill) in [
            ("rest", color::primary()),
            ("hover", color::primary_hover()),
            ("pressed", color::primary_pressed()),
        ] {
            let bg = over(fill, panel);
            report.add(
                "primary label",
                &format!("{name} primary {state}"),
                color::on_primary(),
                bg,
                Need::Text,
            );
        }
        report.add(
            "focus ring",
            &format!("{name} panel"),
            color::focus_ring(),
            panel,
            Need::Glyph,
        );
        // The switch: the knob against the track in each state.
        for accent in Accent::ALL {
            let on = accent.color();
            report.add(
                "switch knob on track",
                &format!(
                    "{name} {}",
                    accent.title(crate::ui::i18n::Language::English)
                ),
                color::on_primary(),
                on,
                Need::Glyph,
            );
        }
        for (state, track) in [
            ("off", color::switch_track_off()),
            ("off, row focused", color::switch_track_off_on_focus()),
        ] {
            report.add(
                "switch knob on track",
                &format!("{name} {state}"),
                Color32::WHITE,
                over(track, panel),
                Need::Glyph,
            );
        }
    }
}

/// The initial in the avatar of the profile button: on the middle of the accent's gradient.
fn avatars(report: &mut Report) {
    for accent in Accent::ALL {
        let bg = crate::screens::home::avatar_middle(accent.color());
        report.add(
            &format!(
                "avatar initial, {}",
                accent.title(crate::ui::i18n::Language::English)
            ),
            "profile button",
            theme::readable_ink(bg),
            bg,
            Need::Text,
        );
    }
    // The first letter on the calm colours of a person without a picture (rows, cards, the
    // Friends header and the profile): one measure for every size the letter grows to.
    for account in 0..6 {
        let bg =
            crate::screens::online::monogram_colour(anyps5_core::online::api::AccountId(account));
        report.add(
            &format!("monogram initial, colour {}", account + 1),
            "avatar without a picture",
            theme::readable_ink(bg),
            bg,
            Need::Text,
        );
    }
}

/// The accent where it is the only cue left: the unlock progress line beside the action row, on
/// the backdrops of the home screen's hero.
fn accents(report: &mut Report) {
    let at = pos2(500.0, 653.0);
    for theme in Theme::ALL {
        for ground in grounds(theme) {
            let bg = home_backdrop(&ground, at);
            for accent in Accent::ALL {
                report.add(
                    &format!(
                        "progress line, {}",
                        accent.title(crate::ui::i18n::Language::English)
                    ),
                    &ground.name,
                    accent.color(),
                    bg,
                    Need::Glyph,
                );
            }
        }
    }
}

fn everything() -> Report {
    let mut report = Report(Vec::new());
    home(&mut report);
    panel(&mut report);
    accents(&mut report);
    avatars(&mut report);
    report
}

fn print(lines: &[&Line]) {
    for l in lines {
        println!(
            "{:5.2}  need {:.1}  {:<26} {}",
            l.ratio,
            l.need.min(),
            l.what,
            l.ground
        );
    }
}

#[test]
fn every_ink_reaches_its_ratio_on_every_theme() {
    let report = everything();
    let failing: Vec<&Line> = report.0.iter().filter(|l| !l.ok()).collect();
    // Weakest pair of each ink, so a round of fixes can see how much room is left.
    let mut names: Vec<&str> = report.0.iter().map(|l| l.what.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    println!("weakest pair per ink ({} pairs measured)", report.0.len());
    let weakest: Vec<&Line> = names
        .iter()
        .filter_map(|n| {
            report
                .0
                .iter()
                .filter(|l| l.what == *n)
                .min_by(|a, b| (a.ratio - a.need.min()).total_cmp(&(b.ratio - b.need.min())))
        })
        .collect();
    print(&weakest);
    if !failing.is_empty() {
        println!("\nFAILING ({})", failing.len());
        print(&failing);
    }
    assert!(
        failing.is_empty(),
        "{} pairs miss their ratio",
        failing.len()
    );
}

#[test]
fn the_formula_matches_the_published_values() {
    // Black on white is 21:1, white on white is 1:1, and #777 on white is the classic 4.48:1.
    assert!((ratio(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 1e-9);
    assert!((ratio(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-9);
    let grey = Color32::from_rgb(0x77, 0x77, 0x77);
    assert!((ratio(grey, Color32::WHITE) - 4.48).abs() < 0.01);
    assert_eq!(over(white(0.5), Color32::BLACK).r(), 128);
}

#[test]
fn the_text_roles_are_sized_as_the_standard_reads_them() {
    assert_eq!(Need::of(ty::DETAIL), Need::Text);
    assert_eq!(Need::of(ty::BUTTON), Need::Large); // 19 dp semibold: large text starts at 18.66 bold
    assert_eq!(Need::of(ty::ROW_TITLE), Need::Text);
    assert_eq!(Need::of(ty::HERO_TITLE), Need::Large);
    assert_eq!(Need::of(ty::TOP_BAR_TITLE), Need::Large);
}
