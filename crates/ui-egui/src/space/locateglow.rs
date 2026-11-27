//! A fading accent outline for returning to a Space item, separate from AI.

use egui::{Rect, Stroke, StrokeKind, Ui};
use crate::theme::Tokens;

pub(super) fn draw(ui: &Ui, rect: Rect, key: u128, reveal: &Option<(u128, bool, f64)>, t: &Tokens) {
    let Some((_, _, until)) = reveal.filter(|r| r.0 == key) else { return; };
    let now = ui.input(|i| i.time);
    let strength = ((until - now) / 5.0).clamp(0.0, 1.0) as f32;
    if strength <= 0.0 { return; }
    for (inset, width, alpha) in [(0.5, 1.5, 0.9), (2.0, 3.0, 0.32), (4.0, 5.0, 0.12)] {
        ui.painter().rect_stroke(rect.shrink(inset), 8.0,
            Stroke::new(width, t.accent.gamma_multiply(alpha * strength)), StrokeKind::Inside);
    }
    ui.ctx().request_repaint_after_secs(1.0 / 30.0);
}
