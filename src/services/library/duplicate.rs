//! The shelf's "Duplicate": a second instance of one thing, asked for by name.
//!
//! A duplicate is the app's own object from the moment it exists, so a BOOK always
//! duplicates into the library's own store — never into the folder the original reads
//! from, whatever its origin.

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

pub fn duplicate_row(state: AppState, row_id: &str) {
    duplicate_rows(state, std::slice::from_ref(&row_id.to_string()));
}

pub fn duplicate_shelf(state: AppState, shelf_id: &str) {
    duplicate_rows(state, std::slice::from_ref(&shelf_id.to_string()));
}

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
        covers::backfill_missing(state);
        crate::storage::persist_library(state.library);
        toast(state, report(&landed));
    });
}

struct Duplicated {
    name: String,
    shelf: bool,
}

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

/// Every such refusal has already said so on the toast. The two id kinds are disjoint by prefix, so the row list answering "not mine" is the shelf list's turn.
async fn duplicate_one(state: AppState, id: &str) -> Option<Duplicated> {
    let Some(row) = state.library.row(id) else {
        return duplicate_shelf_row(state, id).map(|name| Duplicated { name, shelf: true });
    };
    let name = match row {
        Row::Link { name, target, .. } => Some(duplicate_link(state, id, &name, &target)),
        Row::Book(book) => {
            if book.missing {
                return None;
            }
            duplicate_book(state, book).await
        }
    }?;
    Some(Duplicated { name, shelf: false })
}

fn duplicate_link(state: AppState, row_id: &str, name: &str, target: &str) -> String {
    let now = now_ms();
    let title = name_for(state, name);
    let dup = Row::link(id::next_id(now), title.clone(), target.to_string(), now);
    let dup_id = dup.id().to_string();
    state.library.books.update(|rows| rows.push(dup));
    file_beside(state, row_id, &dup_id);
    title
}

async fn duplicate_book(state: AppState, book: Book) -> Option<String> {
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
            src: book.origin.source().map(str::to_string),
            store: new_store,
        },
        now_ms(),
    );
    dup.title = Some(title.clone());
    // A base the file itself carried underscores in ("harry_potter_1") reads as snake-case debris to the title rule, and a name the reader just asked for is not debris.
    dup.title_locked = true;
    dup.adopt_measurement(measured);
    let dup_id = dup.id.clone();
    state.library.books.update(|rows| rows.push(Row::Book(dup)));
    file_beside(state, &original_id, &dup_id);
    Some(title)
}

fn duplicate_shelf_row(state: AppState, shelf_id: &str) -> Option<String> {
    let shelves = state.library.shelves.get_untracked();
    let original = shelves_ops::find(&shelves, shelf_id)?;
    let now = now_ms();
    let name = next_shelf_name(&shelves, original.parent.as_deref(), &original.name);

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
            // The subtree keeps its shape and closes no loop; a parent the walk could not reach is no parent at all rather than an edge at the ORIGINAL.
            parent: if at == 0 {
                original.parent.clone()
            } else {
                shelf.parent.as_deref().and_then(|p| fresh.get(p).cloned())
            },
            manual_parent: false,
        })
        .collect();

    state.library.shelves.update(|shelves| {
        // Right behind the original's own row: the shelf list IS the render order, so a copy appended to the end would be a shelf the reader has to go and find.
        let at = shelves
            .iter()
            .position(|s| s.id == shelf_id)
            .map_or(shelves.len(), |at| at + 1);
        shelves.splice(at..at, copies);
    });
    Some(name)
}

// The file manager's counter, counted against the level the reader clicked from, because the collision that matters is the one they can see.
fn name_for(state: AppState, display: &str) -> String {
    let (rows, shelves) = (
        state.library.books.get_untracked(),
        state.library.shelves.get_untracked(),
    );
    let level = state.library.shelf.get_untracked();
    next_name(&rows, &shelves, &level, display)
}

/// `place` is the drag's spelling — remove, insert at the index — which is what "beside the row you pointed at" means on a list the reader can see.
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
