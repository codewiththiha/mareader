//! The level's own name question: an import's three answers, a move's four.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Book, find_book_mut, find_by_id, fold_books};
use library_core::conflict::{Placement, PlacementAsk};
use library_core::shelf;

use super::{ConflictAsk, advance, apply_placement, member_slot, minted_name};
use crate::services::arrange::{
    Departed, ReadingData, convert_to_stored, converts_on_move_to, drop_row, memberships, move_row,
    purge_books, unlist_row, write_moved_stones,
};
use crate::services::covers;
use crate::services::import;

/// The name sheet's one answer: [`super::apply_placement`] runs it.
pub fn answer_placement(state: crate::context::LibraryContext, answer: Placement) {
    let Some(ask) = state.library.conflict.ask.get_untracked() else {
        return;
    };
    // No row to write: it went while the sheet was up.
    if ask.arrival.moving.is_none() && !ask.arrival.is_import() {
        advance(state);
        return;
    }
    apply_placement(state, &ask.placement(state), answer);
    advance(state);
}

// Four entry points, one per answer that has a row on the other side.

pub(super) fn as_new_placement(state: crate::context::LibraryContext, ask: &PlacementAsk) {
    as_new(state, &ask_of(ask));
}

pub(super) fn link_to_row(state: crate::context::LibraryContext, ask: &PlacementAsk, row_id: &str) {
    let own = ask_of(ask);
    // A move's *make link* is not the import's: the held row goes.
    let Some(gone_id) = own.arrival.moving.clone() else {
        add_link_at_target(state, &own, row_id);
        return;
    };
    let gone_book = state
        .library
        .books
        .with_untracked(|rows| find_by_id(rows, &gone_id).cloned());
    if let Some(book) = &gone_book
        && survivor_is_the_copy_of(state, row_id, book)
    {
        write_moved_stones(state, book, Some(row_id));
    }
    // The pointers at the dissolved row go with it, as in every removal.
    unlist_row(state, &gone_id);
    add_link_at_target(state, &own, row_id);
    crate::services::persist_library(state.library);
}

pub(super) fn merge_into_row(state: crate::context::LibraryContext, ask: &PlacementAsk) {
    merge(state, &ask_of(ask));
}

pub(super) fn replace_row(state: crate::context::LibraryContext, ask: &PlacementAsk) {
    replace(state, &ask_of(ask));
}

/// A bridge, not a rewrite: [`ConflictAsk`] carries the same three facts.
fn ask_of(ask: &PlacementAsk) -> ConflictAsk {
    ConflictAsk {
        arrival: ask.arrival.clone(),
        existing_id: ask.existing.id().to_string(),
        existing_name: ask.existing_name.clone(),
        kind: super::AskKind::NameCollision,
    }
}

/// Files a row onto every shelf named, in one write.
fn file_on_all(state: crate::context::LibraryContext, row_id: &str, shelves_named: &[String]) {
    state.library.shelves.update(|shelves| {
        for one in shelves.iter_mut() {
            if shelves_named.contains(&one.id) {
                shelf::shelf_add(one, row_id);
            }
        }
    });
}

/// Whether the survivor is the library's own copy of the row that dissolves.
fn survivor_is_the_copy_of(
    state: crate::context::LibraryContext,
    survivor: &str,
    gone: &Book,
) -> bool {
    state.library.books.with_untracked(|rows| {
        find_by_id(rows, survivor).is_some_and(|keep| keep.origin.is_store_copy_of(gone.path()))
    })
}

