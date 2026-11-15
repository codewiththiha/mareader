//! The page pass: every page, and flat fills, as tinted quads.

use std::sync::{Arc, OnceLock};

use eframe::egui_wgpu::{self, CallbackResources, ScreenDescriptor, wgpu};
use egui::{Color32, Pos2, Rect};
use mareader_appearance::{Blend, MAX_FILTER_OPS, PATTERN_PERIOD, PageTone, TextureMode};
use wgpu::util::DeviceExt as _;

/// A page raster on the GPU, ready to bind.
#[derive(Debug)]
pub struct GpuPage {
    pub(super) bind_group: wgpu::BindGroup,
    _texture: wgpu::Texture,
}

/// One quad: a page, or a flat fill when `texture` is none.
#[derive(Clone, Debug)]
pub struct Quad {
    pub rect: Rect,
    /// What clips it on screen: its pane's scroll area.
    pub clip: Rect,
    pub tint: Color32,
    pub texture: Option<Arc<GpuPage>>,
    /// Its pane's theme; fills keep [`PageTone::NONE`].
    pub tone: PageTone,
    /// Where its pattern starts: one per pane, so pages share one sheet.
    pub anchor: Anchor,
    /// The rounded rect it shows within: its own, or its pane's.
    pub round: Rect,
    /// That rect's corner radius in points; 0 keeps it square.
    pub radius: f32,
}

/// A pattern's origin on screen, and its points per page point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub origin: Pos2,
    pub zoom: f32,
}

impl Anchor {
    /// The page drawn in `rect`, `width` page points wide.
    pub fn page(rect: Rect, width: f32) -> Self {
        Self {
            origin: rect.min,
            zoom: rect.width() / width.max(1e-3),
        }
    }
}

impl Quad {
    pub fn fill(rect: Rect, colour: Color32) -> Self {
        Self {
            rect,
            clip: rect,
            tint: colour,
            texture: None,
            tone: PageTone::NONE,
            anchor: Anchor {
                origin: rect.min,
                zoom: 1.0,
            },
            round: rect,
            radius: 0.0,
        }
    }

    /// The paper behind a pane's pages: a backdrop `tone` over white.
    pub fn backdrop(rect: Rect, radius: f32, tone: PageTone, anchor: Anchor) -> Self {
        Self {
            rect,
            clip: rect,
            tint: Color32::WHITE,
            texture: None,
            tone,
            anchor,
            round: rect,
            radius,
        }
    }

    /// A page raster, or a blank page while none is drawn, themed by `tone`.
    pub fn page(
        (rect, clip): (Rect, Rect),
        texture: Option<Arc<GpuPage>>,
        tone: PageTone,
        anchor: Anchor,
    ) -> Self {
        Self {
            rect,
            clip,
            tint: Color32::WHITE,
            texture,
            tone,
            anchor,
            round: rect,
            radius: 0.0,
        }
    }

    /// Hides what falls outside `frame`'s rounded corners.
    pub fn round_to(&mut self, frame: Rect, radius: f32) {
        self.round = frame;
        self.radius = radius;
    }
}

/// Bytes of one tone in the shader's `Tone` struct.
const TONE_BYTES: u64 = (MAX_FILTER_OPS as u64 * 3 + 5) * 16;

/// One draw's tones on the GPU, each once, and which one each quad uses.
pub(super) struct Tones {
    group: wgpu::BindGroup,
    offsets: Vec<u32>,
}

