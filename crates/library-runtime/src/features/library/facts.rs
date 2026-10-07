//! The facts a shelf surface paints, read back by id.

use leptos::prelude::*;

use library_core::book::find_by_id;
use library_core::text::page_line;

#[derive(Clone)]
pub(crate) struct BookFacts {
    /// The cover cache's key and the fallback line for two books of
    /// one name.
    pub path: String,
    pub title: String,
    pub author: Option<String>,
    pub author_line: String,
    pub page_line: String,
    pub progress: Option<f64>,
    pub missing: bool,
}

impl BookFacts {
    pub(crate) fn percent(&self) -> Option<String> {
        self.progress.map(|p| format!("{:.0}%", p * 100.0))
    }
}

/// One derive, because the facts move together; `None` is the beat
/// before a removal lands.
pub(crate) fn book_facts(
    state: crate::context::LibraryContext,
    book_id: &str,
) -> Signal<Option<BookFacts>> {
    let id = book_id.to_string();
    Signal::derive(move || {
        state.library.books.with(|rows| {
            find_by_id(rows, &id).map(|b| BookFacts {
                path: b.path().to_string(),
                title: b.title(),
                author_line: b.author().unwrap_or_else(|| page_line(b.page, b.num_pages)),
                author: b.author(),
                page_line: page_line(b.page, b.num_pages),
                progress: b.progress(),
                missing: b.missing,
            })
        })
    })
}
