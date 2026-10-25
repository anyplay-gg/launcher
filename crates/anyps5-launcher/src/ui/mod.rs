//! UI foundation of the launcher (docs/DESIGN.md): tokens, motion, backgrounds, components,
//! sounds, localisation and input. Screens build on this module only; nothing here knows about
//! games, preferences or the core crate.
//!
//! | Module | What it gives the screens |
//! |---|---|
//! | [`theme`] | colours, radii, sizes, shadows, type scale, fonts, window scale |
//! | [`anim`] | springs, easings, `ctx.animate_to(...)` / `ctx.focus_t(...)`, endless loops |
//! | [`artwork`] | game artwork over the backdrop: aspect-fill, drift, scrims |
//! | [`background`] | the three animated themes, [`Backdrop`](background::Backdrop) driver |
//! | [`blur`], [`glass`] | material blur and frosted panels |
//! | [`components`], [`chrome`] | buttons, switch, progress, hints, tile, rows, panels, toast |
//! | [`text`] | text layout with line height, tracking, truncation, tabular figures |
//! | [`icons`] | Phosphor glyphs |
//! | [`sound`] | the five synthesised cues and the home music |
//! | [`i18n`] | 212 keys in English and Turkish |
//! | [`input`], [`focus`], [`gamepad`] | commands, wheel, gamepad, home and panel focus models |
//! | [`intro`] | the launch intro: fade from black, parts rising into place |
//!
//! A screen host creates a [`Shell`] once and calls [`Shell::begin_frame`] at the start of every
//! frame; see its documentation.

// The foundation is consumed by the screens of later PRs; until then most of its API is unused.
#![allow(dead_code)]

pub mod a11y;
pub mod anim;
pub mod artwork;
pub mod background;
pub mod blur;
pub mod caption;
pub mod chrome;
pub mod components;
#[cfg(test)]
mod contrast;
pub mod focus;
pub mod gallery;
pub mod gamepad;
pub mod glass;
pub mod gpu;
pub mod home_button;
pub mod i18n;
pub mod icons;
pub mod input;
pub mod intro;
pub mod shade;
pub mod sound;
pub mod text;
pub mod theme;

use std::sync::Arc;

use eframe::egui_wgpu;
use egui::{Context, Painter, Rect};

use self::anim::AnimExt;
use self::background::Backdrop;
use self::home_button::HomeButton;
use self::i18n::Language;
use self::input::{Input, InputEvent};
use self::sound::hover::{Frame as HoverFrame, HoverGate};
use self::sound::{Cue, Sounds};
use self::theme::{Accent, Theme};

/// Long-lived UI state every screen needs: input pipeline, sounds, backdrop, language and accent.
pub struct Shell {
    pub input: Input,
    /// The Home button while a game runs (read by the input threads).
    pub home: Arc<HomeButton>,
    pub sounds: Sounds,
    hover: HoverGate,
    /// A cue other than the hover tick played since the last [`Shell::finish_frame`].
    cue_played: bool,
    pub backdrop: Backdrop,
    pub language: Language,
    /// The accent colour in use (the theme's default unless the user picked one).
    pub accent: Accent,
    scale: f32,
}

/// What [`Shell::begin_frame`] hands to the screen.
pub struct FrameInput {
    /// Global scale in use (1 dp = 1 egui point once it is applied).
    pub scale: f32,
    /// Commands, wheel steps and shortcuts of this frame, in order.
    pub events: Vec<InputEvent>,
}

