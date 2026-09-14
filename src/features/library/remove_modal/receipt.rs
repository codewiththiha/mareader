//! The removal's receipt: what a removal takes, counted over the SET the
//! confirm button will act on rather than over what was clicked.
//!
//! The tree arithmetic a cascade depends on — which shelves are below the asked
//! ones, and which of them has to go first — is pure over the shelf list and
//! host-tested at the bottom of this file: "which shelves go" is exactly the
//! question that should not need a browser to answer.

use leptos::prelude::*;

use library_core::book::{Book, Row};
use library_core::shelf::{Shelf, ancestors, children_of, subtree_ids};
use library_core::text::{human_size, plural};

use crate::services::library::memberships;
use crate::state::AppState;

/// Everything the receipt lines are made of, read once per open.
pub(super) struct Receipt {
    pub(super) books: Vec<Book>,
    /// The names of the LINK rows the removal takes. A pointer costs nothing
    /// but itself — no resume point, no highlights, no cover, no store copy —
    /// and the book it points at stays in the library, which is a sentence the
    /// sheet owes a reader who clicked a link's ✕ and got a receipt about
    /// books.
    pub(super) links: Vec<String>,
    /// The ids the confirm button will purge: what was asked, plus everything
    /// inside the asked shelves when the cascade is on. Separate from [`Self::books`]
    /// only because the button needs ids and the rows need the rows' own facts.
    pub(super) book_ids: Vec<String>,
    /// The ids the confirm button will delete: what was asked, plus every
    /// descendant when the cascade is on.
    pub(super) shelf_ids: Vec<String>,
    /// Whether this receipt was built with the cascade on, which is what the shelf
    /// rows and the button's own wording have to agree with.
    pub(super) cascade: bool,
    /// The name of the one shelf asked about, when exactly one was. A cascade pulls
    /// books and further shelves into the receipt, and without this the heading
    /// would answer "3 books" to a reader who clicked a shelf — a heading about
    /// something they did not click, on the one sheet whose whole job is to say what
    /// the click means.
    pub(super) asked_name: Option<String>,
    /// What sits inside the asked shelves whatever the switch says — the books
    /// anywhere inside them, and the shelves nested inside them at any depth. The
    /// switch's own visibility is decided by these, so it cannot depend on itself.
    pub(super) inside_books: usize,
    pub(super) inside_shelves: usize,
    /// Highlight marks stored against these addresses.
    pub(super) marks: usize,
    pub(super) covers: usize,
    /// The names of the shelves any of these books is filed on, deduped: ten books
    /// on one shelf is one placement to name, not ten.
    pub(super) placements: Vec<String>,
    /// At least one of them came from a folder that is still being watched, so the
    /// removal has a consequence worth one sentence: it will stay out.
    pub(super) watched: bool,
    /// The app's own copies among them, and what they occupy.
    pub(super) stored_count: usize,
    pub(super) stored_bytes: u64,
    /// The shelves being taken apart, and what survives each of them.
    pub(super) shelves: Vec<ShelfLine>,
}


/// One shelf the removal takes apart. A shelf is a list of ids and never held a
/// byte, so its row is about what SURVIVES it rather than about what goes.
pub(super) struct ShelfLine {
    pub(super) name: String,
    pub(super) books: usize,
    /// Shelves filed inside it, which move up to the level it was on. Always zero
    /// under a cascade, where nothing survives inside to be lifted.
    pub(super) lifted: usize,
    /// Cut from a folder that is still watched, so the shelf returns if the
    /// folder ever places a book in it again. Worth one sentence on the receipt
    /// because it is the one consequence a reader cannot see coming.
    pub(super) watched: bool,
}


impl Receipt {
    pub(super) fn many(&self) -> bool {
        self.books.len() + self.links.len() + self.shelves.len() != 1
    }

    /// The heading: one thing's own name, or a count.
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

