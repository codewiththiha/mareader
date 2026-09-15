//! The removal's receipt: what a removal takes, counted over the set the
//! confirm button will act on rather than over what was clicked.
//!
//! The tree arithmetic a cascade depends on is pure over the shelf list and
//! host-tested at the bottom of this file.

use leptos::prelude::*;

use library_core::book::{Book, Row};
use library_core::shelf::{Shelf, children_of, subtree_ids};
use library_core::text::{human_size, plural};

use crate::services::library::memberships;
use crate::state::AppState;

pub(super) struct Receipt {
    pub(super) books: Vec<Book>,
    /// A pointer costs nothing but itself — no resume point, highlights,
    /// cover or store copy — and its book stays in the library: a sentence the
    /// sheet owes a reader who clicked a link's ✕.
    pub(super) links: Vec<String>,
    /// Separate from [`Self::books`]: the button needs ids, the rows need the
    /// rows' own facts.
    pub(super) book_ids: Vec<String>,
    pub(super) shelf_ids: Vec<String>,
    pub(super) cascade: bool,
    /// A cascade pulls books and further shelves into the receipt; without
    /// this the heading would answer "3 books" to a reader who clicked a
    /// shelf.
    pub(super) asked_name: Option<String>,
    // The switch's visibility is decided by these, so it cannot depend on
    // itself.
    pub(super) inside_books: usize,
    pub(super) inside_shelves: usize,
    pub(super) marks: usize,
    /// How many books carry the reader's own work — a resume point or a name
    /// they gave it. With the marks, this is the sheet's one question;
    /// counting it here keeps the question's visibility off its own answer.
    pub(super) wrote: usize,
    pub(super) covers: usize,
    pub(super) placements: Vec<String>,
    pub(super) watched: bool,
    pub(super) stored_count: usize,
    pub(super) stored_bytes: u64,
    pub(super) shelves: Vec<ShelfLine>,
}

pub(super) struct ShelfLine {
    pub(super) name: String,
    pub(super) books: usize,
    pub(super) lifted: usize,
    pub(super) watched: bool,
}

impl Receipt {
    pub(super) fn many(&self) -> bool {
        self.books.len() + self.links.len() + self.shelves.len() != 1
    }

    pub(super) fn heading(&self) -> String {
        if let Some(name) = &self.asked_name {
            return name.clone();
        }
        if !self.books.is_empty() && self.links.is_empty() {
            match self.books.first() {
                Some(book) if !self.many() => book.title(),
                _ => plural(self.books.len(), "book", "books"),
            }
        } else if !self.links.is_empty() && self.books.is_empty() {
            match self.links.first() {
                Some(name) if !self.many() => name.clone(),
                _ => plural(self.links.len(), "link", "links"),
            }
        } else if !self.books.is_empty() {
            format!(
                "{} and {}",
                plural(self.books.len(), "book", "books"),
                plural(self.links.len(), "link", "links")
            )
        } else {
            match self.shelves.first() {
                Some(shelf) if !self.many() => shelf.name.clone(),
                _ => plural(self.shelves.len(), "shelf", "shelves"),
            }
        }
    }

    /// Whether there is anything of the reader's for the sheet's question to
    /// decide: a control with nothing to decide is one the reader reads and
    /// then ignores.
    pub(super) fn offers_data(&self) -> bool {
        self.marks > 0 || self.wrote > 0
    }

    /// A placeholder's "size" is the length of its path — a receipt number
    /// that would mean nothing. A shelves-only receipt says the one thing a
    /// reader worries about: nothing else goes with them.
    pub(super) fn subtitle(&self) -> String {
        if self.books.is_empty() {
            if !self.links.is_empty() {
                return "A link is a pointer: the book it goes to stays in the library"
                    .to_string();
            }
            let kept: usize = self.shelves.iter().map(|s| s.books).sum();
            return if kept == 0 {
                "Nothing else goes with them".to_string()
            } else {
                format!("{} stay in the library", plural(kept, "book", "books"))
            };
        }
        let measured: Vec<u64> = self
            .books
            .iter()
            .filter(|b| !b.fp_pending)
            .map(|b| b.fp.size)
            .collect();
        let formats: Vec<&str> = {
            let mut seen: Vec<&str> = Vec::new();
            for book in &self.books {
                let label = book.format.label();
                if !seen.contains(&label) {
                    seen.push(label);
                }
            }
            seen
        };
        let kinds = formats.join(" · ");
        if measured.is_empty() {
            kinds
        } else {
            let total: u64 = measured.iter().sum();
            format!("{kinds} · {}", human_size(total))
        }
    }
}

