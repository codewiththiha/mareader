//! Bounds the light around bends in a sampled selection path.

use egui::{Pos2, Vec2};

pub(super) const INSET: f32 = 1.5;

fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// Limits inward and outward offsets before neighbouring normals cross.
pub(super) fn limits(points: &[(Pos2, Vec2)], winding: f32, reach: f32) -> Vec<(f32, f32)> {
    let mut limits = vec![(INSET, reach); points.len()];
    for i in 0..points.len() {
        let j = (i + 1) % points.len();
        let (a, na) = points[i];
        let (b, nb) = points[j];
        let edge = b - a;
        let turn = cross(na, nb) * winding;
        let left = (cross(na, edge) * winding).max(0.0);
        let right = (cross(nb, edge) * winding).max(0.0);
        if turn < -1e-6 {
            limits[j].1 = limits[j].1.min(left / -turn * 0.95);
            limits[i].1 = limits[i].1.min(right / -turn * 0.95);
        } else if turn > 1e-6 {
            limits[j].0 = limits[j].0.min(left / turn * 0.95);
            limits[i].0 = limits[i].0.min(right / turn * 0.95);
        }
    }
    limits
}
