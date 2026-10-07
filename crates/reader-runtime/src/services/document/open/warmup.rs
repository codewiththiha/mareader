//! Warming the thumbnail cache after the reader has settled.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::components::shell::sidebar::panels::thumbnails::geometry::THUMB_SCALE;

/// How many pages to pre-render. The rail shows roughly this many at once.
const WARM_PAGES: u32 = 16;

/// How long to wait before starting.
const DELAY_MS: u64 = 1500;

/// Pre-warm the thumbnail cache so the first sidebar open is all
/// cache blits.
pub(super) fn prewarm_thumbs(ctx: &crate::context::ReaderContext, num_pages: u32, stamp: u64) {
    let pages = num_pages.min(WARM_PAGES);
    let pane = ctx.pane;
    _ = set_timeout_with_handle(
        move || {
            // The pane's generation tells the timer whose warm-up it is.
            if !pane.owns_generation(stamp) {
                return;
            }
            let pdf = pane.pdf();
            spawn_local(async move {
                for p in 1..=pages {
                    // Same check per page; a dispose stops feeding the lane.
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