/// How many of these books the reader left something in: a resume point or a
/// name. Marks are counted apart from the rows because they are the one thing
/// this file reads out of the store rather than off the row.
fn wrote_in(books: &[Book]) -> usize {
    books
        .iter()
        .filter(|book| book.page > 1 || book.fraction.is_some() || book.title_locked)
        .count()
}

/// The walk itself is `library_core::shelf::subtree_ids`: a cascade and a
/// copy answering "which shelves go with this one" differently would be two
/// rules wearing one name.
fn subtree(shelves: &[Shelf], roots: &[String]) -> Vec<Shelf> {
    subtree_ids(shelves, roots)
        .into_iter()
        .filter_map(|id| shelves.iter().find(|s| s.id == id).cloned())
        .collect()
}

/// `None` when none of the books or shelves are there any more, which makes
/// a sheet left open across a removal harmless rather than a panic.
/// Everything below is measured over the cascade's set, not over what was
/// clicked.
pub(super) fn receipt(
    state: AppState,
    ids: &[String],
    shelf_ids: &[String],
    cascade: bool,
) -> Option<Receipt> {
    let gloss = crate::storage::load_gloss();
    let all: Vec<Shelf> = state.library.shelves.get_untracked();
    let asked: Vec<Shelf> = all
        .iter()
        .filter(|s| shelf_ids.contains(&s.id))
        .cloned()
        .collect();
    let descendants = subtree(&all, shelf_ids);
    let delete_shelves: Vec<Shelf> = if cascade {
        asked.iter().chain(descendants.iter()).cloned().collect()
    } else {
        asked.clone()
    };
    let inside_ids: Vec<String> = {
        let mut acc: Vec<String> = Vec::new();
        for shelf in all.iter().filter(|s| {
            shelf_ids.contains(&s.id) || descendants.iter().any(|each| each.id == s.id)
        }) {
            for book in &shelf.books {
                if !acc.contains(book) {
                    acc.push(book.clone());
                }
            }
        }
        acc
    };
    let mut effective: Vec<String> = Vec::new();
    for id in ids {
        if !effective.contains(id) {
            effective.push(id.clone());
        }
    }
    if cascade {
        for id in &inside_ids {
            if !effective.contains(id) {
                effective.push(id.clone());
            }
        }
    }
    let (books, links): (Vec<Book>, Vec<String>) =
        state.library.books.with_untracked(|all| {
            let mut books = Vec::new();
            let mut links = Vec::new();
            for row in all.iter().filter(|r| effective.iter().any(|id| id == r.id())) {
                match row {
                    Row::Book(b) => books.push(b.clone()),
                    Row::Link { name, .. } => links.push(name.clone()),
                }
            }
            (books, links)
        });
    if books.is_empty() && links.is_empty() && asked.is_empty() {
        return None;
    }
    let shelf_lines: Vec<ShelfLine> = delete_shelves
        .iter()
        .map(|s| ShelfLine {
            name: s.name.clone(),
            books: s.books.len(),
            lifted: if cascade {
                0
            } else {
                children_of(&all, Some(s.id.as_str())).len()
            },
            watched: state.library.shelf_tracked_untracked(&s.id),
        })
        .collect();
    let mut marks = 0usize;
    let mut covers = 0usize;
    let mut stored_count = 0usize;
    let mut stored_bytes = 0u64;
    let mut placement_names: Vec<String> = Vec::new();
    for book in &books {
        let path = book.path();
        marks += gloss.get(&book.id).map(Vec::len).unwrap_or(0);
        if state
            .library
            .covers
            .with_untracked(|covers| covers.contains_key(path))
        {
            covers += 1;
        }
        if let library_core::book::Origin::Stored { .. } = &book.origin {
            stored_count += 1;
            if !book.fp_pending {
                stored_bytes += book.fp.size;
            }
        }
        for (_, name) in memberships(state, &book.id) {
            if !placement_names.contains(&name) {
                placement_names.push(name);
            }
        }
    }
    let wrote = wrote_in(&books);
    let fingerprints: Vec<_> = books.iter().map(|b| b.fp).collect();
    let measured = books.iter().all(|b| !b.fp_pending);
    let watched = measured
        && state.library.folders.with_untracked(|folders| {
            folders.iter().any(|f| {
                // Asked exactly as the walk asks it (`library_core::folder::WatchedFolder::owes_walk`), because a note promising a scan the walk is not owed is a promise nothing keeps.
                f.owes_walk()
                    && fingerprints
                        .iter()
                        .any(|fp| f.placed.contains(fp) || f.is_ignored(fp))
            })
        });
    Some(Receipt {
        book_ids: effective.clone(),
        books,
        links,
        shelf_ids: delete_shelves.iter().map(|s| s.id.clone()).collect(),
        cascade,
        asked_name: match asked.as_slice() {
            [only] => Some(only.name.clone()),
            _ => None,
        },
        inside_books: inside_ids.len(),
        inside_shelves: descendants.len(),
        marks,
        wrote,
        covers,
        placements: placement_names,
        watched,
        stored_count,
        stored_bytes,
        shelves: shelf_lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::shelf::ShelfKind;

    fn shelf(id: &str, parent: Option<&str>, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: ShelfKind::Virtual,
            books: books.iter().map(|each| each.to_string()).collect(),
            parent: parent.map(str::to_string),
            manual_parent: false,
        }
    }

    fn ids(shelves: &[Shelf]) -> Vec<String> {
        shelves.iter().map(|each| each.id.clone()).collect()
    }

    fn tree() -> Vec<Shelf> {
        vec![
            shelf("a", None, &[]),
            shelf("b", Some("a"), &["b1", "b2"]),
            shelf("c", Some("b"), &["c1"]),
            shelf("d", Some("a"), &[]),
        ]
    }

    #[test]
    fn the_subtree_is_everything_below_and_never_the_root_itself() {
        let tree = tree();
        let mut under_a = ids(&subtree(&tree, &["a".to_string()]));
        under_a.sort();
        assert_eq!(under_a, ["b", "c", "d"], "the root is asked about, not inside");

        let under_b = ids(&subtree(&tree, &["b".to_string()]));
        assert_eq!(under_b, ["c"]);

        assert!(
            subtree(&tree, &["d".to_string()]).is_empty(),
            "an empty leaf has no subtree, which is why it gets no cascade switch"
        );
    }

    #[test]
    fn two_roots_sharing_a_descendant_count_it_once() {
        // One removal takes a shared shelf apart once, and a receipt that listed
        // it twice would be a receipt the reader could not reconcile with what
        // actually went.
        let tree = tree();
        let under_both = ids(&subtree(&tree, &["a".to_string(), "b".to_string()]));
        assert_eq!(under_both.len(), under_both.iter().collect::<std::collections::HashSet<_>>().len());
        assert!(under_both.iter().any(|id| id == "c"));
        assert!(
            !under_both.iter().any(|id| id == "b"),
            "a root is never reported as its own descendant"
        );
    }

    #[test]
    fn a_shelf_inside_itself_terminates_rather_than_repeating() {
        // `sanitize` cuts cycles out of a loaded blob, but the receipt reads a
        // signal that can be caught between two writes, and a walk that spun here
        // would hang the sheet rather than answer it.
        let looped = vec![
            shelf("x", Some("y"), &[]),
            shelf("y", Some("x"), &[]),
        ];
        let mut found = ids(&subtree(&looped, &["x".to_string()]));
        found.sort();
        assert_eq!(found, ["y"]);
    }

    #[test]
    fn only_books_the_reader_wrote_in_ask_the_data_question() {
        let untouched = library_core::testkit::book("b1");
        let started = Book {
            page: 3,
            ..library_core::testkit::book("b2")
        };
        let streamed = Book {
            fraction: Some(0.2),
            ..library_core::testkit::book("b3")
        };
        let renamed = Book {
            title_locked: true,
            ..library_core::testkit::book("b4")
        };
        assert_eq!(
            wrote_in(&[untouched, started, streamed, renamed]),
            3,
            "a book the reader never opened is a book with nothing of theirs to keep"
        );
    }

}
