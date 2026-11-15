//! Duplicates: the library's own copies of books, shelves and links.

use std::collections::HashMap;
use std::path::PathBuf;

use mareader_library::{BookId, Copied, Found, Library, LinkTo};

use super::Out;
use super::{Pick, State, wall_ms};
use crate::toast::Toast;

impl State {
    /// Copies the books the picks want, then duplicates them.
    pub fn duplicate(&mut self, picks: Vec<Pick>, out: &mut Vec<Out>) {
        let mut files: Vec<Found> = Vec::new();
        for book in picks.iter().flat_map(|p| self.wants(*p)) {
            let found = self.found_of(book);
            if let Some(found) = found.filter(|f| files.iter().all(|g| g.path != f.path)) {
                files.push(found);
            }
        }
        match files.is_empty() {
            // Shelf links and empty shelves copy no bytes.
            true => self.duplicated(&picks, &HashMap::new(), out),
            false => self.copy_to_duplicate(picks, files, out),
        }
    }

    /// The books one pick's duplicate copies; missing ones stay out.
    fn wants(&self, pick: Pick) -> Vec<BookId> {
        let here = |id: BookId| (self.model.book(id)).is_some_and(|b| !b.missing());
        match pick {
            Pick::Book(id) => [id].into_iter().filter(|b| here(*b)).collect(),
            Pick::Shelf(id) => self.model.duplicate_wants(id),
            Pick::Link(id) => match self.model.link(id).map(|l| l.target) {
                Some(LinkTo::Book(b)) => [b].into_iter().filter(|b| here(*b)).collect(),
                _ => Vec::new(),
            },
        }
    }

    /// Makes the duplicates from the copies made, then tells of them.
    pub fn duplicated(
        &mut self,
        picks: &[Pick],
        copies: &HashMap<PathBuf, Copied>,
        out: &mut Vec<Out>,
    ) {
        let wall = wall_ms();
        let mut landed: Vec<(String, bool)> = Vec::new();
        for pick in picks {
            let made = match *pick {
                Pick::Book(id) => copy_of(&self.model, copies, id)
                    .and_then(|copy| self.model.duplicate_book(id, &copy, wall))
                    .and_then(|new| title(&self.model, new)),
                Pick::Shelf(id) => {
                    let wants = self.model.duplicate_wants(id);
                    let copies: HashMap<BookId, Copied> = (wants.into_iter())
                        .filter_map(|b| copy_of(&self.model, copies, b).map(|c| (b, c)))
                        .collect();
                    let done = self.model.duplicate_shelf(id, &copies, wall);
                    let shelf = done.shelf.and_then(|s| self.model.shelf(s));
                    let name = shelf.map(|s| s.name.value.clone());
                    landed.extend(name.map(|n| (n, true)));
                    continue;
                }
                Pick::Link(id) => self.duplicate_link(id, copies, wall),
            };
            landed.extend(made.map(|n| (n, false)));
        }
        if let Some(note) = report(&landed) {
            self.settle();
            out.push(Out::Toast(Toast::info(note)));
        }
    }

    /// A book's link copies its book where the link is; a shelf's, itself.
    fn duplicate_link(
        &mut self,
        id: mareader_library::LinkId,
        copies: &HashMap<PathBuf, Copied>,
        wall: u64,
    ) -> Option<String> {
        let link = self.model.link(id)?;
        let (target, level) = (link.target, link.level.value);
        match target {
            LinkTo::Shelf(_) => {
                let made = self.model.duplicate_link(id, wall)?;
                self.model.link(made).map(|l| l.name.value.clone())
            }
            LinkTo::Book(book) => {
                let copy = copy_of(&self.model, copies, book)?;
                let new = self.model.duplicate_book_on(book, &copy, level, wall)?;
                title(&self.model, new)
            }
        }
    }
}

/// The copy made of a book's file, keyed as the copier was handed it.
fn copy_of(model: &Library, copies: &HashMap<PathBuf, Copied>, id: BookId) -> Option<Copied> {
    let home = model.book(id)?.home()?;
    copies.get(&home.path).cloned()
}

fn title(model: &Library, id: BookId) -> Option<String> {
    model.book(id).map(|b| b.title.value.clone())
}

/// "Duplicated as “Dune_1”.", or how many of which kind.
fn report(landed: &[(String, bool)]) -> Option<String> {
    if let [(name, _)] = landed {
        return Some(format!("Duplicated as “{name}”."));
    }
    let shelves = landed.iter().filter(|(_, shelf)| *shelf).count();
    let noun = match (landed.len() - shelves, shelves) {
        (0, 0) => return None,
        (_, 0) => "books",
        (0, _) => "shelves",
        _ => "shelves and books",
    };
    Some(format!("Duplicated {} {noun}.", landed.len()))
}
