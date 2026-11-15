//! The bars' glass: the strip behind each, redrawn small and blurred.

use std::sync::Arc;

use eframe::egui_wgpu::{self, CallbackResources, ScreenDescriptor, wgpu};
use egui::{Color32, Rect, pos2};

use super::page_pass::{self, PagePass, Quad};

/// Blur sigma in points and saturation; 20 points smeared text to grey.
pub const BLUR: f32 = 12.0;
pub const SATURATE: f32 = 1.8;
/// Sigma in strip texels; the strip shrinks to keep it there.
const SIGMA_TEXELS: f32 = 4.0;
/// The coarsest shrink, for very large scale factors.
const MAX_SHRINK: f32 = 16.0;
/// Glass panes per frame: the title bar and a page bar per pane.
pub const MAX_SLOTS: usize = 17;

/// Which side of a bar its content scrolls in from, to blur early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reach {
    /// The title bar: the strip runs on below it.
    Below,
    /// A page bar along a pane's foot: the strip runs on above it.
    Above,
}

/// One frame's backdrop for the bar: pane fills, then pages.
#[derive(Clone, Debug)]
pub struct Glass {
    pub pass: Arc<PagePass>,
    /// Shows between panes; the window clears to it.
    pub canvas: Color32,
    pub quads: Vec<Quad>,
}

impl Glass {
    /// The glass behind `bar`; each one in a frame needs its own slot.
    pub fn shape(&self, bar: Rect, screen: Rect, look: Look) -> egui::Shape {
        // Reaches past the bar so content blurs in before it arrives.
        let strip = match look.reach {
            Reach::Below => {
                let bottom = (bar.bottom() + 3.0 * BLUR).min(screen.bottom());
                Rect::from_min_max(bar.min, pos2(bar.right(), bottom.max(bar.bottom())))
            }
            Reach::Above => {
                let top = (bar.top() - 3.0 * BLUR).max(screen.top());
                Rect::from_min_max(pos2(bar.left(), top.min(bar.top())), bar.max)
            }
        };
        let quads = self
            .quads
            .iter()
            .filter(|q| q.rect.intersects(strip) && q.clip.intersects(strip))
            .cloned()
            .collect();
        let callback = GlassCallback {
            pass: Arc::clone(&self.pass),
            canvas: self.canvas,
            quads,
            bar,
            strip,
            look,
        };
        egui::Shape::Callback(egui_wgpu::Callback::new_paint_callback(bar, callback))
    }
}

/// How one glass is drawn: its slot, its strip's side and its fade.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub slot: usize,
    pub reach: Reach,
    pub opacity: f32,
}

struct GlassCallback {
    pass: Arc<PagePass>,
    canvas: Color32,
    quads: Vec<Quad>,
    bar: Rect,
    strip: Rect,
    look: Look,
}

/// Pipelines, and per slot a strip's targets, kept across frames.
struct Kit {
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    slots: Vec<Slot>,
}

/// One glass's uniforms and strip; every glass prepares before any paints.
struct Slot {
    /// Horizontal blur, vertical blur, composite.
    params: [wgpu::Buffer; 3],
    targets: Option<Targets>,
}

/// The strip ping-pongs between `a` and `b`; the result ends in `a`.
struct Targets {
    size: [u32; 2],
    a: wgpu::TextureView,
    b: wgpu::TextureView,
    /// Bindings matching `Kit::params`.
    groups: [wgpu::BindGroup; 3],
}

impl Kit {
    fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glass"),
            source: wgpu::ShaderSource::Wgsl(include_str!("glass.wgsl").into()),
        });
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: fragment,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: fragment,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: fragment,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        Self {
            blur: pipeline("fs_blur", None),
            composite: pipeline(
                "fs_composite",
                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
            ),
            layout,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("glass"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            slots: Vec::new(),
        }
    }
}

impl Slot {
    /// Slot `i` of `slots`, made on first use; none past the cap.
    fn get<'s>(slots: &'s mut Vec<Slot>, device: &wgpu::Device, i: usize) -> Option<&'s mut Slot> {
        if i >= MAX_SLOTS {
            return None;
        }
        while slots.len() <= i {
            let param = || {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("glass params"),
                    size: PARAMS,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            };
            slots.push(Slot {
                params: [param(), param(), param()],
                targets: None,
            });
        }
        slots.get_mut(i)
    }

    /// Targets of `size`, remade only when the strip resizes.
    fn targets(&mut self, device: &wgpu::Device, size: [u32; 2], kit: Shared<'_>) {
        if self.targets.as_ref().is_some_and(|t| t.size == size) {
            return;
        }
        let format = kit.format;
        let view = || {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("glass strip"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let (a, b) = (view(), view());
        let [h, v, c] = &self.params;
        let group = |src: &wgpu::TextureView, params: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("glass"),
                layout: kit.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(kit.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params.as_entire_binding(),
                    },
                ],
            })
        };
        let groups = [group(&a, h), group(&b, v), group(&a, c)];
        self.targets = Some(Targets { size, a, b, groups });
    }
}

