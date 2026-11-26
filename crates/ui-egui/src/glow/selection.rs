//! The shared flare sampled around a selection without offset mesh joins.

mod field;

use std::sync::Arc;
use egui::{Color32, ColorImage, Context, Painter, Pos2, Rect, TextureHandle, TextureOptions};
use crate::theme::Tokens;
use super::{FALLOFF, Glow};
use field::Field;

const CACHE_LIMIT: usize = 8;
const DISTANCES: usize = 64;
const SHARES: usize = 256;

#[derive(Clone)]
pub(crate) struct Cache {
    key: (usize, usize),
    signature: u64,
    field: Arc<Field>,
    texture: TextureHandle,
}

/// Draw a clipped selection with the shared palette and timing.
pub(crate) fn paint(
    (ctx, painter, tokens): (&Context, &Painter, &Tokens),
    points: &[Pos2],
    reference: Rect,
    (glow, travel_time): (Glow, f64),
    caches: &mut Vec<Cache>,
    key: (usize, usize),
) -> bool {
    let reach = super::reach(reference);
    let visible = points.iter().zip(points.iter().cycle().skip(1)).any(|(a,b)| Rect::from_two_pos(*a,*b).expand(reach).intersects(painter.clip_rect()));
    if !visible { return false; }
    let Some((origin, local, signature)) = field::local(points, reach, ctx.pixels_per_point()) else { return false; };
    let index = if let Some(index) = caches.iter().position(|c| c.key == key && c.signature == signature) {
        index
    } else {
        let Some(field) = Field::build(&local, reach, ctx.pixels_per_point()) else { return false; };
        caches.retain(|c| c.key != key);
        if caches.len() >= CACHE_LIMIT { caches.remove(0); }
        let image = ColorImage::new([1,1], vec![Color32::TRANSPARENT]);
        caches.push(Cache { key, signature, field: Arc::new(field), texture: ctx.load_texture("selection-flare", image, TextureOptions::LINEAR) });
        caches.len() - 1
    };
    let Some(cache) = caches.get_mut(index) else { return false; };
    let look = super::look_at(ctx, tokens, glow, cache.field.length, travel_time);
    let colors = palette(&look, cache.field.length);
    let pixels = cache.field.hits.iter().map(|h| {
        if h.distance == 255 { Color32::TRANSPARENT } else {
            let distance = h.distance as usize * (DISTANCES - 1) / 254;
            colors.get(h.share as usize * DISTANCES + distance).copied().unwrap_or(Color32::TRANSPARENT)
        }
    }).collect();
    cache.texture.set(ColorImage::new(cache.field.size, pixels), TextureOptions::LINEAR);
    let rect = cache.field.bounds.translate(origin.to_vec2());
    painter.image(cache.texture.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    super::wake(ctx, cache.field.length);
    true
}

fn palette(look: &super::Look, length: f32) -> Vec<Color32> {
    let mut colors = Vec::with_capacity(SHARES * DISTANCES);
    for share in 0..SHARES {
        let (rgb, level) = look.at(share as f32 / SHARES as f32, length);
        let flare = 0.45 + 0.55 * level;
        for distance in 0..DISTANCES {
            let fraction = distance as f32 / (DISTANCES - 1) as f32 / flare;
            let keep = falloff(fraction);
            colors.push(look.color(rgb, level * look.strength * keep));
        }
    }
    colors
}

fn falloff(at: f32) -> f32 {
    if at >= 1.0 { return 0.0; }
    for pair in FALLOFF.windows(2) {
        let [a, b] = pair else { continue };
        if at <= b.0 {
            let t = ((at - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
            return a.1 + (b.1 - a.1) * t;
        }
    }
    0.0
}
