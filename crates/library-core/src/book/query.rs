//! Looking a row up: by id, by address, by whatever the reader is holding.

use super::{Book, Row, book_rows, book_rows_mut};

/// The FIRST row at the address, which is the answer a shared address has:
/// every shared row there agrees. A caller that knows which row the reader
/// means wants [`find_by_id`] instead.
pub fn find_by_path<'a>(rows: &'a [Row], path: &str) -> Option<&'a Book> {
    book_rows(rows).find(|b| b.path() == path)
}

/// The lookup every row-addressed caller should make: an id survives a relink, a rename and a move between shelves.
pub fn find_by_id<'a>(rows: &'a [Row], id: &str) -> Option<&'a Book> {
    book_rows(rows).find(|b| b.id == id)
}

/// Steps over links the way [`book_rows_mut`] does, so a link can never be written through as though it were a book.
pub fn find_book_mut<'a>(rows: &'a mut [Row], id: &str) -> Option<&'a mut Book> {
    book_rows_mut(rows).find(|b| b.id == id)
}

/// Id → row, in one pass: the index a level's members resolve against rather
/// than a walk of the library per member. First wins on a list that somehow
/// holds one id twice.
pub fn index_by_id(rows: &[Row]) -> std::collections::HashMap<&str, &Row> {
    let mut index = std::collections::HashMap::with_capacity(rows.len());
    for row in rows {
        index.entry(row.id()).or_insert(row);
    }
    index
}

/// The resume page and the fractional stream position (see [`Book::fraction`]),
/// clamped the way [`ReadPoint::settled`] clamps a write.
///
/// The row the reader named decides when the address holds more than one; with
/// no row named — a drop, an open-with, a dialog — the first row answers.
pub fn resume_point(rows: &[Row], book_id: Option<&str>, path: &str) -> (u32, Option<f64>) {
    let book = book_id
        .and_then(|id| find_by_id(rows, id))
        .filter(|b| b.path() == path)
        .or_else(|| find_by_path(rows, path));
    match book {
        Some(b) => (
            b.page.max(1),
            b.fraction.filter(|f| (0.0..=1.0).contains(f)),
        ),
        None => (1, None),
    }
}
