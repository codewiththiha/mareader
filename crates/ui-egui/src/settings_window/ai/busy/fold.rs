//! The backup list's fold, drawn with egui: its title row turns it, and
//! shut it keeps one line naming the backups in order.

use egui::{CursorIcon, Id, Rect, Sense, Ui, UiBuilder, Vec2, pos2, vec2};

use super::super::super::ROW_H;
use crate::icons::Icon;
use crate::theme::{self, Tokens};

/// The section's title.
pub(super) const NAME: &str = "Backup models";
/// The title row's height.
const TITLE_H: f32 = 22.0;

fn key() -> Id {
    Id::new("ai-backups-fold")
}

/// Whether the reader left the list open; it starts open.
fn open(ui: &Ui) -> bool {
    ui.data_mut(|d| d.get_persisted::<bool>(key()))
        .unwrap_or(true)
}

fn set(ui: &Ui, open: bool) {
    ui.data_mut(|d| d.insert_persisted(key(), open));
}

/// The title with its "?", the count and a chevron; a click anywhere else
/// on the row turns the fold. How open it is, eased from 0 to 1.
pub(super) fn title(ui: &mut Ui, n: usize, help: &[(&str, &str)]) -> f32 {
    let t = Tokens::get(ui.ctx());
    let mut open = open(ui);
    ui.add_space(12.0);
    let size = vec2(ui.available_width(), TITLE_H);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let r = ui.interact(rect, key().with("title"), Sense::click());
    if r.clicked() {
        open = !open;
        set(ui, open);
    }
    let kind = egui::WidgetType::CollapsingHeader;
    r.widget_info(|| egui::WidgetInfo::selected(kind, true, open, NAME));
    let hot = r.hovered();
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let ink = if hot { t.ink } else { t.muted };
    let y = rect.center().y;
    let font = theme::medium(t.text_small);
    let galley = ui.painter().layout_no_wrap(NAME.to_owned(), font, ink);
    let words = galley.size().x;
    ui.painter()
        .galley(pos2(rect.left(), y - galley.size().y / 2.0), galley, ink);
    let dot = Rect::from_min_size(pos2(rect.left() + words + 6.0, y - 8.0), Vec2::splat(16.0));
    ui.scope_builder(UiBuilder::new().max_rect(dot), |ui| {
        crate::help_dot::show(ui, NAME, help);
    });
    let how = ui.ctx().animate_bool_responsive(key(), open);
    let chevron = Rect::from_center_size(pos2(rect.right() - 12.0, y), Vec2::splat(12.0));
    let icon = Icon::ChevronRight.image(ui, 12.0).tint(ink);
    let turn = how * std::f32::consts::FRAC_PI_2;
    icon.rotate(turn, Vec2::splat(0.5)).paint_at(ui, chevron);
    crate::fold::pill(ui, n, chevron.left() - 6.0, y);
    ui.add_space(6.0);
    how
}

/// The card's inside: `whole` while open; shut, one line naming `names`
/// in order, which opens it. Between, the two cross-fade as it grows.
pub(super) fn body(ui: &mut Ui, how: f32, names: &[String], whole: impl FnOnce(&mut Ui)) {
    let tall_key = key().with("tall");
    if how >= 1.0 {
        let top = ui.cursor().top();
        whole(ui);
        let tall = ui.cursor().top() - top;
        ui.data_mut(|d| d.insert_temp(tall_key, tall));
        return;
    }
    let tall = ui
        .data(|d| d.get_temp::<f32>(tall_key))
        .unwrap_or(ROW_H * 4.0);
    let high = ROW_H + (tall - ROW_H) * how;
    let (slot, _) = ui.allocate_exact_size(vec2(ui.available_width(), high), Sense::hover());
    let clip = slot.intersect(ui.clip_rect());
    let layout = egui::Layout::top_down(egui::Align::Min);
    if how > 0.0 {
        let room = Rect::from_min_size(slot.min, vec2(slot.width(), tall));
        let mut inner = ui.new_child(UiBuilder::new().max_rect(room).layout(layout));
        inner.set_clip_rect(clip);
        inner.multiply_opacity(how);
        whole(&mut inner);
    }
    let line = Rect::from_min_size(slot.min, vec2(slot.width(), ROW_H));
    let mut shut = ui.new_child(UiBuilder::new().max_rect(line).layout(layout));
    shut.set_clip_rect(clip);
    shut.multiply_opacity(1.0 - how);
    if summary(&mut shut, line, names) {
        set(ui, true);
    }
}

/// The shut list's one line: the backups in order; true when clicked.
fn summary(ui: &mut Ui, rect: Rect, names: &[String]) -> bool {
    let t = Tokens::get(ui.ctx());
    let r = ui.interact(rect, key().with("summary"), Sense::click());
    let kind = egui::WidgetType::Button;
    r.widget_info(|| egui::WidgetInfo::labeled(kind, true, "Show all backups"));
    let ink = if r.hovered() { t.ink } else { t.muted };
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let words = match names {
        [] => "No backups".to_owned(),
        _ => names.join("  ›  "),
    };
    let font = egui::FontId::proportional(t.text);
    let room = rect.width() - 8.0;
    let mut job = egui::text::LayoutJob::simple_singleline(words, font, ink);
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let galley = ui.painter().layout_job(job);
    let at = pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, ink);
    r.clicked()
}
