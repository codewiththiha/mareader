//! The moves a reader makes by hand: a drag between shelves, a lift out to the root, a second
//! membership, and the gate every hand-move rides when the row it holds reads in place
//! ([`super::departure`]).

use leptos::prelude::*;

use library_core::book::{Row, find_row};
use library_core::conflict::Arrival;
use library_core::shelf::{self as shelf, ALL_SHELF, shelf_add};

use crate::services::library::conflict;
use crate::state::AppState;

use super::departure::{bind_returned, convert_departures};

/// A whole drag in one call, whatever it held. The blob is written once for it — the rule
/// [`purge_books`] gives for a bulk removal, for the same reason: a reader who closes the
/// window halfway through a move should find all of it or none of it.
pub fn move_many_to_shelf(
    state: AppState,
    book_ids: &[String],
    from: Option<String>,
    to: String,
    index: Option<usize>,
) {
    seat_many(state, book_ids, from, to, index, &[]);
}

/// The public entry has converted nothing yet, so it passes an empty list and every stored
/// book it lands can bind a folder's moved-out log as a return. The departure gate's retry
/// passes the rows it just copied, and those land without binding: a departure is not a
/// return.
fn seat_many(
    state: AppState,
    book_ids: &[String],
    from: Option<String>,
    to: String,
    index: Option<usize>,
    departed: &[String],
) {
    if book_ids.is_empty() {
        return;
    }
    // A copy that fails costs that book its move and nothing else: it stays where it was. Every
    // screen, sheet and shelf write downstream sees the books as what they are about to be.
    if to != ALL_SHELF && from.as_deref().is_some_and(|f| f != to) {
        let (lifted_from, landed_on) = (from.clone(), to.clone());
        if convert_departures(state, book_ids, &to, move |rest, gone| {
            seat_many(state, &rest, lifted_from, landed_on, index, &gone)
        }) {
            return;
        }
    }
    if to == ALL_SHELF {
        let (clean, conflicts) = conflict::screen(
            state,
            moved_arrivals(state, book_ids, &to, index, from.as_deref()),
        );
        let clean_ids = clean_move_ids(clean);
        if !clean_ids.is_empty() {
            state
                .library
                .books
                .update(|rows| reorder_root(rows, &clean_ids, index));
            crate::storage::persist_library(state.library);
        }
        conflict::raise(state, conflicts);
        return;
    }

    let (clean, conflicts) = conflict::screen(
        state,
        moved_arrivals(state, book_ids, &to, index, from.as_deref()),
    );
    let book_ids = clean_move_ids(clean);
    if !book_ids.is_empty() {
        state.library.shelves.update(|shelves| {
            if let Some(from) = from.as_deref().filter(|id| *id != to)
                && let Some(shelf) = shelf::find_mut(shelves, from)
            {
                for book_id in &book_ids {
                    shelf::forget(&mut shelf.books, book_id);
                }
            }
            if let Some(shelf) = shelf::find_mut(shelves, &to) {
                place_many(&mut shelf.books, &book_ids, index);
            }
        });
        crate::storage::persist_library(state.library);
        for book_id in &book_ids {
            if !departed.contains(book_id) {
                bind_returned(state, book_id, &to);
            }
        }
    }
    conflict::raise(state, conflicts);
}

/// A drag of four books is four arrivals, and a row that went between the lift and the drop
/// is not one of them. The name is read here rather than by the rule, because the rule is pure
/// and holds no rows.
fn moved_arrivals(
    state: AppState,
    row_ids: &[String],
    to: &str,
    index: Option<usize>,
    from: Option<&str>,
) -> Vec<Arrival> {
    state.library.books.with_untracked(|rows| {
        row_ids
            .iter()
            .filter_map(|row_id| {
                let row = find_row(rows, row_id)?;
                let arrival = Arrival::moved(
                    row_id.clone(),
                    row.display_name(),
                    to.to_string(),
                    index,
                );
                Some(match from {
                    Some(from) => arrival.leaving(from),
                    None => arrival,
                })
            })
            .collect()
    })
}

/// The import half of a screen is [`crate::services::library::import::land_stored_copy`]'s business instead; nothing in this module raises one.
fn clean_move_ids(clean: Vec<Arrival>) -> Vec<String> {
    clean.into_iter().filter_map(|a| a.moving).collect()
}

/// A value rather than a boolean at the call site: a departure writes a moved-out log and the copy then lands, often on another shelf of the very folder it left.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Departed {
    ThisGesture,
    No,
}

/// Off every shelf it was on and onto the one named, at the slot the drop pointed at. The root has no member list, so a move there is a lift out of every shelf.
pub fn move_row(
    state: AppState,
    row_id: &str,
    shelf_id: &str,
    index: Option<usize>,
    departed: Departed,
) {
    {
        let (row, landed_on) = (row_id.to_string(), shelf_id.to_string());
        if convert_departures(
            state,
            std::slice::from_ref(&row),
            shelf_id,
            move |rest, gone| {
                if let Some(one) = rest.into_iter().next() {
                    let departed = if gone.contains(&one) {
                        Departed::ThisGesture
                    } else {
                        Departed::No
                    };
                    move_row(state, &one, &landed_on, index, departed);
                }
            },
        ) {
            return;
        }
    }
    if shelf_id == ALL_SHELF {
        state
            .library
            .shelves
            .update(|shelves| shelf::forget_everywhere(shelves, row_id));
        if index.is_some() {
            state
                .library
                .books
                .update(|rows| reorder_root(rows, &[row_id.to_string()], index));
        }
        crate::storage::persist_library(state.library);
        return;
    }
    state.library.shelves.update(|shelves| {
        shelf::forget_everywhere(shelves, row_id);
        if let Some(shelf) = shelf::find_mut(shelves, shelf_id) {
            shelf::place(&mut shelf.books, row_id, index);
        }
    });
    crate::storage::persist_library(state.library);
    if departed == Departed::No {
        bind_returned(state, row_id, shelf_id);
    }
}

