//! The deck while it is made: blank cards the size of the real ones drift
//! in a loose stack, their lines shimmering as if being written.

use std::f32::consts::TAU;

use egui::emath::Rot2;
use egui::epaint::Mesh;
use egui::{Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use super::flip_card;
use crate::motion;
use crate::theme::Tokens;

/// Cards in the stack, the top one last.
const CARDS: usize = 3;
/// Seconds a card takes to rise and settle, and the rise.
const FLOAT: f32 = 2.6;
const RISE: f32 = 7.0;
/// Each card's tilt step, in degrees, and the stagger of their drift.
const TILT: f32 = 4.5;
const STAGGER: f32 = 0.3;
/// Seconds a shimmer takes to sweep across a card.
const SWEEP: f32 = 1.8;

/// The stack in a band `size` wide and tall, plus the room it drifts in.
pub fn show(ui: &mut Ui, size: Vec2) {
    let room = size + vec2(0.0, RISE + 6.0 * CARDS as f32);
    let (band, _) = ui.allocate_exact_size(room, Sense::hover());
    let ctx = ui.ctx().clone();
    let moving = motion::secs(&ctx, 1.0) > 0.0;
    let now = match moving {
        true => ui.input(|i| i.time) as f32,
        false => 0.0,
    };
    let t = Tokens::get(&ctx);
    let mut tess = flip_card::tessellator(&ctx);
    let centre = band.center() + vec2(0.0, RISE / 2.0);
    for i in (0..CARDS).rev() {
        let depth = i as f32;
        let phase = ((now - depth * STAGGER) / FLOAT * TAU).cos();
        let pose = Pose {
            at: centre + vec2(0.0, depth * 6.0 - RISE * (0.5 - 0.5 * phase)),
            turn: Rot2::from_angle(((depth - 1.0) * TILT).to_radians()),
            scale: 1.0 - depth * 0.04,
        };
        let mut shapes = sheet(size, &t, 1.0 - depth * 0.35);
        lines(size, now - depth * 0.4, &t, &mut shapes);
        let mut mesh = Mesh::default();
        for shape in shapes {
            tess.tessellate_shape(shape, &mut mesh);
        }
        let fade = 1.0 - depth * 0.22;
        for v in &mut mesh.vertices {
            v.pos = pose.place(v.pos);
            v.color = v.color.gamma_multiply(fade);
        }
        ui.painter().add(Shape::mesh(mesh));
    }
    if moving {
        ctx.request_repaint();
    }
}

/// Where a card sits: its middle, tilt and size.
struct Pose {
    at: Pos2,
    turn: Rot2,
    scale: f32,
}

impl Pose {
    fn place(&self, local: Pos2) -> Pos2 {
        self.at + self.turn * (local.to_vec2() * self.scale)
    }
}

/// A card's shadow, paper and edge, about the origin.
fn sheet(size: Vec2, t: &Tokens, shadow: f32) -> Vec<Shape> {
    let r = Rect::from_center_size(Pos2::ZERO, size);
    let radius = t.radius_lg * 1.5;
    let shade = egui::Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: t.shadow.gamma_multiply(shadow),
    };
    let edge = Stroke::new(1.0, t.line);
    vec![
        Shape::Rect(shade.as_shape(r, radius)),
        Shape::rect_filled(r, radius, t.surface),
        Shape::rect_stroke(r, radius, edge, StrokeKind::Inside),
    ]
}

/// The lines a card will hold: its band, its word, a meaning and two tags,
/// each lit as the shimmer passes at `now`.
fn lines(size: Vec2, now: f32, t: &Tokens, out: &mut Vec<Shape>) {
    let (w, h) = (size.x, size.y);
    let bars = [
        (pos2(w / 2.0 - 38.0, -h / 2.0 + 27.0), vec2(40.0, 22.0)),
        (pos2(0.0, -h * 0.1), vec2(w * 0.42, 30.0)),
        (pos2(0.0, h * 0.1), vec2(w * 0.6, 11.0)),
        (pos2(0.0, h * 0.1 + 20.0), vec2(w * 0.44, 11.0)),
        (pos2(-34.0, h * 0.3), vec2(56.0, 20.0)),
        (pos2(34.0, h * 0.3), vec2(64.0, 20.0)),
    ];
    for (at, bar) in bars {
        let rect = Rect::from_center_size(at, bar);
        let lit = shimmer(now, at.x / w);
        out.push(Shape::rect_filled(rect, bar.y / 2.0, tone(t, lit)));
    }
}

/// How lit a line is, 0 to 1, at `x` across the card from -0.5 to 0.5.
fn shimmer(now: f32, x: f32) -> f32 {
    let wave = (now / SWEEP - x) * TAU;
    (0.5 + 0.5 * wave.sin()).powi(3)
}

/// A line's ink: faint, deepening while lit.
fn tone(t: &Tokens, lit: f32) -> Color32 {
    t.ink.gamma_multiply(0.07 + 0.07 * lit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shimmer_stays_in_range() {
        for i in 0..50 {
            let lit = shimmer(i as f32 * 0.13, (i as f32 / 50.0) - 0.5);
            assert!((0.0..=1.0).contains(&lit), "{lit}");
        }
    }

    #[test]
    fn a_pose_turns_and_scales_about_its_middle() {
        let pose = Pose {
            at: pos2(100.0, 50.0),
            turn: Rot2::from_angle(0.0),
            scale: 0.5,
        };
        let placed = pose.place(pos2(20.0, -10.0));
        assert!((placed - pos2(110.0, 45.0)).length() < 1e-4);
    }
}
