//! Persist the reader's position in the current book.

use std::time::Duration;

use leptos::prelude::*;

use reader_core::document::DocStatus;
use runtime_contract::boundary::ShellApi;

/// Debounce for the library save: one write per settled position.
const SAVE_MS: u64 = 400;

/// Must be called once per pane (its mount), alongside the zoom sources.
pub fn reading_progress(state: crate::context::ReaderContext) {
    // Derived once, not per run: the effect below re-runs on every page turn.
    let zooming = state.reader.viewer.zooming();
    // The debounce handle, parked and re-armed on each update.
    let timer = StoredValue::new_local(None::<TimeoutHandle>);
    // Parked AND released, so a close mid-debounce clears the write.
    on_cleanup(move || {
        if let Some(handle) = timer.try_get_value().flatten() {
            handle.clear();
        }
    });
    // The session's own last SENT point; the launch resume seeds it.
    let last_sent = StoredValue::new_local(None::<(u32, Option<f64>)>);

    Effect::new(move || {
        // Read deps unconditionally at the top (see navigation_sync).
        let status = state.reader.document.status.get();
        let path = state.reader.document.path.get();
        let page = state.reader.viewer.page.get();
        // The stream's fractional position rides along with the page.
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
        // Stand down through a zoom transaction; the read is TRACKED.
        if zooming.get() {
            return;
        }

        if status != DocStatus::Ready {
            return;
        }
        let Some(_path) = path else {
            return;
        };
        // Never record an invalid position: 0 or past the document.
        if page == 0 || page > state.reader.document.num_pages.get_untracked() {
            return;
        }

        // No-op write guard: only touch the library when the position
        // moved.
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

        // Capture the VALUE, not the signal, so a teardown fire is safe.
        if let Some(h) = timer.try_get_value().flatten() {
            h.clear();
        }
        // Capture the CONTEXT (Copy handles) for the same reason.
        let ctx2 = state;
        let handle = set_timeout_with_handle(
            move || {
                // The timer can outlive the session, which then has nothing to
                // record.
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
