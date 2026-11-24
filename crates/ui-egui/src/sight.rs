//! Whether something drawn in a pane is in the reader's sight: the one
//! rule the AI stack's visuals and the read-aloud player share.

use egui::Rect;

use crate::page_bar::BAR_H;

/// Whether `rect` shows in `room`, the part of a pane the reader sees.
pub fn visible(room: Rect, rect: Rect) -> bool {
    rect.is_finite() && room.intersects(rect)
}

/// The part of `pane` its words read in: above the page bar's band, which
/// the bar covers when it comes up.
pub fn reading_room(pane: Rect) -> Rect {
    pane.with_max_y(pane.bottom() - BAR_H)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    #[test]
    fn a_rect_shows_while_any_of_it_is_in_the_room() {
        let pane = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 400.0));
        let word = |y: f32| Rect::from_min_size(pos2(20.0, y), vec2(40.0, 12.0));
        assert!(visible(pane, word(100.0)));
        assert!(visible(pane, word(395.0)), "a sliver still shows");
        assert!(!visible(pane, word(420.0)), "scrolled past");
        let room = reading_room(pane);
        assert!(!visible(room, word(380.0)), "under the page bar's band");
        assert!(!visible(pane, Rect::NOTHING));
    }
}