impl Shell {
    /// Installs fonts and the dark visuals, builds the GPU pipelines of the backdrop and starts the
    /// gamepad thread. Call from the eframe app creator.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            background::init(render_state);
        } else {
            tracing::warn!("no wgpu render state: backdrop and glass are unavailable");
        }
        let home = Arc::new(HomeButton::new(&cc.egui_ctx));
        let pad = gamepad::spawn(cc.egui_ctx.clone(), home.clone());
        home_button::spawn_xinput(home.clone());
        let language = Language::system();
        Self {
            input: Input::new(Some(pad)),
            home,
            sounds: Sounds::new(),
            hover: HoverGate::default(),
            cue_played: false,
            backdrop: Backdrop::new(Theme::Cosmic),
            language,
            accent: Theme::Cosmic.default_accent(),
            scale: 1.0,
        }
    }

    /// A shell for tests and the gallery: no gamepad, silent.
    pub fn headless(cc: &eframe::CreationContext<'_>, theme: Theme) -> Self {
        theme::install(&cc.egui_ctx);
        crate::startup::mark("fonts and visuals installed");
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            background::init(render_state);
        }
        crate::startup::mark("GPU pipelines built");
        let sounds = Sounds::silent();
        Self {
            input: Input::new(None),
            home: Arc::new(HomeButton::new(&cc.egui_ctx)),
            sounds,
            hover: HoverGate::default(),
            cue_played: false,
            backdrop: Backdrop::new(theme),
            language: Language::English,
            accent: theme.default_accent(),
            scale: 1.0,
        }
    }

    /// Call first thing in every frame: applies the window scale and collects input events.
    pub fn begin_frame(&mut self, ctx: &Context) -> FrameInput {
        self.scale = theme::sync_scale(ctx);
        text::set_pixels_per_point(ctx.pixels_per_point());
        FrameInput {
            scale: self.scale,
            events: self.input.poll(ctx),
        }
    }

    /// Switches the theme and, unless the user chose an accent, the accent with it.
    pub fn set_theme(&mut self, theme: Theme, user_accent: Option<Accent>) {
        self.backdrop.set_theme(theme);
        self.accent = user_accent.unwrap_or_else(|| theme.default_accent());
    }

    /// "Animations: Reduced": swaps springs for short eases and freezes the background.
    pub fn set_reduced_motion(&mut self, ctx: &Context, reduced: bool) {
        ctx.set_reduced_motion(reduced);
    }

    /// Updates the clock and paints the backdrop into `rect` (usually the whole window). Call
    /// before any other UI.
    pub fn paint_backdrop(&mut self, ctx: &Context, painter: &Painter, rect: Rect) {
        self.backdrop.update(ctx);
        self.backdrop.paint(ctx, painter, rect);
    }

    /// Plays a cue (no-op when sounds are off).
    pub fn play(&mut self, cue: Cue) {
        self.cue_played |= cue != Cue::Hover;
        self.sounds.play(cue);
    }

    /// Call last in every frame: plays the hover tick when the pointer arrived on a new
    /// interactive element (see [`sound::hover`]). `allowed` is false while a game starts or
    /// runs and while the launcher window is not in front. The element is whatever
    /// [`input::hovered`] reported during the frame.
    pub fn finish_frame(&mut self, ctx: &Context, allowed: bool) {
        let hovered = input::take_hovered(ctx);
        let (now, moved, down, clicked, scrolling) = ctx.input(|i| {
            (
                i.time,
                i.pointer.delta() != egui::Vec2::ZERO
                    || i.events
                        .iter()
                        .any(|e| matches!(e, egui::Event::PointerMoved(_))),
                i.pointer.any_down(),
                i.pointer.any_click(),
                i.smooth_scroll_delta != egui::Vec2::ZERO
                    || i.events
                        .iter()
                        .any(|e| matches!(e, egui::Event::MouseWheel { .. })),
            )
        });
        let other_cue = std::mem::take(&mut self.cue_played);
        let allowed = allowed && self.sounds.enabled;
        if allowed {
            self.sounds.warm();
        }
        let tick = self.hover.update(&HoverFrame {
            now,
            hovered,
            pointer_moved: moved,
            pointer_down: down,
            clicked,
            scrolling,
            other_cue,
            allowed,
        });
        if tick {
            self.sounds.play(Cue::Hover);
        }
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The accent as a colour.
    pub fn accent_color(&self) -> egui::Color32 {
        self.accent.color()
    }
}

