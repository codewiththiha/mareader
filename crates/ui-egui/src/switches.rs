//! Smooth animated switches, segmented controls, and card tab bars.

use egui::{
    Color32, CornerRadius, CursorIcon, FontId, Id, Rect, Response, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::motion;
use crate::theme::{self, Tokens};

pub(crate) const SWITCH: Vec2 = vec2(36.0, 20.0);
const SEGMENT: f32 = 24.0;
const INLINE_H: f32 = 26.0;
const TAB_H: f32 = 28.0;
const SLIDE_SECS: f32 = 0.18;
const HOVER_SECS: f32 = 0.10;
const PRESS_SECS: f32 = 0.08;

/// An on/off switch; `label` names it for screen readers and tests.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(SWITCH, Sense::click());
    let enabled = ui.is_enabled();
    if enabled && resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let value = *on;
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, value, label));
    paint_switch(ui, rect, &resp, value);
    resp
}

/// A switch's track and knob, sliding smoothly with cubic ease-out.
pub(crate) fn paint_switch(ui: &Ui, rect: Rect, resp: &Response, on: bool) {
    let ctx = ui.ctx();
    let t = Tokens::get(ctx);
    let enabled = ui.is_enabled();
    if enabled && resp.hovered() {
        ctx.set_cursor_icon(CursorIcon::PointingHand);
    }
    let how = motion::slide(ctx, resp.id, if on { 1.0 } else { 0.0 }, SLIDE_SECS);
    let hot_target = f32::from(u8::from(enabled && resp.hovered()));
    let hot = motion::slide(ctx, resp.id.with("hot"), hot_target, HOVER_SECS);
    let press_target = f32::from(u8::from(enabled && resp.is_pointer_button_down_on()));
    let press = motion::slide(ctx, resp.id.with("press"), press_target, PRESS_SECS);
    let mut track = t.track.lerp_to_gamma(t.accent, how);
    if hot > 0.0 {
        track = track.lerp_to_gamma(t.ink, 0.08 * hot);
    }
    if !enabled {
        track = track.gamma_multiply(0.55);
    }
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), track);
    let diameter = 16.0;
    let knob_w = diameter + 3.0 * press;
    let x = egui::lerp(rect.left() + 2.0..=rect.right() - 2.0 - knob_w, how);
    let top = rect.center().y - diameter * 0.5;
    let knob = Rect::from_min_size(pos2(x, top), vec2(knob_w, diameter));
    let edge = Stroke::new(1.0, Color32::from_black_alpha(if t.dark { 0 } else { 28 }));
    let fill = if enabled {
        Color32::WHITE
    } else {
        Color32::WHITE.gamma_multiply(0.75)
    };
    ui.painter()
        .rect(knob, CornerRadius::same(8), fill, edge, StrokeKind::Inside);
}

/// Animated selectable buttons, `cols` to a row, sharing the width.
pub fn choices<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    all: &[T],
    label: fn(T) -> &'static str,
    cols: usize,
) {
    segments_of(ui, 28.0, value, all, cols, |v| (None, label(v)));
}

/// Choices as one segmented track, `cols` equal cells to a row.
pub fn segments<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    all: &[T],
    cols: usize,
    face: impl Fn(T) -> (Option<Icon>, &'static str),
) {
    segments_of(ui, SEGMENT, value, all, cols, face);
}

/// Segments with rows `row` tall, sliding with zero click lag.
pub fn segments_of<T: Copy + PartialEq>(
    ui: &mut Ui,
    row: f32,
    value: &mut T,
    all: &[T],
    cols: usize,
    face: impl Fn(T) -> (Option<Icon>, &'static str),
) {
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let enabled = ui.is_enabled();
    // A segment animates on selection because its track stays visible.
    let _act = anim::announce(ui, Act::Click);
    let cols = cols.max(1);
    let rows = all.len().div_ceil(cols);
    let size = vec2(ui.available_width(), row * rows as f32);
    let (track, _) = ui.allocate_exact_size(size, Sense::hover());
    let line = Stroke::new(1.0, t.line);
    ui.painter()
        .rect_stroke(track, CornerRadius::same(6), line, StrokeKind::Inside);
    let bg = ui.painter().add(Shape::Noop);
    let w = track.width() / cols as f32;
    let cell_at = |col: f32, r: f32| {
        let min = track.min + vec2(col * w, r * row);
        Rect::from_min_size(min, vec2(w, row)).shrink(2.0)
    };
    let mut cells = Vec::with_capacity(all.len());
    for (i, &v) in all.iter().enumerate() {
        let (icon, label) = face(v);
        let col = (i % cols) as f32;
        let r = (i / cols) as f32;
        let rect = cell_at(col, r);
        let resp = ui.interact(rect, ui.id().with(("segment", label)), Sense::click());
        if enabled && resp.clicked() {
            *value = v;
        }
        cells.push((v, icon, label, col, r, rect, resp));
    }
    let first = cells.first().map_or("", |c| c.2);
    let thumb = ui.id().with(("segments", first));
    let (tx, ty) = match all.iter().position(|v| v == value) {
        Some(i) => {
            let x = motion::slide(&ctx, thumb.with("x"), (i % cols) as f32, SLIDE_SECS);
            let y = if rows > 1 {
                motion::slide(&ctx, thumb.with("y"), (i / cols) as f32, SLIDE_SECS)
            } else {
                0.0
            };
            let fill = if enabled {
                t.selected
            } else {
                t.selected.gamma_multiply(0.55)
            };
            let shape = Shape::rect_filled(cell_at(x, y), CornerRadius::same(4), fill);
            ui.painter().set(bg, shape);
            (Some(x), y)
        }
        None => (None, 0.0),
    };
    paint_segment_cells(ui, &cells, (first, tx, ty, enabled), value, &t);
}

fn hover_cell(
    ui: &Ui,
    resp: &Response,
    cell: Rect,
    radius: u8,
    (enabled, on): (bool, bool),
    t: &Tokens,
) {
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        if !on {
            ui.painter()
                .rect_filled(cell, CornerRadius::same(radius), t.hover);
        }
    }
}