/// The survivor is the row the reader sees, and what every key names.
fn merge(state: crate::context::LibraryContext, ask: &ConflictAsk) {
    let survivor = ask.existing_id.clone();
    let Some(gone_id) = ask.arrival.moving.clone() else {
        return;
    };
    let gone_book = state
        .library
        .books
        .with_untracked(|rows| find_by_id(rows, &gone_id).cloned());
    if let Some(gone_book) = &gone_book {
        state.library.books.update(|rows| {
            if let Some(keep) = find_book_mut(rows, &survivor) {
                fold_books(keep, gone_book);
            }
        });
    }
    // Folding into the library's copy leaves the folder's file no row.
    if let Some(gone) = &gone_book
        && survivor_is_the_copy_of(state, &survivor, gone)
    {
        write_moved_stones(state, gone, Some(&survivor));
    }
    let inherited: Vec<String> = memberships(state, &gone_id)
        .into_iter()
        .map(|(id, _)| id)
        // The level the move left is the one shelf the survivor does not take.
        .filter(|id| ask.arrival.from.as_deref() != Some(id.as_str()))
        .collect();
    file_on_all(state, &survivor, &inherited);
    drop_row(state, &gone_id);
}

/// The arrival takes the displaced row's slot and every other shelf it was on.
fn replace(state: crate::context::LibraryContext, ask: &ConflictAsk) {
    let Some(moved_id) = ask.arrival.moving.clone() else {
        return;
    };
    // Read the world before writing: the slot is about the row about to go.
    let seat = member_slot(state, &ask.arrival.shelf_id, &ask.existing_id);
    let inherited: Vec<String> = memberships(state, &ask.existing_id)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    // The sheet said the cost: the row going, and its highlights with it.
    purge_books(
        state,
        std::slice::from_ref(&ask.existing_id),
        ReadingData::Delete,
    );
    let shelf_id = ask.arrival.shelf_id.clone();
    let index = seat.or(ask.arrival.index);
    // A read-at-place arrival becomes a copy before it is seated.
    if tauri_bridge::has_tauri() && converts_on_move_to(state, &moved_id, &shelf_id) {
        let name = state.library.row_name(&moved_id);
        spawn_local(async move {
            // A card of its own: the copy is one file through the store.
            let task = import::begin_task(state, name);
            let departed = match convert_to_stored(state, &moved_id, &task).await {
                Ok(()) => {
                    import::finish_task(state, &task, 1, 0);
                    Departed::ThisGesture
                }
                Err(message) => {
                    import::fail_task(state, &task, message);
                    Departed::No
                }
            };
            covers::backfill_missing(state);
            seat_replace(state, &moved_id, &shelf_id, index, &inherited, departed);
        });
        return;
    }
    seat_replace(state, &moved_id, &shelf_id, index, &inherited, Departed::No);
}

/// `departed` travels because a departure must not bind the log it wrote.
fn seat_replace(
    state: crate::context::LibraryContext,
    moved_id: &str,
    shelf_id: &str,
    index: Option<usize>,
    inherited: &[String],
    departed: Departed,
) {
    move_row(state, moved_id, shelf_id, index, departed);
    file_on_all(state, moved_id, inherited);
    crate::services::persist_library(state.library);
}

/// One spelling for the two answers that leave a link behind.
fn add_link_at_target(state: crate::context::LibraryContext, ask: &ConflictAsk, target: &str) {
    let name = state.library.row_name(target);
    let name = if name.trim().is_empty() {
        ask.arrival.name.clone()
    } else {
        name
    };
    crate::services::arrange::add_link(state, &name, target, &ask.arrival.shelf_id);
}

/// A moved row is renamed then moved: the rename frees the collision.
fn as_new(state: crate::context::LibraryContext, ask: &ConflictAsk) {
    let name = minted_name(state, ask);
    match &ask.arrival.moving {
        Some(row_id) => {
            crate::services::arrange::rename_row(state, row_id, &name);
            move_row(
                state,
                row_id,
                &ask.arrival.shelf_id,
                ask.arrival.index,
                Departed::No,
            );
        }
        None => {
            let Some(file) = ask.arrival.file.as_ref() else {
                return;
            };
            crate::services::import::land_stored_copy(
                state,
                file.clone(),
                Some(name),
                ask.arrival.shelf_id.clone(),
                ask.arrival.index,
            );
        }
    }
}
