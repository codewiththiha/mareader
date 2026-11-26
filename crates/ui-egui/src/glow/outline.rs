//! Bounded, non-overlapping mesh geometry for an irregular glow outline.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{Pos2, Rect, pos2};
use i_triangle::float::uniform::UniformTriangulatable;
use i_triangle::i_overlay::core::fill_rule::FillRule;
use i_triangle::i_overlay::core::overlay_rule::OverlayRule;
use i_triangle::i_overlay::float::single::SingleFloatOverlay;
use i_triangle::i_overlay::mesh::float::outline::offset::OutlineOffset;
use i_triangle::i_overlay::mesh::float::style::{LineJoin, OutlineStyle};

const CACHE_LIMIT: usize = 8;
const MAX_POINTS: usize = 4096;
const MAX_VERTICES: usize = 50_000;
const MAX_BUCKET_ENTRIES: usize = 1_000_000;
const BUCKET: f32 = 64.0;

#[derive(Clone)]
pub(crate) struct Cache {
    key: (usize, usize),
    signature: u64,
    geometry: Arc<Geometry>,
}

pub(super) struct Sample {
    pub point: Pos2,
    pub share: f32,
    /// Distance from the outline; negative on its half-point inner edge.
    pub distance: f32,
}

pub(super) struct Geometry {
    pub samples: Vec<Sample>,
    pub indices: Vec<u32>,
    pub length: f32,
}

/// Build once in local coordinates so scrolling does not rebuild the mesh.
pub(super) fn cached<'a>(
    points: &[Pos2],
    reach: f32,
    caches: &'a mut Vec<Cache>,
    key: (usize, usize),
) -> Option<(Pos2, &'a Geometry)> {
    let origin = *points.first()?;
    if points.len() < 3 || points.len() > MAX_POINTS || points.iter().any(|p| !p.is_finite()) {
        return None;
    }
    let quantize = |v: f32| (v * 32.0).round() / 32.0;
    let mut local: Vec<_> = points
        .iter()
        .map(|p| pos2(quantize(p.x - origin.x), quantize(p.y - origin.y)))
        .collect();
    if local.iter().any(|p| !p.is_finite()) {
        return None;
    }
    local.dedup();
    if local.len() > 1 && local.first() == local.last() {
        local.pop();
    }
    if local.len() < 3 {
        return None;
    }
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    for p in &local {
        p.x.to_bits().hash(&mut hash);
        p.y.to_bits().hash(&mut hash);
    }
    quantize(reach).to_bits().hash(&mut hash);
    let signature = hash.finish();
    let index = if let Some(index) = caches
        .iter()
        .position(|c| c.key == key && c.signature == signature)
    {
        index
    } else {
        let geometry = Arc::new(Geometry::build(&local, reach)?);
        caches.retain(|c| c.key != key);
        if caches.len() >= CACHE_LIMIT {
            caches.remove(0);
        }
        caches.push(Cache {
            key,
            signature,
            geometry,
        });
        caches.len() - 1
    };
    Some((origin, &caches.get(index)?.geometry))
}

struct Edge {
    a: Pos2,
    delta: egui::Vec2,
    length: f32,
    start: f32,
    share_length: f32,
}

