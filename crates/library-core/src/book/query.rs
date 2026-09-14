//! Looking a row up: by id, by address, by whatever the reader is holding.
//!
//! The reader does not always have an id — a gloss, a resume and a dropped file all
//! arrive with an address and sometimes a row, and the library has to answer with
//! the same book whichever of the two it was given.

use super::{Book, Row, book_rows, book_rows_mut};

/// The book an address resolves to. A relink changes the address a book
/// answers to, so this is the one lookup every path-keyed caller should make.
///
/// The FIRST row at the address, which is the answer a shared address has:
/// every shared row there agrees. A caller that knows which row the reader
/// means wants [`find_by_id`] instead — an address can hold a book of its own
/// beside its twins, and the two do not agree about anything.
pub fn find_by_path<'a>(rows: &'a [Row], path: &str) -> Option<&'a Book> {
    book_rows(rows).find(|b| b.path() == path)
}

/// The book an id names. The lookup every row-addressed caller should make:
/// an id survives a relink, a rename and a move between shelves, and it is the
/// only thing that tells two rows of one address apart.
pub fn find_by_id<'a>(rows: &'a [Row], id: &str) -> Option<&'a Book> {
    book_rows(rows).find(|b| b.id == id)
}

/// The same, for a write.
///
/// [`find_row_mut`] is the row-addressed answer and this is the book-addressed
/// one, which is what every writer of a resume point, an origin or a
/// measurement wants: a link has none of the three, and a caller that took the
/// row and then asked it for its book was spelling this out. Steps over links
/// the way [`book_rows_mut`] does, so a link can never be written through as
/// though it were a book.
pub fn find_book_mut<'a>(rows: &'a mut [Row], id: &str) -> Option<&'a mut Book> {
    book_rows_mut(rows).find(|b| b.id == id)
}

/// Id → row, in one pass: the index a level's members resolve against rather
/// than a walk of the library per member. Two modules each hand-rolled this
/// map — the sort that renders a shelf and the collision that screens an
/// arrival — and a third would have been a third first-wins rule. First wins
/// on a list that somehow holds one id twice, which is [`crate::book::add_book`]'s
/// own resolution and a state [`crate::book::sanitize`] heals on load.
pub fn index_by_id(rows: &[Row]) -> std::collections::HashMap<&str, &Row> {
    let mut index = std::collections::HashMap::with_capacity(rows.len());
    for row in rows {
        index.entry(row.id()).or_insert(row);
    }
    index
}

/// Where the reader left off in the book they are about to open: the resume
/// page and the fractional stream position (see [`Book::fraction`]), clamped
/// the way [`ReadPoint::settled`] clamps a write.
///
/// The row the reader named decides when the address holds more than one, so
/// a private book resumes where its own reader left off rather than where the
/// twin did. With no row named — a drop, an "open with", a dialog — the first
/// row at the address answers, which every shared row agrees on. One walk of
/// the list per open rather than one per question.
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
