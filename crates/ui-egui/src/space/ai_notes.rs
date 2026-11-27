//! Saved answers on selected passages, under the regular notes.

use egui::{Frame, Margin, RichText, Sense, Stroke, Ui};
use mareader_engine::space::AiNoteGroup;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use super::note_selection::{Item, Selection};

pub(super) fn show(ui: &mut Ui, groups: &[AiNoteGroup], selection: &mut Selection,
    reveal: &mut Option<(u128, bool, f64)>, t: &Tokens, out: &mut Commands) {
    if groups.is_empty() { return; }
    ui.add_space(8.0);
    ui.label(RichText::new("AI selection results").strong().size(t.text + 3.0).color(t.ink));
    for group in groups {
        ui.label(RichText::new(&group.title).strong().color(t.ink));
        for row in &group.rows {
            let id = Item::Ai(row.id);
            let frame = Frame::new().fill(if selection.selected(id) { t.accent_soft } else { t.card })
                .stroke(Stroke::new(1.0, t.line)).corner_radius(12.0)
                .inner_margin(Margin::same(14)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&row.name).strong().color(t.ink));
                    let source = egui::Button::new(RichText::new(&row.source).color(t.accent))
                        .frame(false).wrap();
                    if ui.add(source).on_hover_text("Go to this selection in the book").clicked() {
                        out.push(Sent::Ui(UiCommand::OpenSpaceAi { book: group.book, id: row.id }));
                    }
                    egui::ScrollArea::vertical().id_salt(("space-ai-answer", row.id.0))
                        .max_height(150.0).auto_shrink([false, true]).show(ui, |ui| {
                            ui.add(egui::Label::new(&row.result).wrap().selectable(true));
                        });
                });
            let rect = frame.response.rect;
            let hit = ui.interact(rect, ui.id().with(("space-ai-row", row.id.0)), Sense::click());
            if rect.intersect(ui.clip_rect()).is_positive() { selection.visible(id); }
            selection.menu(&hit, id, group.book, out);
            if hit.clicked() && selection.active() { selection.toggle(id); }
            if let Some((key, pending, _)) = reveal && *key == row.id.0 && *pending {
                ui.scroll_to_rect(rect, Some(egui::Align::Center));
                *pending = false;
            }
            super::locateglow::draw(ui, rect, row.id.0, reveal, t);
        }
    }
}