impl Geometry {
    fn build(points: &[Pos2], reach: f32) -> Option<Self> {
        if !reach.is_finite() || reach <= 0.0 {
            return None;
        }
        let mut bounds = Rect::NOTHING;
        for point in points {
            bounds.extend_with(*point);
        }
        if !bounds.is_positive() || bounds.width() > 16384.0 || bounds.height() > 16384.0 {
            return None;
        }
        // Like rectangular rings, travel is measured on a middle offset ring.
        let signed: f32 = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(a, b)| a.x * b.y - a.y * b.x)
            .sum();
        if !signed.is_finite() || signed.abs() < 0.001 {
            return None;
        }
        let previous = points.iter().cycle().skip(points.len() - 1);
        let next = points.iter().cycle().skip(1);
        let normals = previous
            .zip(points)
            .zip(next)
            .map(|((&a, &b), &c)| {
                let mut tangent = c - a;
                if tangent.length_sq() < 1e-8 {
                    tangent = c - b;
                }
                let normal = egui::vec2(tangent.y, -tangent.x).normalized() * signed.signum();
                (b, normal)
            })
            .collect();
        let ring = super::Ring::from_points(normals, reach);
        let length = ring.length;
        if !length.is_finite() || length < 1.0 || length > 100_000.0 {
            return None;
        }
        let mut edges = Vec::new();
        let next_share = ring.shares.iter().cycle().skip(1);
        for (((a, b), &start), &end) in points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .zip(&ring.shares)
            .zip(next_share)
        {
            let delta = *b - *a;
            let size = delta.length();
            if size > 0.001 {
                edges.push(Edge {
                    a: *a,
                    delta,
                    length: size,
                    start,
                    share_length: (end - start).rem_euclid(1.0),
                });
            }
        }
        if edges.len() < 3 {
            return None;
        }
        // Every point in the light is within reach of at least one edge.
        // Bucket expanded edge bounds so sampling long selections stays bounded.
        let cell = |v: f32| (v / BUCKET).floor() as i32;
        let mut buckets: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        let mut entries = 0usize;
        for (index, edge) in edges.iter().enumerate() {
            let bounds = Rect::from_two_pos(edge.a, edge.a + edge.delta).expand(reach + 1.0);
            let (x0, x1) = (cell(bounds.left()), cell(bounds.right()));
            let (y0, y1) = (cell(bounds.top()), cell(bounds.bottom()));
            let width = usize::try_from(x1 - x0 + 1).ok()?;
            let height = usize::try_from(y1 - y0 + 1).ok()?;
            entries = entries.checked_add(width.checked_mul(height)?)?;
            if entries > MAX_BUCKET_ENTRIES {
                return None;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    buckets.entry((x, y)).or_default().push(index);
                }
            }
        }
        let shape = vec![points.iter().map(|p| [p.x, p.y]).collect::<Vec<_>>()];
        let outer = shape.outline(&OutlineStyle::new(reach).line_join(LineJoin::Round(0.25)));
        let halo = outer.overlay(&shape, OverlayRule::Difference, FillRule::EvenOdd);
        let inset = shape.outline(&OutlineStyle::new(-0.5).line_join(LineJoin::Round(0.25)));
        let inner = shape.overlay(&inset, OverlayRule::Difference, FillRule::EvenOdd);
        // Limit the mesh for unusually long outlines without scaling the whole glow.
        let step = (length * reach / 20_000.0)
            .sqrt()
            .max((bounds.area() / 50_000.0).sqrt())
            .max(2.0);
        let mut samples = Vec::new();
        let mut indices = Vec::new();
        for (shapes, inside) in [(&halo, false), (&inner, true)] {
            for polygon in shapes {
                let mesh = polygon.uniform_triangulate(step).to_triangulation::<u32>();
                if mesh.points.len().checked_add(samples.len())? > MAX_VERTICES {
                    return None;
                }
                let base = u32::try_from(samples.len()).ok()?;
                for [x, y] in mesh.points {
                    let point = pos2(x, y);
                    let mut best = f32::INFINITY;
                    let mut share = 0.0;
                    let candidates = buckets.get(&(cell(x), cell(y)))?;
                    for index in candidates {
                        let edge = edges.get(*index)?;
                        let along = ((point - edge.a).dot(edge.delta)
                            / (edge.length * edge.length))
                            .clamp(0.0, 1.0);
                        let distance = (point - (edge.a + edge.delta * along)).length_sq();
                        if distance < best {
                            best = distance;
                            share = edge.start + along * edge.share_length;
                        }
                    }
                    let distance = best.sqrt();
                    samples.push(Sample {
                        point,
                        share,
                        distance: if inside { -distance } else { distance },
                    });
                }
                for index in mesh.indices {
                    indices.push(base.checked_add(index)?);
                }
            }
        }
        (!indices.is_empty()).then_some(Self {
            samples,
            indices,
            length,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape() -> Vec<Pos2> {
        vec![
            pos2(0.0, 0.0),
            pos2(220.0, 0.0),
            pos2(220.0, 24.0),
            pos2(90.0, 24.0),
            pos2(90.0, 55.0),
            pos2(0.0, 55.0),
        ]
    }

    fn covers(mesh: &Geometry, p: Pos2) -> usize {
        fn cross(a: egui::Vec2, b: egui::Vec2) -> f32 {
            a.x * b.y - a.y * b.x
        }
        mesh.indices
            .chunks_exact(3)
            .filter(|tri| {
                let (Some(a), Some(b), Some(c)) = (
                    mesh.samples.get(tri[0] as usize),
                    mesh.samples.get(tri[1] as usize),
                    mesh.samples.get(tri[2] as usize),
                ) else {
                    return false;
                };
                let (a, b, c) = (a.point, b.point, c.point);
                let area = cross(b - a, c - a);
                if area.abs() < 1e-6 {
                    return false;
                }
                let u = cross(b - a, p - a) / area;
                let v = cross(c - b, p - b) / area;
                let w = cross(a - c, p - c) / area;
                u > 1e-5 && v > 1e-5 && w > 1e-5
            })
            .count()
    }

    #[test]
    fn outside_concave_corner_is_covered_once_without_lighting_text() {
        let mesh = Geometry::build(&shape(), 12.0).unwrap();
        for y in 26..34 {
            for x in 92..100 {
                let p = pos2(x as f32 + 0.17, y as f32 + 0.31);
                assert_eq!(covers(&mesh, p), 1, "gap or overlap at {p:?}");
            }
        }
        assert_eq!(covers(&mesh, pos2(20.17, 20.31)), 0);
        assert!(mesh.samples.iter().any(|v| v.distance < -0.25));
        assert!(mesh.samples.iter().all(|v| v.distance > -0.55));
    }

    #[test]
    fn invalid_and_extreme_inputs_are_rejected() {
        let mut cache = Vec::new();
        assert!(cached(&[], 12.0, &mut cache, (0, 0)).is_none());
        assert!(cached(&[pos2(f32::NAN, 0.0); 3], 12.0, &mut cache, (0, 0)).is_none());
        let giant = [
            pos2(0.0, 0.0),
            pos2(20000.0, 0.0),
            pos2(20000.0, 20.0),
            pos2(0.0, 20.0),
        ];
        assert!(cached(&giant, 12.0, &mut cache, (0, 0)).is_none());
    }
}
