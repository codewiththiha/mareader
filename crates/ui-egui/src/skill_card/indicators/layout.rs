//! Nearest clear positions on a page's end edge.

use egui::{Rect, pos2, vec2};

use super::{GAP, HEIGHT, INSET};

pub(super) struct Layout {
    pub buttons: Vec<Rect>,
    pub more: Option<Rect>,
}

/// Keep each answer near its text, with a shared overflow if the edge fills.
pub(super) fn arrange(room: Rect, width: f32, centers: &[f32], bubbles: &[Rect]) -> Layout {
    let right = room.right() - INSET;
    let mut result = column(room, width, right, centers, bubbles);
    if result.buttons.is_empty()
        && result.more.is_none()
        && let Some(left) = bubbles.iter().map(|r| r.left()).reduce(f32::min)
    {
        result = column(room, width, left - GAP, centers, bubbles);
    }
    result
}

fn column(room: Rect, width: f32, right: f32, centers: &[f32], bubbles: &[Rect]) -> Layout {
    let mut buttons = Vec::new();
    if right - width < room.left() || room.height() < HEIGHT + INSET * 2.0 {
        return Layout {
            buttons,
            more: None,
        };
    }
    let mut used = bubbles.to_vec();
    for center in centers {
        let Some(rect) = nearest(room, width, right, *center, &used) else {
            let more = buttons.pop();
            return Layout { buttons, more };
        };
        buttons.push(rect);
        used.push(rect);
    }
    Layout {
        buttons,
        more: None,
    }
}

fn nearest(room: Rect, width: f32, right: f32, center: f32, used: &[Rect]) -> Option<Rect> {
    let min_y = room.top() + INSET;
    let max_y = room.bottom() - INSET - HEIGHT;
    let target = (center - HEIGHT / 2.0).clamp(min_y, max_y);
    let mut candidates = vec![target, min_y, max_y];
    for r in used {
        candidates.push((r.top() - GAP - HEIGHT).clamp(min_y, max_y));
        candidates.push((r.bottom() + GAP).clamp(min_y, max_y));
    }
    candidates.sort_by(|a, b| {
        (a - target)
            .abs()
            .total_cmp(&(b - target).abs())
            .then_with(|| b.total_cmp(a))
    });
    candidates.into_iter().find_map(|y| {
        let rect = Rect::from_min_size(pos2(right - width, y), vec2(width, HEIGHT));
        used.iter()
            .all(|r| {
                rect.right() + GAP <= r.left()
                    || r.right() + GAP <= rect.left()
                    || rect.bottom() + GAP <= r.top()
                    || r.bottom() + GAP <= rect.top()
            })
            .then_some(rect)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_follow_centers_and_avoid_bubbles_and_other_answers() {
        let room = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 400.0));
        let bubble = Rect::from_min_size(pos2(270.0, 90.0), vec2(22.0, 22.0));
        let layout = arrange(room, 90.0, &[200.0, 300.0, 200.0, 100.0], &[bubble]);
        assert_eq!(layout.buttons.len(), 4);
        assert_eq!(layout.buttons[0].center().y, 200.0);
        assert_eq!(layout.buttons[1].center().y, 300.0);
        for (i, r) in layout.buttons.iter().enumerate() {
            assert!(room.contains_rect(*r) && !r.intersects(bubble));
            assert!(
                layout
                    .buttons
                    .iter()
                    .skip(i + 1)
                    .all(|other| !r.intersects(*other))
            );
        }
    }

    #[test]
    fn crowded_edge_keeps_a_more_slot() {
        let room = Rect::from_min_size(pos2(0.0, 0.0), vec2(150.0, 100.0));
        let layout = arrange(room, 80.0, &[40.0; 20], &[]);
        assert!(layout.more.is_some());
        assert!(layout.buttons.len() < 20);
        assert!(
            layout
                .buttons
                .iter()
                .all(|r| !r.intersects(layout.more.unwrap()))
        );
    }
}
