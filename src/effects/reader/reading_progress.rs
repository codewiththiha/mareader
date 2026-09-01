//! Persist the reader's position in the current book.
//!
//! Watches `viewer.page` (which page-tracking keeps in sync with scrolling in
//! both view modes) and, while a document is open, keeps the current path's
//! `RecentBook.page` in the library up to date — so the next open of that book
//! resumes where the reader left off. Persistence is debounced so a fast
//! scroll through continuous mode is one localStorage write, not one per row.
//!
//! It stands down for the whole of a zoom transaction. The page counter is not
//! trustworthy while one is open — navigation_sync's dominant arm is standing
//! down and a held jump has not been replayed — and this is the one effect
//! that writes that untrustworthy value somewhere permanent.

use std::time::Duration;

use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use crate::state::AppState;
use crate::storage::save_library;

/// Debounce for the library save: reading position settles this fast, and a
/// continuous scroll writes once instead of once per row boundary.
const SAVE_MS: u64 = 400;

/// Must be called once from the app root (ReaderPage), alongside the zoom sources.
pub fn reading_progress(state: AppState) {
    // Derived once, not per run: the effect below re-runs on every page turn.
    let zooming = state.reader.viewer.zooming();
    // Debounce timer handle, parked so it can never fire against a torn-down
    // app, and re-armed on each update.
    let timer = StoredValue::new_local(None::<TimeoutHandle>);

    Effect::new(move || {
        // Read deps unconditionally at the top (see navigation_sync for the
        // subscription gotcha): status/path/page must all be subscribed.
        let status = state.reader.document.status.get();
        let path = state.reader.document.path.get();
        let page = state.reader.viewer.page.get();
        // A zoom transaction owns the geometry, and the page counter is not
        // trustworthy while it does: the dominant arm stands down for the
        // duration and a held navigation (the resume jump on open) has not
        // been replayed yet, so the page on show is the pre-jump one. Saving
        // it here is exactly how an open from the shelf used to overwrite a
        // real resume point with page 1. The read is TRACKED, so the effect
        // re-runs — with the settled page — on the frame the transaction
        // closes; nothing is lost by waiting.
        if zooming.get() {
            return;
        }

        if status != DocStatus::Ready {
            return;
        }
        let Some(path) = path else {
            return;
        };
        // Never record an invalid position: a page of 0 (or one past the
        // document) is a transient that escaped the syncs, and persisting it
        // would make the next open resume there.
        if page == 0 || page > state.reader.document.num_pages.get_untracked() {
            return;
        }

        // No-op write guard: only touch the library when the page actually
        // moved, so the page-tracking syncs (which can re-write an equal page)
        // never dirty the list or trigger a save.
        let mut changed = false;
        state.library.books.update(|books| {
            if let Some(b) = books.iter_mut().find(|b| b.path == path)
                && b.page != page
            {
                b.page = page;
                changed = true;
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
        let snapshot = state.library.books.with_untracked(|books| books.clone());
        let handle = set_timeout_with_handle(
            move || {
                if let Err(e) = save_library(&snapshot) {
                    e.report();
                }
            },
            Duration::from_millis(SAVE_MS),
        )
        .ok();
        timer.set_value(handle);
    });
}
