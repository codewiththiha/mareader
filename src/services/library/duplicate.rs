//! The shelf's "Duplicate": a second instance of one thing, asked for by name.
//!
//! A duplicate is the app's own object from the moment it exists, so a BOOK
//! always duplicates into the library's own store — never into the folder the
//! original reads from, whatever its origin. The store's own batch names the
//! copy after the row's new id, stamps it with its own modification time, and
//! answers with the copy's own measurement, so the row is known by that
//! measurement from the first moment; the source the original recorded stays the
//! provenance both wear.
//!
//! A read-at-place book used to duplicate as a second FILE beside the first, on
//! the reader's own disk. That sibling was still linked, so a later move off its
//! folder's shelf copied it AGAIN into the store and left the first copy
//! orphaned on disk, owned by nothing and invisible to the library. Copying
//! straight into the store removes the sibling, the orphan and the second
//! conversion at once: the duplicate was never linked, so the departure rule
//! never sees it.
//!
//! Either way the duplicate is a row of its own in the persisted ledger: its
//! own id, its own name — the level's counter, the conflict sheet's own
//! convention — its own resume point and highlights, and no memory of the
//! reader's place in the original. It is filed where the original is filed,
//! every shelf of it, right behind the row the reader pointed at: a duplicate
//! that landed somewhere else would be a book the reader has to go and find.
//!
//! A link duplicates as a link: a pointer is a row like any other, and the
//! second pointer costs no bytes. A book whose address died has nothing to
//! copy and says so by not being offered (the menu's `missing` rule, which is
//! the Open row's own).
//!
//! A SHELF duplicates as a shelf, and is the other answer rather than a batch
//! of book duplicates: a shelf holds membership and never held a byte, so its
//! duplicate is a second shelf of the reader's own holding the same books, with
//! the whole tree inside it copied along (`duplicate_shelf_row`). The
//! right-click lands on both kinds and the selection holds both, so one door
//! answers for both and the report counts them by kind.

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Book, Fingerprint, Origin, Row};
use library_core::conflict::{next_name, next_shelf_name};
use library_core::id;
use library_core::shelf::{self as shelves_ops, Shelf, ShelfKind};

use crate::services::library::covers;
use crate::services::library::toast;
use crate::services::library as wire;
use crate::state::AppState;
use crate::time::now_ms;

/// Duplicate one row: a card's or a list line's own right-click. Returns at
/// once — the copy is the shell's work, the dock owes nothing for one file,
/// and the toast is the whole report.
pub fn duplicate_row(state: AppState, row_id: &str) {
    duplicate_rows(state, std::slice::from_ref(&row_id.to_string()));
}

/// Duplicate one shelf: a folder card's or a shelf row's own right-click. The
/// same door as [`duplicate_row`], because the two kinds of thing a right-click
/// lands on are one gesture and one report.
pub fn duplicate_shelf(state: AppState, shelf_id: &str) {
    duplicate_rows(state, std::slice::from_ref(&shelf_id.to_string()));
}

/// Duplicate a set — the selection's right-click, which can hold books and
/// shelves together — as one task.
///
/// Sequential on purpose: two duplicates of one book asked in the same tick
/// would probe the same free counter name and race for it, and a set that
/// lands nine of ten with one refusal each is a report nobody can read. One
/// task, one pass, one toast that counts what landed.
pub fn duplicate_rows(state: AppState, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let ids = ids.to_vec();
    spawn_local(async move {
        let mut landed: Vec<Duplicated> = Vec::new();
        for id in ids {
            if let Some(one) = duplicate_one(state, &id).await {
                landed.push(one);
            }
        }
        if landed.is_empty() {
            return;
        }
        // A new row is a plate without art and a blob without its write: the
        // import's own tail, because a duplicate is an import of one file the
        // reader already had. A shelf's copy has no art of its own, but a set
        // can hold both and the covers are a question of the books in it.
        covers::backfill_missing(state);
        crate::storage::persist_library(state.library);
        toast(state, report(&landed));
    });
}

/// What one duplicate landed as: the name the reader sees, and whether the
/// thing duplicated was a shelf or a row.
struct Duplicated {
    name: String,
    shelf: bool,
}

/// The sentence a batch of duplicates owes. One name is worth saying out loud;
/// a set is worth counting, and counted by kind, because "3 books" for a set
/// that held two shelves is a report the reader cannot check against the page.
fn report(landed: &[Duplicated]) -> String {
    if landed.len() == 1 {
        return format!("Duplicated as “{}”.", landed[0].name);
    }
    let shelves = landed.iter().filter(|one| one.shelf).count();
    let books = landed.len() - shelves;
    let noun = match (books, shelves) {
        (_, 0) => "books",
        (0, _) => "shelves",
        _ => "shelves and books",
    };
    format!("Duplicated {} {noun}.", landed.len())
}