impl Tones {
    pub(super) fn new(device: &wgpu::Device, pass: &PagePass, quads: &[Quad]) -> Self {
        let align = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let stride = TONE_BYTES.div_ceil(align) * align;
        let mut unique: Vec<PageTone> = Vec::new();
        let mut offsets = Vec::with_capacity(quads.len());
        for q in quads {
            let i = unique.iter().position(|t| *t == q.tone).unwrap_or_else(|| {
                unique.push(q.tone);
                unique.len() - 1
            });
            offsets.push((i as u64 * stride) as u32);
        }
        if unique.is_empty() {
            unique.push(PageTone::NONE);
        }
        let mut bytes = vec![0u8; unique.len() * stride as usize];
        for (chunk, tone) in bytes.chunks_mut(stride as usize).zip(&unique) {
            for (to, from) in chunk.iter_mut().zip(tone_bytes(tone)) {
                *to = from;
            }
        }
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("page tones"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("page tones"),
            layout: &pass.tone_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(TONE_BYTES),
                }),
            }],
        });
        Self { group, offsets }
    }
}

/// `tone` laid out as the shader's `Tone`.
fn tone_bytes(tone: &PageTone) -> Vec<u8> {
    let mut floats = Vec::with_capacity(TONE_BYTES as usize / 4);
    for m in &tone.matrices {
        floats.extend(m.0.iter().flatten());
    }
    floats.extend(tone.paper);
    floats.push(1.0);
    let mut bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_ne_bytes()).collect();
    let blend = match (tone.blend, tone.ink) {
        (None, _) => 0u32,
        (Some(_), Some(_)) => 4,
        (Some(Blend::Multiply), None) => 1,
        (Some(Blend::Screen), None) => 2,
        (Some(Blend::SoftLight), None) => 3,
    };
    let count = tone.count.min(MAX_FILTER_OPS) as u32;
    let p = &tone.pattern;
    let texture = match p.mode {
        TextureMode::None => 0u32,
        TextureMode::Paper => 1,
        TextureMode::Lined => 2,
        TextureMode::Grid => 3,
        TextureMode::Dotted => 4,
        TextureMode::Cross => 5,
    };
    let grain = if p.grain_blend == Blend::Screen { 2 } else { 1 };
    for word in [count, blend, texture, grain] {
        bytes.extend_from_slice(&word.to_ne_bytes());
    }
    let tail = p.stroke.into_iter().chain(p.grain);
    let [r, g, b] = tone.ink.unwrap_or([0.0; 3]);
    for f in tail.chain([p.opacity, r, g, b]) {
        bytes.extend_from_slice(&f.to_ne_bytes());
    }
    bytes
}

/// Pipelines and shared bindings; one per renderer.
#[derive(Debug)]
pub struct PagePass {
    /// Draws to the screen, and to the glass strip at a fraction of it.
    pub(super) screen: wgpu::RenderPipeline,
    pub(super) strip: wgpu::RenderPipeline,
    /// Highlighter fills under the ink: multiplied, or screened on dark.
    multiply: wgpu::RenderPipeline,
    lighten: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    tone_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// A white texel: flat fills are white tinted.
    white: wgpu::BindGroup,
    pub format: wgpu::TextureFormat,
}

/// Lattice noise, prepended to the shaders that grain.
pub(super) const NOISE: &str = include_str!("noise.wgsl");

/// Raster bytes per pixel; RGBA8 throughout.
const TEXEL: u32 = 4;
/// Per instance: a clip-space rect, tint, pattern, shape and round rect.
const INSTANCE: u64 = 20 * 4;

