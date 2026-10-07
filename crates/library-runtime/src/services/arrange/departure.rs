//! A read-at-place book leaving its ground becomes the library's copy.

use leptos::prelude::*;

use library_core::book::{Book, Origin, Row, find_book_mut, find_row};
use library_core::conflict::same_name;
use library_core::folder::{self as folder_ops, Tombstone, WatchedFolder};
use library_core::ledger::tombstone;
use library_core::shelf::ALL_SHELF;

use crate::services as ipc;
use crate::services::covers::{self, prune_now};
use crate::services::import;
use crate::services::toast;
use runtime_contract::time::now_ms;

use super::folder_shelf_of;

/// The rows a move to `to` takes into the store, in gesture order.
pub(super) fn converting_rows(
    state: crate::context::LibraryContext,
    ids: &[String],
    to: &str,
) -> Vec<String> {
    let books = state.library.books.get_untracked();
    let folders = state.library.folders.get_untracked();
    ids.iter()
        .filter(|id| converts_on_move(&books, &folders, id, to))
        .cloned()
        .collect()
}

/// The same question for one row, rather than a whole gesture.
pub(crate) fn converts_on_move_to(
    state: crate::context::LibraryContext,
    row_id: &str,
    to: &str,
) -> bool {
    let books = state.library.books.get_untracked();
    let folders = state.library.folders.get_untracked();
    converts_on_move(&books, &folders, row_id, to)
}

/// A stored book simply moves; a book no in-place folder placed owes nothing.
fn converts_on_move(books: &[Row], folders: &[WatchedFolder], row_id: &str, to: &str) -> bool {
    let Some((fp, path)) = find_row(books, row_id)
        .and_then(|row| row.book())
        .filter(|book| matches!(book.origin, Origin::Linked { .. }))
        .map(|book| (book.fp, book.path().to_string()))
    else {
        return false;
    };
    // A deleted rung answers `None`, so it never matches the destination.
    let placing: Vec<&WatchedFolder> = folders
        .iter()
        .filter(|f| f.mode().reads_in_place() && f.placed.contains(&fp))
        .collect();
    if placing.is_empty() {
        return false;
    }
    to == ALL_SHELF || !placing.iter().any(|f| f.rungs_for(&path).0 == Some(to))
}

/// The one primitive every departure copy is made through.
pub(super) async fn depart(state: crate::context::LibraryContext, rows: &[String]) -> Vec<String> {
    // Nothing to convert is no run to report — no card, no count.
    if rows.is_empty() {
        return Vec::new();
    }
    // One card for the whole gesture, not fifty.
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

/// Make a read-at-place book the library's own stored copy.
pub(crate) async fn convert_to_stored(
    state: crate::context::LibraryContext,
    row_id: &str,
    task: &str,
) -> Result<(), String> {
    let Some(book) = state
        .library
        .books
        .with_untracked(|rows| find_row(rows, row_id).and_then(|row| row.book()).cloned())
    else {
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
    crate::services::persist_library(state.library);
    Ok(())
}

/// Record that a placed book left as the library's own copy.
pub(crate) fn write_moved_stones(
    state: crate::context::LibraryContext,
    book: &Book,
    returned_row: Option<&str>,
) {
    let home = {
        let shelves = state.library.shelves.get_untracked();
        let folders = state.library.folders.get_untracked();
        folders
            .iter()
            .find(|f| f.placed.contains(&book.fp))
            .and_then(|f| folder_shelf_of(&shelves, &f.id, &book.id))
    };
    // Spelling eight fields would be a second place to remember a new one.
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

/// The bind is by ADDRESS: the log's `last_path` meets the row's own
/// `origin.source()`.
pub(super) fn bind_returned(state: crate::context::LibraryContext, row_id: &str, shelf_id: &str) {
    if shelf_id == ALL_SHELF {
        return;
    }
    let Some((name, src, measured)) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id).and_then(|row| {
            let book = row.book().filter(|b| b.origin.is_stored())?;
            Some((
                row.display_name(),
                book.origin.source().map(str::to_string),
                !book.fp_pending,
            ))
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
        if let Some(entry) = folder.ignored.iter_mut().find(|entry| is_the_one(entry))
            && entry.returned_row.as_deref() != Some(row_id)
        {
            entry.returned_row = Some(row_id.to_string());
            bound = true;
        }
    });
    if bound {
        crate::services::persist_library(state.library);
    }
}
