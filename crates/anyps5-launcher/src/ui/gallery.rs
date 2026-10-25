//! Hidden `--gallery` mode: renders the components and the three backgrounds so they can be
//! screenshotted and compared with the original (`docs/screenshots/launcher-themes.jpg`).
//!
//! ```text
//! anyplay-launcher --gallery[=components|panels|artwork|themes|bg0|bg1|bg2]
//!                 [--screenshot=out.png] [--size=1600x900] [--time=260] [--live]
//! ```
//!
//! With `--screenshot` the window renders a few frames, writes the PNG and exits. `--time` fixes
//! the shader clock (the original's preview stills use 260 s for a full frame and 200 s for the
//! theme cards). `--live` lets the clock run, and on a background page Tab switches the theme; with
//! `ANYPS5_SHADER_DIR` set (see [`crate::ui::background`]) shader edits show up while it runs.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use egui::{
    Align2, Color32, ColorImage, Context, Id, Rect, TextureHandle, TextureOptions, Ui, pos2, vec2,
};

use crate::ui::Shell;
use crate::ui::anim::AnimExt;
use crate::ui::background::{
    Background, BackgroundClock, SLOT_CARDS, STILL_TIME_CARD, STILL_TIME_FULL,
};
use crate::ui::chrome::{
    self, Fact, KeyCapSpec, PanelRow, RowControl, TileImage, TileOverlay, TileStyle,
};
use crate::ui::components::{
    self, ConsoleButton, Force, IconButton, IndeterminateLine, ProgressLine, Switch, hint_bar,
};
use crate::ui::glass::{GlassStyle, paint_glass};
use crate::ui::i18n::{Key, Language};
use crate::ui::icons::Icon;
use crate::ui::text;
use crate::ui::theme::{self, Accent, Theme, color, radius, size, ty, white};

/// Which gallery page to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Components,
    Panels,
    Artwork,
    Themes,
    Background(Theme),
    /// A panel of the launcher (`screens::gallery`).
    Screen(crate::screens::gallery::ScreenPage),
}

/// Parsed command line of the gallery.
#[derive(Clone, Debug)]
pub struct Options {
    pub page: Page,
    pub screenshot: Option<PathBuf>,
    pub size: (f32, f32),
    pub time: Option<f32>,
    pub language: Language,
    pub live: bool,
}

/// Returns the gallery options when `--gallery` is on the command line.
pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Option<Options> {
    let mut options = Options {
        page: Page::Components,
        screenshot: None,
        size: (1600.0, 900.0),
        time: None,
        language: Language::English,
        live: false,
    };
    let mut seen = false;
    for arg in args {
        if arg == "--gallery" {
            seen = true;
        } else if let Some(page) = arg.strip_prefix("--gallery=") {
            seen = true;
            options.page = match page {
                "panels" => Page::Panels,
                "artwork" => Page::Artwork,
                "themes" => Page::Themes,
                "bg0" | "cosmic" => Page::Background(Theme::Cosmic),
                "bg1" | "aurora" => Page::Background(Theme::Aurora),
                "bg2" | "sunset" => Page::Background(Theme::Sunset),
                other => {
                    crate::screens::gallery::parse(other).map_or(Page::Components, Page::Screen)
                }
            };
        } else if let Some(path) = arg.strip_prefix("--screenshot=") {
            options.screenshot = Some(PathBuf::from(path));
        } else if let Some(dims) = arg.strip_prefix("--size=") {
            if let Some((Ok(w), Ok(h))) = dims.split_once('x').map(|(w, h)| (w.parse(), h.parse()))
            {
                options.size = (w, h);
            }
        } else if let Some(t) = arg.strip_prefix("--time=") {
            options.time = t.parse().ok();
        } else if arg == "--tr" {
            options.language = Language::Turkish;
        } else if arg == "--live" {
            options.live = true;
        }
    }
    seen.then_some(options)
}

