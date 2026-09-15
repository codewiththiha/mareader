//! The departure: a read-at-place book leaving the ground that made it becomes the library's own
//! stored copy on the way out — its bytes its own, the ORIGINAL fingerprint left free for the
//! folder's log to keep, and its highlights following the address.

use leptos::prelude::*;

use library_core::book::{Book, Origin, Row, find_book_mut, find_row};
use library_core::conflict::same_name;
use library_core::folder::{self as folder_ops, Tombstone, WatchedFolder};
use library_core::ledger::tombstone;
use library_core::shelf::ALL_SHELF;

use crate::services::library::import;
use crate::services::library::toast;
use crate::services::library::covers::{self, prune_now};
use crate::services::library as ipc;
use crate::state::AppState;
use crate::time::now_ms;

use super::folder_shelf_of;

/// The rows a move to `to` takes into the store, in the order the gesture held them.
pub(super) fn converting_rows(state: AppState, ids: &[String], to: &str) -> Vec<String> {
    let books = state.library.books.get_untracked();
    let folders = state.library.folders.get_untracked();
    ids.iter()
        .filter(|id| converts_on_move(&books, &folders, id, to))
        .cloned()
        .collect()
}

/// The same question about one row, for the callers that hold a single id rather than a gesture.
pub(crate) fn converts_on_move_to(state: AppState, row_id: &str, to: &str) -> bool {
    let books = state.library.books.get_untracked();
    let folders = state.library.folders.get_untracked();
    converts_on_move(&books, &folders, row_id, to)
}

/// The three negatives are as load-bearing as the positive: a STORED book is already the
/// library's own and simply moves, and a book no in-place folder placed is nobody's
/// departure.
fn converts_on_move(books: &[Row], folders: &[WatchedFolder], row_id: &str, to: &str) -> bool {
    let Some((fp, path)) = find_row(books, row_id)
        .and_then(|row| row.book())
        .filter(|book| matches!(book.origin, Origin::Linked { .. }))
        .map(|book| (book.fp, book.path().to_string()))
    else {
        return false;
    };
    // An EMPTY list is the only thing the length says: no ledger is waiting, so no departure is owed.
    let rungs: Vec<Option<String>> = folders
        .iter()
        .filter(|f| f.mode().reads_in_place() && f.placed.contains(&fp))
        .map(|f| f.rungs_for(&path).0.map(str::to_string))
        .collect();
    if rungs.is_empty() {
        return false;
    }
    to == ALL_SHELF || !rungs.iter().any(|rung| rung.as_deref() == Some(to))
}

/// The ONE "a book is leaving the ground that made it" primitive, so the copy a move buys, the copy
/// a level coming apart buys and the copy a removal buys are one write. A book the store refused is
/// left out of the answer, which is how the caller knows to leave that book where it was.
///
/// `task` names the card the copy's beats land on: a batch of departures shares one card, and a
/// lone departure (a replace's conversion) carries one of its own.
pub(super) async fn depart(state: AppState, rows: &[String]) -> Vec<String> {
    // Nothing to convert is no run to report — no card, no count.
    if rows.is_empty() {
        return Vec::new();
    }
    // One card for the whole gesture: fifty books leaving their ground is one thing the
    // reader asked for, not fifty, and every copy's beats land on this one card.
    let label = match rows.len() {
        1 => state.library.row_name(&rows[0]),
        n => format!("{n} books"),
    };
    let task = import::begin_task(state, label);
    let mut departed: Vec<String> = Vec::with_capacity(rows.len());
    for id in rows {
        match convert_to_stored(state, id, &task).await {
            Ok(()) => departed.push(id.clone()),
            Err(message) => toast(state, message),
        }
    }
    import::finish_task(state, &task, departed.len() as u32, 0);
    covers::backfill_missing(state);
    departed
}

/// Make a read-at-place book the library's own stored copy: the departure half of a move, and
/// the only byte a hand-move ever writes. Why a move copies: the book is leaving the ground
/// that made it.
pub(crate) async fn convert_to_stored(
    state: AppState,
    row_id: &str,
    task: &str,
) -> Result<(), String> {
    let Some(book) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .and_then(|row| row.book())
            .cloned()
    }) else {
        return Err("That book is no longer in the library.".to_string());
    };
    if book.origin.is_stored() {
        return Ok(());
    }
    let path = book.path().to_string();
    let (store, measured) = ipc::copy_one(task, &path, row_id).await?;

    write_moved_stones(state, &book, None);

    state.library.books.update(|rows| {
        if let Some(book) = find_book_mut(rows, row_id) {
            book.become_stored(&path, store.clone(), measured);
        }
    });
    prune_now(state);
    crate::storage::persist_library(state.library);
    Ok(())
}

/// Every folder that placed its fingerprint records that the book left as the library's own
/// copy rather than died. `returned_row` names the row the file is represented by, for the two
/// answers that dissolve a linked row into a book the library already holds.
///
/// Persists nothing itself: every caller — a departure, a merge, a link at the copy — ends its
/// own transaction with a persist, and this row is one write inside it.
pub(crate) fn write_moved_stones(state: AppState, book: &Book, returned_row: Option<&str>) {
    let home = {
        let shelves = state.library.shelves.get_untracked();
        let folders = state.library.folders.get_untracked();
        folders
            .iter()
            .find(|f| f.placed.contains(&book.fp))
            .and_then(|f| folder_shelf_of(&shelves, &f.id, &book.id))
    };
    // Spelling all eight fields here would be a second place a new `Tombstone` field has to be remembered.
    let entry = Tombstone {
        moved: true,
        returned_row: returned_row.map(str::to_string),
        ..Tombstone::of(book, home, now_ms())
    };
    state
        .library
        .folders
        .update(|folders| tombstone(folders, &entry));
}

/// The bind is by ADDRESS, and the address is the one thing a copy cannot change: the log's
/// fp is the file's and the row's is its own copy's stamp, so the fingerprints can never
/// meet again — but the log remembers where the file stood (`last_path`) and the row
/// remembers where its bytes came from (`origin.source()`), and two books called "Dune"
/// in one folder left two logs from two addresses. A row with no address to name — a
/// legacy copy — falls back to the name, and only while it is still wearing its pending
/// placeholder.
pub(super) fn bind_returned(state: AppState, row_id: &str, shelf_id: &str) {
    if shelf_id == ALL_SHELF {
        return;
    }
    let Some((name, src, measured)) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .filter(|row| row.book().is_some_and(|b| b.origin.is_stored()))
            .map(|row| {
                let book = row.book().expect("the filter above held");
                (
                    row.display_name(),
                    book.origin.source().map(str::to_string),
                    !book.fp_pending,
                )
            })
    }) else {
        return;
    };
    let Some(folder_id) = state.library.shelf_folder_id(shelf_id) else {
        return;
    };
    let mut bound = false;
    state.library.folders.update(|folders| {
        let Some(folder) = folder_ops::find_mut(folders, &folder_id) else {
            return;
        };
        let is_the_one = |entry: &Tombstone| {
            if !entry.moved {
                return false;
            }
            match src.as_deref() {
                Some(src) => src == entry.last_path,
                None => !measured && same_name(&entry.label(), &name),
            }
        };
        if let Some(entry) = folder.ignored.iter_mut().find(|entry| is_the_one(entry)) {
            if entry.returned_row.as_deref() != Some(row_id) {
                entry.returned_row = Some(row_id.to_string());
                bound = true;
            }
        }
    });
    if bound {
        crate::storage::persist_library(state.library);
    }
}
