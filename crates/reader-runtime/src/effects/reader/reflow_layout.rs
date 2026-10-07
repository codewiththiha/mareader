//! The reflowable page model behind the paged view modes.

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use reader_core::view::ViewMode;
use reflow_core::geometry::PAGE_HEIGHT;

/// Install the projection for the paged modes' A4 model.
pub fn reflow_layout(state: crate::context::ReaderContext, vertical: Virtualizer) {
    Effect::new(move |_| {
        let format = state.reader.format();
        let mode = state.reader.viewer.mode.get();
        if !format.is_reflowable() {
            return;
        }
        if mode == ViewMode::ScrollVertical {
            // The stream owns this mode; do not fight its seed.
            return;
        }
        let cuts = state.reader.document.content.reflow.cuts.get();
        let scale = state.reader.viewer.zoom.visual_scale();
        let sizes = vec![PAGE_HEIGHT * scale; cuts.len()];

        // Skip the write when the model already agrees.
        if state.reader.document.content.metrics.heights_agree(&sizes) {
            return;
        }
        let metrics = state.reader.document.content.metrics;
        metrics.css_heights.set(sizes);
        let gap = state.reader.viewer.page_gap.get_untracked();
        vertical.rescale(1.0, metrics.strip_sizes(gap));
    });
}
