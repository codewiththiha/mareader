//! Paints the read-aloud start pin and its countdown ring.

use egui::{Color32, Pos2, Rect, Shape, Stroke, Ui, Vec2, vec2};

use crate::theme::Tokens;

/// The knob's radius, and the bar's width under it.
pub(super) const KNOB: f32 = 8.0;
pub(super) const BAR_W: f32 = 3.0;

/// How the pin is drawn this frame.
#[derive(Clone, Copy, Debug)]
pub(super) struct Look {
    pub alpha: f32,
    /// The knob's scale: grown under the pointer, more while held.
    pub swell: f32,
    /// How far the wait to read has gone, 0 to 1; none while held.
    pub ring: Option<f32>,
    pub held: bool,
    pub free: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            swell: 1.0,
            ring: None,
            held: false,
            free: false,
        }
    }
}

/// The knob sits atop the bar like a pin's head.
fn knob(bar: Rect) -> Pos2 {
    bar.center_top() - vec2(0.0, KNOB - 2.0)
}

/// Where a press takes the pin: its knob, and its bar a little wider.
pub(super) fn reach(bar: Rect) -> Rect {
    let head = Rect::from_center_size(knob(bar), Vec2::splat(2.0 * (KNOB + 6.0)));
    head.union(bar.expand2(vec2(6.0, 2.0)))
}

pub(super) fn paint(ui: &Ui, bar: Rect, word: Option<Rect>, look: &Look, t: &Tokens) {
    let p = ui.painter();
    let a = look.alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let ink = t
        .accent
        .gamma_multiply(a * if look.free { 0.6 } else { 1.0 });
    // The word reading starts on, softly lit.
    if let Some(w) = word {
        p.rect_filled(
            w.expand2(vec2(2.0, 1.0)),
            3.0,
            t.accent.gamma_multiply(0.16 * a),
        );
    }
    let head = knob(bar);
    let r = KNOB * look.swell;
    if look.held {
        p.circle_filled(head, r * 1.9, t.accent.gamma_multiply(0.14 * a));
    }
    p.rect_filled(bar, BAR_W * 0.5, ink);
    p.circle_filled(head, r, ink);
    // A play mark in the knob says what resting here will do.
    let w = r * 0.42;
    let tip = head + vec2(w * 0.95, 0.0);
    let tri = vec![head + vec2(-w * 0.55, -w), head + vec2(-w * 0.55, w), tip];
    let light = Color32::WHITE.gamma_multiply(0.95 * a);
    p.add(Shape::convex_polygon(tri, light, Stroke::NONE));
    // The wait to read fills a ring round the knob.
    if let Some(done) = look.ring {
        let ring = r + 3.5;
        p.circle_stroke(
            head,
            ring,
            Stroke::new(2.0, t.accent.gamma_multiply(0.18 * a)),
        );
        if done > 0.0 {
            let n = (48.0 * done).ceil().max(2.0) as usize;
            let arc: Vec<Pos2> = (0..=n)
                .map(|i| {
                    let turn = done * i as f32 / n as f32;
                    let angle = std::f32::consts::TAU * turn - std::f32::consts::FRAC_PI_2;
                    head + ring * Vec2::angled(angle)
                })
                .collect();
            p.add(Shape::line(arc, Stroke::new(2.0, ink)));
        }
    }
}

pub(super) fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

/// Overshoots a touch, then settles: the pin drops into place.
pub(super) fn ease_out_back(x: f32) -> f32 {
    let (c1, c3) = (1.70158_f32, 2.70158_f32);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_lands_where_it_should() {
        assert!(ease_out_back(0.0).abs() < 1e-6);
        assert!((ease_out_back(1.0) - 1.0).abs() < 1e-6);
        assert!(ease_out_back(0.7) > 1.0, "it overshoots");
        assert!((ease_out(1.0) - 1.0).abs() < 1e-6);
    }
}
