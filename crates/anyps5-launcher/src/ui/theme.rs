//! Design tokens (docs/DESIGN.md section 1): colours, radii, shadows, spacing, the type scale,
//! the window scale and the bundled fonts.
//!
//! Every length in this module is in **dp**: one egui point equals one design pixel once
//! [`sync_scale`] has set the zoom factor (the original lays everything out for 1600 x 900 and
//! multiplies by one global scale). Widgets therefore use these numbers as they are.

use std::sync::Arc;

use egui::epaint::Shadow;
use egui::{Color32, Context, FontData, FontDefinitions, FontFamily, FontId, FontTweak, RichText};

use crate::ui::i18n::{Key, Language};

// ---------------------------------------------------------------------------------------------
// Window scale (1.1)
// ---------------------------------------------------------------------------------------------

/// Reference window size the layout is designed for.
pub const REFERENCE_SIZE: (f32, f32) = (1600.0, 900.0);
/// Smallest and largest global scale.
pub const SCALE_RANGE: (f32, f32) = (0.68, 1.45);
/// Minimum window size in OS points.
pub const MIN_WINDOW: (f32, f32) = (1100.0, 650.0);

/// `clamp(min(w / 1600, h / 900), 0.68, 1.45)` for a window content size in OS points.
pub fn scale_for(width: f32, height: f32) -> f32 {
    (width / REFERENCE_SIZE.0)
        .min(height / REFERENCE_SIZE.1)
        .clamp(SCALE_RANGE.0, SCALE_RANGE.1)
}

/// Sets the zoom factor so that 1 egui point = 1 dp. Call once at the start of every frame.
/// Returns the scale in use. A scale change shows from the next frame on.
pub fn sync_scale(ctx: &Context) -> f32 {
    let zoom = ctx.zoom_factor();
    let size = ctx.content_rect().size() * zoom; // window size in OS points
    let scale = scale_for(size.x, size.y);
    if (scale - zoom).abs() > 1e-3 {
        ctx.set_zoom_factor(scale);
    }
    scale
}

// ---------------------------------------------------------------------------------------------
// Colours (1.2)
// ---------------------------------------------------------------------------------------------

/// sRGB colour from float channels in 0..=1 and a straight (not premultiplied) alpha.
pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color32 {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(q(r), q(g), q(b), q(a))
}

/// sRGB colour from float channels in 0..=1.
pub fn rgb(r: f32, g: f32, b: f32) -> Color32 {
    rgba(r, g, b, 1.0)
}

/// White with straight alpha, the building block of all chrome.
pub fn white(alpha: f32) -> Color32 {
    rgba(1.0, 1.0, 1.0, alpha)
}

/// Black with straight alpha.
pub fn black(alpha: f32) -> Color32 {
    rgba(0.0, 0.0, 0.0, alpha)
}

/// The ink for text or a glyph on `bg` (opaque): white, or the dark `on_primary` when that reads
/// better. Whichever is better reaches 4.49:1 at worst, on any colour.
pub fn readable_ink(bg: Color32) -> Color32 {
    let lum = |c: Color32| {
        let lin = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    };
    let dark = color::on_primary();
    let (l, ld) = (lum(bg) + 0.05, lum(dark) + 0.05);
    if 1.05 / l >= l / ld {
        Color32::WHITE
    } else {
        dark
    }
}

/// `color` with its alpha multiplied by `factor`.
pub fn fade(color: Color32, factor: f32) -> Color32 {
    let [r, g, b, a] = color.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(r, g, b, (a as f32 * factor.clamp(0.0, 1.0)).round() as u8)
}

/// Linear interpolation in sRGB on straight (unmultiplied) channels; `t` is clamped to 0..=1.
pub fn mix(from: Color32, to: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let a = from.to_srgba_unmultiplied();
    let b = to.to_srgba_unmultiplied();
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2]), l(a[3], b[3]))
}

/// The three built-in looks: an animated background plus a default accent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Theme {
    #[default]
    Cosmic,
    Aurora,
    Sunset,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Cosmic, Theme::Aurora, Theme::Sunset];

    /// 0 = Cosmic, 1 = Aurora, 2 = Sunset (also the shader's theme index).
    pub const fn index(self) -> u32 {
        match self {
            Theme::Cosmic => 0,
            Theme::Aurora => 1,
            Theme::Sunset => 2,
        }
    }

    pub fn from_index(index: u32) -> Theme {
        match index {
            1 => Theme::Aurora,
            2 => Theme::Sunset,
            _ => Theme::Cosmic,
        }
    }

    /// Localised name.
    pub fn title(self, language: Language) -> &'static str {
        language.text(match self {
            Theme::Cosmic => Key::ThemeCosmic,
            Theme::Aurora => Key::ThemeAurora,
            Theme::Sunset => Key::ThemeSunset,
        })
    }

    /// Deep, middle and bright tones the shader blends (display-referred sRGB floats).
    pub const fn palette(self) -> Palette {
        match self {
            Theme::Cosmic => Palette {
                deep: [0.012, 0.024, 0.090],
                middle: [0.040, 0.150, 0.420],
                bright: [0.420, 0.640, 1.000],
            },
            Theme::Aurora => Palette {
                deep: [0.006, 0.030, 0.045],
                middle: [0.020, 0.300, 0.300],
                bright: [0.560, 1.000, 0.780],
            },
            Theme::Sunset => Palette {
                deep: [0.060, 0.010, 0.040],
                middle: [0.520, 0.110, 0.220],
                bright: [1.000, 0.640, 0.330],
            },
        }
    }

    pub const fn default_accent(self) -> Accent {
        match self {
            Theme::Cosmic => Accent::Blue,
            Theme::Aurora => Accent::Mint,
            Theme::Sunset => Accent::Coral,
        }
    }

    /// Middle-to-deep vertical gradient, the fallback when the GPU shader is unavailable.
    pub fn fallback_gradient(self) -> (Color32, Color32) {
        let p = self.palette();
        (p.middle_color(), p.deep_color())
    }
}

