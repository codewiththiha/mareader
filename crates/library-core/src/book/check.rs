//! The path check: what a walk's measurement does to the rows it lands on.
//!
//! A file that moved inside a watched tree is the same book at a new address,
//! and a row migrated from the previous schema carries a placeholder identity
//! nothing has measured.

use super::{Book, Row, book_rows, book_rows_mut};

/// Apply one path check to every book at that address, and say which books it
/// touched.
///
/// Every book at the address, for [`record_read`]'s reason: two rows of one
/// file share the address's fate, and a check that healed one and left the
/// other pending would hold every watched folder's rescan off forever. An
/// independent row is no exception — this is the one place its opt-out would
/// strand a twin.
pub fn apply_check(rows: &mut [Row], check: &crate::wire::PathCheck) -> Vec<String> {
    let measured = check.fingerprint();
    let mut touched = Vec::new();
    for book in book_rows_mut(rows).filter(|b| b.path() == check.path) {
        let changed = match measured {
            Some(fp) => {
                let changed = book.fp != fp || book.missing || book.fp_pending;
                book.heal(fp);
                changed
            }
            None => {
                // Going missing is news; being told twice is not. A book still owed its
                // first measurement is news too: the pending mark holds a folder's rescan off.
                let changed = !book.missing || book.fp_pending;
                book.missing = true;
                book.fp_pending = false;
                changed
            }
        };
        if changed {
            touched.push(book.id.clone());
        }
    }
    touched
}

/// Add an imported book at the END of the library's order, or return the id of
/// the one already there.
///
/// Content identity decides, not the address: the same file reached through a
/// second watched folder is the same book (see [`crate::ledger`]). The first
/// row wins when the shelf holds duplicates the reader asked to keep.
pub fn add_book(rows: &mut Vec<Row>, book: Book) -> String {
    // A shared row and never an independent one: an import that resolved to it
    // would file a private book on a shelf the question never mentioned.
    if let Some(existing) = book_rows(rows).find(|b| b.fp == book.fp && !b.independent) {
        return existing.id.clone();
    }
    let id = book.id.clone();
    rows.push(Row::Book(book));
    id
}

/// Remove a ROW by id, returning it — a book or a link, since a shelf holds
/// both. The caller decides what else the removal implies: a ledger tombstone
/// ([`crate::ledger::tombstone`]), a shelf membership ([`crate::shelf::forget`]),
/// the store copy when the app owns the bytes, and every link that pointed at a
/// book ([`drop_dangling_links`]).
pub fn remove_row(rows: &mut Vec<Row>, id: &str) -> Option<Row> {
    let at = rows.iter().position(|r| r.id() == id)?;
    Some(rows.remove(at))
}

/// Drop every link whose target is no longer a book in the list: a pointer at
/// nothing is a row that renders, is clicked, and does nothing. Called after any
/// removal that could have taken a book a link was pointing at, and by
/// [`sanitize`] on every load.
pub fn drop_dangling_links(rows: &mut Vec<Row>) {
    // Owned ids, so the set does not borrow the list the retain walks mutably.
    let books: std::collections::HashSet<String> =
        book_rows(rows).map(|b| b.id.clone()).collect();
    rows.retain(|r| {
        !matches!(r, Row::Link { target, .. }
            if !books.contains(target) && !crate::id::is_shelf(target))
    });
}

/// The shelf half of [`drop_dangling_links`], which needs the shelf list to
/// answer.
pub fn drop_dead_shelf_links(rows: &mut Vec<Row>, shelves: &[crate::shelf::Shelf]) {
    rows.retain(|r| match r {
        Row::Link { target, .. } if crate::id::is_shelf(target) => {
            shelves.iter().any(|s| &s.id == target)
        }
        _ => true,
    });
}
