//! The player's sides, dragged to set how wide it is.

use egui::{CornerRadius, CursorIcon, Rect, Sense, Ui, pos2, vec2};

use crate::motion;
use crate::theme::Tokens;
use crate::widgets;

/// Points either side of an edge that take the pointer.
const GRAB: f32 = 5.0;
/// What hovering an edge says.
const TIP: &str = "Drag to resize · double-click to reset";

/// What the sides were asked this frame.
pub(super) enum Resize {
    /// Being dragged: this wide now.
    To(f32),
    /// Let go at this width.
    Set(f32),
    /// Double-clicked: back to what its parts need.
    Reset,
}

/// Both sides of `rect`; dragging one widens it evenly, being centred.
pub(super) fn show(ui: &Ui, rect: Rect, (now, least, most): (f32, f32, f32)) -> Option<Resize> {
    let t = Tokens::get(ui.ctx());
    let mut asked = None;
    for (side, x, name) in [(-1.0, rect.left(), "start"), (1.0, rect.right(), "end")] {
        let band = Rect::from_min_max(
            pos2(x - GRAB, rect.top() + 12.0),
            pos2(x + GRAB, rect.bottom() - 12.0),
        );
        let id = ui.id().with(("read-aloud-edge", name));
        let r = ui.interact(band, id, Sense::click_and_drag());
        let label = format!("Resize player at its {name}");
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
        let held = r.dragged() || r.drag_stopped();
        if r.hovered() || held {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if !held {
            widgets::tip(&r, TIP, None);
        }
        if r.double_clicked() {
            asked = Some(Resize::Reset);
        } else if r.drag_stopped() {
            asked = Some(Resize::Set(now));
        } else if r.dragged() {
            let to = now + 2.0 * side * r.drag_delta().x;
            asked = Some(Resize::To(to.clamp(least, most.max(least))));
        }
        grip(ui, (rect, x, side), r.hovered() || r.dragged(), t.muted);
    }
    asked
}

/// A short bar just inside the edge, shown while it can be dragged.
fn grip(ui: &Ui, (rect, x, side): (Rect, f32, f32), hot: bool, colour: egui::Color32) {
    let id = ui.id().with(("read-aloud-grip", side as i32));
    let k = ui
        .ctx()
        .animate_bool_with_time(id, hot, motion::secs(ui.ctx(), 0.15));
    if k <= 0.01 {
        return;
    }
    let tall = (rect.height() * 0.4).min(24.0);
    let at = pos2(x - side * 4.0, rect.center().y);
    let bar = Rect::from_center_size(at, vec2(3.0, tall * (0.6 + 0.4 * k)));
    let fill = colour.gamma_multiply(0.7 * k);
    ui.painter().rect_filled(bar, CornerRadius::same(2), fill);
}
