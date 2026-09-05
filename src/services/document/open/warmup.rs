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
/// The page count is read by the CALLER, not by the fire. This timer is
/// deliberately unowned — the warm-up belongs to the document that was just
/// opened, not to whichever component happens to be alive in a moment — so
/// the one thing the fire must not do is reach into the reader's signal
/// graph: a document closed inside that window would leave it reading an
/// arena that is gone. `prefetch_thumb` is an engine call and answers for
/// whatever is open when it lands, which is all a warm-up is.
pub(super) fn prewarm_thumbs(num_pages: u32) {
    let pages = num_pages.min(WARM_PAGES);
    _ = set_timeout_with_handle(
        move || {
            spawn_local(async move {
                for p in 1..=pages {
                    engine::prefetch_thumb(p, THUMB_SCALE).await;
                }
            });
        },
        std::time::Duration::from_millis(DELAY_MS),
    );
}
