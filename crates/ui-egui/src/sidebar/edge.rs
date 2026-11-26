//! The fixed-width sidebar's clickable edge.

use super::*;

const HIT_WIDTH: f32 = 8.0;

impl Sidebar {
    pub(super) fn edge(&self, ui: &mut Ui, rail: Rect, out: &mut Commands) {
        let t = Tokens::get(ui.ctx());
        let mut line = Stroke::new(1.0, t.line);
        if self.open {
            let hit = Rect::from_min_max(pos2(rail.right() - HIT_WIDTH, rail.top()), rail.max);
            let response = ui.interact(hit, Id::new("sidebar-toggle-edge"), Sense::click());
            let (label, hint) = if self.docked {
                ("Close sidebar at edge", "Click to close sidebar")
            } else {
                ("Keep sidebar open at edge", "Click to keep sidebar open")
            };
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
            if response.hovered() {
                ui.painter()
                    .rect_filled(hit, 0.0, t.accent.gamma_multiply(0.10));
                line = Stroke::new(2.0, t.accent.gamma_multiply(0.7));
            }
            if response.clicked() {
                out.push(Sent::Ui(UiCommand::ToggleSidebar));
            }
            response
                .on_hover_cursor(egui::CursorIcon::Default)
                .on_hover_text_at_pointer(hint);
        }
        ui.painter().vline(rail.right() - 0.5, rail.y_range(), line);
    }
}
