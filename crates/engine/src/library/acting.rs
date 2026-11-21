//! What the page asked for: one router for a menu, a key, a sheet or a drop.

use mareader_library::{BookId, ShelfId};

use super::{Action, BookAction, Named, Out, Pick, PickAction, Sheet, State, wall_ms};
use crate::toast::Toast;

impl State {
    /// Does what the page asked; dialogs are the app's.
    pub fn act(&mut self, action: Action, now: f64, out: &mut Vec<Out>) {
        match action {
            Action::AskFolder(root) => self.ask_folder(root),
            Action::ImportFolder { root, opts } => self.import_folder(root, opts, now, out),
            Action::Rescan => self.rescan(now),
            Action::Answer(answer, all) => self.answer(answer, all, now, out),
            Action::CancelAsks => self.asks.clear(),
            Action::Restore { folder, content } => self.restore(folder, content, out),
            Action::AskMoved { folder, book } => {
                self.sheet = Some(Sheet::Moved { folder, book });
            }
            Action::ShowMoved { folder, book } => self.show_moved(folder, book, now),
            Action::Reveal(pick) => {
                self.close_sheet();
                self.reveal(pick, now);
            }
            Action::Scope(scope) => {
                self.scope = scope;
                self.picks.apply(PickAction::Clear);
            }
            Action::FindAgain(_)
            | Action::Relink { .. }
            | Action::UseAnyway(_)
            | Action::Seek { .. } => self.find_act(action, out),
            Action::CloseSheet => {
                // A note lights its shelf however it's closed.
                let lit = match &self.sheet {
                    Some(Sheet::Note { shelf, .. }) => Some(*shelf),
                    _ => None,
                };
                self.close_sheet();
                if let Some(shelf) = lit {
                    self.reveal(Pick::Shelf(shelf), now);
                }
            }
            Action::AskRename(named) => self.ask_rename(named),
            Action::Rename(named, name) => self.rename(named, &name),
            Action::Design(book, design) => {
                self.model.set_design(book, design, wall_ms());
            }
            Action::Shelf(action) => self.shelf_act(action, out),
            Action::Pick(action) => self.picks.apply(action),
            Action::Book(action) => self.book_act(action, out),
            // A key's ask, answered against the shelf as it stands.
            Action::FindAgainLost
            | Action::AskRenameLast
            | Action::PickRecent(_)
            | Action::AskRemoveRecent(_)
            | Action::AskRemoveShelf(_) => self.key_ask(action, out),
            Action::Search(action) => self.search_act(action, now),
            Action::ApplyOrganized => self.apply_organized(out),
            Action::DiscardOrganized | Action::StopOrganizing => self.organizer_end(),
            Action::UndoOrganized(done) => self.undo_organized(&done),
            // A fixture's shelf, filled as a shot asked for it.
            Action::Seed(_)
            | Action::SeedShelves
            | Action::SeedCopies(_)
            | Action::SeedWords
            | Action::OrganizeDemo(_)
            | Action::OrganizePaused(_)
            | Action::RunsDemo
            | Action::OpenShelf(_) => self.fixture(action, out),
            // Popups need the front end, and the AI its service.
            Action::Organize(_)
            | Action::OrganizeAnew(_)
            | Action::ResumeOrganizing
            | Action::DiscardPaused
            | Action::RestorePaused
            | Action::ShowAdd => {}
        }
    }

    /// What a key asked the shelf for, once the asks before it have landed.
    fn key_ask(&mut self, action: Action, out: &mut Vec<Out>) {
        match action {
            Action::FindAgainLost => match self.first_missing() {
                Some(book) => self.find_act(Action::FindAgain(book), out),
                None => out.push(Out::Toast(Toast::info("No book is missing."))),
            },
            Action::AskRenameLast => match self.last_read() {
                Some(book) => self.ask_rename(Named::Book(book)),
                None => out.push(Out::Toast(Toast::info("No book read yet."))),
            },
            Action::PickRecent(count) => {
                let recent = self.model().recent().into_iter().take(count);
                let picks = recent.map(|b| Pick::Book(b.id)).collect();
                self.picks.apply(PickAction::Only(picks));
            }
            Action::AskRemoveRecent(count) => {
                let recent = self.model().recent().into_iter().take(count);
                let books = recent.map(|b| b.id).collect();
                self.ask_remove(books, Vec::new(), out);
            }
            Action::AskRemoveShelf(name) => {
                let found = self.model().shelves().find(|s| s.name.value == name);
                let shelf = found.map(|s| s.id);
                match shelf {
                    Some(shelf) => self.ask_remove(Vec::new(), vec![shelf], out),
                    None => out.push(Out::Toast(Toast::info("No shelf by that name."))),
                }
            }
            _ => {}
        }
    }

    /// A fixture's shelf: the books, shelves and runs a shot asks for.
    fn fixture(&mut self, action: Action, out: &mut Vec<Out>) {
        match action {
            Action::Seed(count) => self.seed(count),
            Action::SeedShelves => self.seed_shelves(),
            Action::SeedCopies(count) => self.seed_copies(count),
            Action::SeedWords => {
                crate::space::seed(&mut self.model, wall_ms());
            }
            Action::OrganizeDemo(review) => self.organize_demo(review),
            Action::OrganizePaused(ready) => self.organize_paused_demo(ready),
            Action::RunsDemo => self.runs.demo(),
            Action::OpenShelf(path) => self.open_shelf_or_note(path, out),
            _ => {}
        }
    }

    /// A fixture's shelf, or a note saying the path names none.
    fn open_shelf_or_note(&mut self, path: String, out: &mut Vec<Out>) {
        let found = self.open_shelf(&path);
        if !found {
            out.push(Out::Toast(Toast::info("No shelf by that path.")));
        }
    }

    /// The rename sheet, when there is a name to show in it.
    fn ask_rename(&mut self, named: Named) {
        if self.name_of(named).is_some() {
            self.sheet = Some(Sheet::Rename(named));
        }
    }

    /// The removal sheet, told what goes and what stays.
    fn ask_remove(&mut self, books: Vec<BookId>, shelves: Vec<ShelfId>, out: &mut Vec<Out>) {
        let ask = BookAction::AskRemove {
            books,
            shelves,
            links: Vec::new(),
        };
        self.book_act(ask, out);
    }

    /// The book read last, if the shelf has one.
    fn last_read(&self) -> Option<BookId> {
        self.model().recent().first().map(|b| b.id)
    }

    /// The first book whose file has gone, if the shelf has one.
    fn first_missing(&self) -> Option<BookId> {
        self.model().books().find(|b| b.missing()).map(|b| b.id)
    }
}