impl PagePass {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let linear = format!("const LINEAR: bool = {};\n", format.is_srgb());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("page pass"),
            source: wgpu::ShaderSource::Wgsl((linear + NOISE + include_str!("page.wgsl")).into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("page"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let tone_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("page tone"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(TONE_BYTES),
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("page pass"),
            bind_group_layouts: &[Some(&layout), Some(&tone_layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, blend: wgpu::BlendState| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: INSTANCE,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Float32,
                            4 => Float32x3, 5 => Float32x4
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("page"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let white = upload(device, queue, (&layout, &sampler), [1, 1], &[255; 4]);
        Self {
            screen: pipeline("fs", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            strip: pipeline("fs_strip", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            multiply: pipeline(
                "fs",
                under(wgpu::BlendFactor::Dst, wgpu::BlendFactor::OneMinusSrcAlpha),
            ),
            lighten: pipeline(
                "fs",
                under(wgpu::BlendFactor::OneMinusDst, wgpu::BlendFactor::One),
            ),
            layout,
            tone_layout,
            sampler,
            white: white.bind_group,
            format,
        }
    }

    /// Uploads an RGBA8 raster; none if the bytes don't match the size.
    pub fn texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Option<GpuPage> {
        let row = width.checked_mul(TEXEL)?;
        if width == 0 || height == 0 || pixels.len() != (row as usize) * (height as usize) {
            return None;
        }
        let bind = (&self.layout, &self.sampler);
        Some(upload(device, queue, bind, [width, height], pixels))
    }

    /// Draws `quads`, toned by `tones`; `scissor` maps each clip.
    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        (instances, tones): (&wgpu::Buffer, &Tones),
        quads: &[Quad],
        scissor: impl Fn(Rect) -> Option<[u32; 4]>,
    ) {
        // wgpu refuses a slice of an empty buffer.
        if quads.is_empty() {
            return;
        }
        pass.set_vertex_buffer(0, instances.slice(..));
        for ((i, quad), offset) in (0u32..).zip(quads).zip(&tones.offsets) {
            let Some([x, y, w, h]) = scissor(quad.clip) else {
                continue;
            };
            pass.set_scissor_rect(x, y, w, h);
            let group = quad.texture.as_ref().map_or(&self.white, |t| &t.bind_group);
            pass.set_bind_group(0, group, &[]);
            pass.set_bind_group(1, &tones.group, &[*offset]);
            pass.draw(0..4, i..i + 1);
        }
    }
}

/// A premultiplied fill `c·a` blended as `c·a·src + dst·dst_factor`.
fn under(src: wgpu::BlendFactor, dst: wgpu::BlendFactor) -> wgpu::BlendState {
    let keep = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    };
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: src,
            dst_factor: dst,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: keep,
    }
}

/// Uploads checked RGBA8 bytes of `[width, height]`.
fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    (layout, sampler): (&wgpu::BindGroupLayout, &wgpu::Sampler),
    [width, height]: [u32; 2],
    pixels: &[u8],
) -> GpuPage {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("page"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * TEXEL),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("page"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    GpuPage {
        bind_group,
        _texture: texture,
    }
}

/// Instance bytes for `quads` drawn into a target spanning `area`.
pub(super) fn instances(device: &wgpu::Device, quads: &[Quad], area: Rect) -> wgpu::Buffer {
    let size = area.size().max(egui::vec2(1.0, 1.0));
    let x = |v: f32| (v - area.left()) / size.x * 2.0 - 1.0;
    let y = |v: f32| 1.0 - (v - area.top()) / size.y * 2.0;
    let mut bytes = Vec::with_capacity(quads.len() * INSTANCE as usize);
    for q in quads {
        let tint = q.tint.to_array().map(|c| f32::from(c) / 255.0);
        let r = q.rect;
        let floats = [x(r.left()), y(r.top()), x(r.right()), y(r.bottom())];
        let (a, p) = (q.anchor, &q.tone.pattern);
        let lo = sheet(r.min - a.origin, a.zoom * p.scale);
        let size = r.size() / (a.zoom * p.scale).max(1e-3);
        let (x, y) = (lo[0], lo[1]);
        let pattern = [x, y, x + size.x, y + size.y, p.line_width(a.zoom)];
        let shape = [r.width(), r.height(), q.radius];
        // The round rect in the quad's own points.
        let (lo, hi) = (q.round.min - r.min, q.round.max - r.min);
        let round = [lo.x, lo.y, hi.x, hi.y];
        let all = floats.into_iter().chain(tint).chain(pattern);
        let all = all.chain(shape).chain(round);
        for f in all {
            bytes.extend_from_slice(&f.to_ne_bytes());
        }
    }
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("page instances"),
        contents: &bytes,
        usage: wgpu::BufferUsages::VERTEX,
    })
}

/// Units at `offset` from the origin, wrapped where every pattern repeats.
fn sheet(offset: egui::Vec2, points_per_unit: f32) -> [f32; 2] {
    // In f64, so a page a million points down keeps its strokes crisp.
    let k = f64::from(points_per_unit.max(1e-3));
    let wrap = |v: f32| (f64::from(v) / k).rem_euclid(f64::from(PATTERN_PERIOD)) as f32;
    [wrap(offset.x), wrap(offset.y)]
}

/// Scissor in a target of `px` pixels spanning `area`; none if empty.
pub(super) fn scissor(clip: Rect, area: Rect, px: [u32; 2]) -> Option<[u32; 4]> {
    let scale = egui::vec2(px[0] as f32, px[1] as f32) / area.size().max(egui::vec2(1.0, 1.0));
    let r = clip.intersect(area);
    if !r.is_positive() {
        return None;
    }
    let min = ((r.min - area.min) * scale).floor();
    let max = ((r.max - area.min) * scale)
        .ceil()
        .min(egui::vec2(px[0] as f32, px[1] as f32));
    let (x, y) = (min.x.max(0.0) as u32, min.y.max(0.0) as u32);
    let (w, h) = (
        (max.x as u32).saturating_sub(x),
        (max.y as u32).saturating_sub(y),
    );
    (w > 0 && h > 0).then_some([x, y, w, h])
}

/// One pane's pages, drawn into egui's pass.
pub struct PagesCallback {
    pass: Arc<PagePass>,
    quads: Vec<Quad>,
    /// Set for highlighter fills: how they meet the page.
    under: Option<Blend>,
    instances: OnceLock<(wgpu::Buffer, Tones, Rect, [u32; 2])>,
}

impl PagesCallback {
    pub fn shape(pass: Arc<PagePass>, clip: Rect, quads: Vec<Quad>) -> egui::Shape {
        Self::with(pass, clip, quads, None)
    }

    /// Highlighter fills, `blend`ed under the ink already drawn.
    pub fn marks(pass: Arc<PagePass>, clip: Rect, quads: Vec<Quad>, blend: Blend) -> egui::Shape {
        Self::with(pass, clip, quads, Some(blend))
    }

    fn with(
        pass: Arc<PagePass>,
        clip: Rect,
        quads: Vec<Quad>,
        under: Option<Blend>,
    ) -> egui::Shape {
        let callback = Self {
            pass,
            quads,
            under,
            instances: OnceLock::new(),
        };
        egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(clip, callback))
    }
}

impl egui_wgpu::CallbackTrait for PagesCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        _resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let px = screen.size_in_pixels;
        let ppp = screen.pixels_per_point;
        let area = Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(px[0] as f32, px[1] as f32) / ppp,
        );
        let buffer = instances(device, &self.quads, area);
        let tones = Tones::new(device, &self.pass, &self.quads);
        let _ = self.instances.set((buffer, tones, area, px));
        Vec::new()
    }

    fn paint(
        &self,
        info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        _resources: &CallbackResources,
    ) {
        let Some((buffer, tones, area, px)) = self.instances.get() else {
            return;
        };
        // egui's clamped viewport squashes pages hanging off screen.
        pass.set_viewport(0.0, 0.0, px[0] as f32, px[1] as f32, 0.0, 1.0);
        let pipeline = match self.under {
            None => &self.pass.screen,
            Some(Blend::Screen) => &self.pass.lighten,
            Some(_) => &self.pass.multiply,
        };
        pass.set_pipeline(pipeline);
        let clip = info.clip_rect;
        self.pass.draw(pass, (buffer, tones), &self.quads, |q| {
            scissor(q.intersect(clip), *area, *px)
        });
    }
}
