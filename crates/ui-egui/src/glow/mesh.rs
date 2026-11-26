//! The shared coloured mesh for every AI flare outline.

use super::*;
use egui::epaint::Mesh;

/// A geometry vertex before the shared flare colours are applied.
struct Vertex {
    point: Pos2,
    rgb: [f32; 3],
    alpha: f32,
}

/// Apply the same light to either outline's geometry.
fn mesh(look: &Look, vertices: Vec<Vertex>, indices: Vec<u32>) -> Mesh {
    let mut mesh = Mesh::default();
    mesh.reserve_vertices(vertices.len());
    for vertex in vertices {
        mesh.colored_vertex(vertex.point, look.color(vertex.rgb, vertex.alpha));
    }
    mesh.indices = indices;
    mesh
}

pub(super) fn path(geometry: &outline::Geometry, origin: Pos2, reach: f32, look: &Look) -> Mesh {
    let mut vertices = Vec::with_capacity(geometry.samples.len());
    for sample in &geometry.samples {
        let (rgb, level) = look.at(sample.share, geometry.length);
        let flare = reach * (0.45 + 0.55 * level);
        let keep = if sample.distance < 0.0 {
            (1.0 + sample.distance / 0.5).clamp(0.0, 1.0)
        } else {
            falloff(sample.distance / flare)
        };
        vertices.push(Vertex {
            point: sample.point + origin.to_vec2(),
            rgb,
            alpha: level * look.strength * keep,
        });
    }
    mesh(look, vertices, geometry.indices.clone())
}

fn falloff(at: f32) -> f32 {
    if at >= 1.0 {
        return 0.0;
    }
    for pair in FALLOFF.windows(2) {
        let [a, b] = pair else { continue };
        if at <= b.0 {
            let t = ((at - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
            return a.1 + (b.1 - a.1) * t;
        }
    }
    0.0
}

impl Ring {
    /// Light from the outline outward, reaching further where it burns hot.
    pub(super) fn mesh(&self, look: &Look) -> Mesh {
        let k = FALLOFF.len() as u32 + 1;
        let n = self.points.len() as u32;
        let mut vertices = Vec::with_capacity(self.points.len() * k as usize);
        let mut indices = Vec::with_capacity(self.points.len() * (k as usize - 1) * 6);
        for ((p, normal), share) in self.points.iter().zip(&self.shares) {
            let (rgb, level) = look.at(*share, self.length);
            let (alpha, flare) = (level * look.strength, self.reach * (0.45 + 0.55 * level));
            // Half a point inside the outline smooths its edge.
            vertices.push(Vertex {
                point: *p - *normal * 0.5,
                rgb,
                alpha: 0.0,
            });
            for (off, keep) in FALLOFF {
                vertices.push(Vertex {
                    point: *p + *normal * off * flare,
                    rgb,
                    alpha: alpha * keep,
                });
            }
        }
        for i in 0..n {
            let j = (i + 1) % n;
            for band in 0..k - 1 {
                let (a, b) = (i * k + band, j * k + band);
                indices.extend([a, a + 1, b, a + 1, b + 1, b]);
            }
        }
        mesh(look, vertices, indices)
    }
}