/// The three tones of a theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub deep: [f32; 3],
    pub middle: [f32; 3],
    pub bright: [f32; 3],
}

impl Palette {
    pub fn deep_color(&self) -> Color32 {
        rgb(self.deep[0], self.deep[1], self.deep[2])
    }
    pub fn middle_color(&self) -> Color32 {
        rgb(self.middle[0], self.middle[1], self.middle[2])
    }
    pub fn bright_color(&self) -> Color32 {
        rgb(self.bright[0], self.bright[1], self.bright[2])
    }
}

/// User-selectable accent colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Accent {
    #[default]
    Blue,
    Mint,
    Coral,
    Violet,
    Gold,
    White,
}

impl Accent {
    pub const ALL: [Accent; 6] = [
        Accent::Blue,
        Accent::Mint,
        Accent::Coral,
        Accent::Violet,
        Accent::Gold,
        Accent::White,
    ];

    pub fn color(self) -> Color32 {
        match self {
            Accent::Blue => rgb(0.40, 0.64, 1.0),
            Accent::Mint => rgb(0.36, 0.92, 0.74),
            Accent::Coral => rgb(1.0, 0.50, 0.40),
            Accent::Violet => rgb(0.70, 0.57, 1.0),
            Accent::Gold => rgb(1.0, 0.80, 0.36),
            Accent::White => rgb(0.94, 0.94, 0.94),
        }
    }

    /// Localised name (not part of the string table in the original either).
    pub fn title(self, language: Language) -> &'static str {
        let tr = language == Language::Turkish;
        match self {
            Accent::Blue => {
                if tr {
                    "Mavi"
                } else {
                    "Blue"
                }
            }
            Accent::Mint => {
                if tr {
                    "Nane"
                } else {
                    "Mint"
                }
            }
            Accent::Coral => {
                if tr {
                    "Mercan"
                } else {
                    "Coral"
                }
            }
            Accent::Violet => {
                if tr {
                    "Mor"
                } else {
                    "Violet"
                }
            }
            Accent::Gold => {
                if tr {
                    "Altın"
                } else {
                    "Gold"
                }
            }
            Accent::White => {
                if tr {
                    "Beyaz"
                } else {
                    "White"
                }
            }
        }
    }
}

/// Trophy grades with their colour and point value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Grade {
    Platinum,
    Gold,
    Silver,
    Bronze,
}

impl Grade {
    pub const ALL: [Grade; 4] = [Grade::Platinum, Grade::Gold, Grade::Silver, Grade::Bronze];

    pub fn color(self) -> Color32 {
        match self {
            Grade::Platinum => rgb(0.74, 0.84, 0.97),
            Grade::Gold => rgb(0.97, 0.78, 0.34),
            Grade::Silver => rgb(0.80, 0.82, 0.86),
            Grade::Bronze => rgb(0.83, 0.55, 0.36),
        }
    }

    pub const fn points(self) -> u32 {
        match self {
            Grade::Platinum => 300,
            Grade::Gold => 90,
            Grade::Silver => 30,
            Grade::Bronze => 15,
        }
    }

    pub fn title(self, language: Language) -> &'static str {
        language.text(match self {
            Grade::Platinum => Key::GradePlatinum,
            Grade::Gold => Key::GradeGold,
            Grade::Silver => Key::GradeSilver,
            Grade::Bronze => Key::GradeBronze,
        })
    }
}

/// `unlocked points * 100 / total points`, integer division, 0 when the total is 0.
pub fn trophy_percent(unlocked_points: u32, total_points: u32) -> u32 {
    if total_points == 0 {
        0
    } else {
        (u64::from(unlocked_points) * 100 / u64::from(total_points)) as u32
    }
}

/// Fixed UI colours (all sRGB, straight alpha). Named by role, never by value.
pub mod color {
    use super::*;