#[allow(clippy::type_complexity)]
fn paint_segment_cells<T: Copy + PartialEq>(
    ui: &mut Ui,
    cells: &[(T, Option<Icon>, &'static str, f32, f32, Rect, Response)],
    (first, tx, ty, enabled): (&'static str, Option<f32>, f32, bool),
    value: &T,
    t: &Tokens,
) {
    let base_ink = if enabled { t.ink } else { t.muted };
    let pick_ink = if enabled { t.accent } else { t.muted };
    let font = FontId::proportional(t.text);
    for &(v, icon, label, col, r, cell, ref resp) in cells {
        let on = *value == v;
        hover_cell(ui, resp, cell, 4, (enabled, on), t);
        let lit = match tx {
            Some(x) => {
                let dx = (1.0 - (x - col).abs()).clamp(0.0, 1.0);
                let dy = (1.0 - (ty - r).abs()).clamp(0.0, 1.0);
                dx * dy
            }
            None => f32::from(u8::from(on)),
        };
        let ink = base_ink.lerp_to_gamma(pick_ink, lit);
        let galley = ui
            .painter()
            .layout_no_wrap(label.to_owned(), font.clone(), ink);
        let icon_w = if icon.is_some() { 18.0 } else { 0.0 };
        let mut x = cell.center().x - (icon_w + galley.size().x) / 2.0;
        let y = cell.center().y;
        if let Some(icon) = icon {
            let at = Rect::from_min_size(pos2(x, y - 7.0), Vec2::splat(14.0));
            ui.push_id(("segment-icon", first, label), |ui| {
                anim::paint(ui, at, icon, ink, label, resp);
            });
            x += icon_w;
        }
        ui.painter()
            .galley(pos2(x, y - galley.size().y / 2.0), galley, ink);
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, on, label));
    }
}

/// Content-sized segmented buttons in one sliding track, for row controls.
pub fn inline_segments<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    all: &[T],
    label: impl Fn(T) -> &'static str,
) {
    let ctx = ui.ctx().clone();
    let t = Tokens::get(&ctx);
    let enabled = ui.is_enabled();
    let font = FontId::proportional(t.text);
    let mut labels = Vec::with_capacity(all.len());
    let mut widths = Vec::with_capacity(all.len());
    let mut total_w = 4.0;
    for &v in all {
        let text = label(v);
        let text_w = ui
            .painter()
            .layout_no_wrap(text.to_owned(), font.clone(), t.ink)
            .size()
            .x;
        let w = (text_w + 18.0).max(40.0);
        labels.push((v, text));
        widths.push(w);
        total_w += w;
    }
    let (track, _) = ui.allocate_exact_size(vec2(total_w, INLINE_H), Sense::hover());
    ui.painter().rect_stroke(
        track,
        CornerRadius::same(6),
        Stroke::new(1.0, t.line),
        StrokeKind::Inside,
    );
    let bg = ui.painter().add(Shape::Noop);
    let mut x = track.left() + 2.0;
    let mut cells = Vec::with_capacity(all.len());
    for ((v, text), w) in labels.into_iter().zip(widths) {
        let cell = Rect::from_min_size(pos2(x, track.top() + 2.0), vec2(w, INLINE_H - 4.0));
        let resp = ui.interact(cell, ui.id().with(("inline-seg", text)), Sense::click());
        if enabled && resp.clicked() {
            *value = v;
        }
        cells.push((v, text, cell, resp));
        x += w;
    }
    let first = cells.first().map_or("", |c| c.1);
    let thumb = ui.id().with(("inline-segments", first));
    let pill = cells.iter().find(|c| c.0 == *value).map(|c| {
        let edge = track.left();
        let l = motion::slide(&ctx, thumb.with("l"), c.2.left() - edge, SLIDE_SECS);
        let r = motion::slide(&ctx, thumb.with("r"), c.2.right() - edge, SLIDE_SECS);
        let rect = Rect::from_min_max(
            pos2(track.left() + l, track.top() + 2.0),
            pos2(track.left() + r, track.bottom() - 2.0),
        );
        let fill = if enabled {
            t.selected
        } else {
            t.selected.gamma_multiply(0.55)
        };
        let shape = Shape::rect_filled(rect, CornerRadius::same(4), fill);
        ui.painter().set(bg, shape);
        rect
    });
    paint_inline_cells(ui, &cells, pill, enabled, value, &t);
}