/// Opens the gallery window.
pub fn run(options: Options) -> eframe::Result {
    let mut native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("AnyPlay Launcher gallery")
            .with_inner_size([options.size.0, options.size.1])
            .with_resizable(false),
        ..Default::default()
    };
    crate::ui::restrict_backends(&mut native);
    eframe::run_native(
        "AnyPlay Launcher gallery",
        native,
        Box::new(move |cc| Ok(Box::new(Gallery::new(cc, options)))),
    )
}

struct Gallery {
    shell: Shell,
    options: Options,
    frames: u32,
    shot_requested: bool,
    sample_art: Option<TextureHandle>,
    screens: Option<crate::screens::gallery::ScreenGallery>,
}

impl Gallery {
    fn new(cc: &eframe::CreationContext<'_>, options: Options) -> Self {
        let theme = match options.page {
            Page::Background(theme) => theme,
            _ => Theme::Cosmic,
        };
        let page = options.page;
        let mut shell = Shell::headless(cc, theme);
        let language = options.language;
        shell.language = language;
        let clock =
            BackgroundClock::starting_at(f64::from(options.time.unwrap_or(STILL_TIME_FULL)));
        shell.backdrop = crate::ui::background::Backdrop::with_clock(theme, clock);
        shell.backdrop.paused = !options.live;
        Self {
            shell,
            options,
            frames: 0,
            shot_requested: false,
            sample_art: None,
            screens: match page {
                Page::Screen(page) => {
                    Some(crate::screens::gallery::ScreenGallery::new(page, language))
                }
                _ => None,
            },
        }
    }

