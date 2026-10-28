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
pub mod gamecolors;
pub mod gamepad;
pub mod glass;
pub mod gpu;
pub mod haptics;
pub mod home_button;
pub mod i18n;
pub mod icons;
pub mod input;
pub mod intro;
pub mod latency;
pub mod lightbar;
pub mod nav_bindings;
pub mod nudge;
pub mod os_emoji;
pub mod osk;
pub mod pad_hub;
pub mod pad_info;
pub mod pageblur;
pub mod press;
pub mod ps_button;
pub mod reveal;
pub mod shade;
pub mod sound;
pub mod state_page;
pub mod talk;
pub mod text;
pub mod theme;

use std::sync::Arc;

use eframe::egui_wgpu;
use egui::{Context, Painter, Rect};

use self::anim::AnimExt;
use self::background::Backdrop;
use self::haptics::Haptics;
use self::home_button::HomeButton;
use self::i18n::Language;
use self::input::{Input, InputEvent};
use self::nudge::Nudge;
use self::sound::alert::AlertGate;
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
    /// Holds back the chatter of message and notice cues (see [`sound::alert`]).
    alerts: AlertGate,
    /// The time of the frame in seconds, for the gates.
    now: f64,
    /// A cue other than the hover tick played since the last [`Shell::finish_frame`].
    cue_played: bool,
    /// Until when content may still glide under a resting pointer after a drag of a list.
    glide_until: f64,
    pub backdrop: Backdrop,
    pub language: Language,
    /// The accent colour in use (the theme's default unless the user picked one).
    pub accent: Accent,
    /// A colour that stands in for the accent: Game Colors takes it from the selected game.
    pub accent_override: Option<egui::Color32>,
    scale: f32,
    /// The edge bump of the focus ring (see [`nudge`]).
    nudge: Nudge,
    /// "Animations: Reduced": the ring does not lean.
    reduced: bool,
    /// The hold in progress has bumped already.
    bumped_in_hold: bool,
    /// Controller vibration (see [`haptics`]).
    pub haptics: Haptics,
    /// The launcher had the pad last frame (a game in front takes it).
    had_pad: bool,
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
        crate::startup::mark("shell: fonts and visuals");
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            background::init(render_state);
        } else {
            tracing::warn!("no wgpu render state: backdrop and glass are unavailable");
        }
        crate::startup::mark("shell: GPU pipelines");
        let home = Arc::new(HomeButton::new(&cc.egui_ctx));
        let pad = gamepad::spawn(cc.egui_ctx.clone(), home.clone());
        home_button::spawn_xinput(home.clone());
        ps_button::spawn(home.clone());
        crate::startup::mark("shell: input threads");
        let language = Language::system();
        let mut shell = Self {
            input: Input::new(Some(pad)),
            home,
            sounds: Sounds::new(),
            hover: HoverGate::default(),
            alerts: AlertGate::default(),
            now: 0.0,
            cue_played: false,
            glide_until: f64::NEG_INFINITY,
            haptics: Haptics::new(),
            had_pad: true,
            backdrop: Backdrop::new(Theme::Cosmic),
            language,
            accent: Theme::Cosmic.default_accent(),
            accent_override: None,
            scale: 1.0,
            nudge: Nudge::default(),
            reduced: false,
            bumped_in_hold: false,
        };
        // Gaming Mode has no keyboard or mouse, and Console Mode puts the pad first: start with the
        // pad's hints.
        shell.input.pad_in_use = osk::gaming_mode() || osk::console_mode();
        shell
    }

    /// A shell for tests and the gallery: no gamepad, silent.
    pub fn headless(cc: &eframe::CreationContext<'_>, theme: Theme) -> Self {
        theme::install(&cc.egui_ctx);
        crate::startup::mark("fonts and visuals installed");
        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            background::init(render_state);
        }
        crate::startup::mark("GPU pipelines built");
        // `--noring`: a screenshot as it looks after a click (no focus ring).
        if std::env::args().any(|a| a == "--noring") {
            input::set_focus_visible(&cc.egui_ctx, false);
        }
        let sounds = Sounds::silent();
        Self {
            input: Input::new(None),
            home: Arc::new(HomeButton::new(&cc.egui_ctx)),
            sounds,
            hover: HoverGate::default(),
            alerts: AlertGate::default(),
            now: 0.0,
            cue_played: false,
            glide_until: f64::NEG_INFINITY,
            haptics: Haptics::silent(),
            had_pad: true,
            backdrop: Backdrop::new(theme),
            language: Language::English,
            accent: theme.default_accent(),
            accent_override: None,
            scale: 1.0,
            nudge: Nudge::default(),
            reduced: false,
            bumped_in_hold: false,
        }
    }

    /// Call first thing in every frame: applies the window scale and collects input events.
    pub fn begin_frame(&mut self, ctx: &Context) -> FrameInput {
        self.scale = theme::sync_scale(ctx);
        self.now = ctx.input(|i| i.time);
        text::set_pixels_per_point(ctx.pixels_per_point());
        // The first use of the pad starts the vibration thread, so its first pulse is not late.
        if self.input.pad_in_use {
            self.haptics.warm();
        }
        // A game that takes the pad takes the motors with it, mid-pulse too.
        let has_pad = self.home.accepts_pad();
        if self.had_pad && !has_pad {
            self.haptics.stop();
        }
        self.had_pad = has_pad;
        let dt = ctx.input(|i| i.unstable_dt).min(0.05);
        nudge::publish(ctx, self.nudge.step(dt));
        if !self.nudge.is_settled() {
            ctx.request_repaint();
        }
        osk::set_wanted(ctx, self.input.pad_in_use || osk::gaming_mode());
        let events = self.input.poll(ctx);
        // A select from a key or the pad presses the focused control in, like a click does.
        if events
            .iter()
            .any(|e| matches!(e, InputEvent::Command(input::Command::Select)))
        {
            press::trigger(ctx);
        }
        FrameInput {
            scale: self.scale,
            events,
        }
    }

    /// Switches the theme and, unless the user chose an accent, the accent with it.
    pub fn set_theme(&mut self, theme: Theme, user_accent: Option<Accent>) {
        self.backdrop.set_theme(theme);
        self.accent = user_accent.unwrap_or_else(|| theme.default_accent());
    }

    /// "Animations: Reduced": swaps springs for short eases and freezes the background.
    pub fn set_reduced_motion(&mut self, ctx: &Context, reduced: bool) {
        self.reduced = reduced;
        ctx.set_reduced_motion(reduced);
    }

    /// Updates the clock and paints the backdrop into `rect` (usually the whole window). Call
    /// before any other UI.
    pub fn paint_backdrop(&mut self, ctx: &Context, painter: &Painter, rect: Rect) {
        self.backdrop.update(ctx);
        self.backdrop.paint(ctx, painter, rect);
    }

    /// The one place where an action is answered: plays its cue (no-op when sounds are off).
    /// A bump is answered once per press, not on every repeat of a held direction, and leans
    /// the focus ring toward the wall unless motion is reduced.
    pub fn play(&mut self, cue: Cue) {
        // A burst of messages or notices sounds once; their toasts and badges still show.
        if !self.alerts.allow(cue, self.now) {
            return;
        }
        self.cue_played |= cue != Cue::Hover;
        if cue != Cue::Bump {
            self.bumped_in_hold = false;
        } else {
            // One bump per press, and one per hold: the repeats at a wall stay quiet.
            if self.input.repeating && self.bumped_in_hold {
                return;
            }
            self.bumped_in_hold = true;
            if !self.reduced
                && let Some(direction) = self.input.last_direction
            {
                self.nudge.kick(direction);
            }
        }
        self.sounds.play(cue);
        // The hand answers only when the pad is what the player is using, and the launcher has it.
        if self.input.pad_in_use && self.had_pad {
            self.haptics.play(cue);
        }
    }

    /// Sets "Controller Vibration"; off lets go of the motors at once.
    pub fn set_vibration(&mut self, level: haptics::Level) {
        let changed = self.haptics.level != level;
        if level == haptics::Level::Off && changed {
            self.haptics.stop();
        }
        self.haptics.level = level;
        // Changing it on the pad lets the hand try the new strength.
        if changed && self.input.pad_in_use && self.had_pad {
            self.haptics.play(Cue::Select);
        }
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
        // A list let go of keeps gliding for a moment: nothing under it has "arrived".
        let dragging = ctx.input(|i| i.pointer.is_decidedly_dragging());
        if dragging {
            self.glide_until = now + GLIDE_SECONDS;
        }
        let scrolling = scrolling || now < self.glide_until;
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
        self.accent_override.unwrap_or_else(|| self.accent.color())
    }
}

