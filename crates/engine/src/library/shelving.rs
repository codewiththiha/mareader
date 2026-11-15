//! Shelves on the page: where the reader is, and what drags and drops do.

use mareader_library::{BookId, Item, Move, ShelfId};

use super::Out;
use super::{Action, Scope, State, wall_ms};
use crate::command::Command;
use crate::toast::Toast;

pub use super::{Carry, NEW_SHELF, ShelfAction};

/// One step of the breadcrumb; none is the library's top level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Crumb {
    pub id: Option<ShelfId>,
    pub name: String,
    /// A watched folder's next walk would make this shelf again.
    pub watched: bool,
}

impl State {
    /// The shelf open on the page; none is the top level.
    pub fn at(&self) -> Option<ShelfId> {
        self.at
    }

    /// The shelf whose name is being edited in the breadcrumb.
    pub fn renaming(&self) -> Option<ShelfId> {
        self.renaming
    }

    /// The shelf the rail's tree just made, once.
    pub fn take_made(&mut self) -> Option<ShelfId> {
        self.made.take()
    }

    /// Home, then each shelf down to the open one.
    pub fn trail(&self) -> Vec<Crumb> {
        let path = self.at.map(|at| self.model.path_to(at)).unwrap_or_default();
        let path = path.into_iter().map(|s| Crumb {
            id: Some(s.id),
            name: s.name.value.clone(),
            watched: self.model.shelf_tracked(s.id),
        });
        let home = Crumb {
            id: None,
            name: "Home".into(),
            watched: false,
        };
        std::iter::once(home).chain(path).collect()
    }

    /// A shelf removed, here or by a sync, gives way to its nearest parent.
    pub fn settle(&mut self) {
        self.settle_picks();
        let model = &self.model;
        self.unfolded.retain(|&id| model.shelf(id).is_some());
        let Some(at) = self.at else {
            return;
        };
        let open = |id: ShelfId| !self.model.path_to(id).is_empty();
        let mut up = Some(at);
        // Bounded: removed shelves can still loop through their parents.
        for _ in 0..64 {
            match up {
                Some(id) if !open(id) => up = self.model.recorded_parent(id),
                _ => break,
            }
        }
        self.at = up.filter(|&id| open(id));
        self.renaming = self.renaming.filter(|&id| self.model.shelf(id).is_some());
    }

    /// Whether `to` is inside the shelf open now.
    fn goes_down(&self, to: Option<ShelfId>) -> bool {
        let Some(to) = to.filter(|&to| Some(to) != self.at) else {
            return false;
        };
        let path = self.model.path_to(to);
        self.at.is_none_or(|at| path.iter().any(|s| s.id == at))
    }

    /// Picks of removed books, shelves and links fall away.
    fn settle_picks(&mut self) {
        if self.picks.is_empty() {
            return;
        }
        let model = &self.model;
        self.picks.retain(|p| match p {
            super::Pick::Book(id) => model.book(id).is_some(),
            super::Pick::Shelf(id) => model.shelf(id).is_some(),
            super::Pick::Link(id) => model.link(id).is_some(),
        });
    }

    /// Opens the shelf at a path of names from the top; false if none.
    pub fn open_shelf(&mut self, path: &str) -> bool {
        let mut at = None;
        for name in path.split('/').filter(|n| !n.is_empty()) {
            let shelves = self.model.children(at).into_iter();
            let mut named = shelves.filter(|s| s.name.value == name);
            match named.next() {
                Some(shelf) => at = Some(shelf.id),
                None => return false,
            }
        }
        self.at = at;
        true
    }

    /// Made-up shelves for shots, over the seeded books.
    pub fn seed_shelves(&mut self) {
        let now = wall_ms();
        let books: Vec<BookId> = self.model.books().map(|b| b.id).collect();
        let mut make = |name: &str, parent| self.model.create_shelf(name, parent, now).ok();
        let fiction = make("Fiction", None);
        let essays = make("Essays", None);
        let classics = make("Classics", fiction);
        make("To read", None);
        let plan = [
            (fiction, [0, 1, 4, 6].as_slice()),
            (classics, &[0, 1]),
            (essays, &[5, 8]),
        ];
        for (shelf, picks) in plan {
            let picked = picks.iter().filter_map(|&n| books.get(n));
            for (shelf, &book) in picked.filter_map(|b| Some((shelf?, b))) {
                self.model.file(book, shelf, now);
            }
        }
    }

