//! Polished, theme-matched animated checkbox control.

use egui::{
    Color32, CornerRadius, CursorIcon, Rect, Response, Sense, Stroke, StrokeKind, TextStyle,
    TextWrapMode, Ui, WidgetInfo, WidgetText, WidgetType, pos2, vec2,
};

use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};

const BOX: f32 = 18.0;
const SLOT: f32 = 24.0;
const GAP: f32 = 8.0;
const CHECK: f32 = 12.0;
const RADIUS: u8 = 5;
const SLIDE_SECS: f32 = 0.16;
const HOVER_SECS: f32 = 0.10;
const PRESS_SECS: f32 = 0.08;

/// Leading width of box + gap so nested hints align under the label.
pub const CHECKBOX_INDENT: f32 = BOX + GAP;

/// A standalone checkbox centered in a `24×24` slot for right-aligned rows.
pub fn checkbox_labeled(ui: &mut Ui, checked: &mut bool, label: &str) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(SLOT, SLOT), Sense::click());
    let enabled = ui.is_enabled();
    if enabled && resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    let value = *checked;
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, value, label));
    let box_rect = Rect::from_center_size(rect.center(), vec2(BOX, BOX));
    paint_box(ui, box_rect, &resp, value);
    resp
}

/// A theme-matched checkbox with a clickable label beside it.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, text: impl Into<WidgetText>) -> Response {
    let text = text.into();
    let label = text.text().to_owned();
    if label.is_empty() {
        return checkbox_labeled(ui, checked, "");
    }
    let wrap = (ui.available_width() - CHECKBOX_INDENT).max(40.0);
    let galley = text.into_galley(ui, Some(TextWrapMode::Wrap), wrap, TextStyle::Body);
    let size = vec2(CHECKBOX_INDENT + galley.size().x, galley.size().y.max(SLOT));
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    if enabled && resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    let value = *checked;
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, value, &label));
    let center = pos2(rect.left() + BOX * 0.5, rect.center().y);
    let box_rect = Rect::from_center_size(center, vec2(BOX, BOX));
    paint_box(ui, box_rect, &resp, value);
    let t = Tokens::get(ui.ctx());
    let fallback = if enabled { t.ink } else { t.muted };
    let text_y = rect.center().y - galley.size().y * 0.5;
    let text_pos = pos2(rect.left() + CHECKBOX_INDENT, text_y);
    ui.painter().galley(text_pos, galley, fallback);
    resp
}

fn paint_box(ui: &Ui, rect: Rect, resp: &Response, on: bool) {
    let ctx = ui.ctx();
    let t = Tokens::get(ctx);
    let enabled = ui.is_enabled();
    if enabled && resp.hovered() {
        ctx.set_cursor_icon(CursorIcon::PointingHand);
    }
    let how = motion::slide(ctx, resp.id, f32::from(u8::from(on)), SLIDE_SECS);
    let hot_t = f32::from(u8::from(enabled && resp.hovered()));
    let hot = motion::slide(ctx, resp.id.with("hot"), hot_t, HOVER_SECS);
    let press_t = f32::from(u8::from(enabled && resp.is_pointer_button_down_on()));
    let press = motion::slide(ctx, resp.id.with("press"), press_t, PRESS_SECS);
    let box_rect = rect.shrink(0.8 * press);

    let base_fill = if t.dark {
        t.ink.gamma_multiply(0.05)
    } else {
        t.surface
    };
    let off_fill = base_fill.lerp_to_gamma(t.hover, hot);
    let mut fill = off_fill.lerp_to_gamma(t.accent, how);
    if hot > 0.0 && how > 0.0 {
        fill = fill.lerp_to_gamma(Color32::WHITE, 0.08 * hot * how);
    }
    let off_edge = t
        .line
        .lerp_to_gamma(t.ink, if t.dark { 0.24 } else { 0.18 });
    let hot_edge = off_edge.lerp_to_gamma(t.accent, 0.65 * hot);
    let mut edge = hot_edge.lerp_to_gamma(t.accent, how);
    if !enabled {
        fill = fill.gamma_multiply(0.55);
        edge = edge.gamma_multiply(0.55);
    }
    let round = CornerRadius::same(RADIUS);
    let stroke = Stroke::new(1.25, edge);
    ui.painter()
        .rect(box_rect, round, fill, stroke, StrokeKind::Inside);

    if resp.has_focus() {
        let ring = Stroke::new(2.0, t.accent.gamma_multiply(0.45));
        ui.painter().rect_stroke(
            box_rect.expand(2.0),
            CornerRadius::same(RADIUS + 2),
            ring,
            StrokeKind::Outside,
        );
    }

    if how > 0.02 {
        let side = CHECK * (0.65 + 0.35 * how);
        let mark = Rect::from_center_size(box_rect.center(), vec2(side, side));
        let mut ink = theme::ink_on(t.accent).gamma_multiply(how);
        if !enabled {
            ink = ink.gamma_multiply(0.75);
        }
        Icon::Check.paint(ui, mark, ink);
    }
}

#[cfg(test)]
mod tests {
    use egui::accesskit::Role;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable as _;

    use super::*;

    #[test]
    fn checkbox_toggles_and_aligns_in_slot() {
        let mut h = Harness::new_ui_state(
            |ui, (a, b): &mut (bool, bool)| {
                checkbox_labeled(ui, a, "Use default");
                checkbox(ui, b, "Remember");
            },
            (true, false),
        );
        h.step();
        let solo = h.get_by_role_and_label(Role::CheckBox, "Use default");
        let rect = solo.rect();
        assert!((rect.width() - SLOT).abs() < 0.5);
        assert!((rect.height() - SLOT).abs() < 0.5);
        solo.click();
        h.step();
        assert!(!h.state().0);

        let with_text = h.get_by_role_and_label(Role::CheckBox, "Remember");
        with_text.click();
        h.step();
        assert!(h.state().1);
    }
}
