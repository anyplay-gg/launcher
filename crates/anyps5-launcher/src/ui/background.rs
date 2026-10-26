//! Ported from utkuhalis/AnyPS5-ARM (BackgroundRenderer.swift, GPL-2.0-only) and changed since; see README "Credits".
//! The animated theme backgrounds (docs/DESIGN.md sections 3 and 14) as a wgpu paint callback.
//!
//! A [`Background`] renders its theme's shader into a **half-resolution** offscreen texture (a
//! *slot*) and draws that texture, bilinearly upscaled, into the callback rect. The offscreen
//! texture is also the source of the material blur ([`crate::ui::blur`]) that glass panels sample.
//!
//! * [`init`] once at start-up (after the wgpu render state exists).
//! * [`Backdrop`] is the app-level helper: the clock (random start, paused while a game runs, the
//!   window is inactive or motion is reduced), theme change cross-fade, and painting.
//! * [`Background`] is the low-level callback for fixed times (gallery, onboarding theme cards).

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use eframe::egui_wgpu::{self, wgpu};
use egui::{Context, Painter, Rect, pos2};

use crate::ui::anim::{AnimExt, Motion};
use crate::ui::blur::{BlurChain, BlurPipelines, BlurSettings, DOWNSCALE};
use crate::ui::gpu::{self, BACKDROP_FORMAT, Target};
use crate::ui::theme::Theme;

/// Slot used by the main window backdrop (blurred by glass panels).
pub const SLOT_MAIN: u32 = 0;
/// Slot of the outgoing theme during a cross-fade.
pub const SLOT_FADE: u32 = 1;
/// First slot for small previews such as the onboarding theme cards (`SLOT_CARDS + theme index`).
pub const SLOT_CARDS: u32 = 8;

/// Time used for the full-frame theme stills of the original's preview renderer.
pub const STILL_TIME_FULL: f32 = 260.0;
/// Time used for the 660 x 372 theme-card stills.
pub const STILL_TIME_CARD: f32 = 200.0;
/// The shader runs at this fraction of the physical pixel size.
pub const RENDER_SCALE: f32 = 0.5;
/// The pause between frames while only the backdrop (or the artwork drift) moves. 30 ms, not
/// 33: the next present then lands on the second vsync (33.3 ms at 60 Hz) instead of slipping to
/// the third (50 ms). The clock advances by the real frame time, so the drift speed is the same.
pub const IDLE_REPAINT: std::time::Duration = std::time::Duration::from_millis(30);
/// Wraps the clock like the original (`clock mod 3600`).
pub const CLOCK_WRAP: f64 = 3600.0;

// ---------------------------------------------------------------------------------------------
// Clock (3.1)
// ---------------------------------------------------------------------------------------------

/// The shader clock: starts somewhere in the middle of the animation (random in 40..=400 s, not at
/// its quiet first frame) and advances by at most 0.1 s per frame.
#[derive(Clone, Copy, Debug)]
pub struct BackgroundClock {
    clock: f64,
    pub speed: f64,
}

impl BackgroundClock {
    /// A clock starting at `start` seconds.
    pub fn starting_at(start: f64) -> Self {
        Self {
            clock: start,
            speed: 1.0,
        }
    }

    /// A clock with a random start in 40..=400 s.
    pub fn random_start() -> Self {
        Self::starting_at(40.0 + 360.0 * unit_random())
    }

    /// Advances by `dt` (clamped to 0.1 s so pauses do not jump) unless `paused`.
    pub fn advance(&mut self, dt: f64, paused: bool) {
        if !paused {
            self.clock += dt.clamp(0.0, 0.1) * self.speed;
        }
    }

    /// The `time` uniform: `clock mod 3600`.
    pub fn time(&self) -> f32 {
        (self.clock % CLOCK_WRAP) as f32
    }
}

/// A cheap non-cryptographic random number in 0..1 from the system clock.
fn unit_random() -> f64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    (hasher.finish() >> 11) as f64 / (1u64 << 53) as f64
}

// ---------------------------------------------------------------------------------------------
// App-level helper
// ---------------------------------------------------------------------------------------------