    fn art(&mut self, ctx: &Context) -> TextureHandle {
        self.sample_art
            .get_or_insert_with(|| {
                // A procedural stand-in for game artwork (we never bundle any).
                let n = 128usize;
                let mut rgba = Vec::with_capacity(n * n * 4);
                for y in 0..n {
                    for x in 0..n {
                        let (fx, fy) = (x as f32 / n as f32, y as f32 / n as f32);
                        let ring = (((fx - 0.5).powi(2) + (fy - 0.5).powi(2)).sqrt() * 9.0).sin()
                            * 0.5
                            + 0.5;
                        rgba.extend_from_slice(&[
                            (40.0 + 150.0 * fx) as u8,
                            (70.0 + 90.0 * ring) as u8,
                            (140.0 + 90.0 * fy) as u8,
                            255,
                        ]);
                    }
                }
                ctx.load_texture(
                    "sample-art",
                    ColorImage::from_rgba_unmultiplied([n, n], &rgba),
                    TextureOptions::LINEAR,
                )
            })
            .clone()
    }
}

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let _ = self.shell.begin_frame(&ctx);
        let screen = ctx.content_rect();
        let painter = ui.painter().clone();
        self.shell.paint_backdrop(&ctx, &painter, screen);
        match self.options.page {
            Page::Background(theme) => {
                if self.options.live && ctx.input(|i| i.key_pressed(egui::Key::Tab)) {
                    let next = Theme::ALL[(theme.index() as usize + 1) % Theme::ALL.len()];
                    self.options.page = Page::Background(next);
                    self.shell.backdrop.set_theme(next);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
                        "AnyPlay Launcher gallery: {}",
                        next.title(Language::English)
                    )));
                }
            }
            Page::Screen(_) => {
                if let Some(screens) = self.screens.as_mut() {
                    screens.ui(&mut self.shell, ui);
                }
            }
            Page::Themes => themes_page(&ctx, ui, &self.shell, self.options.time),
            Page::Components => components_page(ui, &self.shell),
            Page::Panels => {
                let art = self.art(&ctx);
                panels_page(ui, &self.shell, &art);
            }
            Page::Artwork => {
                let art = self.art(&ctx);
                crate::ui::artwork::paint_artwork(
                    &painter,
                    screen,
                    art.id(),
                    1.0,
                    ctx.clock(),
                    false,
                    1.0,
                );
                panels_page(ui, &self.shell, &art);
            }
        }

        self.frames += 1;
        ctx.request_repaint();
        if let Some(path) = self.options.screenshot.clone()
            && self.frames >= 30
            && !self.shot_requested
        {
            self.shot_requested = true;
            let close_ctx = ctx.clone();
            ctx.request_screenshot(move |image| {
                if let Err(err) = save_png(&path, &image) {
                    eprintln!("screenshot failed: {err}");
                } else {
                    println!("screenshot written to {}", path.display());
                }
                close_ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
    }
}

fn save_png(path: &PathBuf, image: &Arc<ColorImage>) -> Result<(), String> {
    let [w, h] = image.size;
    let mut bytes = Vec::with_capacity(w * h * 4);
    for pixel in &image.pixels {
        bytes.extend_from_slice(&pixel.to_array());
    }
    image::save_buffer(path, &bytes, w as u32, h as u32, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------------------------

fn heading(ui: &mut Ui, text_: &str) {
    ui.add_space(6.0);
    let painter = ui.painter().clone();
    let pos = ui.cursor().min;
    let rect = text::paint(
        &painter,
        pos,
        Align2::LEFT_TOP,
        ty::INFO_OVERLINE,
        &text_.to_uppercase(),
        color::text_overline(),
    );
    ui.add_space(rect.height() + 8.0);
}

fn components_page(ui: &mut Ui, shell: &Shell) {
    let language = shell.language;
    let accent = shell.accent_color();
    let rect = ui.max_rect().shrink2(vec2(64.0, 40.0));
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let ui = &mut ui;
    let title = ty::SCREEN_TITLE.colored("Components", Color32::WHITE);
    ui.label(title);
    ui.add_space(14.0);

    heading(
        ui,
        "Console buttons: rest, hover, press, focused, prominent, destructive, disabled, icon only",
    );
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        ConsoleButton::new(language.text(Key::Play))
            .icon(Icon::PLAY)
            .icon_fill(true)
            .prominent(true)
            .min_width(size::PLAY_MIN_WIDTH)
            .show(ui);
        ConsoleButton::new(language.text(Key::Unlock))
            .icon(Icon::LOCK_OPEN)
            .icon_fill(true)
            .show(ui);
        ConsoleButton::new(language.text(Key::Retry))
            .icon(Icon::ARROW_CLOCKWISE)
            .force(Some(Force::Hover))
            .show(ui);
        ConsoleButton::new(language.text(Key::Cancel))
            .icon(Icon::X)
            .force(Some(Force::Press))
            .show(ui);
        ConsoleButton::new(language.text(Key::Play))
            .icon(Icon::PLAY)
            .icon_fill(true)
            .prominent(true)
            .focused(true)
            .min_width(size::PLAY_MIN_WIDTH)
            .show(ui);
        ConsoleButton::new(language.text(Key::Uninstall))
            .icon(Icon::TRASH)
            .destructive(true)
            .show(ui);
    });
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        ConsoleButton::new(language.text(Key::Options))
            .enabled(false)
            .show(ui);
        ConsoleButton::icon_only(Icon::DOTS_THREE, language.text(Key::Options)).show(ui);
        ConsoleButton::icon_only(Icon::DOTS_THREE, language.text(Key::Options))
            .focused(true)
            .show(ui);
        ConsoleButton::new("8/34")
            .icon(Icon::TROPHY)
            .icon_fill(true)
            .show(ui);
    });
    ui.add_space(26.0);

    heading(ui, "Top-bar icon buttons: rest, hover, focused");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = size::CONTROL_GAP;
        IconButton::new(Icon::SQUARES_FOUR, language.text(Key::Library)).show(ui);
        IconButton::new(Icon::GEAR_SIX, language.text(Key::Settings))
            .force(Some(Force::Hover))
            .show(ui);
        IconButton::new(Icon::GEAR_SIX, language.text(Key::Settings))
            .focused(true)
            .show(ui);
    });
    ui.add_space(26.0);

    heading(
        ui,
        "The family: large, medium, small x rest, hover, press, focus, disabled (ink, secondary, circle)",
    );
    for size_ in [
        components::ControlSize::Large,
        components::ControlSize::Medium,
        components::ControlSize::Small,
    ] {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = size::CONTROL_GAP;
            let states = [
                (None, false, true),
                (Some(Force::Hover), false, true),
                (Some(Force::Press), false, true),
                (None, true, true),
                (None, false, false),
            ];
            for (force, focused, enabled) in states {
                ConsoleButton::new("Play")
                    .icon(Icon::PLAY)
                    .icon_fill(true)
                    .prominent(true)
                    .size(size_)
                    .force(force)
                    .focused(focused)
                    .enabled(enabled)
                    .show(ui);
            }
            for (force, focused, enabled) in states {
                ConsoleButton::new("3/8")
                    .icon(Icon::TROPHY)
                    .icon_fill(true)
                    .tabular(true)
                    .size(size_)
                    .force(force)
                    .focused(focused)
                    .enabled(enabled)
                    .show(ui);
            }
            ConsoleButton::icon_only(Icon::GEAR_SIX, "Settings")
                .size(size_)
                .show(ui);
            ConsoleButton::icon_only(Icon::GEAR_SIX, "Settings")
                .size(size_)
                .focused(true)
                .show(ui);
        });
        ui.add_space(8.0);
    }
    ui.add_space(14.0);

    heading(ui, "Switch, progress, indeterminate");
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 24.0;
        Switch::new(false, accent).show(ui);
        Switch::new(true, accent).show(ui);
        Switch::new(false, accent).focused(true).show(ui);
        ProgressLine::new(0.4, accent).width(260.0).show(ui);
        ProgressLine::new(0.85, Accent::Mint.color())
            .width(180.0)
            .show(ui);
        IndeterminateLine::new(accent).width(260.0).show(ui);
    });
    ui.add_space(26.0);

    heading(ui, "Hint bar and key caps");
    hint_bar(ui, language, true);
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        components::key_cap(ui, "W", true, 0.0);
        components::key_cap(ui, "Space", true, 0.0);
        components::key_cap(ui, "Shift", true, 0.0);
        components::key_cap(ui, language.text(Key::MouseLeft), false, 0.0);
        components::key_cap(ui, "↑", true, 1.0);
    });
    ui.add_space(26.0);

    heading(ui, "Type scale");
    let painter = ui.painter().clone();
    let mut y = ui.cursor().min.y;
    for (label, style) in [
        ("Hero title 58 light", ty::HERO_TITLE),
        ("Screen title 40 light", ty::SCREEN_TITLE),
        ("Top bar 24 semibold", ty::TOP_BAR_TITLE),
        ("Button 19 semibold", ty::BUTTON),
        ("Row title 17 medium  0123456789", ty::ROW_TITLE),
        ("Detail 13 regular  Çok güzel İıŞşĞğ", ty::DETAIL),
    ] {
        let r = text::paint(
            &painter,
            pos2(ui.max_rect().min.x, y),
            Align2::LEFT_TOP,
            style,
            label,
            Color32::WHITE,
        );
        y += r.height() + 4.0;
    }
    let clock = text::paint_tabular(
        &painter,
        pos2(ui.max_rect().min.x + 760.0, ui.cursor().min.y),
        ty::CLOCK,
        "12:34:56",
        white(0.92),
    );
    text::paint(
        &painter,
        pos2(clock.max.x + 16.0, clock.min.y),
        Align2::LEFT_TOP,
        ty::DETAIL,
        "tabular figures",
        color::text_detail(),
    );
}