/// One thing's duplicate, and the name it landed under. `None` is a thing that
/// could not be copied — gone, missing, or a file the shell refused — and every
/// such refusal has already said so on the toast.
///
/// The id is a row's or a shelf's, and the two kinds are disjoint by prefix
/// (`library_core::id::is_shelf`), so the row list answering "not mine" is the
/// shelf list's turn rather than a thing that does not exist: a selection holds
/// both kinds, and a menu that duplicated one and quietly dropped the other
/// would be a report that counted less than the reader asked for.
async fn duplicate_one(state: AppState, id: &str) -> Option<Duplicated> {
    let Some(row) = state.library.row(id) else {
        return duplicate_shelf_row(state, id).map(|name| Duplicated { name, shelf: true });
    };
    let name = match row {
        Row::Link { name, target, .. } => Some(duplicate_link(state, id, &name, &target)),
        Row::Book(book) => {
            // A book whose address died has nothing to copy. The menu does not
            // offer the row; a selection that swept one in skips it the same
            // quiet way, because the sentence was said when it went missing.
            if book.missing {
                return None;
            }
            duplicate_book(state, book).await
        }
    }?;
    Some(Duplicated { name, shelf: false })
}

/// A pointer duplicates as a pointer: one more link, same target, the level's
/// counter name. No bytes, no shell, no measurement — the whole of it is a row
/// and a membership.
fn duplicate_link(state: AppState, row_id: &str, name: &str, target: &str) -> String {
    let now = now_ms();
    let title = name_for(state, name);
    let dup = Row::link(id::next_id(now), title.clone(), target.to_string(), now);
    let dup_id = dup.id().to_string();
    state.library.books.update(|rows| rows.push(dup));
    file_beside(state, row_id, &dup_id);
    title
}

/// A book's duplicate: a second copy in the library's own store, and the row
/// that reads it — whatever origin the original had.
///
/// A duplicate is the app's own object from the moment it exists, so it always
/// lands as an [`Origin::Stored`] copy and NEVER writes into the folder the
/// original reads from. A read-at-place book used to duplicate as a second FILE
/// beside the first, on the reader's own disk; that copy was still linked, so a
/// later move off its folder's shelf copied it AGAIN into the store and left the
/// first sibling orphaned on disk, owned by nothing and invisible to the library.
/// Copying straight into the store means there is no sibling to orphan and no
/// second conversion to make: the duplicate was never linked, so the departure
/// rule never sees it.
///
/// The copy is made from the address the book reads — the source file for a
/// linked book, the store copy for a stored one — through the store's own batch,
/// which names the file after the row's new id and stamps it with its own
/// modification time, so the row is known by the copy's fingerprint from the
/// first moment and the two instances stay two books to every walk that sees
/// them.
async fn duplicate_book(state: AppState, book: Book) -> Option<String> {
    // Minted BEFORE the copy: the stored file wears the id, and a mint after
    // the copy would be a name with nothing to wear it.
    let book_id = id::next_id(now_ms());
    let task = format!("duplicate-{book_id}");
    let from = book.path().to_string();
    let (new_store, measured) = match wire::copy_and_measure(&task, &from, &book_id).await {
        Ok(pair) => pair,
        Err(message) => {
            toast(state, message);
            return None;
        }
    };
    let title = name_for(state, &book.title());
    let original_id = book.id.clone();
    let mut dup = Book::new(
        book_id,
        Fingerprint::placeholder(&new_store),
        book.format,
        Origin::Stored {
            // The provenance the original recorded, worn by the second instance:
            // a duplicate is a sibling of the same source, not a copy of a copy
            // the reader has to trace back. For a linked original that is the
            // address it reads; for a stored one, the source its first copy kept.
            src: book.origin.source().map(str::to_string),
            store: new_store,
        },
        now_ms(),
    );
    dup.title = Some(title.clone());
    // A counter name is a name the library minted at the reader's ask, so it
    // wears the reader's lock: the load-time sweep spares a trailing counter by
    // convention, but a base the file itself carried underscores in
    // ("harry_potter_1" → "harry_potter_2") reads as snake-case debris to the
    // rule, and a name the reader just asked for is not debris.
    dup.title_locked = true;
    // The copy's own measurement is the row's identity; a copy that could not
    // be weighed keeps the pending mark the startup sweep finishes, which is
    // every stored landing's rule and not a duplicate's own.
    dup.adopt_measurement(measured);
    let dup_id = dup.id.clone();
    state.library.books.update(|rows| rows.push(Row::Book(dup)));
    file_beside(state, &original_id, &dup_id);
    Some(title)
}

