//! Selection and the confirmation for removing studied occurrences.

use std::collections::BTreeMap;

use egui::{Context, RichText, Ui};
use mareader_engine::command::Command;
use mareader_engine::space::{self, Action, Row, Scope};
use mareader_library::Library;

use crate::icons::Icon;
use crate::modal::{self, Dialog};
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;

#[derive(Default)]
pub(super) struct Selection {
    selected: BTreeMap<u128, Row>,
    visible: Vec<Row>,
    all: bool,
    delete: Option<Delete>,
}

struct Delete {
    rows: Vec<Row>,
    merged: bool,
    ai: bool,
    sure: bool,
}

impl Selection {
    pub fn begin(&mut self) {
        self.visible.clear();
        self.all = false;
    }

    pub fn visible(&mut self, row: &Row) {
        self.visible.push(row.clone());
    }

    pub fn finish(&mut self) {
        if self.all {
            for row in &self.visible {
                self.selected.insert(row.key, row.clone());
            }
        }
    }

    pub fn is_selected(&self, row: &Row) -> bool {
        self.selected.contains_key(&row.key)
    }

    pub fn active(&self) -> bool {
        !self.selected.is_empty()
    }

    pub fn toggle(&mut self, row: &Row) {
        if self.selected.remove(&row.key).is_none() {
            self.selected.insert(row.key, row.clone());
        }
    }

    pub fn ask_delete(&mut self, row: &Row, scope: Scope) {
        let rows = if self.is_selected(row) {
            self.selected.values().cloned().collect()
        } else {
            vec![row.clone()]
        };
        self.delete = Some(Delete { rows, merged: scope == Scope::Merged, ai: false, sure: false });
    }

    pub fn menu(&mut self, ui: &Ui, hit: &egui::Response, row: &Row, scope: Scope, out: &mut Commands) {
        widgets::context_menu(hit, |ui| {
            let can_open = !row.book_ids.is_empty();
            ui.add_enabled_ui(can_open, |ui| {
                if widgets::menu_row(ui, Icon::Book, "Go to word in book", None, false).clicked() {
                    if let Some(book) = row.book_ids.first().copied() {
                        let quote = row.met_at.as_ref().filter(|at| at.book == book)
                            .map_or_else(|| row.form.clone(), |at| at.quote.clone());
                        out.push(Sent::Ui(UiCommand::FindVocabInBook { book, quote, row: row.key }));
                    }
                }
            });
            let label = if self.is_selected(row) { "Deselect" } else { "Select" };
            if widgets::menu_row(ui, Icon::Check, label, None, false).clicked() { self.toggle(row); }
            if widgets::menu_row(ui, Icon::List, "Select all visible", None, false).clicked() { self.all = true; }
            if self.active() && widgets::menu_row(ui, Icon::Close, "Clear selection", None, false).clicked() { self.selected.clear(); }
            widgets::menu_rule(ui);
            if widgets::menu_row_danger(ui, Icon::Remove, "Delete…").clicked() { self.ask_delete(row, scope); }
        });
        let _ = ui;
    }

    pub fn toolbar(&mut self, ui: &mut Ui, scope: Scope, t: &Tokens) {
        if self.selected.is_empty() { return; }
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} selected", self.selected.len())).color(t.ink));
            if ui.button("Clear selection").clicked() { self.selected.clear(); }
            if ui.button("Delete selected…").clicked() {
                let rows = self.selected.values().cloned().collect();
                self.delete = Some(Delete { rows, merged: scope == Scope::Merged, ai: false, sure: false });
            }
        });
    }

    pub fn dialog(&mut self, ctx: &Context, lib: &Library, t: &Tokens, out: &mut Commands) -> Option<Vec<(String, Option<String>)>> {
        let Some(delete) = &mut self.delete else { return None; };
        let mut cancel = false;
        let mut confirm = false;
        let shown = Dialog::new("space-delete-words").show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.0).clamp(180.0, 390.0));
            modal::heading(ui, Some((Icon::Alert, t.danger)), "Delete studied words?", None, t);
            ui.add_space(10.0);
            let scope = if delete.merged { "all linked books" } else { "the selected books" };
            ui.label(format!("Remove {} item(s) and their learning history from {scope}. This cannot be undone.", delete.rows.len()));
            ui.add_space(8.0);
            if ui.checkbox(&mut delete.ai, "Also delete from AI dictionary").changed() { delete.sure = false; }
            ui.label(RichText::new("AI meanings in all languages for these words will also be removed if checked.").size(t.text_small).color(t.muted));
            ui.add_space(12.0);
            modal::buttons(ui, |ui| {
                let label = if delete.sure { "Are you sure?" } else { "Delete" };
                if modal::primary(ui, label, t.danger, t).clicked() {
                    if delete.sure { confirm = true; } else { delete.sure = true; }
                }
                if modal::quiet(ui, "Cancel", t).clicked() { cancel = true; }
            });
        });
        if shown.should_close() { cancel = true; }
        if cancel { self.delete = None; return None; }
        if !confirm { return None; }
        let delete = self.delete.take()?;
        let mut targets = Vec::new();
        for row in &delete.rows {
            let ids = if delete.merged { space::detail_row(lib, row.clone()).book_ids } else { row.book_ids.clone() };
            for book in ids {
                if !targets.contains(&(book, row.word)) { targets.push((book, row.word)); }
            }
        }
        out.push(Sent::Engine(Command::Space(Action::RemoveOccurrences(targets))));
        self.selected.clear();
        delete.ai.then(|| delete.rows.into_iter().map(|r| (r.lemma, r.pos)).collect())
    }
}