    pub fn window_background() -> Color32 {
        Color32::BLACK
    }
    // Text
    pub fn text() -> Color32 {
        white(1.0)
    }
    pub fn text_label() -> Color32 {
        white(0.9)
    }
    pub fn text_value() -> Color32 {
        white(0.85)
    }
    /// The key name or face button inside a hint keycap.
    pub fn text_hint_key() -> Color32 {
        white(1.0)
    }
    pub fn text_hint() -> Color32 {
        white(0.78)
    }
    pub fn text_subtitle() -> Color32 {
        white(0.72)
    }
    pub fn text_empty() -> Color32 {
        white(0.72)
    }
    pub fn text_detail() -> Color32 {
        white(0.70)
    }
    pub fn text_overline() -> Color32 {
        white(0.70)
    }
    /// The line under a row's title.
    pub fn text_row_detail() -> Color32 {
        white(0.70)
    }
    /// A note under rows, bare on the panel.
    pub fn text_note() -> Color32 {
        white(0.5)
    }
    /// A section caption over a group of rows.
    pub fn text_caption() -> Color32 {
        white(0.55)
    }
    /// The title of a row that cannot be focused (a fact, not a control).
    pub fn text_inert() -> Color32 {
        white(0.70)
    }
    /// The clock in the top bar: a status, quieter than the controls beside it.
    pub fn text_clock() -> Color32 {
        white(0.88)
    }
    pub fn text_count() -> Color32 {
        white(0.5)
    }
    pub fn text_path() -> Color32 {
        white(0.5)
    }
    pub fn text_inactive() -> Color32 {
        white(0.4)
    }
    // Surfaces
    pub fn surface() -> Color32 {
        white(0.10)
    }
    pub fn surface_row() -> Color32 {
        white(0.07)
    }
    pub fn surface_trophy_row() -> Color32 {
        white(0.05)
    }
    pub fn surface_tile_placeholder() -> Color32 {
        white(0.08)
    }
    pub fn surface_prominent() -> Color32 {
        white(0.26)
    }
    pub fn surface_hover() -> Color32 {
        white(0.16)
    }
    /// A control that has the focus: one step above hover (the ring is the indicator, DESIGN.md 3.9).
    pub fn surface_focus() -> Color32 {
        white(0.20)
    }
    /// A control under the pointer button.
    pub fn surface_pressed() -> Color32 {
        white(0.26)
    }
    /// The edge of a secondary control at rest and under the pointer or focus.
    pub fn hairline_control() -> Color32 {
        white(0.12)
    }
    pub fn hairline_hover() -> Color32 {
        white(0.32)
    }
    /// The primary button: ink, not accent (one per view).
    pub fn primary() -> Color32 {
        white(0.92)
    }
    pub fn primary_hover() -> Color32 {
        Color32::WHITE
    }
    pub fn primary_pressed() -> Color32 {
        white(0.78)
    }
    /// The label on the primary button: 16:1 on its fill.
    pub fn on_primary() -> Color32 {
        rgb(0.04, 0.05, 0.09)
    }
    pub fn selected_category() -> Color32 {
        white(0.12)
    }
    // Hairlines
    pub fn hairline() -> Color32 {
        white(0.07)
    }
    pub fn hairline_card() -> Color32 {
        white(0.14)
    }
    pub fn hairline_glass() -> Color32 {
        white(0.09)
    }
    pub fn hairline_panel_edge() -> Color32 {
        white(0.08)
    }
    pub fn track() -> Color32 {
        white(0.16)
    }
    pub fn keycap() -> Color32 {
        white(0.16)
    }
    // Scrims and panels
    pub fn scrim_side_panel() -> Color32 {
        black(0.35)
    }
    pub fn panel_fullscreen() -> Color32 {
        black(0.74)
    }
    pub fn panel_log() -> Color32 {
        black(0.65)
    }
    pub fn splash_overlay() -> Color32 {
        black(0.55)
    }
    /// `Color(white: 0.06)` (a near-black gray, not white with alpha) at 0.92: the side panel fill.
    pub fn side_panel() -> Color32 {
        rgba(0.06, 0.06, 0.06, 0.92)
    }
    /// `Color(white: 0.07)` at 0.94: toast and banner card fill.
    pub fn card_fill() -> Color32 {
        rgba(0.07, 0.07, 0.07, 0.94)
    }
    pub fn switch_track_off() -> Color32 {
        white(0.18)
    }
    /// The off track of a switch whose row has the focus: one step above the lifted row.
    pub fn switch_track_off_on_focus() -> Color32 {
        white(0.30)
    }
    // Status
    pub fn destructive_text() -> Color32 {
        rgb(1.0, 0.72, 0.67)
    }
    pub fn destructive_icon() -> Color32 {
        rgb(1.0, 0.62, 0.57)
    }
    pub fn error_text() -> Color32 {
        rgb(1.0, 0.73, 0.68)
    }
    pub fn error_state() -> Color32 {
        rgb(1.0, 0.72, 0.67)
    }
    pub fn warning_text() -> Color32 {
        rgb(1.0, 0.78, 0.50)
    }
    pub fn update_available() -> Color32 {
        rgb(1.0, 0.82, 0.40)
    }
    /// Secondary state label colour ("ready", "locked", "queued").
    pub fn state_neutral() -> Color32 {
        white(0.75)
    }
    /// A state in progress ("Starting", "Installing", "Playing"): ink, not the accent. The accent
    /// is the player's pick and can sit on its own colour; the glyph and the word are the cue.
    pub fn text_active() -> Color32 {
        white(0.92)
    }
    /// Window caption glyphs (minimise, maximise, close) at rest; white on hover.
    pub fn caption_glyph() -> Color32 {
        white(0.6)
    }
    /// Behind a hovered minimise or maximise button.
    pub fn caption_hover() -> Color32 {
        white(0.08)
    }
    /// Behind a hovered close button: the Windows close red.
    pub fn caption_close() -> Color32 {
        Color32::from_rgb(0xC4, 0x2B, 0x1C)
    }
}

