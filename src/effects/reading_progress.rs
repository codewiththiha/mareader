//! Persist the reader's position in the current book.
//!
//! Watches `viewer.page` (which page-tracking keeps in sync with scrolling in
//! both view modes) and, while a document is open, keeps the current path's
//! `RecentBook.page` in the library up to date — so the next open of that book
//! resumes where the reader left off. Persistence is debounced so a fast
//! scroll through continuous mode is one localStorage write, not one per row.

use std::time::Duration;

use leptos::prelude::*;

use crate::core::document::DocStatus;
use crate::core::state::AppState;
use crate::util::storage::save_library;

/// Debounce for the library save: reading position settles this fast, and a
/// continuous scroll writes once instead of once per row boundary.
const SAVE_MS: u64 = 400;

/// Must be called once from the app root (ReaderView), alongside `fit_effect`.
pub fn reading_progress(state: AppState) {
    // Debounce timer handle, parked so it can never fire against a torn-down
    // app, and re-armed on each update.
    let timer = StoredValue::new_local(None::<TimeoutHandle>);

    Effect::new(move || {
        // Read deps unconditionally at the top (see page_tracking for the
        // subscription gotcha): status/path/page must all be subscribed.
        let status = state.doc.status.get();
        let path = state.doc.path.get();
        let page = state.viewer.page.get();

        if status != DocStatus::Ready {
            return;
        }
        let Some(path) = path else {
            return;
        };

        // No-op write guard: only touch the library when the page actually
        // moved, so the page-tracking syncs (which can re-write an equal page)
        // never dirty the list or trigger a save.
        let mut changed = false;
        state.library.update(|books| {
            if let Some(b) = books.iter_mut().find(|b| b.path == path) {
                if b.page != page {
                    b.page = page;
                    changed = true;
                }
            }
        });
        if !changed {
            return;
        }

        // Debounced persist. Capture the VALUE (not the signal) so the timer
        // can never read a disposed signal if it fires during teardown; a
        // further page change clears and re-arms this handle with a fresh copy.
        if let Some(h) = timer.get_value() {
            h.clear();
        }
        let snapshot = state.library.get_untracked();
        let handle = set_timeout_with_handle(
            move || save_library(&snapshot),
            Duration::from_millis(SAVE_MS),
        )
        .ok();
        timer.set_value(handle);
    });
}