/// The environment variable that picks the graphics API (`metal`, `dx12`, `vulkan` or `both`); the launcher
/// sets it to `vulkan` itself when DirectX 12 has no usable GPU.
pub const GPU_ENV: &str = "ANYPS5_GPU";

/// Chooses the graphics API and the GPU of the window.
///
/// * One API, never OpenGL (its probing crashes some AMD drivers). On Windows that is DirectX 12:
///   enumerating Vulkan as well loads every Vulkan driver layer and probes each GPU, seconds
///   before the first frame, for a choice DirectX 12 makes as well. macOS uses Metal; Linux uses Vulkan.
///   `ANYPS5_GPU` overrides it.
/// * The low-power GPU first, so a laptop does not wake its discrete GPU to draw a launcher.
///   `ANYPS5_POWER=high` turns that round. A software renderer is no choice on Windows: with
///   nothing else the start fails and [`can_fall_back_to_vulkan`] tells the caller to retry.
pub fn restrict_backends(options: &mut eframe::NativeOptions) {
    use egui_wgpu::WgpuSetup;
    use egui_wgpu::wgpu::{Adapter, Backends, DeviceType, PowerPreference};
    let WgpuSetup::CreateNew(create_new) = &mut options.wgpu_options.wgpu_setup else {
        return;
    };
    create_new.instance_descriptor.backends = match std::env::var(GPU_ENV).as_deref() {
        Ok("vulkan") => Backends::VULKAN,
        Ok("both") => Backends::VULKAN | Backends::DX12,
        Ok("dx12") => Backends::DX12,
        Ok("metal") => Backends::METAL,
        _ if cfg!(windows) => Backends::DX12,
        _ if cfg!(target_os = "macos") => Backends::METAL,
        _ => Backends::VULKAN,
    };
    let low_power = std::env::var("ANYPS5_POWER").as_deref() != Ok("high");
    create_new.power_preference = if low_power {
        PowerPreference::LowPower
    } else {
        PowerPreference::HighPerformance
    };
    let no_software = can_fall_back_to_vulkan();
    create_new.native_adapter_selector = Some(std::sync::Arc::new(move |adapters, surface| {
        let rank = |adapter: &Adapter| -> Option<u8> {
            if surface.is_some_and(|s| !adapter.is_surface_supported(s)) {
                return None;
            }
            match (adapter.get_info().device_type, low_power) {
                (DeviceType::Cpu, _) if no_software => None,
                (DeviceType::Cpu, _) => Some(3),
                (DeviceType::IntegratedGpu, true) | (DeviceType::DiscreteGpu, false) => Some(0),
                (DeviceType::DiscreteGpu, true) | (DeviceType::IntegratedGpu, false) => Some(1),
                _ => Some(2),
            }
        };
        adapters
            .iter()
            .filter_map(|adapter| rank(adapter).map(|rank| (rank, adapter)))
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, adapter)| adapter.clone())
            .ok_or_else(|| "no usable GPU".to_owned())
    }));
}

/// Whether a failed start may be retried on Vulkan: on Windows, when DirectX 12 was the default.
pub fn can_fall_back_to_vulkan() -> bool {
    cfg!(windows) && std::env::var_os(GPU_ENV).is_none()
}

#[cfg(test)]
impl Shell {
    /// A shell without GPU, gamepad or sound for tests that drive whole frames.
    pub fn for_test(ctx: &Context, theme: Theme) -> Self {
        theme::install(ctx);
        let sounds = Sounds::silent();
        Self {
            input: Input::new(None),
            home: Arc::new(HomeButton::new(ctx)),
            sounds,
            hover: HoverGate::default(),
            cue_played: false,
            backdrop: Backdrop::new(theme),
            language: Language::English,
            accent: theme.default_accent(),
            scale: 1.0,
        }
    }
}
