//! What a flip of the viewer's mode owes the reader.
//!
//! Changing shape — single page, spread, continuous stream, horizontal strip —
//! is one write to `viewer.mode`, and four things have to follow it:
//!
//! * the incoming strip mounts fresh and anchors itself to the reader's page,
//!   so until it has, its scroll position is not the reader's page, and the
//!   scroll→page sync has to stand down for that flush;
//! * the continuous text stream has no page to fit, so entering it drops the
//!   fit and the zoom with it;
//! * the outgoing view's rasters are nobody's after the flip, and nothing
//!   necessarily renders right after, so the engine's own sweep would never
//!   fire — it is fired here;
//! * the fit is the next mode's to resolve, and reinterpreting the outgoing
//!   layout's fit against a new axis is a zoom jump, so the flip hands
//!   ownership over instead.
//!
//! This lived in `features/reader/page.rs`, in the run of effects before that
//! file's `view!`; the page installs it, after the layout prefs and the reflow
//! effects and before the zoom controller.

use leptos::prelude::*;

use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;

use crate::state::AppState;

/// Install the mode-change effect.
pub fn mode_change(state: AppState) {
    let vs = state.reader;

    let prev_mode = StoredValue::new(vs.viewer.mode.get_untracked());
    Effect::new(move |_| {
        let mode = vs.viewer.mode.get();
        let prev = prev_mode.get_value();
        if mode == prev {
            return;
        }
        prev_mode.set_value(mode);
        // The incoming view's strip (if it has one) mounts fresh and anchors
        // itself to `viewer.page` in `ScrollShell`; until it has, its
        // dominant is not the reader's page. Raised HERE, in the same flush
        // as the mode flip, so the scroll→page arm that re-runs for the flip
        // sees it and stands down rather than reading the unplaced strip.
        if matches!(mode, ViewMode::ScrollVertical | ViewMode::ScrollHorizontal) {
            vs.viewer.awaiting_anchor.set(true);
        }
        // Entering the continuous text stream resets the zoom to 1: the
        // stream has no page to fit — the window is the page, and type
        // size belongs to the typography settings — so a fit resolved
        // against the A4 page model would only shrink the text below its
        // setting. The paged modes re-resolve their own fit on entry (the
        // branch below), so nothing needs restoring on the way out.
        if mode == ViewMode::ScrollVertical
            && vs.reflow_streaming()
            && !vs.viewer.zooming().get_untracked()
        {
            vs.viewer.fit.set(FitMode::None);
            vs.viewer.zoom.initialize(1.0);
        }
        // A mode flip leaves the outgoing view's rasters behind and nothing
        // necessarily renders right after, so the engine's own sweep (which
        // only runs inside a render) would never fire. Release now.
        pdf_engine::api::sweep();
        let auto = state.settings.with(|s| s.layout.auto_scale);
        if mode == ViewMode::ScrollHorizontal {
            // Horizontal is one page per virtual item. Do not reinterpret the
            // outgoing layout's fit against the new axis: a single/vertical
            // width fit would become a height fit here and drop the readout
            // by almost half, while a spread width fit would jump the other
            // way. Hand ownership to the already-resolved `desired` scale so
            // every mode switch preserves the reader's zoom.
            vs.viewer.fit.set(FitMode::None);
        } else if matches!(mode, ViewMode::Spread) || (auto && mode.is_paginated()) {
            vs.viewer.fit.set(FitMode::Width);
        }
    });
}