fn panels_page(ui: &mut Ui, shell: &Shell, art: &TextureHandle) {
    let language = shell.language;
    let accent = shell.accent_color();
    let ctx = ui.ctx().clone();
    let area = ui.max_rect().shrink2(vec2(64.0, 40.0));

    // Game tiles in every state.
    let painter = ui.painter().clone();
    let time = ctx.clock();
    let image = Some(TileImage {
        texture: art.id(),
        aspect: 1.0,
    });
    let overlays = [
        ("ready", TileOverlay::None),
        ("locked", TileOverlay::Locked),
        ("unreadable", TileOverlay::Unreadable),
        ("queued", TileOverlay::Queued),
        ("installing", TileOverlay::Installing(0.4)),
        ("failed", TileOverlay::Failed),
        ("starting", TileOverlay::Starting),
        ("running", TileOverlay::Running),
    ];
    let mut x = area.min.x;
    for (i, (name, overlay)) in overlays.iter().enumerate() {
        let side = if i == 0 {
            size::TILE_LARGE
        } else {
            size::TILE_SMALL
        };
        let rect = Rect::from_min_size(
            pos2(x, area.min.y + if i == 0 { 0.0 } else { 20.0 }),
            vec2(side, side),
        );
        let mut style = TileStyle::new(accent);
        style.image = image;
        style.overlay = *overlay;
        style.focus = if i == 0 { 1.0 } else { 0.0 };
        style.ring = i == 0;
        style.time = time;
        chrome::paint_tile(&painter, rect, &style);
        text::paint(
            &painter,
            pos2(rect.center().x, rect.max.y + 14.0),
            Align2::CENTER_TOP,
            ty::STATE_LABEL,
            name,
            color::state_neutral(),
        );
        x += side + 22.0;
    }
    let mut no_icon = TileStyle::new(accent);
    no_icon.overlay = TileOverlay::None;
    chrome::paint_tile(
        &painter,
        Rect::from_min_size(
            pos2(x, area.min.y + 20.0),
            egui::Vec2::splat(size::TILE_SMALL),
        ),
        &no_icon,
    );

    // Facts line.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(
        pos2(area.min.x, area.min.y + 250.0),
        vec2(1100.0, 100.0),
    )));
    chrome::facts_line(
        &mut child,
        &[
            Fact {
                icon: Icon::CIRCLE_DASHED,
                title: language.text(Key::Status),
                value: language.text(Key::Ready),
                value_color: None,
            },
            Fact {
                icon: Icon::HARD_DRIVE,
                title: language.text(Key::Size),
                value: "8,39 GB",
                value_color: None,
            },
            Fact {
                icon: Icon::HASH,
                title: language.text(Key::TitleID),
                value: "PPSA12544",
                value_color: None,
            },
            Fact {
                icon: Icon::TAG,
                title: language.text(Key::Version),
                value: "01.000.010",
                value_color: None,
            },
        ],
        language,
    );

    // Panel rows in a side-panel-wide column.
    let rows_rect = Rect::from_min_size(
        pos2(area.min.x, area.min.y + 380.0),
        vec2(size::SIDE_PANEL_WIDTH - 72.0, 480.0),
    );
    let mut col = ui.new_child(egui::UiBuilder::new().max_rect(rows_rect));
    col.spacing_mut().item_spacing.y = size::ROW_GAP;
    let mut name = String::from("Alec");
    let caps = [
        KeyCapSpec {
            label: "Space".into(),
            keyboard: true,
        },
        KeyCapSpec {
            label: language.text(Key::MouseLeft).into(),
            keyboard: false,
        },
    ];
    PanelRow::new(
        language.text(Key::Theme),
        RowControl::Choice {
            value: "Aurora",
            can_decrement: true,
            can_increment: true,
        },
    )
    .icon(Icon::PALETTE)
    .show(&mut col);
    PanelRow::new(
        language.text(Key::Sounds),
        RowControl::Toggle { on: true, accent },
    )
    .icon(Icon::SPEAKER_HIGH)
    .focused(true)
    .show(&mut col);
    PanelRow::new(
        language.text(Key::Resolution),
        RowControl::Stepper {
            value: "1080",
            can_decrement: true,
            can_increment: true,
        },
    )
    .icon(Icon::GAUGE)
    .show(&mut col);
    PanelRow::new(language.text(Key::Folder), RowControl::Action)
        .icon(Icon::FOLDER)
        .detail("C:/Users/alec/Games/PS5/dumps/very/long/path/to/a/folder")
        .show(&mut col);
    PanelRow::new(
        language.text(Key::ProfileName),
        RowControl::Text { value: &mut name },
    )
    .icon(Icon::USER_CIRCLE)
    .show(&mut col);
    PanelRow::new(
        language.text(Key::Controls),
        RowControl::Keys {
            caps: &caps,
            capturing: false,
            none_label: language.text(Key::NoKey),
            press_label: language.text(Key::PressKey),
        },
    )
    .icon(Icon::KEYBOARD)
    .show(&mut col);
    PanelRow::new(language.text(Key::Uninstall), RowControl::Action)
        .icon(Icon::TRASH)
        .destructive(true)
        .show(&mut col);
    PanelRow::new(language.text(Key::About), RowControl::Info { value: None }).show(&mut col);

    // Glass, toast, language cards, dots.
    let right = area.min.x + 800.0;
    let glass_rect = Rect::from_min_size(pos2(right, area.min.y + 390.0), vec2(300.0, 120.0));
    paint_glass(&ctx, &painter, glass_rect, GlassStyle::card());
    text::paint(
        &painter,
        glass_rect.center(),
        Align2::CENTER_CENTER,
        ty::ROW_TITLE,
        "Glass 0.38",
        Color32::WHITE,
    );
    let mut cards = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(
        pos2(right, area.min.y + 540.0),
        vec2(700.0, 160.0),
    )));
    cards.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 26.0;
        chrome::language_card(ui, Language::Turkish, false, None);
        chrome::language_card(ui, Language::English, true, None);
    });
    let mut dots = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(
        pos2(right, area.min.y + 740.0),
        vec2(300.0, 20.0),
    )));
    chrome::onboarding_dots(&mut dots, 3, 1);
    chrome::toast(
        &ctx,
        "gallery-toast",
        Icon::LOCK_OPEN,
        "Hollow Game · Ready to play",
        1.0,
    );
}