    /// The line under the heading: the format, and a size the library has actually
    /// measured. A book never measured has no honest size, and a placeholder's
    /// "size" is the length of its path — a number on a receipt that would mean
    /// nothing. A shelves-only receipt has no formats to name, so it says the one
    /// thing a reader worries about: that nothing else goes with them.
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


/// Every shelf below any of `roots`, at any depth, in no particular order and
/// without repeats.
///
/// The walk itself is `library_core::shelf::subtree_ids` — the departure's ride
/// asks it the same question, and a cascade and a copy that answered "which
/// shelves go with this one" differently would be two rules wearing one name.
/// Pure over the shelf list rather than over the state, so the arithmetic a
/// cascade depends on is testable on the host: this is the function that decides
/// which shelves a removal deletes, and "which shelves go" is exactly the
/// question that should not need a browser to answer.
fn subtree(shelves: &[Shelf], roots: &[String]) -> Vec<Shelf> {
    subtree_ids(shelves, roots)
        .into_iter()
        .filter_map(|id| shelves.iter().find(|s| s.id == id).cloned())
        .collect()
}


/// A cascade's delete order: deepest first.
///
/// `delete_shelf` lifts a shelf's children to the level it was on before removing
/// it, which is the right thing for one removal and the wrong thing for a cascade:
/// lifting a shelf that is next in line to be deleted moves it somewhere it is
/// about to leave anyway, and moves it past the reader on the way. Deepest first
/// means every lift finds nothing left to lift.
///
/// A stable sort on a reversed key, so two shelves at the same depth keep the
/// order the library stores them in and the receipt's rows match the order they
/// went in.
pub(super) fn deepest_first(shelves: &[Shelf], ids: &[String]) -> Vec<String> {
    let mut with_depth: Vec<(String, usize)> = ids
        .iter()
        .map(|id| (id.clone(), ancestors(shelves, id).len()))
        .collect();
    with_depth.sort_by_key(|one| std::cmp::Reverse(one.1));
    with_depth.into_iter().map(|(id, _)| id).collect()
}


/// Build the receipt. `None` when none of the books or shelves are there any
/// more, which is what makes a sheet left open across a removal harmless rather
/// than a panic.
///
/// `cascade` decides which SET the receipt is of, and everything below — the books
/// row, the highlight and cover counts, the store copies, the placements, the
/// button's own wording — is measured over that set rather than over what was
/// clicked. A receipt that itemised the selection and then removed the selection
/// plus a shelf's contents would be a receipt for a different removal than the one
/// it confirmed.
pub(super) fn receipt(
    state: AppState,
    ids: &[String],
    shelf_ids: &[String],
    cascade: bool,
) -> Option<Receipt> {
    let gloss = crate::storage::load_gloss();
    // One untracked read of the shelf list, and the tree arithmetic below is pure
    // over it: three nested reads of the same signal were three chances to see a
    // different library than the one the receipt is describing.
    let all: Vec<Shelf> = state.library.shelves.get_untracked();
    let asked: Vec<Shelf> = all
        .iter()
        .filter(|s| shelf_ids.contains(&s.id))
        .cloned()
        .collect();
    // Everything below the asked shelves, deduped against each other and against
    // the asked ones: two selected shelves can share a descendant, and a shelf
    // selected alongside its own parent is already in `asked`.
    let descendants = subtree(&all, shelf_ids);
    let delete_shelves: Vec<Shelf> = if cascade {
        asked.iter().chain(descendants.iter()).cloned().collect()
    } else {
        asked.clone()
    };
    // What is inside, counted whether or not the cascade is on: these two numbers
    // are what the switch's own visibility is decided by, and a switch that only
    // appeared once it was already on could never be turned on.
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
    // The set the removal will actually take: what was asked, plus what is inside
    // when the cascade is on. Deduped, because a book on the asked shelf and inside
    // the asked folder is one book and one tombstone.
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
            // The rung's own answer: the receipt says whether THIS shelf's
            // ground will refill itself, and a subfolder turned off under a
            // watched tree does not.
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
        // The row's own id, which is the row's own list: two rows of one file
        // are two lists, so the count is what THIS removal takes and a twin on
        // the shelf keeps its own.
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
    let fingerprints: Vec<_> = books.iter().map(|b| b.fp).collect();
    let measured = books.iter().all(|b| !b.fp_pending);
    let watched = measured
        && state.library.folders.with_untracked(|folders| {
            folders.iter().any(|f| {
                // Watched anywhere in the tree, which is what the note
                // promises: a folder that still walks — root or one rung the
                // reader kept on — is a folder that can place this book again
                // and offer it back. Asked exactly as the walk asks it
                // (`library_core::folder::WatchedFolder::owes_walk`), because a
                // note promising a scan the walk is not owed is a promise
                // nothing keeps.
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

    /// A virtual shelf, which is the kind a reader makes and the only kind a
    /// cascade can move.
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

    /// `a` at the root, `b` inside it, `c` inside `b`, and an empty `d` beside `b`.
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
    fn a_cascade_deletes_deepest_first() {
        let tree = tree();
        let order = deepest_first(&tree, &["a".to_string(), "b".to_string(), "c".to_string()]);
        assert_eq!(
            order.first().map(String::as_str),
            Some("c"),
            "the deepest goes first, so no lift moves a shelf that is next in line"
        );
        assert_eq!(
            order.last().map(String::as_str),
            Some("a"),
            "and the shelf the reader asked about goes last"
        );
    }

    #[test]
    fn shelves_at_one_depth_keep_the_libraries_own_order() {
        // A stable sort: the receipt's rows and the order the shelves went in are
        // the same order, so a reader can follow what happened.
        let tree = tree();
        let order = deepest_first(&tree, &["d".to_string(), "b".to_string()]);
        assert_eq!(order, ["d", "b"]);
    }
}
