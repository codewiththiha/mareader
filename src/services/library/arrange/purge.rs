//! The removal, and everything the library holds about each removed book: the row, the memberships,
//! the cover, the highlights, the store copy when the app made one, and the tombstone that keeps a
//! watched folder's rescan from putting the book straight back.

use leptos::prelude::*;

use library_core::book::{Book, Row, book_rows, drop_dangling_links, find_row, remove_row};
use library_core::folder::Tombstone;
use library_core::ledger::tombstone;
use library_core::shelf;

use crate::services::library::covers::prune_now;
use crate::services::library as wire;
use crate::state::AppState;
use crate::time::now_ms;

use super::folder_shelf_of;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PurgeOpts {
    /// Only ever offered for a book the app copied; a linked book's bytes belong to the reader and are never touched whatever this says.
    pub delete_store_copy: bool,
}

impl Default for PurgeOpts {
    fn default() -> Self {
        Self {
            delete_store_copy: true,
        }
    }
}


/// One book is a batch of one: the sheet is the only caller and it always holds a list. Seven
/// things have to happen together per book, and doing any of them alone leaves something behind
/// that nothing will ever collect.
pub fn purge_books(state: AppState, row_ids: &[String], opts: PurgeOpts) {
    let doomed: Vec<Row> = state.library.books.with_untracked(|rows| {
        rows.iter()
            .filter(|r| row_ids.iter().any(|id| id == r.id()))
            .cloned()
            .collect()
    });
    if doomed.is_empty() {
        return;
    }
    for row in &doomed {
        purge_one(state, row, opts);
    }

    prune_now(state);
    crate::storage::persist_library(state.library);
    crate::storage::persist_covers(state.library);
}

/// Reads the world before writing any of it, because the tombstone needs the folder that placed this book and the shelf it was filed on.
fn purge_one(state: AppState, row: &Row, opts: PurgeOpts) {
    let row_id = row.id();
    let Some(book) = row.book() else {
        unlist_row(state, row_id);
        return;
    };
    let shelves = state.library.shelves.get_untracked();
    let folders = state.library.folders.get_untracked();
    let placed_by = folders
        .iter()
        .find(|f| f.placed.contains(&book.fp))
        .map(|f| f.id.clone());
    let home = placed_by
        .as_deref()
        .and_then(|folder_id| folder_shelf_of(&shelves, folder_id, &book.id));
    let entry = Tombstone::of(book, home, now_ms());
    let was_stored = book.origin.is_stored();

    state.library.books.update(|rows| {
        remove_row(rows, row_id);
        drop_dangling_links(rows);
    });
    state
        .library
        .shelves
        .update(|shelves| shelf::forget_everywhere(shelves, row_id));
    state
        .library
        .folders
        .update(|folders| tombstone(folders, &entry));
    sweep_book(state, book, was_stored && opts.delete_store_copy);
}

/// The guard is the duplicate rule's other half: two rows of one file share an address, and
/// with it the gloss and the cover keyed by that address, so a sweep that forgot the twin
/// would strip the highlights of a book that is still in the library.
fn sweep_path(state: AppState, path: &str, delete_store: bool) {
    let path_in_use = state.library.books.with_untracked(|rows| {
        book_rows(rows).any(|book| book.path() == path)
    });
    if path_in_use {
        return;
    }
    state.library.covers.update(|covers| {
        covers.remove(path);
    });
    if delete_store {
        wire::delete_stored(path);
    }
}

/// Every row's marks are keyed by its id, so removing a row always takes its own list and
/// never its twin's, and the address's other tables are then swept by their own rule.
fn sweep_book(state: AppState, book: &Book, delete_store: bool) {
    crate::storage::remove_gloss(&book.id);
    sweep_path(state, book.path(), delete_store);
}

/// The whole of a removal that is NOT a sweep: no tombstone, no cover, no highlights, no
/// store copy. One spelling because three callers wanted exactly this, and the half that is
/// easy to forget is the expensive one.
pub(crate) fn unlist_row(state: AppState, row_id: &str) {
    state.library.books.update(|rows| {
        remove_row(rows, row_id);
        drop_dangling_links(rows);
    });
    state
        .library
        .shelves
        .update(|shelves| shelf::forget_everywhere(shelves, row_id));
}

/// Remove one row, everywhere it is filed, and sweep the side data only it
/// used. Returns the row that went.
///
/// The conflict sheet's removal — a Replace's displaced row and a Merge's
/// dissolving one both go through here — and lighter than [`purge_one`] in
/// exactly one way: no tombstone. The content stays in the library through the
/// row on the other side of the question, so a folder rescan that re-found it
/// would resolve to that row, and a tombstone for a fingerprint the library
/// still holds is noise in the folder's restore menu until the next scan
/// prunes it.
pub(crate) fn drop_row(state: AppState, row_id: &str) -> Option<Row> {
    let row = state
        .library
        .books
        .with_untracked(|rows| find_row(rows, row_id).cloned())?;
    unlist_row(state, row_id);
    if let Some(book) = row.book() {
        sweep_book(state, book, book.origin.is_stored());
    }
    crate::storage::persist_library(state.library);
    Some(row)
}