// ---------------------------------------------------------------------------------------------
// Radii, sizes, shadows (1.3 - 1.5)
// ---------------------------------------------------------------------------------------------

/// Corner radii in dp.
pub mod radius {
    pub const CONSOLE_BUTTON: f32 = 14.0;
    pub const ICON_BUTTON: f32 = 22.0;
    pub const CARD: f32 = 16.0;
    pub const ROW: f32 = 12.0;
    pub const TROPHY_ROW: f32 = 16.0;
    pub const LANGUAGE_CARD: f32 = 20.0;
    pub const THEME_CARD: f32 = 18.0;
    pub const KEYCAP_HINT: f32 = 6.0;
    pub const KEYCAP: f32 = 7.0;
    pub const TOAST: f32 = 28.0;
    pub const BANNER: f32 = 20.0;
    pub const BOX: f32 = 14.0;

    /// Game tile radius for a tile of `size` dp.
    pub fn tile(size: f32) -> f32 {
        size * 0.12
    }
    /// Trophy icon radius.
    pub fn trophy_icon(size: f32) -> f32 {
        size * 0.18
    }
}

/// Sizes and paddings in dp.
pub mod size {
    pub const TILE_SMALL: f32 = 118.0;
    pub const TILE_LARGE: f32 = 176.0;
    pub const TILE_GAP: f32 = 16.0;
    pub const TILE_LABEL_COLUMN: f32 = 10.0;
    pub const LIBRARY_TILE: f32 = 196.0;
    pub const SPLASH_TILE: f32 = 200.0;
    pub const ONBOARDING_TILE: f32 = 84.0;
    /// The control family's three heights (DESIGN.md 3.9): the action row and dialogs, the top bar
    /// and an answer inside a line of text. Every control is a capsule.
    pub const CONTROL_L: f32 = 54.0;
    pub const CONTROL_M: f32 = 44.0;
    pub const CONTROL_S: f32 = 32.0;
    /// Between neighbouring controls, everywhere.
    pub const CONTROL_GAP: f32 = 12.0;
    /// The focus ring of a control: white, 2 dp wide, outside the shape. Tiles and cards use 3.
    pub const FOCUS_RING: f32 = 2.0;
    pub const BUTTON_MIN_HEIGHT: f32 = CONTROL_L;
    pub const BUTTON_PADDING: f32 = 30.0;
    pub const BUTTON_PADDING_ICON_ONLY: f32 = 16.0;
    pub const BUTTON_ICON: f32 = 17.0;
    /// The glyph of a button without text, such as the "more" dots.
    pub const BUTTON_ICON_ONLY: f32 = 22.0;
    pub const BUTTON_GAP: f32 = 10.0;
    /// A chip: 32 high, 14 beside its text, 12 beside a glyph, a 14 dp glyph.
    pub const CHIP_HEIGHT: f32 = 32.0;
    pub const CHIP_PADDING: f32 = 14.0;
    pub const CHIP_ICON: f32 = 14.0;
    pub const CHIP_GAP: f32 = 6.0;
    pub const PLAY_MIN_WIDTH: f32 = 210.0;
    pub const ICON_BUTTON: f32 = 44.0;
    pub const ICON_BUTTON_GLYPH: f32 = 20.0;
    pub const AVATAR: f32 = 36.0;
    pub const INFO_CARD: (f32, f32) = (236.0, 96.0);
    pub const INFO_CARD_PADDING: f32 = 18.0;
    pub const INFO_CARD_GAP: f32 = 14.0;
    pub const SIDE_PANEL_WIDTH: f32 = 560.0;
    pub const SIDE_PANEL_PADDING: f32 = 36.0;
    /// Inset of row lists inside their panel (room for the focused row to grow).
    pub const LIST_INSET: f32 = 6.0;
    pub const SETTINGS_CATEGORY_WIDTH: f32 = 330.0;
    pub const SETTINGS_CATEGORY_ROW: f32 = 52.0;
    pub const ROW_MIN_HEIGHT: f32 = 58.0;
    pub const ROW_PADDING_X: f32 = 18.0;
    pub const ROW_GAP: f32 = 8.0;
    pub const ROW_ICON_BOX: f32 = 24.0;
    pub const ROW_ICON: f32 = 17.0;
    pub const CHEVRON_BOX: f32 = 26.0;
    pub const CHEVRON_GLYPH: f32 = 13.0;
    pub const CHOICE_VALUE_WIDTH: (f32, f32) = (100.0, 190.0);
    pub const STEPPER_VALUE_WIDTH: f32 = 100.0;
    pub const TEXT_FIELD_WIDTH: f32 = 200.0;
    /// A window caption button, in points at zoom 1 (the Windows 11 size, not scaled with the UI).
    pub const CAPTION_BUTTON: (f32, f32) = (46.0, 32.0);
    pub const CAPTION_GLYPH: f32 = 10.0;
    /// The band along the window edges that resizes, in points at zoom 1.
    pub const RESIZE_EDGE: f32 = 6.0;
    pub const SWITCH: (f32, f32) = (50.0, 28.0);
    pub const SWITCH_INSET: f32 = 3.0;
    pub const KEYCAP_MIN: (f32, f32) = (30.0, 28.0);
    pub const KEYCAP_PADDING_X: f32 = 9.0;
    pub const KEYCAP_GAP: f32 = 6.0;
    pub const TROPHY_ICON: f32 = 68.0;
    pub const TROPHY_HEADER_ICON: f32 = 112.0;
    pub const LANGUAGE_CARD: (f32, f32) = (280.0, 150.0);
    pub const THEME_CARD_WIDTH: f32 = 330.0;
    pub const THEME_CARD_PREVIEW: (f32, f32) = (330.0, 186.0);
    pub const PROGRESS_HEIGHT: f32 = 4.0;
    pub const HERO_PROGRESS_WIDTH: f32 = 260.0;
    pub const TOAST_TOP: f32 = 28.0;
    pub const HINT_BAR_GAP: f32 = 28.0;
    pub const HINT_GAP: f32 = 8.0;
    /// A hint key: a 26 dp circle for a face button, a 26 dp high keycap for a key name.
    pub const HINT_CAP: f32 = 26.0;
    pub const HINT_CAP_PADDING: f32 = 8.0;
    pub const HINT_GLYPH: f32 = 12.0;

