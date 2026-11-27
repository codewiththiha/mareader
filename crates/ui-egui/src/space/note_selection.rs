//! Selection and confirmation for Space's notes and bookmarks.

use std::collections::BTreeSet;
use egui::{Context, RichText, Ui};
use mareader_engine::command::Command;
use mareader_engine::study::Action;
use mareader_library::MarkId;
use crate::icons::Icon;
use crate::modal::{self, Dialog};
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

#[derive(Default)]
pub(super) struct Selection {
    picked: BTreeSet<MarkId>,
    visible: Vec<MarkId>,
    all: bool,
    delete: Option<(Vec<MarkId>, bool)>,
}

impl Selection {
    pub fn begin(&mut self) { self.visible.clear(); self.all = false; }
    pub fn visible(&mut self, id: MarkId) { self.visible.push(id); }
    pub fn finish(&mut self) { if self.all { self.picked.extend(self.visible.iter().copied()); } }
    pub fn selected(&self, id: MarkId) -> bool { self.picked.contains(&id) }
    pub fn active(&self) -> bool { !self.picked.is_empty() }
    pub fn toggle(&mut self, id: MarkId) {
        if !self.picked.remove(&id) { self.picked.insert(id); }
    }
    fn ask(&mut self, id: MarkId) {
        let ids = if self.selected(id) { self.picked.iter().copied().collect() } else { vec![id] };
        self.delete = Some((ids, false));
    }
    pub fn menu(&mut self, hit: &egui::Response, id: MarkId, out: &mut Commands) {
        widgets::context_menu(hit, |ui| {
            if widgets::menu_row(ui, Icon::Book, "Go to note in book", None, false).clicked() {
                out.push(Sent::Engine(Command::OpenMark(id)));
            }
            let label = if self.selected(id) { "Deselect" } else { "Select" };
            if widgets::menu_row(ui, Icon::Check, label, None, false).clicked() { self.toggle(id); }
            if widgets::menu_row(ui, Icon::List, "Select all visible", None, false).clicked() { self.all = true; }
            if self.active() && widgets::menu_row(ui, Icon::Close, "Clear selection", None, false).clicked() { self.picked.clear(); }
            widgets::menu_rule(ui);
            if widgets::menu_row_danger(ui, Icon::Remove, "Delete…").clicked() { self.ask(id); }
        });
    }
    pub fn toolbar(&mut self, ui: &mut Ui, t: &Tokens) {
        if self.picked.is_empty() { return; }
        egui::Frame::new().fill(t.accent_soft).corner_radius(t.radius)
            .inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{} selected", self.picked.len())).color(t.ink));
                    if super::look::text_button(ui, (Icon::Close, "Clear"), "Clear selection", t.ink).clicked() { self.picked.clear(); }
                    if super::look::text_button(ui, (Icon::Remove, "Delete selected"), "Delete selected…", t.danger).clicked() {
                        self.delete = Some((self.picked.iter().copied().collect(), false));
                    }
                });
            });
    }
    pub fn dialog(&mut self, ctx: &Context, t: &Tokens, out: &mut Commands) {
        let Some((ids, sure)) = &mut self.delete else { return; };
        let mut confirm = false;
        let mut cancel = false;
        let shown = Dialog::new("space-delete-notes").show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(180.0, 390.0));
            modal::heading(ui, Some((Icon::Alert, t.danger)), "Delete selected notes?", None, t);
            ui.add_space(10.0);
            ui.label(format!("Remove {} selected highlights, notes or bookmarks from their books?", ids.len()));
            ui.add_space(12.0);
            modal::buttons(ui, |ui| {
                if modal::primary(ui, if *sure { "Are you sure?" } else { "Delete" }, t.danger, t).clicked() {
                    if *sure { confirm = true; } else { *sure = true; }
                }
                if modal::quiet(ui, "Cancel", t).clicked() { cancel = true; }
            });
        });
        if shown.should_close() { cancel = true; }
        if confirm {
            out.push(Sent::Engine(Command::Study(Action::Remove(ids.clone()))));
            self.picked.clear();
        }
        if confirm || cancel { self.delete = None; }
    }
}
