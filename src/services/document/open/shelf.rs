//! The library's record of the book that was just opened: the last step of
//! both open tails, and the only place the reader's own progress becomes a
//! library row. Everything about the DECISION — whether this address is a book
//! the library already holds, which rows a read belongs to, what a resume
//! point is allowed to be — is `library_core::book::record_read` and its
//! row-addressed `record_read_row`; this is the wiring to the signals and the
//! save.

use leptos::prelude::*;

use library_core::book::{ReadPoint, record_read, record_read_row};

use crate::services::library::covers::prune_now;
use crate::state::AppState;

/// Record the open: the book's resume point, its name and author, and the stamp
/// of the read. Persists immediately rather than on the progress debounce,
/// because an open is a moment the app could be closed right after.
///
/// A book the library did not have joins it as a linked book at the front of the
/// "All" order — reading in place is the default, and a file opened from a dialog
/// or a drop is not a file the app should copy anywhere.
pub(crate) fn record(state: AppState, path: &str, title: Option<String>, point: ReadPoint) {
    // Persist last path (the settings-watch effect writes localStorage
    // automatically). Kept for schema stability; the library below is the real
    // store.
    state
        .settings
        .update(|s| s.last_path = Some(path.to_string()));

    // The author is the document's own, already on the reader's identity by the
    // time an open reaches here. Read untracked: this is a write path, not a
    // view, and a subscription would only re-run it on somebody else's change.
    let author = state.reader.document.author.get_untracked();
    let now = crate::time::now_ms();
    // The row the reader opened by name — a card, a list row, the context
    // menu's Open — records against that row, so a book of its own keeps the
    // position its own reader reached instead of handing it to its twin at the
    // address. An open that arrived as nothing but an address (a drop, an
    // "open with", a dialog) has no row to name and follows the address.
    let book_id = state.reader.document.book_id.get_untracked();
    let mut books = state.library.books.get_untracked();
    let created = match book_id.as_deref() {
        Some(book_id) => record_read_row(&mut books, book_id, path, title, author, point, now),
        None => record_read(&mut books, path, title, author, point, now),
    };
    state.library.books.set(books);
    crate::storage::persist_library(state.library);

    if let Some(book) = created {
        // An address-only open now has a row of its own. Publish that identity
        // before the open tail derives the gloss key and loads highlights.
        state.reader.document.book_id.set(Some(book.id.clone()));
        // The open proved this file is readable and measured nothing about it,
        // so the row it just created carries a placeholder identity. One
        // metadata read fixes that — without it a watched folder would refuse
        // to rescan until the next launch.
        crate::services::library::verify_one(state, book.path().to_string());
        // A book joining the library can push the cover cache over its budget.
        // Pruning here rather than on a timer is what makes the cap a cap: the
        // cache is largest exactly when a new book arrives, and the cover this
        // open is about to render (see `super::cover`) lands after the prune.
        prune_now(state);
        crate::storage::persist_covers(state.library);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_open_publishes_the_created_row_as_the_session_identity() {
        let owner = Owner::new();
        owner.set();
        let state = AppState::default();

        record(state, "/books/dune.md", Some("Dune".to_string()), ReadPoint::fresh());

        let id = state.library.books.with_untracked(|rows| rows[0].id().to_string());
        assert_eq!(state.reader.document.book_id.get_untracked().as_deref(), Some(id.as_str()));
        assert_eq!(crate::services::document::gloss_key(&state.reader), id);
    }
}
