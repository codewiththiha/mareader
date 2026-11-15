//! The library's window half: cover textures, grid motion, and view adapters.

mod add;
pub mod asks;
mod copying;
mod covers;
pub mod crumbs;
mod import_sheet;
mod living;
pub(crate) mod menu;
mod menus;
mod remove_alert;
mod review;
pub mod runs;
pub mod search;
pub mod sheets;
pub mod view;

use std::collections::BTreeSet;
use std::path::Path;

use egui::Context;
use mareader_library::{BookId, Library, Options, Pick, Picks, ShelfId};

use crate::ui_command::{Commands, Sent, UiCommand};
pub use add::ADD_MENU;
pub use covers::Covers;
use mareader_engine::library::{
    Crumb, Field, Organizer as EngineOrganizer, Out, Question, Sheet, State,
};
pub use menu::LIBRARY_VIEW_MENU;
pub use menus::{Organizer, organize_rows};

/// The library's window-only state: textures, grid motion, and drag hover.
#[derive(Default)]
pub struct LibraryUi {
    covers: Covers,
    motion: living::Motion,
    /// The shelf under files dragged in from outside, if any.
    pub(crate) file_target: Option<ShelfId>,
}

/// Read-only view of the engine's library state beside its window covers.
pub struct LibraryView<'a> {
    state: &'a State,
    covers: &'a Covers,
}

impl<'a> LibraryView<'a> {
    pub(crate) fn new(state: &'a State, covers: &'a Covers) -> Self {
        Self { state, covers }
    }

    pub fn covers(&self) -> &'a Covers {
        self.covers
    }

    pub fn organizer(&self) -> &'a EngineOrganizer {
        self.state.organizer()
    }

    pub fn sheet(&self) -> Option<&'a Sheet> {
        self.state.sheet()
    }

    pub fn picks(&self) -> &'a Picks {
        self.state.picks()
    }

    pub fn held(&self) -> &'a BTreeSet<BookId> {
        self.state.held()
    }

    pub fn search_text(&self) -> &'a str {
        self.state.search_text()
    }

    pub fn questions(&self) -> std::collections::vec_deque::Iter<'a, Question> {
        self.state.asks.iter()
    }

    pub fn at(&self) -> Option<ShelfId> {
        self.state.at()
    }

    pub fn renaming(&self) -> Option<ShelfId> {
        self.state.renaming()
    }

    pub fn trail(&self) -> Vec<Crumb> {
        self.state.trail()
    }

    pub fn revealed(&self) -> Option<Pick> {
        self.state.revealed()
    }

    pub fn field(&self) -> Field {
        self.state.field()
    }

    pub fn last_options(&self) -> Options {
        self.state.last_options()
    }
}

impl LibraryUi {
    /// Paints `book`'s cover in `rect`; false when it isn't shelved.
    pub fn paint_cover(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        model: &Library,
        book: BookId,
    ) -> bool {
        let Some(found) = model.book(book) else {
            return false;
        };
        let t = crate::theme::Tokens::get(ui.ctx());
        view::thumb(ui, rect, found, &mut self.covers, &t);
        true
    }

    /// Cover decoding runs on the frame when `inline` is true.
    pub fn set_inline(&mut self, inline: bool) {
        self.covers.set_inline(inline);
    }

    pub fn covers(&self) -> &Covers {
        &self.covers
    }

    pub fn covers_mut(&mut self) -> &mut Covers {
        &mut self.covers
    }

    /// Hands the frame's cover asks to their worker.
    pub fn end_frame(&mut self, ctx: &Context, cache: Option<&Path>) {
        self.covers.end_frame(ctx, cache);
    }
}

/// Converts engine library outputs into front-end commands and toasts.
pub(crate) fn push_out(asked: Vec<Out>, out: &mut Commands) {
    for ask in asked {
        match ask {
            Out::Toast(toast) => out.push(Sent::Ui(UiCommand::Notify(toast))),
            Out::Command(command) => out.push(Sent::Engine(command)),
        }
    }
}
