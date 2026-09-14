//! The facts about a book that a shelf surface paints, read back out of the library by id on the
//! frame they are asked for.
//!
//! A card and a row are keyed by id, and a keyed row is NOT re-created when its content changes
//! — a startup measurement marking the book missing, a relink moving its address, a rename, a
//! fold merging a twin into it — so every fact that can move is read here rather than captured.

use leptos::prelude::*;

use library_core::book::find_by_id;
use library_core::text::page_line;

use crate::state::AppState;

#[derive(Clone)]
pub(crate) struct BookFacts {
    /// The line a card falls back to when a title the document supplied gives the reader no way to tell two books called "Report" apart — and the key the cover cache answers to, which is why a relink has to move it.
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

/// One derive rather than one per field: the facts all move together, and a surface that read six signals would subscribe six times to one list. `None` is the beat between a removal and the list catching up.
pub(crate) fn book_facts(state: AppState, book_id: &str) -> Signal<Option<BookFacts>> {
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
