//! A fading accent outline for returning to a Space item, separate from AI.

use crate::theme::Tokens;
use egui::{Rect, Stroke, StrokeKind, Ui};

pub(super) fn draw(ui: &Ui, rect: Rect, key: u128, reveal: &Option<(u128, bool, f64)>, t: &Tokens) {
    let Some((_, _, until)) = reveal.filter(|r| r.0 == key) else {
        return;
    };
    let now = ui.input(|i| i.time);
    let strength = if crate::motion::enabled(ui.ctx()) {
        ((until - now) / 5.0).clamp(0.0, 1.0) as f32
    } else if now < until { 1.0 } else { 0.0 };
    if strength <= 0.0 {
        return;
    }
    for (inset, width, alpha) in [(0.5, 1.5, 0.9), (2.0, 3.0, 0.32), (4.0, 5.0, 0.12)] {
        ui.painter().rect_stroke(
            rect.shrink(inset),
            8.0,
            Stroke::new(width, t.accent.gamma_multiply(alpha * strength)),
            StrokeKind::Inside,
        );
    }
    if crate::motion::enabled(ui.ctx()) {
        ui.ctx().request_repaint_after_secs(1.0 / 30.0);
    } else {
        ui.ctx().request_repaint_after_secs((until - now).max(0.0) as f32);
    }
}
