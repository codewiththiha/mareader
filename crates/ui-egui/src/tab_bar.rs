//! Sliding-pill tab bar for card headers.

use egui::{
    CornerRadius, CursorIcon, FontId, Id, Rect, Response, Sense, Shape, Ui, Vec2, WidgetInfo,
    WidgetType, pos2, vec2,
};

use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::motion;
use crate::theme::{self, Tokens};

const TAB_H: f32 = 28.0;
const SLIDE_SECS: f32 = 0.18;

/// Elevated sliding-pill tabs for card headers; true when the tab changed.
pub fn tab_bar<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: Id,
    value: &mut T,
    all: &[T],
    face: impl Fn(T) -> (Option<Icon>, &'static str),
) -> bool {
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let enabled = ui.is_enabled();
    let _act = anim::announce(ui, Act::Click);
    let count = all.len().max(1);
    let (track, _) = ui.allocate_exact_size(vec2(ui.available_width(), TAB_H), Sense::hover());
    let well = t.ink.gamma_multiply(if t.dark { 0.08 } else { 0.055 });
    ui.painter().rect_filled(track, CornerRadius::same(7), well);
    let bg = ui.painter().add(Shape::Noop);
    let inner = track.shrink(2.0);
    let w = inner.width() / count as f32;
    let cell_at = |col: f32| {
        let min = pos2(inner.left() + col * w, inner.top());
        Rect::from_min_size(min, vec2(w, inner.height()))
    };
    let mut changed = false;
    let mut cells = Vec::with_capacity(all.len());
    for (i, &v) in all.iter().enumerate() {
        let (icon, label) = face(v);
        let rect = cell_at(i as f32);
        let resp = ui.interact(rect, id.with(("tab", label)), Sense::click());
        if enabled && resp.clicked() && *value != v {
            *value = v;
            changed = true;
        }
        cells.push((v, icon, label, i as f32, rect, resp));
    }
    let tx = all.iter().position(|v| v == value).map(|i| {
        let x = motion::slide(&ctx, id.with("x"), i as f32, SLIDE_SECS);
        let pill = cell_at(x);
        let fill = if t.dark { t.selected } else { t.surface };
        let edge = t.line.gamma_multiply(if t.dark { 0.8 } else { 0.9 });
        let shape = Shape::Vec(vec![
            Shape::rect_filled(pill, CornerRadius::same(5), edge),
            Shape::rect_filled(pill.shrink(1.0), CornerRadius::same(4), fill),
        ]);
        ui.painter().set(bg, shape);
        x
    });
    paint_tab_cells(ui, id, &cells, (tx, enabled), value, &t);
    changed
}

#[allow(clippy::type_complexity)]
fn paint_tab_cells<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: Id,
    cells: &[(T, Option<Icon>, &'static str, f32, Rect, Response)],
    (tx, enabled): (Option<f32>, bool),
    value: &T,
    t: &Tokens,
) {
    let font_on = theme::medium(t.text_small);
    let font_off = FontId::proportional(t.text_small);
    for &(v, icon, label, col, cell, ref resp) in cells {
        let on = *value == v;
        if enabled && resp.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            if !on {
                ui.painter()
                    .rect_filled(cell, CornerRadius::same(5), t.hover);
            }
        }
        let lit = match tx {
            Some(x) => (1.0 - (x - col).abs()).clamp(0.0, 1.0),
            None => f32::from(u8::from(on)),
        };
        let pick_ink = if t.dark { t.accent } else { t.ink };
        let ink = t.muted.lerp_to_gamma(pick_ink, lit);
        let font = if lit > 0.5 {
            font_on.clone()
        } else {
            font_off.clone()
        };
        let galley = ui.painter().layout_no_wrap(label.to_owned(), font, ink);
        let icon_w = if icon.is_some() { 16.0 } else { 0.0 };
        let mut x = cell.center().x - (icon_w + galley.size().x) * 0.5;
        let y = cell.center().y;
        if let Some(icon) = icon {
            let at = Rect::from_min_size(pos2(x, y - 6.5), Vec2::splat(13.0));
            ui.push_id((id, "tab-icon", label), |ui| {
                anim::paint(ui, at, icon, ink, label, resp);
            });
            x += icon_w;
        }
        ui.painter()
            .galley(pos2(x, y - galley.size().y * 0.5), galley, ink);
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, on, label));
    }
}
