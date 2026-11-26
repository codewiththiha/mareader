//! Bulk removal from the current library page.

use std::collections::BTreeSet;

use egui::Ui;
use mareader_engine::command::Command;
use mareader_engine::library::{Action, BookAction, offline};
use mareader_library::{BookId, FolderId, Library, Pick, ShelfId};

use crate::icons::Icon;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

pub(super) fn books(
    model: &Library,
    sequence: &[Pick],
    offline_folders: &BTreeSet<FolderId>,
    searching: bool,
    at: Option<ShelfId>,
) -> Vec<BookId> {
    if searching {
        return Vec::new();
    }
    sequence
        .iter()
        .filter_map(|pick| match pick {
            Pick::Book(id) => model.book(*id),
            _ => None,
        })
        .filter(|book| book.missing() && !offline(book, offline_folders))
        .filter(|book| at.is_some() || model.shelves_of(book.id).is_empty())
        .map(|book| book.id)
        .collect()
}

pub(super) fn menu_row(ui: &mut Ui, missing: &[BookId], out: &mut Commands) {
    if missing.len() > 1 {
        widgets::menu_rule(ui);
        let row = widgets::menu_row_danger(ui, Icon::Remove, "Remove all missing books…")
            .on_hover_text("In this shelf, or unshelved at library home");
        if row.clicked() {
            let action = Action::Book(BookAction::AskRemoveMissing(missing.to_vec()));
            out.push(Sent::Engine(Command::Library(action)));
            ui.close();
        }
    }
}
