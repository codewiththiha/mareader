//! Reusable GPU instance and tone buffers shared across frames.

use eframe::egui_wgpu::wgpu;
use egui::Rect;
use mareader_appearance::{Blend, MAX_FILTER_OPS, PATTERN_PERIOD, PageTone, TextureMode};

use super::page_pass::Quad;

/// Bytes of one tone in the shader's `Tone` struct.
pub(super) const TONE_BYTES: u64 = (MAX_FILTER_OPS as u64 * 3 + 5) * 16;
/// Per instance: a clip-space rect, tint, pattern, shape and round rect.
pub(super) const INSTANCE: u64 = 20 * 4;

/// Grow-only vertex and uniform buffers reused across frames.
#[derive(Default)]
pub(super) struct DrawBuffers {
    instances_cpu: Vec<u8>,
    instances_gpu: Option<(wgpu::Buffer, u64)>,
    tones_cpu: Vec<u8>,
    tones_gpu: Option<(wgpu::Buffer, wgpu::BindGroup, u64)>,
    unique: Vec<PageTone>,
}

impl DrawBuffers {
    pub(super) fn clear(&mut self) {
        self.instances_cpu.clear();
        self.tones_cpu.clear();
        self.unique.clear();
    }

    /// Appends `quads` into CPU staging, returning the first instance index.
    pub(super) fn stage(
        &mut self,
        device: &wgpu::Device,
        quads: &[Quad],
        area: Rect,
        offsets: &mut Vec<u32>,
    ) -> u32 {
        offsets.clear();
        if quads.is_empty() {
            return 0;
        }
        let first = (self.instances_cpu.len() / INSTANCE as usize) as u32;
        append_instances(quads, area, &mut self.instances_cpu);
        let align = u64::from(device.limits().min_uniform_buffer_offset_alignment);
        let stride = (TONE_BYTES.div_ceil(align) * align) as usize;
        offsets.reserve(quads.len());
        for q in quads {
            let i = self
                .unique
                .iter()
                .position(|t| *t == q.tone)
                .unwrap_or_else(|| {
                    let idx = self.unique.len();
                    self.unique.push(q.tone);
                    let start = self.tones_cpu.len();
                    self.tones_cpu.resize(start + stride, 0);
                    if let Some(slot) = self.tones_cpu.get_mut(start..start + stride) {
                        write_tone(&q.tone, slot);
                    }
                    idx
                });
            offsets.push((i * stride) as u32);
        }
        first
    }

    /// Uploads all staged bytes once, growing GPU buffers only when needed.
    pub(super) fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        tone_layout: &wgpu::BindGroupLayout,
    ) {
        if !self.instances_cpu.is_empty() {
            let need = self.instances_cpu.len() as u64;
            let grow = self
                .instances_gpu
                .as_ref()
                .is_none_or(|(_, cap)| *cap < need);
            if grow {
                let cap = self
                    .instances_gpu
                    .as_ref()
                    .map_or(0, |(_, c)| c.saturating_mul(2))
                    .max(need)
                    .max(INSTANCE);
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("page instances"),
                    size: cap,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.instances_gpu = Some((buffer, cap));
            }
            if let Some((buf, _)) = &self.instances_gpu {
                queue.write_buffer(buf, 0, &self.instances_cpu);
            }
        }
        if !self.tones_cpu.is_empty() {
            let need = self.tones_cpu.len() as u64;
            let grow = self
                .tones_gpu
                .as_ref()
                .is_none_or(|(_, _, cap)| *cap < need);
            if grow {
                let cap = self
                    .tones_gpu
                    .as_ref()
                    .map_or(0, |(_, _, c)| c.saturating_mul(2))
                    .max(need)
                    .max(TONE_BYTES);
                let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("page tones"),
                    size: cap,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("page tones"),
                    layout: tone_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &buffer,
                            offset: 0,
                            size: wgpu::BufferSize::new(TONE_BYTES),
                        }),
                    }],
                });
                self.tones_gpu = Some((buffer, group, cap));
            }
            if let Some((buf, _, _)) = &self.tones_gpu {
                queue.write_buffer(buf, 0, &self.tones_cpu);
            }
        }
    }

    pub(super) fn gpu(&self) -> Option<(&wgpu::Buffer, &wgpu::BindGroup)> {
        let (instances, _) = self.instances_gpu.as_ref()?;
        let (_, tones, _) = self.tones_gpu.as_ref()?;
        Some((instances, tones))
    }
}

/// Shared staging pool in `CallbackResources` for every `PagesCallback`.
pub(super) struct FrameStaging {
    buffers: DrawBuffers,
    uploaded: bool,
}

impl Default for FrameStaging {
    fn default() -> Self {
        Self {
            buffers: DrawBuffers::default(),
            uploaded: true,
        }
    }
}

impl FrameStaging {
    pub(super) fn stage(
        &mut self,
        device: &wgpu::Device,
        quads: &[Quad],
        area: Rect,
    ) -> (u32, Vec<u32>) {
        if self.uploaded {
            self.buffers.clear();
            self.uploaded = false;
        }
        let mut offsets = Vec::new();
        let first = self.buffers.stage(device, quads, area, &mut offsets);
        (first, offsets)
    }

    pub(super) fn upload_once(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        tone_layout: &wgpu::BindGroupLayout,
    ) {
        if !self.uploaded {
            self.uploaded = true;
            self.buffers.upload(device, queue, tone_layout);
        }
    }

    pub(super) fn gpu(&self) -> Option<(&wgpu::Buffer, &wgpu::BindGroup)> {
        self.buffers.gpu()
    }
}

/// Instance bytes for `quads` drawn into a target spanning `area`.
fn append_instances(quads: &[Quad], area: Rect, bytes: &mut Vec<u8>) {
    let size = area.size().max(egui::vec2(1.0, 1.0));
    let x = |v: f32| (v - area.left()) / size.x * 2.0 - 1.0;
    let y = |v: f32| 1.0 - (v - area.top()) / size.y * 2.0;
    bytes.reserve(quads.len() * INSTANCE as usize);
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
        for f in all.chain(shape).chain(round) {
            bytes.extend_from_slice(&f.to_ne_bytes());
        }
    }
}

/// Units at `offset` from the origin, wrapped where every pattern repeats.
fn sheet(offset: egui::Vec2, points_per_unit: f32) -> [f32; 2] {
    // In f64, so a page a million points down keeps its strokes crisp.
    let k = f64::from(points_per_unit.max(1e-3));
    let wrap = |v: f32| (f64::from(v) / k).rem_euclid(f64::from(PATTERN_PERIOD)) as f32;
    [wrap(offset.x), wrap(offset.y)]
}

/// Writes `tone` as the shader's `Tone` struct straight into `out`.
fn write_tone(tone: &PageTone, out: &mut [u8]) {
    let mut cursor = 0usize;
    let mut put = |bytes: [u8; 4]| {
        if let Some(dst) = out.get_mut(cursor..cursor + 4) {
            dst.copy_from_slice(&bytes);
            cursor += 4;
        }
    };
    for m in &tone.matrices {
        for row in &m.0 {
            for &f in row {
                put(f.to_ne_bytes());
            }
        }
    }
    for f in tone.paper {
        put(f.to_ne_bytes());
    }
    put(1.0f32.to_ne_bytes());
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
        put(word.to_ne_bytes());
    }
    let [r, g, b] = tone.ink.unwrap_or([0.0; 3]);
    for f in p
        .stroke
        .into_iter()
        .chain(p.grain)
        .chain([p.opacity, r, g, b])
    {
        put(f.to_ne_bytes());
    }
}
