//! The removal, and everything the library holds about each removed book: the row, the memberships,
//! the tombstone that keeps a watched folder's rescan from putting the book straight back, the
//! cover, and the app's own copy — a book the library no longer holds is a file nothing will ever
//! read again.
//!
//! The reader's own data about the book — the highlights, the resume point and the name they gave
//! it — is the one thing a removal does not decide by itself: [`ReadingData`] carries the answer.

use leptos::prelude::*;

use library_core::book::{Book, Row, book_rows, drop_dangling_links, find_row, remove_row};
use library_core::folder::Tombstone;
use library_core::ledger::tombstone;
use library_core::shelf;

use crate::services as ipc;
use crate::services::covers::prune_now;
use runtime_contract::time::now_ms;

use super::folder_shelf_of;

/// What becomes of the reader's own data about a book a removal takes: the highlights, the place
/// they stopped at, and the name they gave it.
///
/// The removal sheet asks and this carries the answer. Every other removal keeps it, because a
/// purge nobody was asked about is not a licence to drop what a reader wrote — and the one removal
/// that is an answer of its own, the conflict sheet's Replace, says so on its own receipt before
/// the click and drops the data with the row ([`drop_row`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingData {
    /// It waits in the kept store for the file to come back, and the import that lands that file
    /// puts it back on the row it lands (`storage::kept`).
    Keep,
    /// It goes with the book.
    Delete,
}

/// One book is a batch of one: the sheet is the only caller and it always holds a list. Seven
/// things have to happen together per book, and doing any of them alone leaves something behind
/// that nothing will ever collect.
pub fn purge_books(state: crate::context::LibraryContext, row_ids: &[String], data: ReadingData) {
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
        purge_one(state, row, data);
    }

    prune_now(state);
    crate::services::persist_library(state.library);
    crate::services::persist_covers(state.library);
}

/// Reads the world before writing any of it: the tombstone needs the folder
/// that placed this book and the shelf it was filed on.
fn purge_one(state: crate::context::LibraryContext, row: &Row, data: ReadingData) {
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

    settle_reading_data(book, data);
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
    sweep_book(state, book);
}

/// The reader's own work about a book that is leaving: it either waits in the kept store for the
/// file to come back, or it goes with the book.
fn settle_reading_data(book: &Book, data: ReadingData) {
    match data {
        ReadingData::Keep => {
            let marks = storage::take_gloss(&book.id);
            storage::kept::remember(book, marks);
        }
        ReadingData::Delete => {
            storage::kept::forget(book);
            storage::remove_gloss(&book.id);
        }
    }
}

/// The cover and the bytes, once the row that read them is gone.
///
/// The guard is the duplicate rule's other half: two rows of one file share an address, and with
/// it the cover keyed by that address, so a sweep that forgot the twin would strip the cover of a
/// book that is still in the library — and, for a copy the app made, delete the file that book
/// reads.
///
/// A copy the app made goes whole, which is the removal's promise: nothing the library no longer
/// holds is left in the store. A linked book's bytes are the reader's and are never touched — and
/// the shell refuses a path outside the store whatever it is asked, so the two agree by rule and
/// not by the caller remembering.
fn sweep_book(state: crate::context::LibraryContext, book: &Book) {
    let path = book.path();
    let path_in_use = state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).any(|each| each.path() == path));
    if path_in_use {
        return;
    }
    state.library.covers.update(|covers| {
        covers.remove(path);
    });
    if book.origin.is_stored() {
        ipc::delete_stored(path);
    }
}

/// The whole of a removal that is NOT a sweep: no tombstone, no cover, no highlights, no
/// store copy. One spelling because three callers wanted exactly this, and the half that is
/// easy to forget is the expensive one.
pub(crate) fn unlist_row(state: crate::context::LibraryContext, row_id: &str) {
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
pub(crate) fn drop_row(state: crate::context::LibraryContext, row_id: &str) -> Option<Row> {
    let row = state
        .library
        .books
        .with_untracked(|rows| find_row(rows, row_id).cloned())?;
    unlist_row(state, row_id);
    if let Some(book) = row.book() {
        // The one destructive answer on the conflict sheet says so on its receipt — the name of the
        // row going and the highlights leaving with it — so this is an answer the reader gave and
        // not a purge nobody was asked about: the list and anything waiting for the file both go.
        storage::remove_gloss(&book.id);
        storage::kept::forget(book);
        sweep_book(state, book);
    }
    crate::services::persist_library(state.library);
    Some(row)
}