fn paint_inline_cells<T: Copy + PartialEq>(
    ui: &Ui,
    cells: &[(T, &'static str, Rect, Response)],
    pill: Option<Rect>,
    enabled: bool,
    value: &T,
    t: &Tokens,
) {
    let base_ink = if enabled { t.ink } else { t.muted };
    let pick_ink = if enabled { t.accent } else { t.muted };
    let font = FontId::proportional(t.text);
    for &(v, text, cell, ref resp) in cells {
        let on = *value == v;
        hover_cell(ui, resp, cell, 4, (enabled, on), t);
        let lit = pill.map_or(f32::from(u8::from(on)), |p| {
            let cut = p.intersect(cell);
            let ratio = cut.width() / cell.width().max(1.0);
            if cut.is_positive() {
                ratio.clamp(0.0, 1.0)
            } else {
                0.0
            }
        });
        let ink = base_ink.lerp_to_gamma(pick_ink, lit);
        let galley = ui
            .painter()
            .layout_no_wrap(text.to_owned(), font.clone(), ink);
        let pos = pos2(
            cell.center().x - galley.size().x * 0.5,
            cell.center().y - galley.size().y * 0.5,
        );
        ui.painter().galley(pos, galley, ink);
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, enabled, on, text));
    }
}

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
        hover_cell(ui, resp, cell, 5, (enabled, on), t);
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

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, Modifiers, PointerButton, RawInput};

    fn click_events(pos: egui::Pos2) -> Vec<Event> {
        let btn = |pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        vec![Event::PointerMoved(pos), btn(true), btn(false)]
    }

    #[test]
    fn segments_update_on_click_and_glide_across_frames() {
        let ctx = egui::Context::default();
        let mut value = 0u8;
        let all = [0u8, 1, 2];
        let mut target_pos = egui::Pos2::ZERO;
        let run = |time: f64, events: Vec<Event>, val: &mut u8, pos: &mut egui::Pos2| {
            let raw = RawInput {
                time: Some(time),
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(240.0, 60.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                segments(ui, val, &all, 3, |v| {
                    let name = match v {
                        0 => "One",
                        1 => "Two",
                        _ => "Three",
                    };
                    (None, name)
                });
                *pos = pos2(200.0, 12.0);
            });
            out.textures_delta.clear();
            out.viewport_output
                .values()
                .any(|v| v.repaint_delay.is_zero())
        };
        let _ = run(0.0, Vec::new(), &mut value, &mut target_pos);
        let _ = run(0.05, Vec::new(), &mut value, &mut target_pos);
        assert!(!run(0.08, Vec::new(), &mut value, &mut target_pos));
        assert_eq!(value, 0);
        // Clicking the third segment updates at once and starts sliding.
        assert!(run(
            0.1,
            click_events(target_pos),
            &mut value,
            &mut target_pos
        ));
        assert_eq!(value, 2);
        // Mid-glide keeps animating; once settled, stops requesting repaints.
        assert!(run(0.18, Vec::new(), &mut value, &mut target_pos));
        let _ = run(0.4, Vec::new(), &mut value, &mut target_pos);
        assert!(!run(0.5, Vec::new(), &mut value, &mut target_pos));
    }

    #[test]
    fn inline_segments_respect_disabled_state() {
        let ctx = egui::Context::default();
        let mut value = 0u8;
        let all = [0u8, 1];
        let raw = RawInput {
            time: Some(0.0),
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 40.0))),
            events: click_events(pos2(70.0, 13.0)),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            ui.add_enabled_ui(false, |ui| {
                inline_segments(ui, &mut value, &all, |v| if v == 0 { "A" } else { "B" });
            });
        });
        out.textures_delta.clear();
        assert_eq!(value, 0);
    }
}
