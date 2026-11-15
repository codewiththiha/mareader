//! Frameless windows get no system resize edges, so the outer 5 pt grab.

use egui::viewport::ResizeDirection;
use egui::{Context, CursorIcon, ViewportCommand};

/// The grab band along each window edge, in points.
pub const EDGE: f32 = 5.0;

/// Which edge `pos` grabs inside a window of `size`, if any.
pub fn direction(size: egui::Vec2, pos: egui::Pos2) -> Option<ResizeDirection> {
    let (l, r) = (pos.x <= EDGE, pos.x >= size.x - EDGE);
    let (t, b) = (pos.y <= EDGE, pos.y >= size.y - EDGE);
    match (l, r, t, b) {
        (true, _, true, _) => Some(ResizeDirection::NorthWest),
        (_, true, true, _) => Some(ResizeDirection::NorthEast),
        (true, _, _, true) => Some(ResizeDirection::SouthWest),
        (_, true, _, true) => Some(ResizeDirection::SouthEast),
        (true, ..) => Some(ResizeDirection::West),
        (_, true, ..) => Some(ResizeDirection::East),
        (_, _, true, _) => Some(ResizeDirection::North),
        (.., true) => Some(ResizeDirection::South),
        _ => None,
    }
}

fn cursor(dir: ResizeDirection) -> CursorIcon {
    match dir {
        ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
    }
}

/// Call before laying out widgets so the edge wins the press.
pub fn edges(ctx: &Context, enabled: bool) {
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    if !enabled || maximized {
        return;
    }
    let size = ctx.content_rect().size();
    let Some(dir) = ctx.pointer_hover_pos().and_then(|p| direction(size, p)) else {
        return;
    };
    ctx.set_cursor_icon(cursor(dir));
    if ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    #[test]
    fn corners_win_over_edges_and_the_middle_is_free() {
        let s = vec2(800.0, 600.0);
        assert_eq!(
            direction(s, pos2(1.0, 1.0)),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            direction(s, pos2(799.0, 599.0)),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(direction(s, pos2(400.0, 2.0)), Some(ResizeDirection::North));
        assert_eq!(direction(s, pos2(2.0, 300.0)), Some(ResizeDirection::West));
        assert_eq!(direction(s, pos2(400.0, 300.0)), None);
    }
}
