//! Hit testing, selection and return-to-row scrolling.

use super::*;

pub(super) fn row_hit(ui: &mut Ui, row: &Row, size: egui::Vec2) -> egui::Response {
    let id = ui.make_persistent_id("detail-hit");
    let rect = ui
        .ctx()
        .data(|d| d.get_temp::<egui::Rect>(id))
        .unwrap_or_else(|| egui::Rect::from_min_size(ui.cursor().min, size));
    let hit = ui.interact(
        rect.intersect(ui.clip_rect()),
        id.with("button"),
        Sense::click(),
    );
    hit.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            true,
            format!("Details for {}", row.lemma),
        )
    });
    if hit.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    hit
}

pub(super) fn pick(
    ui: &Ui,
    row: &Row,
    rect: egui::Rect,
    hit: egui::Response,
    picked: &mut Option<(Row, egui::Rect)>,
    selection: &mut Selection,
    scope: space::Scope,
    reveal: &mut Option<(u128, bool, f64)>,
    out: &mut Commands,
) {
    let id = ui.make_persistent_id("detail-hit");
    ui.ctx().data_mut(|d| d.insert_temp(id, rect));
    if rect.intersect(ui.clip_rect()).is_positive() {
        selection.visible(row);
    }
    if let Some((key, pending, _)) = reveal
        && *key == row.key
        && *pending
    {
        ui.scroll_to_rect(rect, Some(Align::Center));
        *pending = false;
    }
    selection.menu(&hit, row, scope, out);
    if hit.clicked() {
        if selection.active() {
            selection.toggle(row);
        } else {
            *picked = Some((row.clone(), rect.intersect(ui.clip_rect())));
        }
    }
}