/// Everything the home backdrop needs. Call [`update`](Self::update) then [`paint`](Self::paint)
/// first thing in every frame, before any UI.
pub struct Backdrop {
    clock: BackgroundClock,
    theme: Theme,
    fade_from: Option<Theme>,
    fade_id: u32,
    painted_once: bool,
    /// Paint a frozen frame (a game runs, the window is inactive, reduced motion).
    pub paused: bool,
    /// Glass panels need the blurred copy; turn it off while no panel is visible to save the work.
    pub blur: Option<BlurSettings>,
    /// Mac style: the window is see-through. The backdrop is a still of the theme over a dark
    /// plate, seen through at `GLASS_VEIL`; nothing moves, so nothing is redrawn.
    pub window_glass: bool,
}

/// The theme still's opacity over the plate while the window is see-through (docs/DESIGN.md 19.2).
pub const GLASS_VEIL: f32 = 0.62;
/// Black under the veil: the least dark the window gets over the system blur (alpha 0..1).
pub const GLASS_PLATE: f32 = 0.5;

impl Backdrop {
    pub fn new(theme: Theme) -> Self {
        Self {
            clock: BackgroundClock::random_start(),
            theme,
            fade_from: None,
            fade_id: 0,
            painted_once: false,
            paused: false,
            blur: Some(BlurSettings::default()),
            window_glass: false,
        }
    }

    /// For tests and stills: a fixed clock.
    pub fn with_clock(theme: Theme, clock: BackgroundClock) -> Self {
        Self {
            clock,
            ..Self::new(theme)
        }
    }

    pub fn theme(&self) -> Theme {
        self.theme
    }

    /// Changes the theme; the old one fades out over `Motion::Fade` (easeInOut 0.55 s).
    pub fn set_theme(&mut self, theme: Theme) {
        if theme != self.theme {
            self.fade_from = Some(self.theme);
            self.fade_id = self.fade_id.wrapping_add(1);
            self.theme = theme;
        }
    }

    /// Advances the clock and requests the next frame (about 30 fps while idle) while it runs.
    /// Springs, the theme cross-fade and panel motion ask for their own frames and stay at the
    /// display rate; input draws at once.
    pub fn update(&mut self, ctx: &Context) {
        let dt = f64::from(ctx.input(|i| i.unstable_dt));
        let paused = self.paused || ctx.reduced_motion() || self.window_glass;
        self.clock.advance(dt, paused);
        if !paused {
            ctx.request_repaint_after(IDLE_REPAINT);
        }
    }

    /// Paints the backdrop into `rect`.
    pub fn paint(&mut self, ctx: &Context, painter: &Painter, rect: Rect) {
        crate::ui::glass::begin_frame();
        crate::ui::glass::set_window_glass(self.window_glass);
        let (time, veil) = if self.window_glass {
            painter.rect_filled(rect, 0.0, crate::ui::theme::black(GLASS_PLATE));
            (STILL_TIME_FULL, GLASS_VEIL)
        } else {
            (self.clock.time(), 1.0)
        };
        let mut layer = Background::new(SLOT_MAIN, self.theme, time);
        layer.blur = self.blur;
        layer.alpha = veil;
        layer.paint(painter, rect);
        if let Some(from) = self.fade_from {
            let alpha =
                ctx.animate_from_to(("backdrop/fade", self.fade_id), 1.0, 0.0, Motion::Fade);
            if alpha <= 0.001 {
                self.fade_from = None;
            } else {
                let mut old = Background::new(SLOT_FADE, from, time);
                old.alpha = alpha * veil;
                old.paint(painter, rect);
            }
        }
        self.painted_once = true;
    }
}

// ---------------------------------------------------------------------------------------------
// The paint callback
// ---------------------------------------------------------------------------------------------

/// One background layer.
#[derive(Clone, Copy, Debug)]
pub struct Background {
    /// Offscreen texture to render into; layers with different content need different slots.
    pub slot: u32,
    pub theme: Theme,
    pub time: f32,
    /// Opacity of the layer when drawn (cross-fade).
    pub alpha: f32,
    /// Also produce the blurred copy for glass panels.
    pub blur: Option<BlurSettings>,
    /// Part of the offscreen image to show (0..1 uv rectangle); the full image by default.
    pub uv: Rect,
    /// Corner radius in dp (theme cards); 0 = square corners.
    pub radius: f32,
}

