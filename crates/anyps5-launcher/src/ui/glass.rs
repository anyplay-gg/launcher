//! Frosted glass (docs/DESIGN.md section 4): rounded rectangles filled from the blurred backdrop.
//!
//! [`paint_glass`] draws one panel; [`GlassStyle`] has a preset for every use in the original
//! (info cards, toast, log boxes, full-screen panels, side panel, splash).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bytemuck::{Pod, Zeroable};
use eframe::egui_wgpu::{self, wgpu};
use egui::emath::GuiRounding;
use egui::{Color32, Context, Painter, Rect, Stroke, StrokeKind, epaint::RectShape};

use crate::ui::background::{BackdropGpu, SLOT_MAIN};
use crate::ui::gpu;
use crate::ui::theme::{self, black, color};

/// Most glass panels drawn in one frame.
pub const MAX_PANELS: usize = 512;
/// Uniform buffer offset alignment (the wgpu default limit).
const STRIDE: u64 = 256;

/// How a glass panel is composed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassStyle {
    /// Corner radius in dp.
    pub radius: f32,
    /// Share of the material over the sharp backdrop (`.ultraThinMaterial.opacity(0.55)` = 0.55;
    /// a plain `.background(.ultraThinMaterial)` = 1).
    pub material_mix: f32,
    /// Black laid over the material (the `opacity` of `.glass`).
    pub black: f32,
    /// Colour laid over everything (straight alpha), for the side panel.
    pub overlay: Color32,
    /// 1 px inner stroke of white 0.09.
    pub stroke: bool,
    /// Opacity of the whole panel (a panel inside a fading screen); [`paint_glass_on`] also
    /// multiplies in the painter's own opacity.
    pub alpha: f32,
}

/// Material tint toward white and saturation boost of the blurred copy (estimates, DESIGN.md 12).
const MATERIAL_TINT: f32 = 0.06;
const MATERIAL_SATURATION: f32 = 1.25;

impl GlassStyle {
    /// `.glass(radius, opacity:)` of the original.
    pub const fn glass(radius: f32, black: f32) -> Self {
        Self {
            radius,
            material_mix: 0.55,
            black,
            overlay: Color32::TRANSPARENT,
            stroke: true,
            alpha: 1.0,
        }
    }

    /// Info cards and default glass: radius 16, black 0.38.
    pub const fn card() -> Self {
        Self::glass(theme::radius::CARD, 0.38)
    }

    /// Toast: radius 28, black 0.6.
    pub const fn toast() -> Self {
        Self::glass(theme::radius::TOAST, 0.6)
    }

    /// Diagnosis box in the log: radius 14, black 0.5.
    pub const fn diagnosis_box() -> Self {
        Self::glass(theme::radius::BOX, 0.5)
    }

    /// Log text box: radius 14, black 0.55.
    pub const fn log_box() -> Self {
        Self::glass(theme::radius::BOX, 0.55)
    }

    /// Full-screen panels (settings, library, trophies): material plus black 0.74.
    pub const fn fullscreen_panel() -> Self {
        Self {
            radius: 0.0,
            material_mix: 1.0,
            black: 0.74,
            overlay: Color32::TRANSPARENT,
            stroke: false,
            alpha: 1.0,
        }
    }

    /// Log view: material plus black 0.65.
    pub const fn log_panel() -> Self {
        Self {
            black: 0.65,
            ..Self::fullscreen_panel()
        }
    }

    /// Launch splash: material plus black 0.55.
    pub const fn splash() -> Self {
        Self {
            black: 0.55,
            ..Self::fullscreen_panel()
        }
    }

    /// Side panel: material plus a near-black gray at 0.92.
    pub fn side_panel() -> Self {
        Self {
            radius: 0.0,
            material_mix: 1.0,
            black: 0.0,
            overlay: color::side_panel(),
            stroke: false,
            alpha: 1.0,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// GPU
// ---------------------------------------------------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GlassUniforms {
    rect: [f32; 4],
    backdrop: [f32; 4],
    overlay: [f32; 4],
    radius: f32,
    material_mix: f32,
    tint: f32,
    black: f32,
    saturation: f32,
    linearize: u32,
    alpha: f32,
    _pad: f32,
}

/// Layout of the per-slot texture bind group (sharp, blurred, sampler).
pub(crate) fn texture_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("glass textures"),
        entries: &[
            gpu::texture_entry(0),
            gpu::texture_entry(1),
            gpu::sampler_entry(2),
        ],
    })
}

pub(crate) fn texture_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sharp: &wgpu::TextureView,
    blurred: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glass textures"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(sharp),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(blurred),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

/// Pipeline and uniform ring of the glass pass.
pub(crate) struct GlassGpu {
    pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    uniform_bind: wgpu::BindGroup,
}

impl GlassGpu {
    pub(crate) fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        texture_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass"),
            source: wgpu::ShaderSource::Wgsl(GLASS_WGSL.into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass uniforms"),
            entries: &[gpu::uniform_entry(0, true)],
        });
        let pipeline = gpu::triangle_pipeline(
            device,
            "glass",
            &module,
            "fs",
            &[&uniform_layout, texture_layout],
            target_format,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glass uniform ring"),
            size: STRIDE * MAX_PANELS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass uniform bind"),
            layout: &uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniforms,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<GlassUniforms>() as u64),
                }),
            }],
        });
        Self {
            pipeline,
            uniforms,
            uniform_bind,
        }
    }
}

/// The WGSL source of the glass pass.
pub const GLASS_WGSL: &str = include_str!("shaders/glass.wgsl");