    /// Progress line height inside a tile: `max(3, size * 0.03)`.
    pub fn tile_progress_height(tile: f32) -> f32 {
        (tile * 0.03).max(3.0)
    }
}

/// Spacing between sections of screens, in dp.
pub mod pad {
    pub const HOME_X: f32 = 64.0;
    pub const HOME_TOP: f32 = 34.0;
    pub const HERO_X: f32 = 64.0;
    pub const HINT_Y: f32 = 28.0;
    pub const ONBOARDING: (f32, f32) = (110.0, 80.0);
    /// The top of a full-screen panel's header: the Windows caption buttons are 32 dp tall in the
    /// corner, and the header keeps this much clear of them.
    pub const PANEL_TOP: f32 = super::size::CAPTION_BUTTON.1 + 32.0;
    pub const SETTINGS: (f32, f32, f32) = (90.0, 70.0, 50.0); // horizontal, top, bottom
    pub const LIBRARY: (f32, f32) = (90.0, PANEL_TOP);
    pub const TROPHIES: (f32, f32) = (90.0, PANEL_TOP);
    pub const LOG: f32 = 90.0;
}

/// A SwiftUI `shadow(radius: r, y:)` as an egui shadow (`blur = 2 * r`, estimate from DESIGN.md 1.4).
pub fn shadow(color: Color32, swiftui_radius: f32, y: f32) -> Shadow {
    Shadow {
        offset: [0, y.round().clamp(-127.0, 127.0) as i8],
        blur: (swiftui_radius * 2.0).round().clamp(0.0, 255.0) as u8,
        spread: 0,
        color,
    }
}

/// Named shadows, softened from section 1.4 (DESIGN.md 3.7): elevation is a quiet dark shadow,
/// focus is the white fill or ring, with only a faint halo behind it. `t` is the focus amount
/// (0..=1) where a shadow animates.
pub mod shadows {
    use super::*;

    /// Tile: black 0.22 -> 0.32, radius 12 -> 18 when focused, y 6 -> 8.
    pub fn tile(t: f32) -> Shadow {
        let t = t.clamp(0.0, 1.0);
        shadow(black(0.22 + 0.10 * t), 12.0 + 6.0 * t, 6.0 + 2.0 * t)
    }
    /// Halo behind a focus ring: white 0.14, radius 8, scaled by the focus amount.
    pub fn focus_glow(t: f32) -> Shadow {
        shadow(white(0.14 * t.clamp(0.0, 1.0)), 8.0, 0.0)
    }
    pub fn toast() -> Shadow {
        shadow(black(0.30), 16.0, 6.0)
    }
    pub fn banner() -> Shadow {
        shadow(black(0.34), 16.0, 6.0)
    }
    pub fn switch_knob() -> Shadow {
        shadow(black(0.25), 2.0, 1.0)
    }
    pub fn progress_glow(accent: Color32) -> Shadow {
        shadow(fade(accent, 0.35), 4.0, 0.0)
    }
}

// ---------------------------------------------------------------------------------------------
// Typography (1.6)
// ---------------------------------------------------------------------------------------------

/// Font weights the design uses (numbers are the CSS weights).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Light,
    Regular,
    Medium,
    SemiBold,
    Bold,
}

impl Weight {
    pub const ALL: [Weight; 5] = [
        Weight::Light,
        Weight::Regular,
        Weight::Medium,
        Weight::SemiBold,
        Weight::Bold,
    ];

    pub const fn value(self) -> f32 {
        match self {
            Weight::Light => 300.0,
            Weight::Regular => 400.0,
            Weight::Medium => 500.0,
            Weight::SemiBold => 600.0,
            Weight::Bold => 700.0,
        }
    }