/// A shelf's duplicate: a second shelf of the reader's own, holding the same
/// books, with the whole tree inside it copied along. Answers the name it
/// landed under; `None` is a shelf that is not there, which is nothing to say —
/// a right-click on a card a rescan has just replaced is a click on no shelf.
///
/// A shelf holds membership and never held a byte, so this is the one duplicate
/// that costs the reader no disk and no measurement: what is copied is the list,
/// and the books are the same books on two shelves, which is the folder's own
/// "also show it here" asked of a whole level at once. Every copied shelf is the
/// reader's OWN, whatever the original was, and that is a rule rather than a
/// simplification:
///
///   * a folder shelf IS the OS directory, and one directory is one linked shelf
///     — a folder's map names one shelf per rung, so a second folder shelf of one
///     rung would be two doors to one directory with only one of them on the
///     ledger, and the rescan that re-hangs the tree would be re-hanging a shelf
///     the reader made;
///   * a `Virtual` shelf is no scan's business, so the copy keeps the place this
///     put it and never answers to a walk, a fold or a departure — the three
///     questions a folder shelf owes its ground.
///
/// The copy's root wears the LEVEL's counter name (`next_shelf_name`, the shelf
/// half of the counter a book's duplicate wears), because the collision that
/// matters is the one the reader can see: two shelves of one name on one level
/// are two doors they cannot tell apart. Everything inside it keeps its own
/// name, since the copy's levels are fresh and hold nothing to collide with. The
/// copy lands right behind the original's own row, which is `file_beside`'s
/// answer one kind up: a duplicate that appeared at the end of the level would
/// be a shelf the reader has to go and find.
fn duplicate_shelf_row(state: AppState, shelf_id: &str) -> Option<String> {
    let shelves = state.library.shelves.get_untracked();
    let original = shelves_ops::find(&shelves, shelf_id)?;
    let now = now_ms();
    let name = next_shelf_name(&shelves, original.parent.as_deref(), &original.name);

    // The subtree, walked with an explicit stack and a seen-set rather than
    // recursively: the forest is finite because a load cuts cycles out of it,
    // but this reads a list that can be caught between two writes, and a
    // recursion over a graph with a loop in it is a stack that never unwinds.
    // Children are taken in the order the level holds them, which is the only
    // order a level reads — the copy's siblings have to be in the original's
    // order or the copy is a rearrangement nobody asked for.
    let mut order: Vec<&Shelf> = vec![original];
    let mut seen: HashSet<String> = HashSet::from([original.id.clone()]);
    let mut stack: Vec<&str> = vec![original.id.as_str()];
    while let Some(parent) = stack.pop() {
        for child in shelves_ops::children_of(&shelves, Some(parent)) {
            if !seen.insert(child.id.clone()) {
                continue;
            }
            order.push(child);
            stack.push(child.id.as_str());
        }
    }

    // Every id minted before any shelf is built: the copy's edges point at the
    // copy's own ids, and a parent minted after its child would be an edge at a
    // name nothing wears yet.
    let mut fresh: HashMap<String, String> = HashMap::with_capacity(order.len());
    for shelf in &order {
        fresh.insert(shelf.id.clone(), id::next_shelf_id(now));
    }
    let copies: Vec<Shelf> = order
        .iter()
        .enumerate()
        .map(|(at, shelf)| Shelf {
            id: fresh.get(&shelf.id).cloned().unwrap_or_default(),
            name: if at == 0 {
                name.clone()
            } else {
                shelf.name.clone()
            },
            kind: ShelfKind::Virtual,
            books: shelf.books.clone(),
            // The copy's root hangs where the original hangs, which is a place
            // the graph already allows; every shelf below it hangs off its own
            // copy's parent, so the subtree keeps its shape and closes no loop.
            // A parent the walk could not reach — which is only a blob carrying
            // a cycle — is no parent at all rather than an edge at the ORIGINAL,
            // an edge that would file the copy inside the shelf it came from.
            parent: if at == 0 {
                original.parent.clone()
            } else {
                shelf.parent.as_deref().and_then(|p| fresh.get(p).cloned())
            },
            manual_parent: false,
        })
        .collect();

    state.library.shelves.update(|shelves| {
        // Right behind the original's own row: the shelf list IS the render
        // order, and `children_of` filters it in order, so a copy appended to
        // the end would be a shelf the reader has to go and find. A shelf that
        // went between the read and the write is a copy at the end of the level
        // rather than a copy that lands nowhere.
        let at = shelves
            .iter()
            .position(|s| s.id == shelf_id)
            .map_or(shelves.len(), |at| at + 1);
        shelves.splice(at..at, copies);
    });
    Some(name)
}