/// How long the hover tick stays quiet after a drag of a list ends: the momentum of the content.
const GLIDE_SECONDS: f64 = 0.8;

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
    crate::startup::mark("GPU settings made");
    // DirectX 12 creates a device on every GPU it lists, and that wakes a sleeping discrete GPU:
    // about 3 s on a laptop, for a GPU the launcher then does not use. Ask DXGI (which creates
    // nothing) which GPU is wanted and let wgpu see that one only.
    #[cfg(windows)]
    if create_new.instance_descriptor.backends == Backends::DX12 {
        preselect_dx12_adapter(low_power);
    }
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
        // The start-up log splits the wait for the GPU here: before this line the adapters were
        // enumerated, after it the device and the surface are made.
        crate::startup::mark(&format!("{} GPUs listed", adapters.len()));
        adapters
            .iter()
            .filter_map(|adapter| rank(adapter).map(|rank| (rank, adapter)))
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, adapter)| adapter.clone())
            .ok_or_else(|| "no usable GPU".to_owned())
    }));
}

/// Set (to anything) to let DirectX 12 look at every GPU again, as before `preselect_dx12_adapter`.
/// A start that fails with one GPU preselected is retried with it.
pub const GPU_PROBE_ENV: &str = "ANYPS5_GPU_PROBE";

