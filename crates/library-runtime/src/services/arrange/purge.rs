//! The removal, and everything the library holds about each book it takes.

use leptos::prelude::*;

use library_core::book::{Book, Row, book_rows, drop_dangling_links, find_row, remove_row};
use library_core::folder::Tombstone;
use library_core::ledger::tombstone;
use library_core::shelf;

use crate::services as ipc;
use crate::services::covers::prune_now;
use runtime_contract::time::now_ms;

use super::folder_shelf_of;

/// What becomes of the reader's own data about a book a removal takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadingData {
    /// It waits in the kept store for the file to come back.
    Keep,
    /// It goes with the book.
    Delete,
}

/// One book is a batch of one; seven things happen together per book.
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

/// Reads the world before writing: the tombstone needs its ground.
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

/// The reader's work: it waits in the kept store, or goes with the book.
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

/// A removal that is NOT a sweep: no tombstone, cover or store copy.
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

/// Remove one row everywhere it is filed, sweeping its side data.
pub(crate) fn drop_row(state: crate::context::LibraryContext, row_id: &str) -> Option<Row> {
    let row = state
        .library
        .books
        .with_untracked(|rows| find_row(rows, row_id).cloned())?;
    unlist_row(state, row_id);
    if let Some(book) = row.book() {
        // The destructive answer says so on its receipt: the data goes.
        storage::remove_gloss(&book.id);
        storage::kept::forget(book);
        sweep_book(state, book);
    }
    crate::services::persist_library(state.library);
    Some(row)
}