/// The duplicate's name: the file manager's counter — `Dune` → `Dune_1`, and
/// a duplicate of a duplicate steps rather than stacks — counted against the
/// level the reader clicked from, because the collision that matters is the
/// one they can see, which is the conflict sheet's own per-level convention
/// (`library_core::conflict::next_name`). A second spelling of the counter
/// would be a second convention.
fn name_for(state: AppState, display: &str) -> String {
    let (rows, shelves) = (
        state.library.books.get_untracked(),
        state.library.shelves.get_untracked(),
    );
    let level = state.library.shelf.get_untracked();
    next_name(&rows, &shelves, &level, display)
}

/// The duplicate's memberships: every shelf the original stands on, at the
/// position right behind it. `place` is the drag's spelling — remove, insert
/// at the index — which is what "beside the row you pointed at" means on a
/// list the reader can see; an original no shelf holds leaves the duplicate
/// in "All", where the original is.
fn file_beside(state: AppState, original_id: &str, dup_id: &str) {
    let seats: Vec<(String, usize)> = state.library.shelves.with_untracked(|shelves| {
        shelves_ops::containing(shelves, original_id)
            .into_iter()
            .filter_map(|shelf| {
                let at = shelf.books.iter().position(|m| m == original_id)?;
                Some((shelf.id.clone(), at + 1))
            })
            .collect()
    });
    if seats.is_empty() {
        return;
    }
    state.library.shelves.update(|shelves| {
        for (shelf_id, index) in &seats {
            if let Some(shelf) = shelves_ops::find_mut(shelves, shelf_id) {
                shelves_ops::place(&mut shelf.books, dup_id, Some(*index));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duplicate_of_a_link_is_a_link_beside_it() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        state.library.books.set(vec![
            library_core::testkit::row_at("b1", "/books/dune.md"),
            library_core::testkit::link("l1", "Dune", "b1"),
        ]);
        let mut shelf = library_core::testkit::plain_shelf("s1", &["b1", "l1"]);
        shelf.name = "Shelf".into();
        state.library.shelves.set(vec![shelf]);
        // The level the reader clicked from is the level the counter counts
        // against.
        state.library.shelf.set("s1".to_string());

        let name = duplicate_link(state, "l1", "Dune", "b1");
        assert_eq!(name, "Dune_1", "the level's counter, not a collision");
        let rows = state.library.books.get_untracked();
        assert_eq!(rows.len(), 3);
        let dup = rows.iter().find(|r| r.id() != "b1" && r.id() != "l1").unwrap();
        match dup {
            Row::Link { target, name, .. } => {
                assert_eq!(target, "b1", "the pointer points where the pointer pointed");
                assert_eq!(name, "Dune_1");
            }
            Row::Book(_) => panic!("a link duplicates as a link"),
        }
        let shelves = state.library.shelves.get_untracked();
        assert_eq!(
            shelves[0].books,
            vec!["b1".to_string(), "l1".to_string(), dup.id().to_string()],
            "filed right behind the row the reader pointed at"
        );
    }

    // -------------------------------------------------------------------
    // A shelf's duplicate.
    // -------------------------------------------------------------------

    /// A shelf of the reader's own with one book and a shelf inside it, hanging
    /// at the root level beside another shelf: the shape the copy has to keep.
    fn nested_state() -> (AppState, Owner) {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();
        state.library.books.set(vec![
            library_core::testkit::markdown_row("b1"),
            library_core::testkit::markdown_row("b2"),
        ]);
        state.library.shelves.set(vec![
            library_core::testkit::shelf("s1", "Shelf", &["b1"], None),
            library_core::testkit::shelf("s2", "Inside", &["b2"], Some("s1")),
            library_core::testkit::shelf("s3", "Other", &[], None),
        ]);
        (state, owner)
    }

    /// The copy's root, found by the name the counter gave it.
    fn copy_of(shelves: &[Shelf], name: &str) -> Shelf {
        shelves
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no shelf called {name}"))
            .clone()
    }

    #[test]
    fn a_shelf_duplicates_as_a_second_shelf_beside_it() {
        let (state, _owner) = nested_state();
        let name = duplicate_shelf_row(state, "s1").expect("the shelf is there");
        assert_eq!(name, "Shelf_1", "the level's counter, not a collision");

        let shelves = state.library.shelves.get_untracked();
        assert_eq!(shelves.len(), 5, "the shelf and the one inside it, copied");
        let root = copy_of(&shelves, "Shelf_1");
        assert_eq!(root.books, vec!["b1".to_string()], "the same book, not a copy of it");
        assert_eq!(root.parent, None, "on the level the original hangs on");
        assert!(
            !root.is_folder(),
            "the reader's own, whatever the original was"
        );
        // The level's own order is the list's, so the copy sits right behind the
        // shelf it came from rather than at the end of the page.
        let level: Vec<&str> = library_core::shelf::children_of(&shelves, None)
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(level, vec!["Shelf", "Shelf_1", "Other"]);
        // And the shelf inside it came along, keeping its own name and its place
        // under the copy rather than under the original.
        let inside: Vec<&str> = library_core::shelf::children_of(&shelves, Some(root.id.as_str()))
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(inside, vec!["Inside"]);
        assert_eq!(
            library_core::shelf::children_of(&shelves, Some("s1"))
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Inside"],
            "the original keeps its own"
        );
    }

    #[test]
    fn a_folder_shelf_duplicates_as_a_shelf_of_the_readers_own() {
        // One directory is one linked shelf: a folder's map names one shelf per
        // rung, so a second folder shelf of one rung would be two doors to one
        // directory with only one of them on the ledger — and a rescan would
        // re-hang a shelf the reader made. The copy is the reader's own second
        // door onto the same books, which no walk, fold or departure answers for.
        let (state, _owner) = nested_state();
        state.library.shelves.update(|shelves| {
            shelves[0] = library_core::testkit::folder_shelf(
                "s1",
                "Books",
                "f1",
                None,
                &["b1"],
                None,
            );
        });
        let name = duplicate_shelf_row(state, "s1").expect("the shelf is there");
        let shelves = state.library.shelves.get_untracked();
        let root = copy_of(&shelves, &name);
        assert_eq!(name, "Books_1");
        assert!(
            root.kind.folder_id().is_none(),
            "the copy is not a second shelf of the folder"
        );
        assert_eq!(root.books, vec!["b1".to_string()]);
        // The original is untouched: the copy is not a rung of the folder, so a
        // walk that mints the tree again mints the tree it already had.
        let original = library_core::shelf::find(&shelves, "s1").expect("the original stands");
        assert_eq!(original.kind.folder_id(), Some("f1"));
    }

    #[test]
    fn a_duplicate_of_a_duplicate_steps_the_shelf_counter() {
        let (state, _owner) = nested_state();
        assert_eq!(
            duplicate_shelf_row(state, "s1").as_deref(),
            Some("Shelf_1")
        );
        // Duplicating the copy steps rather than stacks, the reading a file
        // manager gives: the counter is not part of the name.
        let shelves = state.library.shelves.get_untracked();
        let first = copy_of(&shelves, "Shelf_1");
        assert_eq!(
            duplicate_shelf_row(state, &first.id).as_deref(),
            Some("Shelf_2")
        );
    }

    #[test]
    fn a_shelf_that_is_not_there_duplicates_into_nothing() {
        let (state, _owner) = nested_state();
        assert!(duplicate_shelf_row(state, "gone").is_none());
        // "All" is the book list and not a shelf, so it has no second instance
        // to make either — the pseudo-shelf's own answer everywhere else.
        assert!(duplicate_shelf_row(state, library_core::shelf::ALL_SHELF).is_none());
        assert_eq!(state.library.shelves.get_untracked().len(), 3);
    }

    #[test]
    fn a_report_counts_the_kinds_it_landed() {
        let one = |name: &str, shelf| Duplicated {
            name: name.to_string(),
            shelf,
        };
        assert_eq!(report(&[one("Dune_1", false)]), "Duplicated as “Dune_1”.");
        assert_eq!(report(&[one("Shelf_1", true)]), "Duplicated as “Shelf_1”.");
        assert_eq!(
            report(&[one("a", false), one("b", false)]),
            "Duplicated 2 books."
        );
        assert_eq!(
            report(&[one("a", true), one("b", true)]),
            "Duplicated 2 shelves."
        );
        assert_eq!(
            report(&[one("a", true), one("b", false), one("c", false)]),
            "Duplicated 3 shelves and books."
        );
    }
}