fn themes_page(ctx: &Context, ui: &mut Ui, shell: &Shell, time: Option<f32>) {
    // The onboarding's theme step: three cards, middle one focused, as in launcher-themes.jpg.
    let painter = ui.painter().clone();
    let screen = ctx.content_rect();
    let (w, h) = (size::THEME_CARD_PREVIEW.0, size::THEME_CARD_PREVIEW.1);
    let gap = 26.0;
    let total = 3.0 * w + 2.0 * gap;
    let mut x = screen.center().x - total * 0.5;
    let y = screen.center().y - h * 0.5;
    let time = time.unwrap_or(STILL_TIME_CARD);
    for (i, theme) in Theme::ALL.iter().enumerate() {
        let focused = i == 0;
        let t = ctx.focus_t(Id::unique(("theme-card", i)), focused);
        let scale = theme::scale_for(1600.0, 900.0);
        let _ = scale;
        let factor = if focused { 1.04 } else { 0.97 };
        let rect = components::scale_rect(Rect::from_min_size(pos2(x, y), vec2(w, h)), factor);
        let mut layer = Background::new(SLOT_CARDS + theme.index(), *theme, time);
        layer.radius = radius::THEME_CARD * factor;
        layer.alpha = if focused { 1.0 } else { 0.72 };
        components::paint_focus_glow(&painter, rect, radius::THEME_CARD * factor, 3.0, t);
        layer.paint(&painter, rect);
        components::paint_focus_stroke(&painter, rect, radius::THEME_CARD * factor, 3.0, t);
        let name_style = if focused {
            ty::THEME_CARD_NAME_FOCUSED
        } else {
            ty::THEME_CARD_NAME
        };
        let label = theme.title(shell.language);
        let r = text::paint(
            &painter,
            pos2(rect.min.x, rect.max.y + 18.0),
            Align2::LEFT_TOP,
            name_style,
            label,
            Color32::WHITE,
        );
        painter.circle_filled(
            pos2(r.max.x + 14.0, r.center().y),
            6.0,
            theme.default_accent().color(),
        );
        x += w + gap;
    }
}
