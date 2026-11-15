//! The replacement renderer is held against the SVG path it replaces.

#![allow(dead_code)]

use resvg::{tiny_skia, usvg};

const HOLE: char = '\u{1}';
const WIDTH_HOLE: char = '\u{2}';
const STROKE: f32 = 2.0;
const GRID: f32 = 24.0;
const PAD: f32 = 8.5;
const FRAME_RATE: f32 = 60.0;

#[path = "source_model.rs"]
mod source_model;

use source_model::{Hole, Pack, Part, Prop, Seg};

#[allow(clippy::approx_constant, clippy::excessive_precision)]
#[path = "source.rs"]
mod source;

fn frame_svg(pack: &Pack, t: f32, pad: f32) -> String {
    let mut vals: Vec<Vals> = pack.parts.iter().map(Vals::rest).collect();
    for seg in pack.segs {
        let Some(v) = vals.get_mut(seg.part as usize) else {
            continue;
        };
        let x = at(seg, clock(seg, t));
        match seg.prop {
            Prop::X => v.x = x,
            Prop::Y => v.y = x,
            Prop::Scale => v.scale = x,
            Prop::ScaleX => v.scale_x = x,
            Prop::ScaleY => v.scale_y = x,
            Prop::Rotate => v.rot = x,
            Prop::SkewX => v.skew_x = x,
            Prop::SkewY => v.skew_y = x,
            Prop::Opacity => v.op = x,
            Prop::PathLen => v.len = x,
            Prop::PathOff => v.off = x,
            Prop::StrokeW => v.w = x,
        }
    }
    let mut body = String::with_capacity(pack.svg.len() + 32);
    let mut next = 0usize;
    for c in pack.svg.chars() {
        if c != HOLE && c != WIDTH_HOLE {
            body.push(c);
            continue;
        }
        let Some(hole) = pack.holes.get(next) else {
            continue;
        };
        next += 1;
        let (Some(part), Some(v)) = (
            pack.parts.get(hole.part as usize),
            vals.get(hole.part as usize),
        ) else {
            continue;
        };
        let bit = if hole.width { width(v) } else { moves(part, v) };
        body.push_str(&bit);
    }
    // The padded viewport preserves authored overshoot.
    let left = num(-pad);
    let span = num(GRID + 2.0 * pad);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{span}" height="{span}" viewBox="{left} {left} {span} {span}" fill="none" stroke="white" stroke-width="{STROKE}" stroke-linecap="round" stroke-linejoin="round">{body}</svg>"#
    )
}

/// What one part of a glyph is doing at this instant.
#[derive(Clone, Copy)]
struct Vals {
    x: f32,
    y: f32,
    scale: f32,
    scale_x: f32,
    scale_y: f32,
    rot: f32,
    skew_x: f32,
    skew_y: f32,
    op: f32,
    len: f32,
    off: f32,
    w: f32,
}

impl Vals {
    /// The glyph as drawn: nothing moved, nothing hidden, its own stroke.
    fn rest(part: &Part) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rot: 0.0,
            skew_x: 0.0,
            skew_y: 0.0,
            op: 1.0,
            len: 1.0,
            off: 0.0,
            w: part.w,
        }
    }

    fn moved(&self) -> bool {
        self.x != 0.0
            || self.y != 0.0
            || self.scale != 1.0
            || self.scale_x != 1.0
            || self.scale_y != 1.0
            || self.rot != 0.0
            || self.skew_x != 0.0
            || self.skew_y != 0.0
    }
}

