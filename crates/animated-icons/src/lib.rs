//! Generated resting glyphs and bounded part atlases for Lucide icons.

use tiny_skia::{Pixmap, Transform};

pub const GRID: f32 = 24.0;
pub const PAD: f32 = 8.5;
pub const SPAN: f32 = GRID + 2.0 * PAD;
pub const FRAME_RATE: f32 = 60.0;
pub const MAX_SIDE: u32 = 192;
pub const MAX_ATLAS_BYTES: usize = 8 * 1024 * 1024;

mod atlas;
mod raster;
mod spring;
mod timeline;

use atlas::{crop, raster_part_atlas};
use raster::render_nodes;
use timeline::values;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Prop {
    X,
    Y,
    Scale,
    ScaleX,
    ScaleY,
    Rotate,
    SkewX,
    SkewY,
    Opacity,
    PathLen,
    PathOff,
    StrokeW,
}

struct Seg {
    part: u8,
    prop: Prop,
    t0: f32,
    dur: f32,
    keys: &'static [f32],
    times: &'static [f32],
    eases: &'static [u8],
}

struct Part {
    ox: f32,
    oy: f32,
    len: f32,
    width: f32,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum Command {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Cubic(f32, f32, f32, f32, f32, f32),
    Close,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum FillRule {
    Winding,
    EvenOdd,
}

#[derive(Clone, Copy)]
struct Fill {
    opacity: f32,
    rule: FillRule,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum LineCap {
    Butt,
    Round,
    Square,
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Clone, Copy)]
struct Stroke {
    opacity: f32,
    width: f32,
    cap: LineCap,
    join: LineJoin,
    miter: f32,
}

struct Shape {
    commands: &'static [Command],
    fill: Option<Fill>,
    stroke: Option<Stroke>,
}

enum Node {
    Group {
        part: Option<u8>,
        width_part: Option<u8>,
        opacity: f32,
        children: &'static [Node],
    },
    Shape(&'static Shape),
}

struct Layer {
    parts: &'static [u8],
    scene: &'static [Node],
}

pub struct Pack {
    id: &'static str,
    dur: f32,
    parts: &'static [Part],
    segs: &'static [Seg],
    layers: &'static [Layer],
}

impl Pack {
    pub fn id(&self) -> &'static str {
        self.id
    }

    pub fn duration(&self) -> f32 {
        self.dur
    }

    pub fn frame_count(&self) -> u16 {
        (self.dur * FRAME_RATE)
            .ceil()
            .clamp(1.0, f32::from(u16::MAX)) as u16
    }
}

#[allow(clippy::approx_constant, clippy::excessive_precision)]
mod generated {
    use super::*;

    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

use generated::PACKS;

pub fn find(id: &str) -> Option<&'static Pack> {
    PACKS.iter().copied().find(|pack| pack.id == id)
}

pub fn all() -> impl ExactSizeIterator<Item = &'static Pack> {
    PACKS.iter().copied()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtlasKind {
    Parts,
}

#[derive(Clone, Copy, Debug)]
pub struct AtlasCell {
    pub atlas_x: u32,
    pub atlas_y: u32,
    pub target_x: u32,
    pub target_y: u32,
    pub width: u32,
    pub height: u32,
}

pub struct Atlas {
    pub kind: AtlasKind,
    pub width: u32,
    pub height: u32,
    pub side: u32,
    pub frames: u16,
    pub layers: u16,
    pub cells: Vec<AtlasCell>,
    pub pixels: Vec<u8>,
}

impl Atlas {
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }

    pub fn cell(&self, frame: u16, layer: u16) -> Option<AtlasCell> {
        let frame = frame.min(self.frames.saturating_sub(1));
        let layer = layer.min(self.layers.saturating_sub(1));
        let index = usize::from(frame)
            .checked_mul(usize::from(self.layers))?
            .checked_add(usize::from(layer))?;
        self.cells.get(index).copied()
    }

    pub fn uv(&self, cell: AtlasCell) -> [f32; 4] {
        [
            cell.atlas_x as f32 / self.width.max(1) as f32,
            cell.atlas_y as f32 / self.height.max(1) as f32,
            cell.atlas_x.saturating_add(cell.width) as f32 / self.width.max(1) as f32,
            cell.atlas_y.saturating_add(cell.height) as f32 / self.height.max(1) as f32,
        ]
    }
}

/// One composited resting glyph cropped from its 24-unit canvas.
pub struct RestingGlyph {
    pub side: u32,
    pub target_x: u32,
    pub target_y: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RestingGlyph {
    pub fn byte_len(&self) -> usize {
        self.pixels.len()
    }
}

/// Rasterises only the canonical first sample, never the animation atlas.
pub fn raster_resting(pack: &'static Pack, requested_side: u32) -> Option<RestingGlyph> {
    let side = requested_side.clamp(8, MAX_SIDE);
    let vals = values(pack, 0.0);
    let mut pixmap = Pixmap::new(side, side)?;
    let scale = side as f32 / GRID;
    let canvas = Transform::from_row(scale, 0.0, 0.0, scale, 0.0, 0.0);
    for layer in pack.layers {
        render_nodes(
            layer.scene,
            pack.parts,
            &vals,
            None,
            None,
            canvas,
            &mut pixmap,
        )?;
    }
    let (target_x, target_y, width, height, pixels) = crop(&pixmap)?;
    Some(RestingGlyph {
        side,
        target_x,
        target_y,
        width,
        height,
        pixels,
    })
}

pub fn raster_atlas(pack: &'static Pack, requested_side: u32) -> Option<Atlas> {
    raster_part_atlas(pack, requested_side.clamp(8, MAX_SIDE))
}

#[cfg(test)]
fn raster_frame(pack: &'static Pack, frame: u16, requested_side: u32) -> Option<Vec<u8>> {
    let side = requested_side.clamp(8, MAX_SIDE);
    let time = f32::from(frame.min(pack.frame_count().saturating_sub(1))) / FRAME_RATE;
    let vals = values(pack, time);
    let mut pixmap = Pixmap::new(side, side)?;
    let scale = side as f32 / SPAN;
    let canvas = Transform::from_row(scale, 0.0, 0.0, scale, PAD * scale, PAD * scale);
    for layer in pack.layers {
        render_nodes(
            layer.scene,
            pack.parts,
            &vals,
            None,
            None,
            canvas,
            &mut pixmap,
        )?;
    }
    Some(pixmap.take())
}

#[cfg(test)]
mod parity;