    const fn slug(self) -> &'static str {
        match self {
            Weight::Light => "light",
            Weight::Regular => "regular",
            Weight::Medium => "medium",
            Weight::SemiBold => "semibold",
            Weight::Bold => "bold",
        }
    }
}

/// At and above this size (dp) the Display optical size is used (SF Pro switches at 20 pt).
pub const DISPLAY_SIZE_FROM: f32 = 20.0;

fn inter_family(weight: Weight, display: bool) -> FontFamily {
    let name = if display {
        format!("inter-display-{}", weight.slug())
    } else {
        format!("inter-{}", weight.slug())
    };
    FontFamily::Name(name.into())
}

/// Inter at `weight`; sizes of 20 dp and up use the Display optical size.
pub fn font(size: f32, weight: Weight) -> FontId {
    FontId::new(size, inter_family(weight, size >= DISPLAY_SIZE_FROM))
}

/// JetBrains Mono Regular (log view).
pub fn font_mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("mono".into()))
}

/// Phosphor icon font, regular weight.
pub fn font_icon(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("phosphor".into()))
}

/// Phosphor icon font, fill weight.
pub fn font_icon_fill(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("phosphor-fill".into()))
}

/// A named text role from the type scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f32,
    pub weight: Weight,
    /// Extra letter spacing in dp (uppercase overlines only).
    pub tracking: f32,
}

impl TextStyle {
    pub const fn new(size: f32, weight: Weight) -> Self {
        Self {
            size,
            weight,
            tracking: 0.0,
        }
    }

    pub const fn tracked(size: f32, weight: Weight, tracking: f32) -> Self {
        Self {
            size,
            weight,
            tracking,
        }
    }

    pub fn font(&self) -> FontId {
        font(self.size, self.weight)
    }

    /// Natural line height: 1.2 * size.
    pub fn line_height(&self) -> f32 {
        1.2 * self.size
    }

    pub fn rich(&self, text: impl Into<String>) -> RichText {
        let mut rich = RichText::new(text)
            .font(self.font())
            .line_height(Some(self.line_height()));
        if self.tracking != 0.0 {
            rich = rich.extra_letter_spacing(self.tracking);
        }
        rich
    }

    /// Rich text with a colour.
    pub fn colored(&self, text: impl Into<String>, color: Color32) -> RichText {
        self.rich(text).color(color)
    }
}

/// The type scale of section 1.6. Overlines are uppercase: call `.to_uppercase()` on the text.
pub mod ty {
    use super::{TextStyle, Weight};

