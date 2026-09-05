//! The reflowable page model behind the paged view modes.
//!
//! The three paged modes (single, spread, horizontal) lay text pages out as
//! real A4 sheets: hosts are `A4 × scale` and the size model is
//! `PAGE_HEIGHT`. Vertical reading is NOT one of them anymore — the
//! continuous stream virtualizes blocks directly (see
//! `components::formats::reflow::stream`), so the flow-sized page units this effect
//! used to project for the vertical strip are gone with the strip itself.
//!
//! What stays is the A4 model's upkeep: whenever the cut or the settled
//! scale moves while a text document is open in a paged mode, the shared
//! measurement store (`css_heights`, which the zoom engine anchors against
//! and rescales in place every frame) is re-projected from the page count.
//! It never tracks the zoom scale itself: it writes at the settled scale,
//! and the engine's per-frame factors do the scaling afterwards.

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use reader_core::view::ViewMode;
use reflow_core::geometry::PAGE_HEIGHT;

use crate::state::AppState;

/// Install the projection. Needs the vertical strip's virtualizer because a
/// model change must rebuild its layout in the same flush.
pub fn reflow_layout(state: AppState, vertical: Virtualizer) {
    Effect::new(move |_| {
        let format = state.reader.format();
        let mode = state.reader.viewer.mode.get();
        if !format.is_reflowable() {
            return;
        }
        if mode == ViewMode::ScrollVertical {
            // The stream owns this mode; the page model has no consumer
            // here (the vertical page strip is not mounted), and writing it
            // would only fight the open flow's seed from the sidelines.
            return;
        }
        let cuts = state.reader.document.content.reflow.cuts.get();
        let scale = state.reader.viewer.zoom.visual_scale();
        let sizes = vec![PAGE_HEIGHT * scale; cuts.len()];

        // Skip the write (and the relayout) when the model already agrees —
        // a zoom tick rescales the store in place, and this effect must not
        // fight the engine back to the settled scale mid-tween.
        let same = state.reader.document.content.metrics.css_heights.with_untracked(|store| {
            store.len() == sizes.len()
                && store.iter().zip(&sizes).all(|(a, b)| (a - b).abs() < 0.5)
        });
        if same {
            return;
        }
        let metrics = state.reader.document.content.metrics;
        metrics.css_heights.set(sizes);
        let gap = state.reader.viewer.page_gap.get_untracked();
        vertical.rescale(1.0, metrics.strip_sizes(gap));
    });
}
