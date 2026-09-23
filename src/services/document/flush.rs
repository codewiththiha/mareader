//! Writing the open book's resume point into the library NOW, instead of
//! leaving it to the progress effect's debounce.
//!
//! [`crate::effects::reader::reading_progress`] batches its localStorage write
//! behind a 400 ms debounce, which is right for a continuous scroll and wrong
//! for anything that ends the session inside that window: a close, and a
//! reload ([`crate::services::reload`]). Both used to be a page turn away from
//! losing the reader's place, so the flush they owe is one function rather
//! than one copy each.

use leptos::prelude::*;

use library_core::book::{Row, rows_for_read};
use pdf_engine::types::DocStatus;
use reader_core::view::ViewMode;

use crate::state::AppState;

/// Carry the open book's position into the library and save it.
///
/// A no-op while nothing is open or the open has not settled — there is no
/// position to have lost.
///
/// The save is unconditional, and that is the point of the function: the
/// progress effect keeps the library SIGNAL current as the reader moves, so a
/// flush that saved only what it had just changed would almost always find
/// nothing to save and leave the durable copy 400 ms behind. A reload replaces
/// the page and takes the pending timer with it; a quit takes the webview.
/// Neither waits.
pub(crate) fn flush_read_point(state: AppState) {
    if state.reader.document.status.get_untracked() != DocStatus::Ready {
        return;
    }
    let Some(path) = state.reader.document.path.get_untracked() else {
        return;
    };
    // Clamped to the book that is open, by the rule an open's resume point
    // goes through: a page of 0 or one past the end is a transient that
    // escaped the syncs, and writing it would be the next open's starting
    // point.
    let num_pages = state.reader.document.num_pages.get_untracked();
    let page = state.reader.viewer.page.get_untracked().clamp(1, num_pages.max(1));
    // The stream's fractional position rides along with the page: a paged
    // mode's resume point is a page, and the continuous stream's is the same
    // place at full precision. The split is the progress effect's own, so a
    // flush and a debounce can never disagree about what "where I got to"
    // means — including that a book read in pages carries no fraction.
    let streaming = state.reader.reflowable_now()
        && state.reader.viewer.mode.get_untracked() == ViewMode::ScrollVertical;
    // The stream lives in the format instance. The host's copy of the
    // fraction is the snapshot, not `stream_fraction`, which reads a tree
    // this heap does not have.
    let fraction = if crate::slot::active() {
        crate::slot::fraction_now()
    } else if streaming {
        state.reader.stream_fraction()
    } else {
        None
    };
    // The rows this read belongs to, by the same rule the progress debounce
    // writes and the open records: the book the reader named when it is a
    // book of its own, every shared row at the address otherwise. Read before
    // the caller's reset, which is what forgets the name.
    let book_id = state.reader.document.book_id.get_untracked();
    state.library.books.update(|books| {
        for i in rows_for_read(books, book_id.as_deref(), &path) {
            if let Some(b) = books.get_mut(i).and_then(Row::as_book_mut) {
                b.page = page;
                b.fraction = fraction;
            }
        }
    });
    crate::storage::persist_library(state.library);
}