/// Whether this start preselected one GPU, so that a failure is worth a retry with all of them.
pub fn can_retry_with_all_gpus() -> bool {
    cfg!(windows)
        && std::env::var_os(GPU_PROBE_ENV).is_none()
        && std::env::var_os(GPU_ENV).is_none()
}

/// Tells wgpu's DirectX 12 backend to expose only the GPU Windows would pick for this power
/// preference: the first non-software one of `IDXGIFactory6::EnumAdapterByGpuPreference`. Without
/// that GPU (old Windows, no DXGI 1.6, only a software renderer) nothing is filtered.
#[cfg(windows)]
fn preselect_dx12_adapter(low_power: bool) {
    use egui_wgpu::wgpu::hal::dx12::set_only_adapter;
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, DXGI_GPU_PREFERENCE,
        DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE, DXGI_GPU_PREFERENCE_MINIMUM_POWER, IDXGIAdapter1,
        IDXGIFactory6,
    };
    if std::env::var_os(GPU_PROBE_ENV).is_some() {
        return;
    }
    let preference: DXGI_GPU_PREFERENCE = if low_power {
        DXGI_GPU_PREFERENCE_MINIMUM_POWER
    } else {
        DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE
    };
    let chosen = (|| {
        let factory: IDXGIFactory6 = unsafe { CreateDXGIFactory1() }.ok()?;
        (0..)
            .map_while(|index| {
                unsafe { factory.EnumAdapterByGpuPreference::<IDXGIAdapter1>(index, preference) }
                    .ok()
            })
            .find_map(|adapter| {
                let desc = unsafe { adapter.GetDesc1() }.ok()?;
                (desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 == 0).then(|| {
                    let name = String::from_utf16_lossy(&desc.Description);
                    (
                        (desc.AdapterLuid.HighPart, desc.AdapterLuid.LowPart),
                        name.trim_end_matches('\0').to_owned(),
                    )
                })
            })
    })();
    if let Some((luid, name)) = chosen {
        set_only_adapter(Some(luid));
        crate::startup::mark(&format!("GPU preselected: {name}"));
    } else {
        crate::startup::mark("no GPU preselected");
    }
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
            alerts: AlertGate::default(),
            now: 0.0,
            cue_played: false,
            glide_until: f64::NEG_INFINITY,
            haptics: Haptics::silent(),
            had_pad: true,
            backdrop: Backdrop::new(theme),
            language: Language::English,
            accent: theme.default_accent(),
            accent_override: None,
            scale: 1.0,
            nudge: Nudge::default(),
            reduced: false,
            bumped_in_hold: false,
        }
    }
}