/// What a slot's bind groups borrow from the kit.
#[derive(Clone, Copy)]
struct Shared<'a> {
    layout: &'a wgpu::BindGroupLayout,
    sampler: &'a wgpu::Sampler,
    format: wgpu::TextureFormat,
}

/// The shader's `Params`: three vec2s and two floats, padded to 32 bytes.
const PARAMS: u64 = 32;

fn params(v: [f32; 2], a: f32, b: f32, o: [f32; 2]) -> Vec<u8> {
    [v[0], v[1], a, b, o[0], o[1], 0.0, 0.0]
        .iter()
        .flat_map(|f| f.to_ne_bytes())
        .collect()
}

/// A colour as the target stores it: sRGB targets want linear.
fn clear(colour: Color32, format: wgpu::TextureFormat) -> wgpu::Color {
    let [r, g, b, a] = if format.is_srgb() {
        egui::Rgba::from(colour).to_array()
    } else {
        colour.to_array().map(|c| f32::from(c) / 255.0)
    };
    wgpu::Color {
        r: r.into(),
        g: g.into(),
        b: b.into(),
        a: a.into(),
    }
}

fn pass<'e>(
    encoder: &'e mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPass<'e> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("glass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

impl egui_wgpu::CallbackTrait for GlassCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        screen: &ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        let format = self.pass.format;
        let ppp = screen.pixels_per_point;
        let shrink = (BLUR * ppp / SIGMA_TEXELS).round().clamp(1.0, MAX_SHRINK);
        let texels = |points: f32| ((points * ppp / shrink).ceil() as u32).max(1);
        let size = [texels(self.strip.width()), texels(self.strip.height())];
        if resources.get::<Kit>().is_none() {
            resources.insert(Kit::new(device, format));
        }
        let Some(kit) = resources.get_mut::<Kit>() else {
            return Vec::new();
        };
        let Kit {
            blur,
            layout,
            sampler,
            slots,
            ..
        } = kit;
        let shared = Shared {
            layout,
            sampler,
            format,
        };
        let Some(slot) = Slot::get(slots, device, self.look.slot) else {
            return Vec::new();
        };
        slot.targets(device, size, shared);
        let Some(targets) = &slot.targets else {
            return Vec::new();
        };
        let sigma = BLUR * ppp / shrink;
        let [w, h] = size.map(|s| s as f32);
        // The bar's share of the strip, at its top or its foot.
        let share = self.bar.height() / self.strip.height().max(1.0);
        let start = match self.look.reach {
            Reach::Below => 0.0,
            Reach::Above => 1.0 - share,
        };
        let [ph, pv, pc] = &slot.params;
        queue.write_buffer(ph, 0, &params([1.0 / w, 0.0], sigma, 0.0, [0.0; 2]));
        queue.write_buffer(pv, 0, &params([0.0, 1.0 / h], sigma, 0.0, [0.0; 2]));
        let fade = self.look.opacity;
        queue.write_buffer(pc, 0, &params([1.0, share], SATURATE, fade, [0.0, start]));

        let instances = page_pass::instances(device, &self.quads, self.strip);
        let tones = page_pass::Tones::new(device, &self.pass, &self.quads);
        {
            let load = wgpu::LoadOp::Clear(clear(self.canvas, format));
            let mut rp = pass(encoder, &targets.a, load);
            rp.set_pipeline(&self.pass.strip);
            let strip = self.strip;
            self.pass
                .draw(&mut rp, (&instances, &tones), &self.quads, |clip| {
                    page_pass::scissor(clip, strip, size)
                });
        }
        let [gh, gv, _] = &targets.groups;
        for (target, group) in [(&targets.b, gh), (&targets.a, gv)] {
            let mut rp = pass(encoder, target, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            rp.set_pipeline(blur);
            rp.set_bind_group(0, group, &[]);
            rp.draw(0..3, 0..1);
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &CallbackResources,
    ) {
        let Some(kit) = resources.get::<Kit>() else {
            return;
        };
        let slot = kit.slots.get(self.look.slot);
        let Some(targets) = slot.and_then(|s| s.targets.as_ref()) else {
            return;
        };
        let [_, _, composite] = &targets.groups;
        pass.set_pipeline(&kit.composite);
        pass.set_bind_group(0, composite, &[]);
        pass.draw(0..3, 0..1);
    }
}