impl Background {
    pub fn new(slot: u32, theme: Theme, time: f32) -> Self {
        Self {
            slot,
            theme,
            time,
            alpha: 1.0,
            blur: None,
            uv: Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            radius: 0.0,
        }
    }

    /// Adds the layer to `painter` (it covers `rect`).
    pub fn paint(self, painter: &Painter, rect: Rect) {
        painter.add(egui_wgpu::Callback::new_paint_callback(
            rect,
            BackgroundCallback { layer: self, rect },
        ));
    }
}

struct BackgroundCallback {
    layer: Background,
    rect: Rect,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BgUniforms {
    deep: [f32; 4],
    middle: [f32; 4],
    bright: [f32; 4],
    size: [f32; 2],
    time: f32,
    theme: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlitUniforms {
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    rect: [f32; 4],
    alpha: f32,
    linearize: u32,
    radius: f32,
    _pad: f32,
}

/// What a slot was last rendered with; the shader pass is skipped when nothing changed.
#[derive(Clone, Copy, PartialEq, Eq)]
struct RenderKey {
    theme: Theme,
    time_bits: u32,
    size: (u32, u32),
}

/// An offscreen backdrop image and everything derived from it.
pub(crate) struct Slot {
    target: Target,
    bg_uniform: wgpu::Buffer,
    bg_bind: wgpu::BindGroup,
    blit_uniform: wgpu::Buffer,
    blit_bind: wgpu::BindGroup,
    last_key: Option<RenderKey>,
    pub(crate) blur: Option<BlurChain>,
    /// Bind group (sharp, blurred, sampler) for the glass pipeline.
    pub(crate) glass_bind: Option<wgpu::BindGroup>,
    blur_stamp: Option<RenderKey>,
}

impl Slot {
    pub(crate) fn size(&self) -> (u32, u32) {
        self.target.size
    }

    pub(crate) fn has_blur(&self) -> bool {
        self.blur.is_some() && self.glass_bind.is_some()
    }
}

/// Shared GPU state, stored in egui's callback resources.
pub(crate) struct BackdropGpu {
    pub(crate) sampler: wgpu::Sampler,
    pub(crate) linearize: bool,
    bg_pipeline: wgpu::RenderPipeline,
    bg_layout: wgpu::BindGroupLayout,
    blit_pipeline: wgpu::RenderPipeline,
    blit_layout: wgpu::BindGroupLayout,
    target_format: wgpu::TextureFormat,
    reload: Option<ShaderReload>,
    pub(crate) blur_pipelines: BlurPipelines,
    pub(crate) slots: HashMap<u32, Slot>,
    glass_texture_layout: wgpu::BindGroupLayout,
}

/// Development aid: with `ANYPS5_SHADER_DIR` set to `crates/anyps5-launcher/src/ui/shaders`, the
/// background and blit shaders are read from that folder and reloaded whenever a file changes. A
/// file that does not validate is logged and the running shaders stay.
struct ShaderReload {
    dir: std::path::PathBuf,
    stamp: Option<std::time::SystemTime>,
    next_check: std::time::Instant,
}

impl ShaderReload {
    const FILES: [&str; 2] = ["background.wgsl", "blit.wgsl"];

    fn from_env() -> Option<Self> {
        let dir = std::path::PathBuf::from(std::env::var_os("ANYPS5_SHADER_DIR")?);
        tracing::info!("backdrop shaders are reloaded from {}", dir.display());
        Some(Self {
            dir,
            stamp: None,
            next_check: std::time::Instant::now(),
        })
    }

    fn stamp(&self) -> Option<std::time::SystemTime> {
        Self::FILES
            .iter()
            .filter_map(|name| std::fs::metadata(self.dir.join(name)).ok()?.modified().ok())
            .max()
    }

    /// The background and blit sources when a file changed since the last call and both validate.
    fn poll(&mut self) -> Option<(String, String)> {
        let now = std::time::Instant::now();
        if now < self.next_check {
            return None;
        }
        self.next_check = now + std::time::Duration::from_millis(250);
        let stamp = self.stamp();
        if stamp == self.stamp {
            return None;
        }
        self.stamp = stamp;
        let [background, blit] = Self::FILES.map(|name| self.read(name));
        Some((background?, blit?))
    }

    fn read(&self, name: &str) -> Option<String> {
        use naga::valid::{Capabilities, ValidationFlags, Validator};
        let source = std::fs::read_to_string(self.dir.join(name))
            .inspect_err(|err| tracing::warn!("{name}: {err}"))
            .ok()?;
        let module = naga::front::wgsl::parse_str(&source)
            .inspect_err(|err| tracing::warn!("{name}:\n{}", err.emit_to_string(&source)))
            .ok()?;
        Validator::new(ValidationFlags::all(), Capabilities::all())
            .validate(&module)
            .inspect_err(|err| tracing::warn!("{name}:\n{}", err.emit_to_string(&source)))
            .ok()?;
        Some(source)
    }
}

/// The theme background and blit pipelines from WGSL sources.
fn build_pipelines(
    device: &wgpu::Device,
    background: &str,
    blit: &str,
    bg_layout: &wgpu::BindGroupLayout,
    blit_layout: &wgpu::BindGroupLayout,
    target_format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let bg_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("theme background"),
        source: wgpu::ShaderSource::Wgsl(background.into()),
    });
    let blit_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("backdrop blit"),
        source: wgpu::ShaderSource::Wgsl(blit.into()),
    });
    let bg_pipeline = gpu::triangle_pipeline(
        device,
        "theme background",
        &bg_module,
        "fs",
        &[bg_layout],
        BACKDROP_FORMAT,
        None,
    );
    let blit_pipeline = gpu::triangle_pipeline(
        device,
        "backdrop blit",
        &blit_module,
        "fs",
        &[blit_layout],
        target_format,
        Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
    );
    (bg_pipeline, blit_pipeline)
}