#[cfg(test)]
mod shell_tests {
    use super::*;

    #[test]
    fn a_wall_bumps_once_per_hold_and_again_for_every_new_press() {
        let ctx = Context::default();
        let mut shell = Shell::for_test(&ctx, Theme::Cosmic);
        shell.input.last_direction = Some(input::Command::Right);
        shell.input.repeating = true;
        shell.play(Cue::Bump);
        let first = shell.nudge.step(0.02).x;
        assert!(first > 0.0, "the first bump of a hold leans");
        shell.play(Cue::Bump);
        // Nothing was added: the second step only follows the first spring on.
        let kicked = shell.nudge.step(0.0).x;
        assert!(kicked <= first + 0.01);
        shell.input.repeating = false;
        shell.play(Cue::Bump);
        assert!(shell.nudge.step(0.01).x > 0.0, "a fresh press bumps again");
    }

    #[test]
    fn reduced_motion_keeps_the_ring_still() {
        let ctx = Context::default();
        let mut shell = Shell::for_test(&ctx, Theme::Cosmic);
        shell.reduced = true;
        shell.input.last_direction = Some(input::Command::Left);
        shell.play(Cue::Bump);
        assert!(shell.nudge.is_settled());
    }

    use self::haptics::Asked;

    fn pad_shell(ctx: &Context) -> Shell {
        let mut shell = Shell::for_test(ctx, Theme::Cosmic);
        shell.input.pad_in_use = true;
        shell
    }

    #[test]
    fn the_hand_answers_only_a_pad_that_the_launcher_has() {
        let ctx = Context::default();
        // A pad in use: select vibrates.
        let mut shell = pad_shell(&ctx);
        shell.play(Cue::Select);
        assert_eq!(shell.haptics.asked, [Asked::Play(Cue::Select)]);
        // The keyboard or the mouse is in use: nothing, even with a pad plugged in.
        let mut shell = Shell::for_test(&ctx, Theme::Cosmic);
        shell.play(Cue::Select);
        assert!(shell.haptics.asked.is_empty());
        // A game has the pad: nothing.
        let mut shell = pad_shell(&ctx);
        shell.had_pad = false;
        shell.play(Cue::Error);
        assert!(shell.haptics.asked.is_empty());
        // Moving the focus never vibrates.
        let mut shell = pad_shell(&ctx);
        shell.play(Cue::Move);
        assert!(shell.haptics.asked.is_empty());
    }

    #[test]
    fn off_lets_go_at_once_and_a_change_is_felt() {
        let ctx = Context::default();
        let mut shell = pad_shell(&ctx);
        shell.set_vibration(haptics::Level::Off);
        assert_eq!(shell.haptics.asked, [Asked::Stop]);
        shell.play(Cue::Select);
        assert_eq!(shell.haptics.asked, [Asked::Stop], "off stays off");
        // Back on: the hand gets a sample of the new strength.
        shell.set_vibration(haptics::Level::Light);
        assert_eq!(shell.haptics.asked, [Asked::Stop, Asked::Play(Cue::Select)]);
        // The same value again changes nothing.
        shell.set_vibration(haptics::Level::Light);
        assert_eq!(shell.haptics.asked.len(), 2);
    }
}
