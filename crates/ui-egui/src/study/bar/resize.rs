//! Four in-card resize edges; the preference is committed on release.
use egui::{CursorIcon, Id, Key, Rect, Sense, Ui, Vec2, pos2};
use mareader_settings::SelectionMenuSize;

#[derive(Clone, Copy, Default)]
enum Edge {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Default)]
struct Drag {
    edge: Edge,
    origin: egui::Pos2,
    size: SelectionMenuSize,
}

fn controls_id() -> Id {
    Id::new("selection-menu-resize-controls")
}

pub(super) fn owns_focus(ctx: &egui::Context) -> bool {
    let focus = ctx.memory(|m| m.focused());
    let ids: Vec<Id> = ctx.data(|d| d.get_temp(controls_id())).unwrap_or_default();
    focus.is_some_and(|id| ids.contains(&id))
}

fn id(menu: Id) -> Id {
    menu.with("menu-resize-drag")
}

pub(super) fn active(ctx: &egui::Context, menu: Id) -> bool {
    ctx.data(|d| d.get_temp::<Drag>(id(menu))).is_some()
}

pub(super) fn current(ui: &Ui, menu: Id, saved: SelectionMenuSize) -> SelectionMenuSize {
    let drag: Option<Drag> = ui.ctx().data(|d| d.get_temp(id(menu)));
    let Some(drag) = drag else { return saved };
    let Some(pointer) = ui.ctx().pointer_interact_pos() else {
        return drag.size;
    };
    let delta = pointer - drag.origin;
    let mut size = drag.size;
    match drag.edge {
        Edge::Left => {
            size.width = (f32::from(size.width) - delta.x)
                .round()
                .clamp(0.0, 65535.0) as u16
        }
        Edge::Right => {
            size.width = (f32::from(size.width) + delta.x)
                .round()
                .clamp(0.0, 65535.0) as u16
        }
        Edge::Top => {
            size.height = (f32::from(size.height) - delta.y)
                .round()
                .clamp(0.0, 65535.0) as u16
        }
        Edge::Bottom => {
            size.height = (f32::from(size.height) + delta.y)
                .round()
                .clamp(0.0, 65535.0) as u16
        }
    }
    size.sanitize();
    size
}

pub(super) fn edges(
    ui: &mut Ui,
    rect: Rect,
    menu: Id,
    saved: SelectionMenuSize,
) -> Option<SelectionMenuSize> {
    let band = 5.0;
    let inset = 7.0;
    let edges = [
        (
            Edge::Left,
            Rect::from_min_max(
                rect.min + Vec2::new(0.0, inset),
                pos2(rect.left() + band, rect.bottom() - inset),
            ),
            CursorIcon::ResizeHorizontal,
            "Resize menu left edge",
        ),
        (
            Edge::Right,
            Rect::from_min_max(
                pos2(rect.right() - band, rect.top() + inset),
                pos2(rect.right(), rect.bottom() - inset),
            ),
            CursorIcon::ResizeHorizontal,
            "Resize menu right edge",
        ),
        (
            Edge::Top,
            Rect::from_min_max(
                rect.min + Vec2::new(inset, 0.0),
                pos2(rect.right() - inset, rect.top() + band),
            ),
            CursorIcon::ResizeVertical,
            "Resize menu top edge",
        ),
        (
            Edge::Bottom,
            Rect::from_min_max(
                pos2(rect.left() + inset, rect.bottom() - band),
                pos2(rect.right() - inset, rect.bottom()),
            ),
            CursorIcon::ResizeVertical,
            "Resize menu bottom edge",
        ),
    ];
    let ids: Vec<Id> = edges
        .iter()
        .map(|(_, _, _, label)| ui.id().with(label))
        .collect();
    ui.ctx().data_mut(|d| d.insert_temp(controls_id(), ids));
    let mut result = None;
    for (edge, zone, cursor, label) in edges {
        let r = ui
            .interact(zone, ui.id().with(label), Sense::click_and_drag())
            .on_hover_cursor(cursor);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        crate::widgets::tip(
            &r,
            "Drag to resize. Plus and minus resize; zero resets.",
            None,
        );
        if r.drag_started()
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            ui.ctx().data_mut(|d| {
                d.insert_temp(
                    id(menu),
                    Drag {
                        edge,
                        origin,
                        size: saved,
                    },
                )
            });
        }
        if r.has_focus() {
            let delta = ui.input_mut(|i| {
                let plus = i.consume_key(egui::Modifiers::NONE, Key::Equals)
                    || i.consume_key(egui::Modifiers::NONE, Key::Plus);
                let minus = i.consume_key(egui::Modifiers::NONE, Key::Minus);
                i32::from(plus) - i32::from(minus)
            });
            let reset = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Num0));
            if reset || delta != 0 {
                r.request_focus();
            }
            if reset {
                result = Some(SelectionMenuSize::default());
            } else if delta != 0 {
                let mut size = saved;
                match edge {
                    Edge::Left | Edge::Right => {
                        size.width = (i32::from(size.width) + delta * 8).clamp(0, 65535) as u16
                    }
                    Edge::Top | Edge::Bottom => {
                        size.height = (i32::from(size.height) + delta * 8).clamp(0, 65535) as u16
                    }
                }
                size.sanitize();
                result = Some(size);
            }
        }
    }
    let dragging: Option<Drag> = ui.ctx().data(|d| d.get_temp(id(menu)));
    if dragging.is_some() && ui.input(|i| i.pointer.any_released()) {
        result = Some(current(ui, menu, saved));
        ui.ctx().data_mut(|d| d.remove_temp::<Drag>(id(menu)));
    }
    result.filter(|s| *s != saved)
}