/// Builds the pipelines. Call once from the app creator.
pub fn init(render_state: &egui_wgpu::RenderState) {
    let device = &render_state.device;
    let sampler = gpu::linear_sampler(device);
    let reload = ShaderReload::from_env();
    if reload.is_some() {
        device.on_uncaptured_error(std::sync::Arc::new(|err| tracing::error!("wgpu: {err}")));
    }
    let bg_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("background layout"),
        entries: &[gpu::uniform_entry(0, false)],
    });
    let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("blit layout"),
        entries: &[
            gpu::uniform_entry(0, false),
            gpu::texture_entry(1),
            gpu::sampler_entry(2),
        ],
    });
    let (bg_pipeline, blit_pipeline) = build_pipelines(
        device,
        BACKGROUND_WGSL,
        BLIT_WGSL,
        &bg_layout,
        &blit_layout,
        render_state.target_format,
    );
    let linearize = gpu::needs_linearize(render_state.target_format);
    let glass_texture_layout = crate::ui::glass::texture_layout(device);
    let glass =
        crate::ui::glass::GlassGpu::new(device, render_state.target_format, &glass_texture_layout);
    let mut renderer = render_state.renderer.write();
    renderer.callback_resources.insert(BackdropGpu {
        sampler,
        linearize,
        bg_pipeline,
        bg_layout,
        blit_pipeline,
        blit_layout,
        target_format: render_state.target_format,
        reload,
        blur_pipelines: BlurPipelines::new(device),
        slots: HashMap::new(),
        glass_texture_layout,
    });
    renderer.callback_resources.insert(glass);
}

/// The WGSL source of the three themes (docs/DESIGN.md 3.4).
pub const BACKGROUND_WGSL: &str = include_str!("shaders/background.wgsl");
/// The WGSL source of the blit.
pub const BLIT_WGSL: &str = include_str!("shaders/blit.wgsl");

/// Finds or (re)creates the slot `id` for an offscreen image of `size`.
fn get_slot<'a>(
    slots: &'a mut HashMap<u32, Slot>,
    bg_layout: &wgpu::BindGroupLayout,
    blit_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    device: &wgpu::Device,
    id: u32,
    size: (u32, u32),
) -> &'a mut Slot {
    let stale = slots.get(&id).is_none_or(|s| s.target.size != size);
    if stale {
        let target = Target::new(device, "backdrop slot", size, BACKDROP_FORMAT);
        let uniform = |label, bytes| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let bg_uniform = uniform(
            "background uniform",
            std::mem::size_of::<BgUniforms>() as u64,
        );
        let blit_uniform = uniform("blit uniform", std::mem::size_of::<BlitUniforms>() as u64);
        let bg_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("background bind"),
            layout: bg_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: bg_uniform.as_entire_binding(),
            }],
        });
        let blit_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("blit bind"),
            layout: blit_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: blit_uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&target.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        slots.insert(
            id,
            Slot {
                target,
                bg_uniform,
                bg_bind,
                blit_uniform,
                blit_bind,
                last_key: None,
                blur: None,
                glass_bind: None,
                blur_stamp: None,
            },
        );
    }
    slots.get_mut(&id).expect("slot was just inserted")
}

