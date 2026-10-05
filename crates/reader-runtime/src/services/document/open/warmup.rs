//! Warming the thumbnail cache after the reader has settled.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

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
/// document just opened, not to whichever component is alive in a moment.
/// The SESSION owns the safety instead: the warm-up is bound to the pane's
/// session as it stands now, every prefetch queues in that session's
/// bounded thumbnail lane, a disposed session refuses them all, and the
/// lifecycle is visible in the session's `stats()` (activePrefetches,
/// started/completed/dropped) — so the dispose baseline sees this fire and
/// proves it drained.
pub(super) fn prewarm_thumbs(ctx: &crate::context::ReaderContext, num_pages: u32, stamp: u64) {
    let pages = num_pages.min(WARM_PAGES);
    let pane = ctx.pane;
    _ = set_timeout_with_handle(
        move || {
            // The timer outlives the open that scheduled it, so the pane's
            // generation is what tells it whose warm-up it is: closed at
            // +500ms and another book opened at +800ms, an unverified fire
            // would ask THIS book's pages of the NEXT book's session.
            if !pane.owns_generation(stamp) {
                return;
            }
            let pdf = pane.pdf();
            spawn_local(async move {
                for p in 1..=pages {
                    // Same check per page: a reopen, a suspend or the dispose
                    // mid-warm-up must not keep feeding the lane (each
                    // orphaned prefetch would at best be a drop the counters
                    // have to account for).
                    if !pane.owns_generation(stamp) || !pdf.still_current(&pane) {
                        return;
                    }
                    pdf.prefetch_thumb(p, THUMB_SCALE).await;
                }
            });
        },
        std::time::Duration::from_millis(DELAY_MS),
    );
}
