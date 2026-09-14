//! Where the reader left off, and how a read is written down.
//!
//! A read is an UPDATE to a row rather than an insert into a second list — which
//! is what ended the drift the old "recent books" record had.

use super::{Book, Fingerprint, Origin, Row, find_by_id};

/// Where the reader is in a book: the resume page, the page count, and (for a
/// reflowable document read as one continuous stream) the fraction along it. One
/// value rather than three loose arguments, because the three always travel
/// together.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ReadPoint {
    pub page: u32,
    pub num_pages: u32,
    pub fraction: Option<f64>,
}

impl ReadPoint {
    pub fn fresh() -> Self {
        Self {
            page: 1,
            num_pages: 0,
            fraction: None,
        }
    }

    /// The point with an impossible fraction dropped and the page clamped to the
    /// first, so no writer can hand the library a resume point it has to
    /// second-guess later.
    pub fn settled(self) -> Self {
        Self {
            page: self.page.max(1),
            num_pages: self.num_pages,
            fraction: self.fraction.filter(|f| (0.0..=1.0).contains(f)),
        }
    }
}

/// Record a read: every book at `path` moves to `now_ms` with the resume point
/// the reader just reached, or the library gains a linked book when the reader
/// opened something it did not know.
///
/// EVERY book at the path, because a shelf can hold two rows of one file — a
/// duplicate the reader asked to keep — and the reading position is a fact about
/// the FILE, not about the row: both copies resume together.
pub fn record_read(
    rows: &mut Vec<Row>,
    path: &str,
    title: Option<String>,
    author: Option<String>,
    point: ReadPoint,
    now_ms: u64,
) -> Option<Book> {
    let at = rows_for_read(rows, None, path);
    if write_read(rows, &at, &title, &author, point, now_ms) {
        return None;
    }
    let point = point.settled();
    let title = crate::text::non_blank(title.as_deref()).map(str::to_string);
    let author = crate::text::non_blank(author.as_deref()).map(str::to_string);
    let book = Book {
        title,
        author,
        last_read_ms: now_ms,
        page: point.page,
        num_pages: point.num_pages,
        fraction: point.fraction,
        fp_pending: true,
        ..Book::new(
            crate::id::next_id(now_ms),
            Fingerprint::placeholder(path),
            reader_core::format::format_of(path),
            Origin::Linked {
                src: path.to_string(),
            },
            now_ms,
        )
    };
    rows.insert(0, Row::Book(book.clone()));
    Some(book)
}

/// Write one read to every row [`rows_for_read`] named, answering whether it
/// wrote anything: a read that found its rows is done, and one that found none
/// owes the library a book.
fn write_read(
    rows: &mut [Row],
    at: &[usize],
    title: &Option<String>,
    author: &Option<String>,
    point: ReadPoint,
    now_ms: u64,
) -> bool {
    if at.is_empty() {
        return false;
    }
    let point = point.settled();
    let title = crate::text::non_blank(title.as_deref());
    let author = crate::text::non_blank(author.as_deref());
    for i in at {
        // A link is never in the list `rows_for_read` answers with, and the `else` keeps that a property of this function rather than of its caller.
        let Some(book) = rows.get_mut(*i).and_then(Row::as_book_mut) else {
            continue;
        };
        book.page = point.page;
        book.num_pages = point.num_pages;
        book.fraction = point.fraction;
        book.last_read_ms = now_ms;
        book.missing = false;
        if crate::text::non_blank(book.title.as_deref()).is_none() {
            if let Some(t) = title {
                book.title = Some(t.to_string());
            }
        }
        if book.author.is_none() {
            book.author = author.map(str::to_string);
        }
    }
    true
}

/// The rows a reading position belongs to: the row the reader NAMED when it is a
/// book of its own (an independent row's resume point is that book's alone), and
/// every shared row at the address otherwise.
pub fn rows_for_read(rows: &[Row], book_id: Option<&str>, path: &str) -> Vec<usize> {
    let named = book_id
        .and_then(|id| find_by_id(rows, id))
        .filter(|b| b.path() == path);
    if let Some(book) = named
        && book.independent
    {
        let id = book.id.clone();
        return rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.id() == id)
            .map(|(i, _)| i)
            .collect();
    }
    let shared: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.book().is_some_and(|b| b.path() == path && !b.independent))
        .map(|(i, _)| i)
        .collect();
    if !shared.is_empty() {
        return shared;
    }
    rows.iter()
        .enumerate()
        .filter(|(_, r)| r.book().is_some_and(|b| b.path() == path))
        .map(|(i, _)| i)
        .collect()
}

/// [`record_read`] for an open that knows WHICH row the reader meant: an id is
/// the only thing that can tell two rows of one address apart. A shared row
/// answers exactly as [`record_read`] does; an independent one moves alone.
pub fn record_read_row(
    rows: &mut Vec<Row>,
    book_id: &str,
    path: &str,
    title: Option<String>,
    author: Option<String>,
    point: ReadPoint,
    now_ms: u64,
) -> Option<Book> {
    let at = rows_for_read(rows, Some(book_id), path);
    if write_read(rows, &at, &title, &author, point, now_ms) {
        return None;
    }
    record_read(rows, path, title, author, point, now_ms)
}