impl egui_wgpu::CallbackTrait for BackgroundCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let Some(gpu) = resources.get_mut::<BackdropGpu>() else {
            return Vec::new();
        };
        if let Some((background, blit)) = gpu.reload.as_mut().and_then(ShaderReload::poll) {
            (gpu.bg_pipeline, gpu.blit_pipeline) = build_pipelines(
                device,
                &background,
                &blit,
                &gpu.bg_layout,
                &gpu.blit_layout,
                gpu.target_format,
            );
            for slot in gpu.slots.values_mut() {
                slot.last_key = None;
                slot.blur_stamp = None;
            }
            tracing::info!("backdrop shaders reloaded");
        }
        let BackdropGpu {
            sampler,
            linearize,
            bg_pipeline,
            bg_layout,
            blit_layout,
            blur_pipelines,
            slots,
            glass_texture_layout,
            ..
        } = gpu;
        let layer = &self.layer;
        let ppp = screen.pixels_per_point;
        let size = (
            (self.rect.width() * ppp * RENDER_SCALE).ceil().max(1.0) as u32,
            (self.rect.height() * ppp * RENDER_SCALE).ceil().max(1.0) as u32,
        );
        let slot = get_slot(
            slots,
            bg_layout,
            blit_layout,
            sampler,
            device,
            layer.slot,
            size,
        );
        let key = RenderKey {
            theme: layer.theme,
            time_bits: layer.time.to_bits(),
            size,
        };
        if slot.last_key != Some(key) {
            let palette = layer.theme.palette();
            let uniforms = BgUniforms {
                deep: [palette.deep[0], palette.deep[1], palette.deep[2], 1.0],
                middle: [palette.middle[0], palette.middle[1], palette.middle[2], 1.0],
                bright: [palette.bright[0], palette.bright[1], palette.bright[2], 1.0],
                size: [size.0 as f32, size.1 as f32],
                time: layer.time,
                theme: layer.theme.index(),
            };
            queue.write_buffer(&slot.bg_uniform, 0, bytemuck::bytes_of(&uniforms));
            let mut pass = gpu::begin_pass(encoder, "theme background", &slot.target.view);
            pass.set_pipeline(bg_pipeline);
            pass.set_bind_group(0, &slot.bg_bind, &[]);
            pass.draw(0..3, 0..1);
            drop(pass);
            slot.last_key = Some(key);
        }
        // Painting happens after all UI: the blurred copy is made only when a panel samples it.
        if let Some(settings) = layer.blur.filter(|_| crate::ui::glass::panels_painted()) {
            let new_chain = slot.blur.as_ref().is_none_or(|c| c.source_size() != size);
            if new_chain {
                slot.blur = Some(BlurChain::new(device, blur_pipelines, sampler, size));
                slot.glass_bind = None;
                slot.blur_stamp = None;
            }
            if slot.blur_stamp != Some(key) {
                if let Some(chain) = slot.blur.as_mut() {
                    let sigma_texels = settings.sigma_dp * ppp * RENDER_SCALE / DOWNSCALE as f32;
                    chain.run(
                        device,
                        queue,
                        encoder,
                        blur_pipelines,
                        sampler,
                        &slot.target.view,
                        new_chain,
                        sigma_texels,
                    );
                }
                slot.blur_stamp = Some(key);
            }
            if let (true, Some(chain)) = (slot.glass_bind.is_none(), slot.blur.as_ref()) {
                slot.glass_bind = Some(crate::ui::glass::texture_bind_group(
                    device,
                    glass_texture_layout,
                    &slot.target.view,
                    chain.output(),
                    sampler,
                ));
            }
        }
        let uv = layer.uv;
        queue.write_buffer(
            &slot.blit_uniform,
            0,
            bytemuck::bytes_of(&BlitUniforms {
                uv_min: [uv.min.x, uv.min.y],
                uv_max: [uv.max.x, uv.max.y],
                rect: [
                    self.rect.min.x * ppp,
                    self.rect.min.y * ppp,
                    self.rect.width() * ppp,
                    self.rect.height() * ppp,
                ],
                alpha: layer.alpha.clamp(0.0, 1.0),
                linearize: u32::from(*linearize),
                radius: layer.radius * ppp,
                _pad: 0.0,
            }),
        );
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(gpu) = resources.get::<BackdropGpu>() else {
            return;
        };
        let Some(slot) = gpu.slots.get(&self.layer.slot) else {
            return;
        };
        pass.set_pipeline(&gpu.blit_pipeline);
        pass.set_bind_group(0, &slot.blit_bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_starts_in_the_middle_and_wraps() {
        for _ in 0..50 {
            let clock = BackgroundClock::random_start();
            assert!(
                (40.0..=400.0).contains(&f64::from(clock.time())),
                "{}",
                clock.time()
            );
        }
        let wrapped = BackgroundClock::starting_at(3599.95);
        let mut c = wrapped;
        c.advance(0.1, false);
        assert!(c.time() < 1.0, "wrapped to {}", c.time());
    }

    #[test]
    fn clock_clamps_dt_and_pauses() {
        let mut c = BackgroundClock::starting_at(100.0);
        c.advance(5.0, false);
        assert!(
            (c.time() - 100.1).abs() < 1e-4,
            "a long frame advances at most 0.1 s"
        );
        c.advance(0.05, true);
        assert!((c.time() - 100.1).abs() < 1e-4, "paused: no advance");
        c.speed = 2.0;
        c.advance(0.05, false);
        assert!((c.time() - 100.2).abs() < 1e-4);
    }

    /// The delay the backdrop asks for after one pass.
    fn repaint_delay(backdrop: &mut Backdrop, reduced: bool) -> std::time::Duration {
        let ctx = Context::default();
        ctx.set_reduced_motion(reduced);
        // A fresh context owes one immediate frame; the second pass shows the real request.
        for _ in 0..2 {
            ctx.run_ui(egui::RawInput::default(), |_| {})
                .textures_delta
                .clear();
        }
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| backdrop.update(&ctx));
        out.textures_delta.clear();
        out.viewport_output[&egui::ViewportId::ROOT].repaint_delay
    }

    #[test]
    fn idle_backdrop_repaints_at_about_30_hz() {
        let mut backdrop = Backdrop::with_clock(Theme::Cosmic, BackgroundClock::starting_at(100.0));
        backdrop.paused = false;
        let delay = repaint_delay(&mut backdrop, false);
        assert!(delay >= std::time::Duration::from_millis(30), "{delay:?}");
        assert!(delay < std::time::Duration::from_millis(34), "{delay:?}");
        assert_eq!(delay, IDLE_REPAINT);
    }

    #[test]
    fn paused_and_reduced_motion_request_no_repaint() {
        let mut backdrop = Backdrop::with_clock(Theme::Cosmic, BackgroundClock::starting_at(100.0));
        backdrop.paused = true;
        assert_eq!(
            repaint_delay(&mut backdrop, false),
            std::time::Duration::MAX
        );
        backdrop.paused = false;
        assert_eq!(repaint_delay(&mut backdrop, true), std::time::Duration::MAX);
    }

    #[test]
    fn the_clock_follows_real_time_at_any_frame_rate() {
        // Same wall time, fewer frames: the drift does not slow down.
        let (mut fast, mut slow) = (
            BackgroundClock::starting_at(10.0),
            BackgroundClock::starting_at(10.0),
        );
        for _ in 0..60 {
            fast.advance(1.0 / 60.0, false);
        }
        for _ in 0..30 {
            slow.advance(1.0 / 30.0, false);
        }
        assert!((fast.time() - slow.time()).abs() < 1e-3);
    }

    #[test]
    fn uniform_blocks_match_the_shader_layout() {
        assert_eq!(std::mem::size_of::<BgUniforms>(), 64);
        assert_eq!(std::mem::size_of::<BlitUniforms>(), 48);
    }
}
