//! The path check: what a walk's measurement does to the rows it lands on.
//!
//! A file that moved inside a watched tree is the same book at a new address;
//! a row migrated from the previous schema carries a placeholder identity
//! nothing has measured yet.

use super::{Book, Row, book_rows, book_rows_mut};

/// Apply one path check to every book at that address, returning the ids it
/// touched.
///
/// Every row at the address shares its fate: a check that healed one row and
/// left a twin pending would hold every watched folder's rescan off forever.
/// Independent rows are no exception — this is the one place their opt-out
/// would strand a twin.
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
                // Going missing is news; being told twice is not. Clearing a
                // pending mark is news too: it releases the folder's rescan.
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

/// Add an imported book at the end of the library's order, or return the id
/// of the one already there.
///
/// Content identity decides, not the address: the same file reached through a
/// second watched folder is the same book (see [`crate::ledger`]). The first
/// row wins when duplicates the reader chose to keep share a fingerprint.
pub fn add_book(rows: &mut Vec<Row>, book: Book) -> String {
    // Never resolve to an independent row: that would file a private book on
    // a shelf the import never mentioned.
    if let Some(existing) = book_rows(rows).find(|b| b.fp == book.fp && !b.independent) {
        return existing.id.clone();
    }
    let id = book.id.clone();
    rows.push(Row::Book(book));
    id
}

/// Remove a row by id, returning it — a book or a link, since a shelf holds
/// both. What else the removal implies (ledger tombstone, shelf membership,
/// store copy, dangling links) is the caller's decision.
pub fn remove_row(rows: &mut Vec<Row>, id: &str) -> Option<Row> {
    let at = rows.iter().position(|r| r.id() == id)?;
    Some(rows.remove(at))
}

/// Drop every link whose target is no longer a book in the list: a pointer at
/// nothing renders, is clicked, and does nothing. Runs after any removal and
/// on every load via [`sanitize`].
pub fn drop_dangling_links(rows: &mut Vec<Row>) {
    // Owned ids: the set must not borrow the list `retain` walks mutably.
    let books: std::collections::HashSet<String> =
        book_rows(rows).map(|b| b.id.clone()).collect();
    rows.retain(|r| {
        !matches!(r, Row::Link { target, .. }
            if !books.contains(target) && !crate::id::is_shelf(target))
    });
}

/// The shelf-target half of [`drop_dangling_links`], which needs the shelf
/// list to answer.
pub fn drop_dead_shelf_links(rows: &mut Vec<Row>, shelves: &[crate::shelf::Shelf]) {
    rows.retain(|r| match r {
        Row::Link { target, .. } if crate::id::is_shelf(target) => {
            shelves.iter().any(|s| &s.id == target)
        }
        _ => true,
    });
}