    pub const HERO_TITLE: TextStyle = TextStyle::new(58.0, Weight::Light);
    pub const ONBOARDING_TITLE: TextStyle = TextStyle::new(54.0, Weight::Light);
    pub const SCREEN_TITLE: TextStyle = TextStyle::new(40.0, Weight::Light);
    pub const LOG_TITLE: TextStyle = TextStyle::new(30.0, Weight::Light);
    pub const EMPTY_TITLE: TextStyle = TextStyle::new(34.0, Weight::Light);
    pub const SPLASH_TITLE: TextStyle = TextStyle::new(34.0, Weight::Light);
    pub const TROPHY_LIST_TITLE: TextStyle = TextStyle::new(36.0, Weight::Light);
    pub const SIDE_PANEL_TITLE: TextStyle = TextStyle::new(30.0, Weight::Light);
    pub const TOP_BAR_TITLE: TextStyle = TextStyle::new(24.0, Weight::SemiBold);
    pub const LIBRARY_COUNT: TextStyle = TextStyle::new(22.0, Weight::Medium);
    pub const TILE_NAME: TextStyle = TextStyle::new(22.0, Weight::SemiBold);
    pub const THEME_CARD_NAME: TextStyle = TextStyle::new(22.0, Weight::Regular);
    pub const THEME_CARD_NAME_FOCUSED: TextStyle = TextStyle::new(22.0, Weight::SemiBold);
    pub const PERCENT: TextStyle = TextStyle::new(22.0, Weight::SemiBold);
    pub const LANGUAGE_NAME: TextStyle = TextStyle::new(28.0, Weight::Medium);
    pub const CLOCK: TextStyle = TextStyle::new(17.0, Weight::Medium);
    pub const BUTTON: TextStyle = TextStyle::new(19.0, Weight::SemiBold);
    /// A button of the top bar's size.
    pub const BUTTON_MEDIUM: TextStyle = TextStyle::new(16.0, Weight::SemiBold);
    pub const CATEGORY: TextStyle = TextStyle::new(19.0, Weight::Regular);
    pub const CATEGORY_SELECTED: TextStyle = TextStyle::new(19.0, Weight::SemiBold);
    pub const ONBOARDING_SUBTITLE: TextStyle = TextStyle::new(19.0, Weight::Regular);
    pub const ROW_TITLE: TextStyle = TextStyle::new(17.0, Weight::Medium);
    /// The question of a confirmation, under the panel title: body text.
    pub const QUESTION: TextStyle = TextStyle::new(17.0, Weight::Regular);
    pub const TROPHY_NAME: TextStyle = TextStyle::new(18.0, Weight::SemiBold);
    pub const ONBOARDING_STATUS: TextStyle = TextStyle::new(18.0, Weight::Medium);
    pub const INFO_VALUE: TextStyle = TextStyle::new(17.0, Weight::SemiBold);
    pub const PROFILE: TextStyle = TextStyle::new(16.0, Weight::Medium);
    pub const TOAST: TextStyle = TextStyle::new(16.0, Weight::Medium);
    pub const CHOICE_VALUE: TextStyle = TextStyle::new(16.0, Weight::Regular);
    pub const DIAGNOSIS_LOG: TextStyle = TextStyle::new(16.0, Weight::Medium);
    pub const ROW_INFO: TextStyle = TextStyle::new(15.0, Weight::Medium);
    pub const PROGRESS_LABEL: TextStyle = TextStyle::new(15.0, Weight::Medium);
    pub const PROGRESS_PERCENT: TextStyle = TextStyle::new(15.0, Weight::SemiBold);
    pub const HINT_LABEL: TextStyle = TextStyle::new(15.0, Weight::Medium);
    pub const HINT_KEY: TextStyle = TextStyle::new(13.0, Weight::SemiBold);
    pub const DETAIL: TextStyle = TextStyle::new(13.0, Weight::Regular);
    /// A section caption over a group of rows, and a note under them.
    pub const SECTION_CAPTION: TextStyle = TextStyle::new(14.0, Weight::SemiBold);
    pub const NOTE: TextStyle = TextStyle::new(14.0, Weight::Regular);
    pub const DETAIL_LARGE: TextStyle = TextStyle::new(14.0, Weight::Regular);
    pub const STATE_LABEL: TextStyle = TextStyle::new(14.0, Weight::Medium);
    pub const STATE_LABEL_SMALL: TextStyle = TextStyle::new(12.0, Weight::Medium);
    /// The small chips of the game page (trophy count, the answers to "How did it run?").
    pub const CHIP: TextStyle = TextStyle::new(14.0, Weight::Medium);
    pub const INFO_OVERLINE: TextStyle = TextStyle::tracked(13.0, Weight::SemiBold, 0.6);
    pub const ONBOARDING_OVERLINE: TextStyle = TextStyle::tracked(14.0, Weight::SemiBold, 0.8);
    pub const TROPHIES_OVERLINE: TextStyle = TextStyle::tracked(13.0, Weight::SemiBold, 0.8);
    pub const KEY_CAP: TextStyle = TextStyle::new(13.5, Weight::SemiBold);
    pub const BANNER_LABEL: TextStyle = TextStyle::new(13.0, Weight::SemiBold);
    pub const BANNER_NAME: TextStyle = TextStyle::new(19.0, Weight::SemiBold);
    pub const BANNER_DETAIL: TextStyle = TextStyle::new(13.0, Weight::Regular);
}

// ---------------------------------------------------------------------------------------------
// Bundled fonts
// ---------------------------------------------------------------------------------------------

const INTER_VARIABLE: &[u8] = include_bytes!("../../assets/fonts/InterVariable.ttf");
const JETBRAINS_MONO: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
const PHOSPHOR: &[u8] = include_bytes!("../../assets/fonts/Phosphor.ttf");
const PHOSPHOR_FILL: &[u8] = include_bytes!("../../assets/fonts/Phosphor-Fill.ttf");

/// Optical size axis values: Inter's text cut (14) and its Display cut (32).
const OPSZ_TEXT: f32 = 14.0;
const OPSZ_DISPLAY: f32 = 32.0;

/// The font definitions of the launcher: Inter (one variable file, ten named instances: five
/// weights at the text and the display optical size), JetBrains Mono and Phosphor.
/// Windows' own fonts for CJK and Arabic names come from eframe's system font fallback.
pub fn font_definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let inter: Arc<[u8]> = Arc::from(INTER_VARIABLE);
    let blob: egui::epaint::text::Blob = Arc::new(inter);
    for display in [false, true] {
        for weight in Weight::ALL {
            let family = inter_family(weight, display);
            let FontFamily::Name(name) = &family else {
                unreachable!()
            };
            let key = name.to_string();
            let tweak = FontTweak {
                coords: egui::epaint::text::VariationCoords::new([
                    (b"wght", weight.value()),
                    (b"opsz", if display { OPSZ_DISPLAY } else { OPSZ_TEXT }),
                ]),
                ..FontTweak::default()
            };
            defs.font_data.insert(
                key.clone(),
                Arc::new(FontData::from_blob(blob.clone(), 0).tweak(tweak)),
            );
            defs.families.insert(family, vec![key]);
        }
    }
    let regular = "inter-regular".to_owned();
    defs.families
        .insert(FontFamily::Proportional, vec![regular.clone()]);
    for (key, bytes, family) in [
        ("jetbrains-mono", JETBRAINS_MONO, "mono"),
        ("phosphor", PHOSPHOR, "phosphor"),
        ("phosphor-fill", PHOSPHOR_FILL, "phosphor-fill"),
    ] {
        defs.font_data
            .insert(key.to_owned(), Arc::new(FontData::from_static(bytes)));
        defs.families
            .insert(FontFamily::Name(family.into()), vec![key.to_owned()]);
    }
    defs.families
        .insert(FontFamily::Monospace, vec!["jetbrains-mono".to_owned()]);
    defs
}

