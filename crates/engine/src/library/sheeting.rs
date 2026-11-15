//! The sheet over the page: whether one holds the keys, and what a rename does.

use super::{Named, Sheet, State, wall_ms};

impl State {
    /// The sheet that's up, if one is.
    pub fn sheet(&self) -> Option<&Sheet> {
        self.sheet.as_ref()
    }

    /// A sheet or a question is over the page, holding the keys.
    pub fn sheet_up(&self) -> bool {
        let review = self.organizer.review().is_some();
        self.sheet.is_some() || !self.asks.is_empty() || review
    }

    /// The name a book, shelf or link shows now; none once it's gone.
    pub fn name_of(&self, named: Named) -> Option<String> {
        let model = &self.model;
        match named {
            Named::Book(id) => model.book(id).map(|b| b.title.value.clone()),
            Named::Shelf(id) => model.shelf(id).map(|s| s.name.value.clone()),
            Named::Link(id) => model.link(id).map(|l| l.name.value.clone()),
        }
    }

    /// Gives it the typed name; a blank one keeps the old.
    pub fn rename(&mut self, named: Named, name: &str) {
        if matches!(self.sheet, Some(Sheet::Rename(_))) {
            self.close_sheet();
        }
        self.renaming = None;
        let now = wall_ms();
        match named {
            Named::Book(id) => {
                self.model.rename_book(id, name, now);
            }
            Named::Shelf(id) => {
                self.model.rename_shelf(id, name, now).ok();
            }
            Named::Link(id) => {
                self.model.rename_link(id, name, now);
            }
        }
    }

    /// Drops the sheet asking to replace the paused run, if it's up.
    pub fn close_replace(&mut self) {
        if matches!(self.sheet, Some(Sheet::Replace(_))) {
            self.close_sheet();
        }
    }
}