/// Uses the browser's fill-box pivot.
fn moves(part: &Part, v: &Vals) -> String {
    let mut s = String::new();
    if v.moved() {
        let mut bits = vec![
            format!("translate({} {})", num(part.ox + v.x), num(part.oy + v.y)),
            format!(
                "scale({} {})",
                num(v.scale * v.scale_x),
                num(v.scale * v.scale_y)
            ),
        ];
        if v.rot != 0.0 {
            bits.push(format!("rotate({})", num(v.rot)));
        }
        if v.skew_x != 0.0 {
            bits.push(format!("skewX({})", num(v.skew_x)));
        }
        if v.skew_y != 0.0 {
            bits.push(format!("skewY({})", num(v.skew_y)));
        }
        bits.push(format!("translate({} {})", num(-part.ox), num(-part.oy)));
        s.push_str(&format!(r#" transform="{}""#, bits.join(" ")));
    }
    // Dash values are shares of the measured path length.
    if ((v.len - 1.0).abs() > 1e-4 || v.off != 0.0) && part.len > 0.0 {
        let p = v.len.clamp(0.0, 1.0);
        let o = v.off.clamp(0.0, 1.0);
        if p <= 0.0 {
            s.push_str(r#" stroke="none""#);
        } else {
            s.push_str(&format!(
                r#" stroke-dasharray="{} {}" stroke-dashoffset="{}""#,
                num(p * part.len),
                num((1.0 - p) * part.len),
                num((1.0 - o) * part.len)
            ));
        }
    }
    if (v.op - 1.0).abs() > 1e-4 {
        s.push_str(&format!(r#" opacity="{}""#, num(v.op.clamp(0.0, 1.0))));
    }
    s
}

/// Writes an animated element-owned stroke width.
fn width(v: &Vals) -> String {
    format!(r#" stroke-width="{}""#, num(v.w.max(0.0)))
}

/// How far through a segment `t` is: held back before it starts, spent after.
fn clock(seg: &Seg, t: f32) -> f32 {
    if seg.dur <= 0.0 {
        return if t >= seg.t0 { 1.0 } else { 0.0 };
    }
    ((t - seg.t0) / seg.dur).clamp(0.0, 1.0)
}

/// The value a segment holds at share `u` of itself, keyframe by keyframe.
fn at(seg: &Seg, u: f32) -> f32 {
    let n = seg.keys.len();
    match n {
        0 => 0.0,
        1 => seg.keys[0],
        _ => {
            let gaps = n - 1;
            let (i, f) = if seg.times.len() == n {
                let mut i = 0usize;
                while i + 2 < n && seg.times[i + 1] <= u {
                    i += 1;
                }
                let span = (seg.times[i + 1] - seg.times[i]).max(1e-6);
                (i, ((u - seg.times[i]) / span).clamp(0.0, 1.0))
            } else {
                let q = (u * gaps as f32).clamp(0.0, gaps as f32);
                let i = (q as usize).min(gaps - 1);
                (i, (q - i as f32).clamp(0.0, 1.0))
            };
            let e = seg
                .eases
                .get(i.min(seg.eases.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0);
            let eased = match source::EASINGS.get(e as usize).copied() {
                Some(source::Ease::Bezier(points)) => curve(points, f),
                Some(source::Ease::Spring(zeta)) => crate::spring::evaluate(zeta, f),
                _ => f,
            };
            let (a, b) = (seg.keys[i], seg.keys[i + 1]);
            a + (b - a) * eased
        }
    }
}

/// The cubic Bézier height at progress `x`.
fn curve([x1, y1, x2, y2]: [f32; 4], x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let axis = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        if axis(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    axis(y1, y2, (lo + hi) * 0.5)
}

/// A number as markup wants it: three decimals, no trailing zeros, no exponent.
fn num(v: f32) -> String {
    let r = (v * 1000.0).round() / 1000.0;
    if r == 0.0 {
        "0".to_owned()
    } else if r.fract() == 0.0 {
        format!("{}", r as i32)
    } else {
        r.to_string()
    }
}

fn original_frame(pack: &Pack, frame: u16, side: u32) -> Option<Vec<u8>> {
    original(pack, f32::from(frame) / FRAME_RATE, PAD, side)
}

fn original(pack: &Pack, time: f32, pad: f32, side: u32) -> Option<Vec<u8>> {
    let svg = frame_svg(pack, time, pad);
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(side, side)?;
    let scale = side as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(pixmap.take())
}

fn tinted_close(left: &[u8], right: &[u8], tint: [u8; 4], max_delta: u8) -> bool {
    left.as_chunks::<4>()
        .0
        .iter()
        .zip(right.as_chunks::<4>().0.iter())
        .all(|(left, right)| {
            (0..4).all(|channel| {
                multiply(left[channel], tint[channel])
                    .abs_diff(multiply(right[channel], tint[channel]))
                    <= max_delta
            })
        })
}

fn multiply(a: u8, b: u8) -> u8 {
    ((u16::from(a) * u16::from(b) + 127) / 255) as u8
}

fn assert_pixels(left: &[u8], right: &[u8], label: &str, max_delta: u8) {
    let first = left
        .iter()
        .zip(right)
        .position(|(left, right)| left.abs_diff(*right) > max_delta);
    if first.is_none() && left.len() == right.len() {
        return;
    }
    let changed = left
        .as_chunks::<4>()
        .0
        .iter()
        .zip(right.as_chunks::<4>().0.iter())
        .filter(|(left, right)| {
            left.iter()
                .zip(right.iter())
                .any(|(left, right)| left.abs_diff(*right) > max_delta)
        })
        .count();
    let peak = left
        .iter()
        .zip(right)
        .map(|(left, right)| left.abs_diff(*right))
        .max()
        .unwrap_or_default();
    let first = first.unwrap_or(left.len().min(right.len()));
    panic!(
        "{label}: {changed} pixels exceed delta {max_delta}; peak is {peak}, first byte is {first}"
    );
}

#[test]
fn generated_geometry_and_atlases_match_every_original_svg_frame() {
    // UI DPI buckets followed by the detail ceiling.
    const SIDES: [u32; 4] = [41, 51, 82, 192];
    const SOURCE_DELTA: u8 = 64;
    const ATLAS_DELTA: u8 = 1;
    const TINTS: [[u8; 4]; 3] = [
        [255, 255, 255, 255],
        [37, 173, 241, 255],
        [230, 91, 44, 153],
    ];

    assert_eq!(source::ALL.len(), super::PACKS.len());
    for reference in source::ALL {
        let Some(pack) = super::find(reference.id) else {
            panic!("{}: generated pack is missing", reference.id);
        };
        assert_eq!(
            pack.frame_count(),
            (reference.dur * FRAME_RATE).ceil() as u16
        );
        for side in SIDES {
            let atlas = super::raster_atlas(pack, side)
                .unwrap_or_else(|| panic!("{}: atlas allocation failed at {side}px", reference.id));
            assert_atlas(&atlas, pack);
            assert_eq!(atlas.kind, super::AtlasKind::Parts);
            for frame in 0..pack.frame_count() {
                let original = original_frame(reference, frame, side).unwrap_or_else(|| {
                    panic!(
                        "{}: SVG frame {frame} at {side}px did not render",
                        reference.id
                    )
                });
                let generated = super::raster_frame(pack, frame, side).unwrap_or_else(|| {
                    panic!(
                        "{}: typed frame {frame} at {side}px did not render",
                        reference.id
                    )
                });
                let label = format!("{}: frame {frame} at {side}px", reference.id);
                // Part opacity and matrices may round isolated edges.
                assert_pixels(&generated, &original, &label, SOURCE_DELTA);
                let assembled = compose_frame(&atlas, frame).unwrap_or_else(|| {
                    panic!(
                        "{}: atlas frame {frame} at {side}px did not compose",
                        reference.id
                    )
                });
                // Layer compositing may round by one 8-bit step.
                assert_pixels(
                    &assembled,
                    &generated,
                    &format!("{label} atlas"),
                    ATLAS_DELTA,
                );
                for tint in TINTS {
                    assert!(
                        tinted_close(&assembled, &original, tint, SOURCE_DELTA + ATLAS_DELTA,),
                        "{label} differs with tint {tint:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn every_resting_glyph_matches_its_first_source_sample() {
    const SIDES: [u32; 4] = [24, 30, 48, 192];
    const SOURCE_DELTA: u8 = 48;

    assert_eq!(source::ALL.len(), super::PACKS.len());
    for reference in source::ALL {
        let Some(pack) = super::find(reference.id) else {
            panic!("{}: generated pack is missing", reference.id);
        };
        for side in SIDES {
            let glyph = super::raster_resting(pack, side).unwrap_or_else(|| {
                panic!(
                    "{}: resting glyph allocation failed at {side}px",
                    reference.id
                )
            });
            assert!(glyph.target_x + glyph.width <= glyph.side);
            assert!(glyph.target_y + glyph.height <= glyph.side);
            assert_eq!(
                glyph.byte_len(),
                glyph.width as usize * glyph.height as usize * 4
            );
            let generated = compose_resting(&glyph).unwrap_or_else(|| {
                panic!(
                    "{}: resting glyph did not compose at {side}px",
                    reference.id
                )
            });
            let original = original(reference, 0.0, 0.0, side).unwrap_or_else(|| {
                panic!("{}: resting SVG did not render at {side}px", reference.id)
            });
            assert_pixels(
                &generated,
                &original,
                &format!("{}: resting glyph at {side}px", reference.id),
                SOURCE_DELTA,
            );
        }
    }
}

fn compose_resting(glyph: &super::RestingGlyph) -> Option<Vec<u8>> {
    let mut output = tiny_skia::Pixmap::new(glyph.side, glyph.side)?;
    let size = tiny_skia::IntSize::from_wh(glyph.width, glyph.height)?;
    let pixmap = tiny_skia::Pixmap::from_vec(glyph.pixels.clone(), size)?;
    output.draw_pixmap(
        glyph.target_x as i32,
        glyph.target_y as i32,
        pixmap.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    Some(output.take())
}

fn assert_atlas(atlas: &super::Atlas, pack: &super::Pack) {
    assert!(atlas.byte_len() <= super::MAX_ATLAS_BYTES);
    assert_eq!(atlas.frames, pack.frame_count());
    assert_eq!(
        atlas.cells.len(),
        usize::from(atlas.frames) * usize::from(atlas.layers)
    );
    for frame in 0..atlas.frames {
        for layer in 0..atlas.layers {
            let cell = atlas.cell(frame, layer).expect("cell is addressable");
            assert!(cell.atlas_x + cell.width <= atlas.width);
            assert!(cell.atlas_y + cell.height <= atlas.height);
            assert!(cell.target_x + cell.width <= atlas.side);
            assert!(cell.target_y + cell.height <= atlas.side);
            let [u0, v0, u1, v1] = atlas.uv(cell);
            assert!(u0 >= 0.0 && v0 >= 0.0 && u1 <= 1.0 && v1 <= 1.0);
            assert!(u0 < u1 && v0 < v1);
        }
    }
}

fn compose_frame(atlas: &super::Atlas, frame: u16) -> Option<Vec<u8>> {
    let mut output = tiny_skia::Pixmap::new(atlas.side, atlas.side)?;
    for layer in 0..atlas.layers {
        let cell = atlas.cell(frame, layer)?;
        let row_bytes = (cell.width as usize).checked_mul(4)?;
        let mut pixels = vec![0; row_bytes.checked_mul(cell.height as usize)?];
        for row in 0..cell.height as usize {
            let source = ((cell.atlas_y as usize + row)
                .checked_mul(atlas.width as usize)?
                .checked_add(cell.atlas_x as usize)?)
            .checked_mul(4)?;
            let target = row.checked_mul(row_bytes)?;
            pixels
                .get_mut(target..target.checked_add(row_bytes)?)?
                .copy_from_slice(atlas.pixels.get(source..source.checked_add(row_bytes)?)?);
        }
        let size = tiny_skia::IntSize::from_wh(cell.width, cell.height)?;
        let pixmap = tiny_skia::Pixmap::from_vec(pixels, size)?;
        output.draw_pixmap(
            cell.target_x as i32,
            cell.target_y as i32,
            pixmap.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            tiny_skia::Transform::identity(),
            None,
        );
    }
    Some(output.take())
}

#[test]
fn nested_opacity_remains_a_part_atlas() {
    for id in ["plus", "ban", "sun"] {
        let pack = super::find(id).expect("generated pack");
        let atlas = super::raster_atlas(pack, 82).expect("bounded part atlas");
        assert_eq!(atlas.kind, super::AtlasKind::Parts, "{id}");
    }
}
