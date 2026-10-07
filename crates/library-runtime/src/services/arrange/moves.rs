//! The moves a reader makes by hand: a drag, a lift, a second seat.

use leptos::prelude::*;

use library_core::book::{Row, find_row};
use library_core::conflict::Arrival;
use library_core::shelf::{self as shelf, ALL_SHELF, shelf_add};

use crate::services::conflict;

use super::asking::ask_move_copy;
use super::departure::bind_returned;

/// A whole drag, one call: the blob holds all of it or none.
pub fn move_many_to_shelf(
    state: crate::context::LibraryContext,
    book_ids: &[String],
    from: Option<String>,
    to: String,
    index: Option<usize>,
) {
    seat_many(state, book_ids, from, to, index, &[]);
}

/// `departed` marks rows a copy question already took: no return binds.
fn seat_many(
    state: crate::context::LibraryContext,
    book_ids: &[String],
    from: Option<String>,
    to: String,
    index: Option<usize>,
    departed: &[String],
) {
    if book_ids.is_empty() {
        return;
    }
    // Moving in place asks nothing; a failed copy costs that book its move.
    let arriving_elsewhere = match from.as_deref() {
        Some(from) => from != to,
        None => to != ALL_SHELF,
    };
    if arriving_elsewhere {
        let hand = RowMove::Seat {
            from: from.clone(),
            to: to.clone(),
            index,
        };
        if ask_move_copy(state, book_ids, &to, hand) {
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
            if from.is_some() {
                state.library.shelves.update(|shelves| {
                    for book_id in &clean_ids {
                        shelf::forget_everywhere(shelves, book_id);
                    }
                });
            }
            state
                .library
                .books
                .update(|rows| reorder_root(rows, &clean_ids, index));
            crate::services::persist_library(state.library);
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
        crate::services::persist_library(state.library);
        for book_id in &book_ids {
            if !departed.contains(book_id) {
                bind_returned(state, book_id, &to);
            }
        }
    }
    conflict::raise(state, conflicts);
}

/// Four books are four arrivals; a row that left is not one of them.
fn moved_arrivals(
    state: crate::context::LibraryContext,
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
                let arrival =
                    Arrival::moved(row_id.clone(), row.display_name(), to.to_string(), index);
                Some(match from {
                    Some(from) => arrival.leaving(from),
                    None => arrival,
                })
            })
            .collect()
    })
}

/// The import half of a screen is
/// [`crate::services::import::land_stored_copy`]'s business;
/// nothing in this module raises one.
fn clean_move_ids(clean: Vec<Arrival>) -> Vec<String> {
    clean.into_iter().filter_map(|a| a.moving).collect()
}

/// A value, not a boolean: a copy lands with no return to bind.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Departed {
    ThisGesture,
    No,
}

/// The gesture a copy question interrupted, finished by the sheet's answer.
#[derive(Clone, PartialEq)]
pub(super) enum RowMove {
    /// A drag or a bulk filing: land on `to`, lifted off `from`.
    Seat {
        from: Option<String>,
        to: String,
        index: Option<usize>,
    },
    /// One row's own move; the conflict sheet rides this form too.
    Row { to: String, index: Option<usize> },
    /// Out of every shelf the row was on, to the library's own top level.
    Unfile { shelf: String },
}

impl RowMove {
    /// Where the rows go: the gate and the sheet's answer both screen it.
    pub(super) fn to(&self) -> &str {
        match self {
            RowMove::Seat { to, .. } | RowMove::Row { to, .. } => to,
            RowMove::Unfile { .. } => ALL_SHELF,
        }
    }

    pub(super) fn resume(
        self,
        state: crate::context::LibraryContext,
        ids: Vec<String>,
        copies: Vec<String>,
    ) {
        match self {
            RowMove::Seat { from, to, index } => seat_many(state, &ids, from, to, index, &copies),
            RowMove::Row { to, index } => {
                let departed = if copies.is_empty() {
                    Departed::No
                } else {
                    Departed::ThisGesture
                };
                for id in &ids {
                    move_row(state, id, &to, index, departed);
                }
            }
            RowMove::Unfile { shelf } => unfile_books(state, &ids, &shelf),
        }
    }
}

/// Onto the named shelf; the root is a lift out of every shelf.
pub fn move_row(
    state: crate::context::LibraryContext,
    row_id: &str,
    shelf_id: &str,
    index: Option<usize>,
    departed: Departed,
) {
    {
        let row = row_id.to_string();
        let hand = RowMove::Row {
            to: shelf_id.to_string(),
            index,
        };
        if ask_move_copy(state, std::slice::from_ref(&row), shelf_id, hand) {
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
        crate::services::persist_library(state.library);
        return;
    }
    state.library.shelves.update(|shelves| {
        shelf::forget_everywhere(shelves, row_id);
        if let Some(shelf) = shelf::find_mut(shelves, shelf_id) {
            shelf::place(&mut shelf.books, row_id, index);
        }
    });
    crate::services::persist_library(state.library);
    if departed == Departed::No {
        bind_returned(state, row_id, shelf_id);
    }
}

/// The books stay in the library; the folder ledger is untouched.
pub fn unfile_books(state: crate::context::LibraryContext, book_ids: &[String], shelf_id: &str) {
    if book_ids.is_empty() {
        return;
    }
    let hand = RowMove::Unfile {
        shelf: shelf_id.to_string(),
    };
    if ask_move_copy(state, book_ids, ALL_SHELF, hand) {
        return;
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
        crate::services::persist_library(state.library);
    }
    conflict::raise(state, conflicts);
}

/// Membership only: the same rule covers a filing and a drag.
pub fn file_many(state: crate::context::LibraryContext, book_ids: &[String], shelf_id: &str) {
    if book_ids.is_empty() {
        return;
    }
    let (clean, conflicts) =
        conflict::screen(state, moved_arrivals(state, book_ids, shelf_id, None, None));
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
        crate::services::persist_library(state.library);
        for book_id in &book_ids {
            bind_returned(state, book_id, shelf_id);
        }
    }
    conflict::raise(state, conflicts);
}

/// One book, two memberships, nothing copied; the ledger stays quiet.
pub fn also_show(state: crate::context::LibraryContext, book_id: &str, shelf_id: &str) {
    file_many(state, &[book_id.to_string()], shelf_id);
}

/// Lifted out and put back together, not one at a time: indexes hold.
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
    let shift = index.map_or(0, |at| lifted.iter().filter(|(was, _)| *was < at).count());
    lifted.sort_by_key(|(_, row)| {
        row_ids
            .iter()
            .position(|row_id| row_id.as_str() == row.id())
            .unwrap_or(usize::MAX)
    });
    insert_many(rows, lifted.into_iter().map(|(_, row)| row), index, shift);
}

/// [`shelf::place`] for a drag, so each index counts one changed list.
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

/// Each one after the last, or the batch lands reversed.
pub(super) fn insert_many<T>(
    list: &mut Vec<T>,
    items: impl Iterator<Item = T>,
    index: Option<usize>,
    shift: usize,
) {
    let mut at = index.map_or(list.len(), |at| at.saturating_sub(shift));
    for item in items {
        at = at.min(list.len());
        list.insert(at, item);
        at += 1;
    }
}
