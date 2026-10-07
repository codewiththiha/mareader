//! The path check: what a measurement does to the rows it lands on.

use super::{Book, Row, book_rows, book_rows_mut};

/// Apply one path check to every book at that address.
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
                // Going missing is news; clearing a pending mark too.
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

/// Add an imported book at the end of the order.
pub fn add_book(rows: &mut Vec<Row>, book: Book) -> String {
    // Never resolve to an independent row.
    if let Some(existing) = book_rows(rows).find(|b| b.fp == book.fp && !b.independent) {
        return existing.id.clone();
    }
    let id = book.id.clone();
    rows.push(Row::Book(book));
    id
}

/// Remove a row by id, returning it.
pub fn remove_row(rows: &mut Vec<Row>, id: &str) -> Option<Row> {
    let at = rows.iter().position(|r| r.id() == id)?;
    Some(rows.remove(at))
}

/// Drop every link whose target is no longer in the list.
pub fn drop_dangling_links(rows: &mut Vec<Row>) {
    // Owned ids: the set must not borrow the list `retain` walks mutably.
    let books: std::collections::HashSet<String> = book_rows(rows).map(|b| b.id.clone()).collect();
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
