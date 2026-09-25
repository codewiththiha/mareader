//! Persist the reader's position in the current book.
//!
//! Watches `viewer.page` (kept in sync with scrolling in both view modes) and,
//! while a document is open, keeps the open book's resume point in the library
//! up to date, so the next open resumes where the reader left off. The write is
//! `library_core::book::record_read`'s — every row that shares the address moves,
//! and a row that is a book of its own moves alone — through the same seam an
//! open records through. Persistence is debounced: a fast scroll through
//! continuous mode is one localStorage write, not one per row.
//!
//! Stands down for the whole of a zoom transaction: the page counter is not
//! trustworthy while one is open — the dominant arm is standing down and a
//! held jump has not replayed — and this is the one effect that writes that
//! value somewhere permanent.

use std::time::Duration;

use leptos::prelude::*;

use app_state::boundary::ShellApi;
use pdf_engine::types::DocStatus;

/// Debounce for the library save: reading position settles this fast, and a
/// continuous scroll writes once instead of once per row boundary.
const SAVE_MS: u64 = 400;

/// Must be called once from the app root (ReaderPage), alongside the zoom sources.
pub fn reading_progress(state: crate::context::ReaderContext) {
    // Derived once, not per run: the effect below re-runs on every page turn.
    let zooming = state.reader.viewer.zooming();
    // Debounce timer handle, parked so it can never fire against a torn-down
    // app, and re-armed on each update.
    let timer = StoredValue::new_local(None::<TimeoutHandle>);
    // Parked AND released: a close that lands mid-debounce clears the pending
    // write here, so the timer cannot fire into a disposed session at all (the
    // `try_read_point` at the callback is the belt to this pair of braces).
    on_cleanup(move || {
        if let Some(handle) = timer.try_get_value().flatten() {
            handle.clear();
        }
    });
    // The session's own last SENT point (page, fraction): the launch resume
    // seeds it, so the first real movement is what writes.
    let last_sent = StoredValue::new_local(None::<(u32, Option<f64>)>);

    Effect::new(move || {
        // Read deps unconditionally at the top (see navigation_sync for the
        // subscription gotcha): status/path/page must all be subscribed.
        let status = state.reader.document.status.get();
        let path = state.reader.document.path.get();
        let page = state.reader.viewer.page.get();
        // The stream's fractional position rides along with the page: the
        // page remains the paged modes' resume point, the fraction is the same
        // position at full precision for the next continuous read. Only the
        // stream writes one — anything else clears a stale fraction an earlier
        // streamed session left behind. The scroll mirror is the tracked input
        // that moves it.
        let streaming = state.reader.reflow_streaming();
        let _scroll = if streaming {
            state.reader.viewer.scroll_top.get()
        } else {
            0.0
        };
        let fraction = if streaming {
            state.reader.stream_fraction()
        } else {
            None
        };
        // Stand down for the whole of a zoom transaction (see the module doc):
        // the read is TRACKED, so the effect re-runs — with the settled page —
        // on the frame the transaction closes, and nothing is lost by waiting.
        if zooming.get() {
            return;
        }

        if status != DocStatus::Ready {
            return;
        }
        let Some(_path) = path else {
            return;
        };
        // Never record an invalid position: a page of 0 (or one past the
        // document) is a transient that escaped the syncs, and persisting it
        // would make the next open resume there.
        if page == 0 || page > state.reader.document.num_pages.get_untracked() {
            return;
        }

        // No-op write guard: only touch the library when the position actually
        // moved, so position-tracking syncs (which can re-write an equal page)
        // never dirty the list or trigger a save. A fraction counts as moved
        // past half a percent — finer steps are scroll noise the debounce
        // would coalesce anyway.
        //
        // Which rows move is `library_core::book::rows_for_read`'s answer and
        // not the address's: the book the reader opened by name keeps its own
        // position when it is a book of its own, and every shared row at the
        // address moves otherwise. Read untracked on purpose — the id is
        // written before the path in an open, so the tracked `path` above is
        // already the subscription that re-runs this on a new document.
        // The moved check runs against the session's own last SENT point (the
        // launch resume seeded it): position-tracking syncs can re-write an
        // equal page, and a write that did not move must not reach the
        // boundary. A fraction counts as moved past half a percent — finer
        // steps are scroll noise the debounce would coalesce anyway.
        let _book_id = state.reader.document.book_id.get_untracked();
        let moved = match last_sent.try_get_value().flatten() {
            None => true,
            Some((lp, lf)) => {
                lp != page
                    || match (lf, fraction) {
                        (Some(old), Some(new)) => (new - old).abs() > 0.005,
                        (None, None) => false,
                        _ => true,
                    }
            }
        };
        if !moved {
            return;
        }
        let _ = last_sent.try_set_value(Some((page, fraction)));

        // Debounced persist. Capture the VALUE (not the signal) so the timer
        // can never read a disposed signal if it fires during teardown; a
        // further page change clears and re-arms this handle with a fresh
        // copy.
        if let Some(h) = timer.try_get_value().flatten() {
            h.clear();
        }
        // Debounced boundary write. Capture the CONTEXT (Copy handles only)
        // so the timer can never touch a disposed signal if it fires during
        // teardown; a further page change clears and re-arms this handle.
        let ctx2 = state;
        let handle = set_timeout_with_handle(
            move || {
                // The timer can outlive the session it was armed for (a close
                // inside the debounce window): a reader whose signals are gone
                // has no position to record, and the dispose flush already
                // carried the durable one across the boundary.
                let Some(point) = ctx2.try_read_point() else {
                    return;
                };
                ctx2.api.read_point(&point);
            },
            Duration::from_millis(SAVE_MS),
        )
        .ok();
        let _ = timer.try_set_value(handle);
    });
}
