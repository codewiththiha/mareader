//! Layer deduplication, cropping and bounded atlas packing.

use tiny_skia::{Pixmap, Transform};

use crate::raster::render_nodes;
use crate::timeline::{Vals, values};
use crate::{Atlas, AtlasCell, AtlasKind, FRAME_RATE, Layer, MAX_ATLAS_BYTES, PAD, Pack, SPAN};

#[derive(Clone, Debug, PartialEq, Eq)]
struct LayerKey {
    layer: u16,
    state: Vec<u32>,
}

struct Variant {
    key: LayerKey,
    target_x: u32,
    target_y: u32,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    atlas_x: u32,
    atlas_y: u32,
}

pub(super) fn raster_part_atlas(pack: &'static Pack, side: u32) -> Option<Atlas> {
    let frames = pack.frame_count();
    let layers = u16::try_from(pack.layers.len()).ok()?;
    if layers == 0 {
        return None;
    }
    let mut variants: Vec<Variant> = Vec::new();
    let mut mapping = Vec::with_capacity(usize::from(frames) * usize::from(layers));
    let mut variant_bytes = 0usize;
    for frame in 0..frames {
        let time = f32::from(frame) / FRAME_RATE;
        let vals = values(pack, time);
        for (layer_index, layer) in pack.layers.iter().enumerate() {
            let key = layer_key(layer, &vals, u16::try_from(layer_index).ok()?)?;
            let variant = variants.iter().position(|variant| variant.key == key);
            let index = if let Some(index) = variant {
                index
            } else {
                let variant = raster_layer(pack, layer, &vals, side, key)?;
                variant_bytes = variant_bytes.checked_add(variant.pixels.len())?;
                if variant_bytes > MAX_ATLAS_BYTES {
                    return None;
                }
                variants.push(variant);
                variants.len() - 1
            };
            mapping.push(index);
        }
    }
    let (width, height) = pack_variants(&mut variants)?;
    let byte_len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;
    if byte_len > MAX_ATLAS_BYTES {
        return None;
    }
    let mut pixels = vec![0; byte_len];
    for variant in &variants {
        copy_rect(
            &variant.pixels,
            variant.width,
            variant.height,
            variant.atlas_x,
            variant.atlas_y,
            width,
            &mut pixels,
        )?;
    }
    let cells = mapping
        .into_iter()
        .map(|index| {
            let variant = variants.get(index)?;
            Some(AtlasCell {
                atlas_x: variant.atlas_x,
                atlas_y: variant.atlas_y,
                target_x: variant.target_x,
                target_y: variant.target_y,
                width: variant.width,
                height: variant.height,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Atlas {
        kind: AtlasKind::Parts,
        width,
        height,
        side,
        frames,
        layers,
        cells,
        pixels,
    })
}

fn layer_key(layer: &Layer, vals: &[Vals], index: u16) -> Option<LayerKey> {
    let mut state = Vec::with_capacity(layer.parts.len().saturating_mul(12));
    for part in layer.parts {
        let value = vals.get(usize::from(*part))?;
        state.extend([
            value.x.to_bits(),
            value.y.to_bits(),
            value.scale.to_bits(),
            value.scale_x.to_bits(),
            value.scale_y.to_bits(),
            value.rot.to_bits(),
            value.skew_x.to_bits(),
            value.skew_y.to_bits(),
            value.op.to_bits(),
            value.len.to_bits(),
            value.off.to_bits(),
            value.width.to_bits(),
        ]);
    }
    Some(LayerKey {
        layer: index,
        state,
    })
}

fn raster_layer(
    pack: &Pack,
    layer: &Layer,
    vals: &[Vals],
    side: u32,
    key: LayerKey,
) -> Option<Variant> {
    let mut pixmap = Pixmap::new(side, side)?;
    let scale = side as f32 / SPAN;
    let canvas = Transform::from_row(scale, 0.0, 0.0, scale, PAD * scale, PAD * scale);
    render_nodes(
        layer.scene,
        pack.parts,
        vals,
        None,
        None,
        canvas,
        &mut pixmap,
    )?;
    let (target_x, target_y, width, height, pixels) = crop(&pixmap)?;
    Some(Variant {
        key,
        target_x,
        target_y,
        width,
        height,
        pixels,
        atlas_x: 0,
        atlas_y: 0,
    })
}

pub(super) fn crop(pixmap: &Pixmap) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    let (width, height) = (pixmap.width(), pixmap.height());
    let mut left = width;
    let mut top = height;
    let mut right = 0u32;
    let mut bottom = 0u32;
    let mut visible = false;
    for y in 0..height {
        for x in 0..width {
            let alpha = *pixmap.data().get(((y * width + x) * 4 + 3) as usize)?;
            if alpha != 0 {
                visible = true;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x);
                bottom = bottom.max(y);
            }
        }
    }
    if !visible {
        return Some((0, 0, 1, 1, vec![0; 4]));
    }
    left = left.saturating_sub(1);
    top = top.saturating_sub(1);
    right = right.saturating_add(2).min(width);
    bottom = bottom.saturating_add(2).min(height);
    let crop_width = right.checked_sub(left)?;
    let crop_height = bottom.checked_sub(top)?;
    let mut pixels = vec![
        0;
        (crop_width as usize)
            .checked_mul(crop_height as usize)?
            .checked_mul(4)?
    ];
    for row in 0..crop_height {
        let src = (((top + row) * width + left) * 4) as usize;
        let dst = (row * crop_width * 4) as usize;
        let bytes = (crop_width * 4) as usize;
        pixels
            .get_mut(dst..dst + bytes)?
            .copy_from_slice(pixmap.data().get(src..src + bytes)?);
    }
    Some((left, top, crop_width, crop_height, pixels))
}

type PackedLayout = (u64, u32, u32, Vec<(u32, u32)>);

fn pack_variants(variants: &mut [Variant]) -> Option<(u32, u32)> {
    const WIDTHS: [u32; 8] = [256, 512, 768, 1024, 1280, 1536, 1792, 2048];
    let mut order: Vec<usize> = (0..variants.len()).collect();
    order.sort_by_key(|index| std::cmp::Reverse(variants[*index].height));
    let mut best: Option<PackedLayout> = None;
    for width in WIDTHS {
        let mut x = 1u32;
        let mut y = 1u32;
        let mut row_height = 0u32;
        let mut positions = vec![(0, 0); variants.len()];
        let mut fits = true;
        for index in &order {
            let variant = variants.get(*index)?;
            let cell_width = variant.width.saturating_add(2);
            let cell_height = variant.height.saturating_add(2);
            if cell_width.saturating_add(2) > width {
                fits = false;
                break;
            }
            if x.saturating_add(cell_width) > width {
                x = 1;
                y = y.saturating_add(row_height);
                row_height = 0;
            }
            positions[*index] = (x + 1, y + 1);
            x = x.saturating_add(cell_width);
            row_height = row_height.max(cell_height);
        }
        if !fits {
            continue;
        }
        let height = y.saturating_add(row_height).saturating_add(1);
        let area = u64::from(width) * u64::from(height);
        if area.saturating_mul(4) > MAX_ATLAS_BYTES as u64 {
            continue;
        }
        if best
            .as_ref()
            .is_none_or(|(best_area, _, _, _)| area < *best_area)
        {
            best = Some((area, width, height, positions));
        }
    }
    let (_, width, height, positions) = best?;
    for (variant, (x, y)) in variants.iter_mut().zip(positions) {
        variant.atlas_x = x;
        variant.atlas_y = y;
    }
    Some((width, height))
}

fn copy_rect(
    source: &[u8],
    source_width: u32,
    source_height: u32,
    x: u32,
    y: u32,
    atlas_width: u32,
    atlas: &mut [u8],
) -> Option<()> {
    let row_bytes = (source_width as usize).checked_mul(4)?;
    for row in 0..source_height as usize {
        let src = row.checked_mul(row_bytes)?;
        let dst = ((y as usize).checked_add(row)?)
            .checked_mul(atlas_width as usize)?
            .checked_add(x as usize)?
            .checked_mul(4)?;
        atlas
            .get_mut(dst..dst.checked_add(row_bytes)?)?
            .copy_from_slice(source.get(src..src.checked_add(row_bytes)?)?);
    }
    Some(())
}