/// The books stay in the library — a shelf holds ids and never held a byte — and the folder ledger is untouched.
pub fn unfile_books(state: AppState, book_ids: &[String], shelf_id: &str) {
    if book_ids.is_empty() {
        return;
    }
    {
        let lifted_from = shelf_id.to_string();
        if convert_departures(state, book_ids, ALL_SHELF, move |rest, _| {
            unfile_books(state, &rest, &lifted_from)
        }) {
            return;
        }
    }
    let (clean, conflicts) = conflict::screen(
        state,
        moved_arrivals(state, book_ids, ALL_SHELF, None, Some(shelf_id)),
    );
    let book_ids = clean_move_ids(clean);
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        let Some(shelf) = shelf::find_mut(shelves, shelf_id) else {
            return;
        };
        for book_id in &book_ids {
            moved |= shelf::forget(&mut shelf.books, book_id);
        }
    });
    if moved {
        crate::storage::persist_library(state.library);
    }
    conflict::raise(state, conflicts);
}

/// Membership only, so the same rule covers a bulk filing as covers a drag: a shelf holds ids, nothing here touches a filesystem, and a book already on the shelf is not moved to the end of it for being named twice.
pub fn file_many(state: AppState, book_ids: &[String], shelf_id: &str) {
    if book_ids.is_empty() {
        return;
    }
    let (clean, conflicts) = conflict::screen(
        state,
        moved_arrivals(state, book_ids, shelf_id, None, None),
    );
    let book_ids = clean_move_ids(clean);
    if !book_ids.is_empty() {
        state.library.shelves.update(|shelves| {
            let Some(shelf) = shelf::find_mut(shelves, shelf_id) else {
                return;
            };
            for book_id in &book_ids {
                shelf_add(shelf, book_id);
            }
        });
        crate::storage::persist_library(state.library);
        for book_id in &book_ids {
            bind_returned(state, book_id, shelf_id);
        }
    }
    conflict::raise(state, conflicts);
}

/// One book, two memberships, and nothing copied anywhere. The folder's ledger is untouched: the book stays placed where it was placed, which is what keeps the next rescan quiet about it.
pub fn also_show(state: AppState, book_id: &str, shelf_id: &str) {
    file_many(state, &[book_id.to_string()], shelf_id);
}

/// Lifted out and put back in together rather than one at a time: each book's removal shifts the tail left, so moving four in sequence would have the second one's index mean something the first one's already changed.
pub(super) fn reorder_root(rows: &mut Vec<Row>, row_ids: &[String], index: Option<usize>) {
    let mut lifted: Vec<(usize, Row)> = row_ids
        .iter()
        .filter_map(|row_id| {
            let was = rows.iter().position(|row| row.id() == row_id.as_str())?;
            Some((was, rows[was].clone()))
        })
        .collect();
    if lifted.is_empty() {
        return;
    }
    let mut positions: Vec<usize> = lifted.iter().map(|(was, _)| *was).collect();
    positions.sort_unstable_by_key(|was| std::cmp::Reverse(*was));
    for was in positions {
        rows.remove(was);
    }
    let shift = index.map_or(0, |at| {
        lifted.iter().filter(|(was, _)| *was < at).count()
    });
    lifted.sort_by_key(|(_, row)| {
        row_ids
            .iter()
            .position(|row_id| row_id.as_str() == row.id())
            .unwrap_or(usize::MAX)
    });
    insert_many(rows, lifted.into_iter().map(|(_, row)| row), index, shift);
}

/// [`shelf::place`] for one book and this for a drag: `place` retains and inserts, which is the same two steps, and doing them per book would leave each one's index counting a list the last one had already changed.
pub(super) fn place_many(members: &mut Vec<String>, book_ids: &[String], index: Option<usize>) {
    let shift = index.map_or(0, |at| {
        book_ids
            .iter()
            .filter(|book_id| {
                members
                    .iter()
                    .position(|member| member.as_str() == book_id.as_str())
                    .is_some_and(|was| was < at)
            })
            .count()
    });
    for book_id in book_ids {
        shelf::forget(members, book_id);
    }
    insert_many(members, book_ids.iter().cloned(), index, shift);
}

/// Each one after the last rather than each one at the same place, which would put them back reversed.
pub(super) fn insert_many<T>(list: &mut Vec<T>, items: impl Iterator<Item = T>, index: Option<usize>, shift: usize) {
    let mut at = index.map_or(list.len(), |at| at.saturating_sub(shift));
    for item in items {
        at = at.min(list.len());
        list.insert(at, item);
        at += 1;
    }
}