/// Visuals the launcher wants from egui itself (dark, no strokes on the default widgets).
pub fn install(ctx: &Context) {
    ctx.set_fonts(font_definitions());
    ctx.options_mut(|o| o.zoom_with_keyboard = false);
    ctx.set_visuals(egui::Visuals {
        panel_fill: color::window_background(),
        window_fill: color::window_background(),
        override_text_color: Some(color::text()),
        ..egui::Visuals::dark()
    });
    ctx.all_styles_mut(|style| {
        style.animation_time = 0.0; // all motion goes through ui::anim
        style.interaction.selectable_labels = false;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_follows_the_smaller_axis_and_clamps() {
        assert!((scale_for(1600.0, 900.0) - 1.0).abs() < 1e-6);
        assert!((scale_for(3200.0, 900.0) - 1.0).abs() < 1e-6); // limited by height
        assert_eq!(scale_for(800.0, 450.0), SCALE_RANGE.0);
        assert_eq!(scale_for(5000.0, 3000.0), SCALE_RANGE.1);
        assert!((scale_for(1100.0, 650.0) - 0.6875).abs() < 1e-3);
    }

    #[test]
    fn palette_hex_matches_the_spec() {
        let hex = |c: Color32| format!("{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
        let p = Theme::Cosmic.palette();
        assert_eq!(hex(p.deep_color()), "030617");
        assert_eq!(hex(p.middle_color()), "0A266B");
        assert_eq!(hex(p.bright_color()), "6BA3FF");
        let p = Theme::Aurora.palette();
        assert_eq!(hex(p.deep_color()), "02080B");
        assert_eq!(hex(p.middle_color()), "054D4D");
        assert_eq!(hex(p.bright_color()), "8FFFC7");
        let p = Theme::Sunset.palette();
        assert_eq!(hex(p.deep_color()), "0F030A");
        assert_eq!(hex(p.middle_color()), "851C38");
        assert_eq!(hex(p.bright_color()), "FFA354");
    }

    #[test]
    fn accent_and_grade_hex_match_the_spec() {
        let hex = |c: Color32| format!("{:02X}{:02X}{:02X}", c.r(), c.g(), c.b());
        let accents = ["66A3FF", "5CEBBD", "FF8066", "B391FF", "FFCC5C", "F0F0F0"];
        for (accent, want) in Accent::ALL.iter().zip(accents) {
            assert_eq!(hex(accent.color()), want, "{accent:?}");
        }
        let grades = ["BDD6F7", "F7C757", "CCD1DB", "D48C5C"];
        for (grade, want) in Grade::ALL.iter().zip(grades) {
            assert_eq!(hex(grade.color()), want, "{grade:?}");
        }
        assert_eq!(
            Grade::ALL.map(Grade::points),
            [300, 90, 30, 15],
            "trophy points"
        );
    }

    #[test]
    fn default_accents_follow_the_theme() {
        assert_eq!(Theme::Cosmic.default_accent(), Accent::Blue);
        assert_eq!(Theme::Aurora.default_accent(), Accent::Mint);
        assert_eq!(Theme::Sunset.default_accent(), Accent::Coral);
    }

    #[test]
    fn trophy_percent_uses_integer_division() {
        assert_eq!(trophy_percent(0, 0), 0);
        assert_eq!(trophy_percent(1, 3), 33);
        assert_eq!(trophy_percent(300, 300), 100);
    }

    #[test]
    fn radii_scale_with_the_tile() {
        assert!((radius::tile(176.0) - 21.12).abs() < 1e-3);
        assert!((radius::tile(118.0) - 14.16).abs() < 1e-3);
        assert!((radius::trophy_icon(112.0) - 20.16).abs() < 1e-3);
    }

    #[test]
    fn display_cut_starts_at_twenty_dp() {
        assert_eq!(
            font(19.9, Weight::Medium).family,
            FontFamily::Name("inter-medium".into())
        );
        assert_eq!(
            font(20.0, Weight::Medium).family,
            FontFamily::Name("inter-display-medium".into())
        );
    }

    #[test]
    fn the_ink_on_a_colour_is_the_one_that_reads_better() {
        assert_eq!(readable_ink(Color32::BLACK), Color32::WHITE);
        assert_eq!(readable_ink(Color32::WHITE), color::on_primary());
        for accent in Accent::ALL {
            let bg = accent.color();
            let ink = readable_ink(bg);
            let (a, b) = (lum_of(ink), lum_of(bg));
            let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
            assert!(ratio >= 4.49, "{accent:?} {ratio}");
        }
    }

    fn lum_of(c: Color32) -> f32 {
        let lin = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    #[test]
    fn straight_alpha_survives_the_conversion() {
        let c = white(0.12);
        assert_eq!(c.to_srgba_unmultiplied(), [255, 255, 255, 31]);
        assert_eq!(
            mix(black(0.0), white(1.0), 0.5).to_srgba_unmultiplied()[3],
            128
        );
    }
}
