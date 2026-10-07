//! Row lookups: by id and by address.

use super::{Book, Row, book_rows, book_rows_mut};

/// The first row at the address.
pub(crate) fn find_by_path<'a>(rows: &'a [Row], path: &str) -> Option<&'a Book> {
    book_rows(rows).find(|b| b.path() == path)
}

/// The lookup every row-addressed caller should make.
pub fn find_by_id<'a>(rows: &'a [Row], id: &str) -> Option<&'a Book> {
    book_rows(rows).find(|b| b.id == id)
}

/// Steps over links, never writing through one.
pub fn find_book_mut<'a>(rows: &'a mut [Row], id: &str) -> Option<&'a mut Book> {
    book_rows_mut(rows).find(|b| b.id == id)
}

/// Id to row in one pass, first wins.
pub fn index_by_id(rows: &[Row]) -> std::collections::HashMap<&str, &Row> {
    let mut index = std::collections::HashMap::with_capacity(rows.len());
    for row in rows {
        index.entry(row.id()).or_insert(row);
    }
    index
}

/// The resume page and stream fraction, clamped.
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
