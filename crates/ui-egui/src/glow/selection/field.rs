//! A bounded distance field around a closed selection outline.

use egui::{Pos2, Rect, Vec2, pos2};
use std::hash::{Hash, Hasher};

const PIXELS: f32 = 360_000.0;
const SIDE: f32 = 1536.0;

#[derive(Clone, Copy, Default)]
pub(super) struct Hit {
    pub distance: u8,
    pub share: u8,
}

pub(super) struct Field {
    pub bounds: Rect,
    pub size: [usize; 2],
    pub hits: Vec<Hit>,
    pub length: f32,
}

/// Coordinates local to the first point stay stable when a page scrolls.
pub(super) fn local(points: &[Pos2], reach: f32, ppp: f32) -> Option<(Pos2, Vec<Pos2>, u64)> {
    let origin = *points.first()?;
    if points.len() < 3 || points.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let quantize = |v: f32| (v * 32.0).round() / 32.0;
    let points: Vec<_> = points
        .iter()
        .map(|p| pos2(quantize(p.x - origin.x), quantize(p.y - origin.y)))
        .collect();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for p in &points {
        p.x.to_bits().hash(&mut h);
        p.y.to_bits().hash(&mut h);
    }
    quantize(reach).to_bits().hash(&mut h);
    quantize(ppp).to_bits().hash(&mut h);
    Some((origin, points, h.finish()))
}

impl Field {
    pub(super) fn build(points: &[Pos2], reach: f32, ppp: f32) -> Option<Self> {
        let mut bounds = Rect::NOTHING;
        for point in points {
            bounds.extend_with(*point);
        }
        if !bounds.is_positive() || !reach.is_finite() || reach <= 0.0 {
            return None;
        }
        bounds = bounds.expand(reach + 1.0);
        let area = bounds.width() * bounds.height();
        let scale = ppp
            .clamp(0.1, 2.0)
            .min((PIXELS / area).sqrt())
            .min(SIDE / bounds.width())
            .min(SIDE / bounds.height());
        if !scale.is_finite() || scale <= 0.0 {
            return None;
        }
        let size = [
            (bounds.width() * scale).ceil() as usize,
            (bounds.height() * scale).ceil() as usize,
        ];
        let count = size[0].checked_mul(size[1])?;
        if count == 0 || count > 370_000 {
            return None;
        }
        let mut distances = vec![f32::INFINITY; count];
        let mut shares = vec![0.0_f32; count];
        let lengths: Vec<f32> = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(a, b)| (*b - *a).length())
            .collect();
        let length: f32 = lengths.iter().sum();
        if !length.is_finite() || length < 1.0 {
            return None;
        }
        let mut along = 0.0;
        for ((a, b), &len) in points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .zip(&lengths)
        {
            let d = *b - *a;
            if len > 0.0001 {
                mark_segment(
                    (*a, d, len, along / length),
                    (bounds, scale, size, reach, length),
                    &mut distances,
                    &mut shares,
                );
            }
            along += len;
        }
        mask_inside(points, bounds, scale, size, &mut distances);
        let hits = distances
            .into_iter()
            .zip(shares)
            .map(|(distance, share)| Hit {
                distance: if distance >= reach {
                    255
                } else {
                    ((distance / reach * 254.0).round() as u8).min(254)
                },
                share: (share.rem_euclid(1.0) * 256.0) as u8,
            })
            .collect();
        Some(Self {
            bounds,
            size,
            hits,
            length,
        })
    }
}

fn mark_segment(
    (a, d, len, start): (Pos2, Vec2, f32, f32),
    (bounds, scale, size, reach, total): (Rect, f32, [usize; 2], f32, f32),
    distances: &mut [f32],
    shares: &mut [f32],
) {
    let r = Rect::from_two_pos(a, a + d).expand(reach);
    let x0 = ((r.left() - bounds.left()) * scale).floor().max(0.0) as usize;
    let y0 = ((r.top() - bounds.top()) * scale).floor().max(0.0) as usize;
    let x1 = (((r.right() - bounds.left()) * scale).ceil() as usize).min(size[0]);
    let y1 = (((r.bottom() - bounds.top()) * scale).ceil() as usize).min(size[1]);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = bounds.min + egui::vec2((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale);
            let t = ((p - a).dot(d) / (len * len)).clamp(0.0, 1.0);
            let distance = (p - (a + d * t)).length();
            let i = y * size[0] + x;
            if let (Some(best), Some(share)) = (distances.get_mut(i), shares.get_mut(i))
                && distance < *best
            {
                *best = distance;
                *share = start + t * len / total;
            }
        }
    }
}

fn mask_inside(points: &[Pos2], bounds: Rect, scale: f32, size: [usize; 2], distances: &mut [f32]) {
    let mut crossings = Vec::new();
    for y in 0..size[1] {
        let yy = bounds.top() + (y as f32 + 0.5) / scale;
        crossings.clear();
        for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
            if (a.y > yy) != (b.y > yy) {
                crossings.push(a.x + (yy - a.y) * (b.x - a.x) / (b.y - a.y));
            }
        }
        crossings.sort_by(f32::total_cmp);
        for &[from, to] in crossings.as_chunks::<2>().0 {
            let left =
                (((from - bounds.left()) * scale - 0.5).ceil().max(0.0) as usize).min(size[0]);
            let right =
                (((to - bounds.left()) * scale - 0.5).ceil().max(0.0) as usize).min(size[0]);
            for x in left..right {
                if let Some(distance) = distances.get_mut(y * size[0] + x) {
                    *distance = f32::INFINITY;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(field: &Field, p: Pos2) -> Hit {
        let x = ((p.x - field.bounds.left()) / field.bounds.width() * field.size[0] as f32).floor()
            as usize;
        let y = ((p.y - field.bounds.top()) / field.bounds.height() * field.size[1] as f32).floor()
            as usize;
        field.hits[y * field.size[0] + x]
    }

    #[test]
    fn narrow_inside_corner_has_no_dark_hole_outside_the_outline() {
        let shape = [
            pos2(0.0, 0.0),
            pos2(100.0, 0.0),
            pos2(100.0, 25.0),
            pos2(60.0, 25.0),
            pos2(60.0, 60.0),
            pos2(0.0, 60.0),
        ];
        let field = Field::build(&shape, 12.0, 2.0).unwrap();
        for y in 26..34 {
            for x in 61..69 {
                let h = hit(&field, pos2(x as f32, y as f32));
                assert!(h.distance < 255, "outside corner at {x},{y} is dark");
            }
        }
        assert_eq!(hit(&field, pos2(20.0, 20.0)).distance, 255);
    }

    #[test]
    fn resolution_and_storage_are_bounded() {
        let shape = [
            pos2(0.0, 0.0),
            pos2(10000.0, 0.0),
            pos2(10000.0, 8000.0),
            pos2(0.0, 8000.0),
        ];
        let field = Field::build(&shape, 18.0, 2.0).unwrap();
        assert!(field.hits.len() <= 370_000);
        assert!(field.size.iter().all(|&v| v <= 1537));
    }
}