    /// Writes a shelf's watch; a new watch looks for books at once.
    fn watch(&mut self, id: ShelfId, on: bool, out: &mut Vec<Out>) {
        let Some(watch) = self.model.shelf_watch(id) else {
            return;
        };
        if !self.model.set_shelf_watch(id, on) {
            return;
        }
        self.settle();
        // The sentence names the ground the decision was about.
        let ground = match &watch.rung {
            Some(rung) => format!("“{rung}” in {}", watch.folder),
            None => watch.folder.clone(),
        };
        let note = match on {
            true => format!("Watching {ground} for new books."),
            false => format!("{ground} is no longer watched for new books."),
        };
        out.push(Out::Toast(Toast::info(note)));
        if on {
            out.push(Out::Command(Command::Library(Action::Rescan)));
        }
    }

    pub fn shelf_act(&mut self, action: ShelfAction, out: &mut Vec<Out>) {
        let Some(action) = self.unheld(action, out) else {
            return;
        };
        let now = wall_ms();
        match action {
            ShelfAction::GoTo(at) => {
                let at = at.filter(|&id| self.model.shelf(id).is_some());
                // Into a shelf the search found: that search is done.
                if self.goes_down(at) {
                    self.search.clear();
                }
                if self.at != at {
                    self.unfolded.clear();
                }
                self.at = at;
                self.scope = Scope::All;
                self.renaming = None;
                self.picks.apply(super::PickAction::Clear);
            }
            ShelfAction::New => {
                if let Ok(id) = self.model.create_shelf(NEW_SHELF, self.at, now) {
                    self.search.clear();
                    self.at = Some(id);
                    self.scope = Scope::All;
                }
            }
            ShelfAction::NewIn(parent) => {
                let parent = parent.filter(|&id| self.model.shelf(id).is_some());
                self.made = self.model.create_shelf(NEW_SHELF, parent, now).ok();
            }
            ShelfAction::StartRename(id) => {
                self.renaming = self.model.shelf(id).map(|s| s.id);
            }
            ShelfAction::CancelRename => self.renaming = None,
            ShelfAction::Apart(id) => self.apart(id, out),
            ShelfAction::Carry(mv) => self.carry(mv, out),
            ShelfAction::Copy(answer) => self.answer_copy(answer, out),
            ShelfAction::Watch(id, on) => self.watch(id, on, out),
            ShelfAction::Unfold(id) => {
                if !self.unfolded.remove(&id) {
                    self.unfolded.insert(id);
                }
            }
            ShelfAction::Reveal(_) => {}
            ShelfAction::Stack {
                onto,
                carry,
                level,
                keep,
            } => self.stack(onto, &carry, level, keep, out),
            ShelfAction::Arrange {
                items,
                level,
                before,
            } => {
                self.model.arrange(level, &items, before, now);
                if items.len() > 1 {
                    self.picks.apply(super::PickAction::Clear);
                }
            }
        }
    }

    /// Makes a shelf where `onto` sits on `level`, and carries both onto it.
    fn stack(
        &mut self,
        onto: BookId,
        carry: &Carry,
        level: Option<ShelfId>,
        keep: bool,
        out: &mut Vec<Out>,
    ) {
        let now = wall_ms();
        let Move {
            books,
            shelves,
            links,
            ..
        } = Move::of(&carry.items);
        let books: Vec<BookId> = (books.into_iter()).filter(|&b| b != onto).collect();
        if books.is_empty() || self.model.book(onto).is_none() {
            return;
        }
        let name = self.stack_name(onto, &books);
        let Ok(id) = self.model.create_shelf(&name, level, now) else {
            return;
        };
        self.picks.apply(super::PickAction::Clear);
        let (shelf, book) = (Item::Shelf(id), Item::Book(onto));
        self.model.arrange(level, &[shelf], Some(book), now);
        let mv = Move {
            books: std::iter::once(onto).chain(books).collect(),
            shelves,
            links,
            from: level,
            to: Some(id),
            keep,
            before: None,
        };
        self.carry(mv, out);
    }

    /// The books' one shared author, else "New shelf".
    fn stack_name(&self, onto: BookId, books: &[BookId]) -> String {
        let author = |b: &BookId| self.model.book(*b).and_then(|b| b.author.value.clone());
        let first = author(&onto).filter(|a| !a.trim().is_empty());
        let shared = first.filter(|a| books.iter().all(|b| author(b).as_ref() == Some(a)));
        shared.unwrap_or_else(|| NEW_SHELF.to_owned())
    }

    pub fn shelf_name(&self, id: ShelfId) -> String {
        let shelf = self.model.shelf(id).map(|s| s.name.value.clone());
        shelf.unwrap_or_default()
    }
}
