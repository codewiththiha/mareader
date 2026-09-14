//! The departure: a read-at-place book leaving the ground that made it becomes the library's own
//! stored copy on the way out — its bytes its own, the ORIGINAL fingerprint left free for the
//! folder's log to keep, and its highlights following the address.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Book, Origin, find_book_mut, find_row};
use library_core::conflict::same_name;
use library_core::folder::{self as folder_ops, Tombstone};
use library_core::ledger::tombstone;
use library_core::shelf::ALL_SHELF;

use crate::services::library::covers::{self, prune_now};
use crate::services::library::toast;
use crate::services::library as wire;
use crate::state::AppState;
use crate::time::now_ms;

use super::folder_shelf_of;

/// Answers whether a conversion started, which is the caller's whole question: it did, so
/// the move has not happened yet. One spelling for the three moves that owe a departure — a
/// drag between shelves, a lift out to the root, and the one-row form the conflict sheet
/// rides.
pub(super) fn convert_departures(
    state: AppState,
    ids: &[String],
    to: &str,
    retry: impl FnOnce(Vec<String>, Vec<String>) + 'static,
) -> bool {
    if !tauri_bridge::has_tauri() {
        return false;
    }
    let converting: Vec<String> = ids
        .iter()
        .filter(|id| converts_on_move_to(state, id, to))
        .cloned()
        .collect();
    if converting.is_empty() {
        return false;
    }
    let all: Vec<String> = ids.to_vec();
    spawn_local(async move {
        let departed = depart(state, &converting).await;
        let failed: Vec<String> = converting
            .into_iter()
            .filter(|id| !departed.contains(id))
            .collect();
        let rest: Vec<String> = all.into_iter().filter(|id| !failed.contains(id)).collect();
        if !rest.is_empty() {
            retry(rest, departed);
        }
    });
    true
}

/// This is the ONE "a read-at-place book is leaving the ground that made it" primitive, and
/// both machines that need it call it rather than each writing their own.
pub(super) async fn depart(state: AppState, rows: &[String]) -> Vec<String> {
    let mut departed: Vec<String> = Vec::with_capacity(rows.len());
    for id in rows {
        match convert_to_stored(state, id).await {
            Ok(()) => departed.push(id.clone()),
            Err(message) => toast(state, message),
        }
    }
    covers::backfill_missing(state);
    departed
}

/// The three negatives are as load-bearing as the positive: a STORED book is already the
/// library's own and simply moves, and a book no in-place folder placed is nobody's
/// departure.
pub(crate) fn converts_on_move_to(state: AppState, row_id: &str, to: &str) -> bool {
    let Some((fp, path)) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .and_then(|row| row.book())
            .filter(|book| matches!(book.origin, Origin::Linked { .. }))
            .map(|book| (book.fp, book.path().to_string()))
    }) else {
        return false;
    };
    // An EMPTY list is the only thing the length says: no ledger is waiting, so no departure is owed.
    let rungs: Vec<Option<String>> = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .filter(|f| f.mode().reads_in_place() && f.placed.contains(&fp))
            .map(|f| f.rungs_for(&path).0.map(str::to_string))
            .collect()
    });
    if rungs.is_empty() {
        return false;
    }
    to == ALL_SHELF || !rungs.iter().any(|rung| rung.as_deref() == Some(to))
}

/// Make a read-at-place book the library's own stored copy: the departure half of a move, and
/// the only byte a hand-move ever writes. Why a move copies: the book is leaving the ground
/// that made it.
pub(crate) async fn convert_to_stored(state: AppState, row_id: &str) -> Result<(), String> {
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
    let (store, measured) =
        wire::copy_and_measure(&format!("move-{row_id}"), &path, row_id).await?;

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
    crate::storage::persist_library(state.library);
}

/// The bind is by NAME, which is the whole of the condition: the log remembers the name the
/// shelf showed, and a row wearing that exact name is the copy that came home.
pub(super) fn bind_returned(state: AppState, row_id: &str, shelf_id: &str) {
    if shelf_id == ALL_SHELF {
        return;
    }
    let Some(name) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .filter(|row| row.book().is_some_and(|b| b.origin.is_stored()))
            .map(|row| row.display_name())
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
        if let Some(entry) = folder
            .ignored
            .iter_mut()
            .find(|entry| entry.moved && same_name(&entry.label(), &name))
        {
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
