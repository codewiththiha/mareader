//! Shared pane clearance under the title bar and close button.

use egui::{Rect, pos2};

/// The first clear y coordinate, with optional space for a pane control.
pub fn top(pane: Rect, bar_bottom: f32, reserve: f32) -> f32 {
    pane.top().max(bar_bottom) + reserve.max(0.0)
}

/// The visible pane below chrome; none if the chrome covers the whole pane.
pub fn room(pane: Rect, bar_bottom: f32, reserve: f32) -> Option<Rect> {
    let y = top(pane, bar_bottom, reserve);
    (pane.is_finite() && y < pane.bottom() && pane.width() > 0.0)
        .then(|| Rect::from_min_max(pos2(pane.left(), y), pane.max))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn clearance_uses_window_band_only_where_it_overlaps() {
        let pane = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 400.0));
        assert_eq!(top(pane, 48.0, 0.0), 48.0);
        assert_eq!(top(pane, 48.0, 38.0), 86.0);
        let lower = pane.translate(vec2(0.0, 200.0));
        assert_eq!(top(lower, 48.0, 0.0), 200.0);
        assert_eq!(top(pane, 0.0, 0.0), 0.0);
        assert!(room(pane, 410.0, 0.0).is_none());
    }
}
