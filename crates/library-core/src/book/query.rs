//! Row lookups: by id and by address.

use super::{Book, Row, book_rows, book_rows_mut};

/// The first row at the address — every shared row there agrees, so the first
/// is the answer. A caller that knows which row the reader means wants
/// [`find_by_id`] instead.
pub fn find_by_path<'a>(rows: &'a [Row], path: &str) -> Option<&'a Book> {
    book_rows(rows).find(|b| b.path() == path)
}

/// The lookup every row-addressed caller should make: an id survives a
/// relink, a rename and a move between shelves.
pub fn find_by_id<'a>(rows: &'a [Row], id: &str) -> Option<&'a Book> {
    book_rows(rows).find(|b| b.id == id)
}

/// Steps over links like [`book_rows_mut`], so a link can never be written
/// through as though it were a book.
pub fn find_book_mut<'a>(rows: &'a mut [Row], id: &str) -> Option<&'a mut Book> {
    book_rows_mut(rows).find(|b| b.id == id)
}

/// Id → row in one pass, so resolving a level's members is a lookup rather
/// than a walk of the library per member. First wins if an id appears twice.
pub fn index_by_id(rows: &[Row]) -> std::collections::HashMap<&str, &Row> {
    let mut index = std::collections::HashMap::with_capacity(rows.len());
    for row in rows {
        index.entry(row.id()).or_insert(row);
    }
    index
}

/// The resume page and fractional stream position (see [`Book::fraction`]),
/// clamped like [`ReadPoint::settled`] clamps a write.
///
/// `book_id` decides when the address holds more than one row; with no id — a
/// drop, an open-with, a dialog — the first row at the path answers.
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
