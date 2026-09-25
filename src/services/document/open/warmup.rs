//! Warming the thumbnail cache after the reader has settled.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use pdf_engine::api as engine;

use crate::components::shell::sidebar::panels::thumbnails::geometry::THUMB_SCALE;

/// How many pages to pre-render. The rail shows roughly this many at once.
const WARM_PAGES: u32 = 16;

/// How long to wait before starting. Well past the reader's own first paints:
/// the resume jump's renders can still be landing a second in on big books,
/// and these offscreen renders must not queue in front of them.
const DELAY_MS: u64 = 1500;

/// Pre-warm the thumbnail cache so the FIRST sidebar open is all cache blits
/// instead of twenty concurrent pdf.js renders fighting the width animation
/// (the same call the auto-center idle prefetch uses). Sequential awaits keep
/// the engine queue from bursting.
///
/// The page count is read by the CALLER, not by the fire: this timer is
/// deliberately unowned on the Rust side — the warm-up belongs to the
/// document just opened, not to whichever component is alive in a moment —
/// and the ENGINE owns the safety instead: every prefetch queues in the
/// bounded thumbnail lane under the document's lane epoch, a prefetch that
/// lands after a close/swap is dropped by that epoch instead of filing into
/// the next document's cache, and the whole lifecycle is visible in
/// `stats()` (activePrefetches, started/completed/dropped) — so the dispose
/// baseline sees this fire and proves it drained.
pub(super) fn prewarm_thumbs(num_pages: u32, stamp: u64) {
    let pages = num_pages.min(WARM_PAGES);
    _ = set_timeout_with_handle(
        move || {
            // The timer outlives the open that scheduled it, so the stamp is
            // the only thing that can tell it whose warm-up it is: closed at
            // +500ms and another book opened at +800ms, an unverified fire
            // would prefetch THIS book's pages into the NEXT book's cache —
            // the engine epoch alone cannot catch that, because the fire is
            // a brand-new prefetch of the current epoch, not a stale one.
            if !crate::services::document::session::owns(stamp) {
                return;
            }
            spawn_local(async move {
                for p in 1..=pages {
                    // Same check per page: a close mid-warm-up must not keep
                    // feeding the lane under a document that no longer owns
                    // it (each orphaned prefetch would at best be a drop the
                    // counters have to account for).
                    if !crate::services::document::session::owns(stamp) {
                        return;
                    }
                    engine::prefetch_thumb(p, THUMB_SCALE).await;
                }
            });
        },
        std::time::Duration::from_millis(DELAY_MS),
    );
}
