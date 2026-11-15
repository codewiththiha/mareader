//! Loose books an AI files on shelves: what it's asked, and its plan.

use std::collections::BTreeSet;

use mareader_library::{Book, BookId, Library, Origin, ShelfId};

use super::{Applied, RunId, counted};
use crate::toast::Toast;

/// A book the model may file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loose {
    pub id: BookId,
    pub title: String,
    pub author: Option<String>,
}

/// A shelf there already, which the model may file onto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
    pub id: ShelfId,
    pub name: String,
    pub parent: Option<ShelfId>,
}

/// What the model is asked to organize.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub books: Vec<Loose>,
    pub shelves: Vec<Standing>,
    /// Every loose book, so books loose later join a resumed run.
    pub whole: bool,
}

/// A run a failure, a stop or quitting left, kept to carry on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paused {
    pub done: usize,
    pub total: usize,
    pub books: BTreeSet<BookId>,
    pub whole: bool,
    /// Every step is done: only its plan waits to be reviewed.
    pub ready: bool,
}

impl Paused {
    /// How far it got, for menus and its card.
    pub fn said(&self) -> String {
        let books = counted(self.books.len(), "book", "books");
        match self.ready {
            true => format!("{books} · plan ready"),
            false => format!("{} of {} steps · {books}", self.done, self.total),
        }
    }

    /// How far it got, in a few words for a menu row.
    pub fn short(&self) -> String {
        match self.ready {
            true => "plan ready".to_owned(),
            false => format!("{} of {}", self.done, self.total),
        }
    }
}

/// A shelf in a plan: one there already, or the plan's `n`th new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Spot {
    Shelf(ShelfId),
    New(usize),
}

/// The model's plan; a new shelf's parent always comes before it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proposal {
    pub shelves: Vec<(String, Option<Spot>)>,
    pub placed: Vec<(BookId, Spot)>,
    pub left: Vec<BookId>,
}

/// Where a plan puts one book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    /// Filed on the shelf of this name, new or standing.
    Shelf(String),
    /// Left where it is.
    Stays,
}

impl Proposal {
    /// Where this plan puts `book`, if the plan names it.
    pub fn bound(&self, model: &Library, book: BookId) -> Option<Bound> {
        if self.left.contains(&book) {
            return Some(Bound::Stays);
        }
        let (_, spot) = self.placed.iter().find(|(b, _)| *b == book)?;
        let name = match *spot {
            Spot::Shelf(id) => model.shelf(id)?.name.value.clone(),
            Spot::New(i) => self.shelves.get(i)?.0.clone(),
        };
        Some(Bound::Shelf(name))
    }
}

/// News from the model, as the library takes it.
#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    /// Requests done of `total`; `filing` once shelves are planned.
    Progress {
        filing: bool,
        done: usize,
        total: usize,
    },
    Plan(Proposal),
    Failed(Toast),
}

/// A run, then its plan under review; its books stay put meanwhile.
#[derive(Debug, Default)]
pub struct Organizer {
    pub run: Option<RunId>,
    pub review: Option<Box<Proposal>>,
    /// A stopped run, its books free until it resumes.
    pub paused: Option<Paused>,
    pub paused_card: Option<RunId>,
}

impl Organizer {
    /// A run is going or its plan waits: no second one starts.
    pub fn busy(&self, held: &BTreeSet<BookId>) -> bool {
        !held.is_empty()
    }

    pub fn review(&self) -> Option<&Proposal> {
        self.review.as_deref()
    }

    pub fn paused(&self) -> Option<&Paused> {
        self.paused.as_ref()
    }
}

/// Whether the AI may file it: an imported copy, present, on no shelf.
pub fn organizable(model: &Library, book: &Book) -> bool {
    let stored = book.home().is_some_and(|l| l.origin == Origin::Store);
    book.alive() && stored && !book.missing() && model.shelves_of(book.id).is_empty()
}

/// Makes the plan's shelves and files its books that are still loose.
pub fn apply(model: &mut Library, plan: &Proposal, wall_ms: u64) -> Applied {
    let mut made: Vec<Option<ShelfId>> = Vec::with_capacity(plan.shelves.len());
    for (name, parent) in &plan.shelves {
        let parent = match parent {
            None => Some(None),
            Some(spot) => locate(model, &made, *spot).map(Some),
        };
        let id = parent.and_then(|p| model.create_shelf(name, p, wall_ms).ok());
        made.push(id);
    }
    let mut done = Applied::default();
    for (book, spot) in &plan.placed {
        let loose = model.book(*book).is_some_and(|b| organizable(model, b));
        let shelf = locate(model, &made, *spot).filter(|_| loose);
        match shelf {
            Some(shelf) if model.file(*book, shelf, wall_ms) => done.filed.push((*book, shelf)),
            _ => done.skipped += 1,
        }
    }
    done.made = made.into_iter().flatten().collect();
    // Shelves whose books all changed meanwhile aren't left empty.
    let emptied = prune(model, &done.made, wall_ms);
    done.made.retain(|s| !emptied.contains(s));
    done
}

/// Takes the books off again, and the new shelves left empty.
pub fn undo(model: &mut Library, done: &Applied, wall_ms: u64) {
    for (book, shelf) in &done.filed {
        model.unfile(*book, *shelf, wall_ms);
    }
    prune(model, &done.made, wall_ms);
}

/// Removes those of `shelves` holding nothing, children first.
fn prune(model: &mut Library, shelves: &[ShelfId], wall_ms: u64) -> BTreeSet<ShelfId> {
    let mut gone = BTreeSet::new();
    for id in shelves.iter().rev() {
        let empty = model.shelf(*id).is_some() && model.counts(*id) == (0, 0);
        if empty && model.remove_shelf(*id, wall_ms).is_some() {
            gone.insert(*id);
        }
    }
    gone
}

/// The shelf a spot names, if it's still there.
fn locate(model: &Library, made: &[Option<ShelfId>], spot: Spot) -> Option<ShelfId> {
    let id = match spot {
        Spot::Shelf(id) => id,
        Spot::New(i) => made.get(i).copied().flatten()?,
    };
    model.shelf(id).map(|s| s.id)
}

/// Titles in the order a reader would file them, whatever their case.
pub fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}
