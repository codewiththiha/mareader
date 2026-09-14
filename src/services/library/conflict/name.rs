//! The level's own name question, and the answers it has: an import's three (go to
//! the row that is here, add as new, make a link) and a move's four.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{find_book_mut, find_by_id, fold_books, Book};
use library_core::conflict::{Placement, PlacementAsk};
use library_core::shelf;

use super::{advance, apply_placement, member_slot, minted_name, ConflictAsk};
use crate::services::library::arrange::{
    ReadingData, converts_on_move_to, convert_to_stored, drop_row, memberships, move_row,
    purge_books, unlist_row, write_moved_stones, Departed,
};
use crate::services::library::covers;
use crate::services::library::toast;
use crate::state::AppState;

/// The one answer function the name sheet calls, whichever shape the question is: an
/// import's three and a move's three are the same five answers read against a different
/// offer list, and [`super::apply_placement`] is what the click reaches.
pub fn answer_placement(state: AppState, answer: Placement) {
    let Some(ask) = state.library.conflict.ask.get_untracked() else {
        return;
    };
    // A move whose arrival names no row has nothing to write: the row went while the sheet was up.
    if ask.arrival.moving.is_none() && !ask.arrival.is_import() {
        advance(state);
        return;
    }
    apply_placement(state, &ask.placement(state), answer);
    advance(state);
}

// Four entry points, one per answer that has a row on the other side of it, each the
// existing rule with the question read off a [`PlacementAsk`] instead of the sheet's own
// ask type.

pub(super) fn as_new_placement(state: AppState, ask: &PlacementAsk) {
    as_new(state, &ask_of(ask));
}

pub(super) fn link_to_row(state: AppState, ask: &PlacementAsk, row_id: &str) {
    let own = ask_of(ask);
    // A MOVE's *make link* is not the import's: the row the reader was holding goes, the
    // pointers at it go with it, and — when the survivor is the library's own copy of the
    // file that row read — the folder that placed it takes a moved-out log naming the
    // survivor.
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
    // The pointers at the dissolved row go with it, as they do in every removal.
    unlist_row(state, &gone_id);
    add_link_at_target(state, &own, row_id);
    crate::storage::persist_library(state.library);
}

pub(super) fn merge_into_row(state: AppState, ask: &PlacementAsk, _row_id: &str) {
    merge(state, &ask_of(ask));
}

pub(super) fn replace_row(state: AppState, ask: &PlacementAsk, _row_id: &str) {
    replace(state, &ask_of(ask));
}

/// A bridge rather than a rewrite: every rule below was written against [`ConflictAsk`],
/// and the two carry the same three facts — the arrival, the id of the thing already
/// there, and its name.
fn ask_of(ask: &PlacementAsk) -> ConflictAsk {
    ConflictAsk {
        arrival: ask.arrival.clone(),
        existing_id: ask.existing.id().to_string(),
        existing_name: ask.existing_name.clone(),
        kind: super::AskKind::NameCollision,
    }
}

/// Whether the row that survives is the library's own copy OF the row that dissolves.
/// One question in one place, because the two answers that dissolve a row — a merge into
/// the copy and a link at it — both write the folder's moved-out log on this condition
/// and nothing else.
fn survivor_is_the_copy_of(state: AppState, survivor: &str, gone: &Book) -> bool {
    state
        .library
        .books
        .with_untracked(|rows| find_by_id(rows, survivor).is_some_and(|keep| {
            keep.origin.is_store_copy_of(gone.path())
        }))
}

/// The survivor is the row the reader can already see here, and its id is what every
/// shelf holding it and every key in storage already names, so it is the one that stays.
/// The marks move while both rows can still be read.
fn merge(state: AppState, ask: &ConflictAsk) {
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
    // A read-at-place book folding into the library's own stored copy of ITS content leaves
    // the folder's file with no row to answer for it, so the folder takes a moved-out log
    // bound to the survivor.
    if let Some(gone) = &gone_book
        && survivor_is_the_copy_of(state, &survivor, gone)
    {
        write_moved_stones(state, gone, Some(&survivor));
    }
    let inherited: Vec<String> = memberships(state, &gone_id)
        .into_iter()
        .map(|(id, _)| id)
        // The level the move left is the one shelf the survivor does NOT take over: the departure
        // is the point of the move.
        .filter(|id| ask.arrival.from.as_deref() != Some(id.as_str()))
        .collect();
    state.library.shelves.update(|shelves| {
        for one in shelves.iter_mut() {
            if inherited.contains(&one.id) {
                shelf::shelf_add(one, &survivor);
            }
        }
    });
    drop_row(state, &gone_id);
}

/// The arrival takes the displaced row's SLOT and every OTHER shelf it was filed on:
/// a replace that quietly took a book off shelves the question never mentioned is a
/// removal the reader did not ask for.
fn replace(state: AppState, ask: &ConflictAsk) {
    let Some(moved_id) = ask.arrival.moving.clone() else {
        return;
    };
    // Read the world before writing any of it: the slot and the memberships are about the row that is about to go.
    let seat = member_slot(state, &ask.arrival.shelf_id, &ask.existing_id);
    let inherited: Vec<String> = memberships(state, &ask.existing_id)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    // The sheet said what this answer costs — the row going, and its highlights with it.
    purge_books(
        state,
        std::slice::from_ref(&ask.existing_id),
        ReadingData::Delete,
    );
    let shelf_id = ask.arrival.shelf_id.clone();
    let index = seat.or(ask.arrival.index);
    // A read-at-place arrival becomes the library's own copy before it is seated, and the seating waits for the copy.
    if tauri_bridge::has_tauri() && converts_on_move_to(state, &moved_id, &shelf_id) {
        spawn_local(async move {
            let departed = match convert_to_stored(state, &moved_id).await {
                Ok(()) => Departed::ThisGesture,
                Err(message) => {
                    toast(state, message);
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

/// `departed` is the replace's own copy of the gate's answer, and it travels because a
/// departure must not bind the moved-out log it just wrote.
fn seat_replace(
    state: AppState,
    moved_id: &str,
    shelf_id: &str,
    index: Option<usize>,
    inherited: &[String],
    departed: Departed,
) {
    move_row(state, moved_id, shelf_id, index, departed);
    state.library.shelves.update(|shelves| {
        for one in shelves.iter_mut() {
            if inherited.contains(&one.id) {
                shelf::shelf_add(one, moved_id);
            }
        }
    });
    crate::storage::persist_library(state.library);
}

/// One spelling for the two answers that leave a link behind, because the name is the
/// whole of what makes the row recognisable beside the book it points at.
fn add_link_at_target(state: AppState, ask: &ConflictAsk, target: &str) {
    let name = state.library.row_name(target);
    let name = if name.trim().is_empty() {
        ask.arrival.name.clone()
    } else {
        name
    };
    state
        .library
        .add_link(&name, target, &ask.arrival.shelf_id);
}

/// A moved row is renamed and then moved: the rename is what frees the collision, and a
/// move that did not rename would ask the same question again on the way in.
fn as_new(state: AppState, ask: &ConflictAsk) {
    let name = minted_name(state, ask);
    match &ask.arrival.moving {
        Some(row_id) => {
            state.library.rename_row(row_id, &name);
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
            crate::services::library::import::land_stored_copy(
                state,
                file.clone(),
                Some(name),
                ask.arrival.shelf_id.clone(),
                ask.arrival.index,
            );
        }
    }
}