struct GlassCallback {
    /// Position in this frame's uniform ring.
    index: usize,
    rect: Rect,
    backdrop: Rect,
    slot: u32,
    style: GlassStyle,
}

impl egui_wgpu::CallbackTrait for GlassCallback {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let linearize = resources
            .get::<BackdropGpu>()
            .is_some_and(|gpu| gpu.linearize);
        let Some(glass) = resources.get::<GlassGpu>() else {
            return Vec::new();
        };
        if self.index >= MAX_PANELS {
            return Vec::new();
        }
        let ppp = screen.pixels_per_point;
        let px = |r: Rect| {
            [
                r.min.x * ppp,
                r.min.y * ppp,
                r.width() * ppp,
                r.height() * ppp,
            ]
        };
        let [r, g, b, a] = self.style.overlay.to_srgba_unmultiplied();
        let uniforms = GlassUniforms {
            rect: px(self.rect),
            backdrop: px(self.backdrop),
            overlay: [
                f32::from(r) / 255.0,
                f32::from(g) / 255.0,
                f32::from(b) / 255.0,
                f32::from(a) / 255.0,
            ],
            radius: self.style.radius * ppp,
            material_mix: self.style.material_mix,
            tint: MATERIAL_TINT,
            black: self.style.black,
            saturation: MATERIAL_SATURATION,
            linearize: u32::from(linearize),
            alpha: self.style.alpha.clamp(0.0, 1.0),
            _pad: 0.0,
        };
        queue.write_buffer(
            &glass.uniforms,
            self.index as u64 * STRIDE,
            bytemuck::bytes_of(&uniforms),
        );
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        if self.index >= MAX_PANELS {
            return;
        }
        let (Some(glass), Some(backdrop)) =
            (resources.get::<GlassGpu>(), resources.get::<BackdropGpu>())
        else {
            return;
        };
        let Some(textures) = backdrop
            .slots
            .get(&self.slot)
            .and_then(|slot| slot.glass_bind.as_ref())
        else {
            return;
        };
        pass.set_pipeline(&glass.pipeline);
        pass.set_bind_group(
            0,
            &glass.uniform_bind,
            &[(self.index as u64 * STRIDE) as u32],
        );
        pass.set_bind_group(1, textures, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Glass panels painted since the last [`begin_frame`]: while there are none, the backdrop skips
/// the blurred copy nobody samples.
static PANELS: AtomicUsize = AtomicUsize::new(0);

/// Starts a frame's count of glass panels (the backdrop calls it before any UI is painted).
pub(crate) fn begin_frame() {
    PANELS.store(0, Ordering::Relaxed);
}

/// Whether a glass panel was painted since [`begin_frame`].
pub(crate) fn panels_painted() -> bool {
    PANELS.load(Ordering::Relaxed) > 0
}

/// Mac style: the window is see-through, so panels let a little of it through (docs/DESIGN.md 19.2).
static WINDOW_GLASS: AtomicBool = AtomicBool::new(false);
/// How much of a panel is opaque while the window is see-through.
pub const WINDOW_GLASS_PANEL: f32 = 0.96;

/// Set by the backdrop at the start of a frame.
pub(crate) fn set_window_glass(on: bool) {
    WINDOW_GLASS.store(on, Ordering::Relaxed);
}

/// Next free position in this pass's uniform ring.
fn next_index(ctx: &Context) -> usize {
    #[derive(Clone, Default)]
    struct Counter {
        pass: u64,
        next: usize,
    }
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let counter =
            d.get_temp_mut_or_default::<Counter>(egui::Id::unique("anyps5-launcher/glass-counter"));
        if counter.pass != pass {
            counter.pass = pass;
            counter.next = 0;
        }
        let index = counter.next;
        counter.next += 1;
        PANELS.store(counter.next, Ordering::Relaxed);
        index
    })
}

/// Draws a glass panel into `rect`, sampling the backdrop in `slot` (usually [`SLOT_MAIN`], whose
/// [`Background`](crate::ui::background::Background) must have its blur enabled and cover
/// `backdrop` - normally the whole window).
pub fn paint_glass_on(
    ctx: &Context,
    painter: &Painter,
    rect: Rect,
    backdrop: Rect,
    slot: u32,
    style: GlassStyle,
) {
    let mut style = style;
    style.alpha *= painter.opacity();
    if WINDOW_GLASS.load(Ordering::Relaxed) {
        style.alpha *= WINDOW_GLASS_PANEL;
    }
    if style.alpha <= 0.0 {
        return;
    }
    // The shader and the stroke share one pixel-aligned rect and one whole-dp radius.
    let rect = rect.round_to_pixels(ctx.pixels_per_point());
    style.radius = style.radius.round();
    painter.add(egui_wgpu::Callback::new_paint_callback(
        rect,
        GlassCallback {
            index: next_index(ctx),
            rect,
            backdrop,
            slot,
            style,
        },
    ));
    if style.stroke {
        // SwiftUI `strokeBorder` draws inside the shape.
        painter.add(RectShape::stroke(
            rect,
            style.radius,
            Stroke::new(1.0 / ctx.pixels_per_point(), color::hairline_glass()),
            StrokeKind::Inside,
        ));
    }
}

/// Draws a glass panel over the main window backdrop (`ctx.content_rect()`).
pub fn paint_glass(ctx: &Context, painter: &Painter, rect: Rect, style: GlassStyle) {
    paint_glass_on(ctx, painter, rect, ctx.content_rect(), SLOT_MAIN, style);
}

/// Fallback when the GPU path is unavailable: the flat black overlay alone.
pub fn paint_flat(painter: &Painter, rect: Rect, style: GlassStyle) {
    painter.rect_filled(rect, style.radius, black(style.black.max(0.3)));
}
