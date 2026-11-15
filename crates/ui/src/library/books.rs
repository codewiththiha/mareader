//! Book work from menus, the selection bar and drops: removal,
//! twins and imports.

use std::path::PathBuf;

use mareader_library::{BookId, LinkId, Move, ShelfId};

use super::{LibraryState, Pick, Sheet, wall_ms};
use crate::command::{Command, Commands};
use crate::toast::Toast;

/// What menus, the selection bar and drops ask of books.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookAction {
    /// Asks first, telling what goes and what stays.
    AskRemove {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        links: Vec<LinkId>,
    },
    /// `cascade` takes what's inside the shelves; `forget`, marks and places.
    Remove {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        links: Vec<LinkId>,
        cascade: bool,
        forget: bool,
    },
    /// A separate book of the same file, with its own place.
    Twin(BookId),
    /// Shows its file in the system's file manager.
    Reveal(BookId),
    /// Shows the file dialog; the picks come back as `Import`.
    PickImport,
    /// Copies in files onto a shelf if `to`; a folder asks how.
    Import {
        paths: Vec<PathBuf>,
        to: Option<ShelfId>,
    },
    /// A new shelf where the reader is, holding these.
    ShelveTogether(Vec<Pick>),
    /// The library's own copies of these, filed beside them.
    Duplicate(Vec<Pick>),
}

impl LibraryState {
    pub(super) fn book_act(&mut self, action: BookAction, out: &mut Commands) {
        let now = wall_ms();
        match action {
            BookAction::AskRemove {
                books,
                shelves,
                links,
            } => {
                let asked = Sheet::Removal {
                    books,
                    shelves,
                    links,
                };
                self.folders.sheet = Some(asked);
            }
            BookAction::Remove {
                books,
                shelves,
                links,
                cascade,
                forget,
            } => {
                self.remove_links(&links, books.is_empty() && shelves.is_empty(), out);
                self.remove_entries((&books, &shelves), cascade, forget, out);
            }
            BookAction::Twin(id) => {
                let Some(twin) = self.model.twin(id, now) else {
                    return;
                };
                let title = |id| self.model.book(id).map(|b| b.title.value.clone());
                let (made, from) = (title(twin), title(id));
                let note = format!(
                    "“{}” reads separately from “{}”.",
                    made.unwrap_or_default(),
                    from.unwrap_or_default()
                );
                out.push(Command::Notify(Toast::info(note)));
            }
            BookAction::Import { paths, to } => self.import_files(paths, to, out),
            BookAction::ShelveTogether(picks) => {
                let picks = self.unheld_picks(picks, out);
                if !picks.is_empty() {
                    self.shelve_together(&picks, out);
                }
            }
            BookAction::Duplicate(picks) => self.duplicate(picks, out),
            // Dialogs and the file manager are the app's.
            BookAction::Reveal(_) | BookAction::PickImport => {}
        }
    }

    /// Links go at once: what they point at stays. `alone` tells of it.
    fn remove_links(&mut self, links: &[LinkId], alone: bool, out: &mut Commands) {
        let first = links.first().and_then(|l| self.model.link(*l));
        let first = first.map(|l| l.name.value.clone()).unwrap_or_default();
        let gone = self.model.remove_links(links, wall_ms());
        if !alone || gone == 0 {
            return;
        }
        self.folders.sheet = None;
        self.picks.apply(super::PickAction::Clear);
        let note = match gone {
            1 => format!("Removed the link “{first}”."),
            n => format!("Removed {n} links."),
        };
        out.push(Command::Notify(Toast::info(note)));
    }

    /// A new shelf where the reader is, the picks added to it.
    fn shelve_together(&mut self, picks: &[Pick], out: &mut Commands) {
        let name = super::shelving::NEW_SHELF;
        let Ok(id) = self.model.create_shelf(name, self.at, wall_ms()) else {
            return;
        };
        let mv = Move {
            from: self.at,
            to: Some(id),
            keep: true,
            ..Move::of(picks)
        };
        self.carry(mv, out);
    }
}

pub use super::counted::counted;
