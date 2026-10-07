//! What a flip of the viewer's mode owes the reader.

use leptos::prelude::*;

use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;

/// Install the mode-change effect.
pub fn mode_change(state: crate::context::ReaderContext) {
    let vs = state.reader;

    let prev_mode = StoredValue::new(vs.viewer.mode.get_untracked());
    Effect::new(move |_| {
        let mode = vs.viewer.mode.get();
        let prev = prev_mode.get_value();
        if mode == prev {
            return;
        }
        prev_mode.set_value(mode);
        // The incoming strip mounts fresh; the sync stands down.
        if matches!(mode, ViewMode::ScrollVertical | ViewMode::ScrollHorizontal) {
            vs.viewer.awaiting_anchor.set(true);
        }
        // Entering the stream resets the zoom to 1.
        if mode == ViewMode::ScrollVertical
            && vs.reflow_streaming()
            && !vs.viewer.zooming().get_untracked()
        {
            vs.viewer.fit.set(FitMode::None);
            vs.viewer.zoom.initialize(1.0);
        }
        // Release the outgoing rasters and zoom masks now.
        #[cfg(feature = "pdf")]
        {
            let pdf = state.pane.pdf();
            pdf.sweep();
            pdf.sweep_snapshots();
        }
        let auto = state.settings.with(|s| s.layout.auto_scale);
        if mode == ViewMode::ScrollHorizontal {
            // Hand ownership to the resolved `desired` scale.
            vs.viewer.fit.set(FitMode::None);
        } else if matches!(mode, ViewMode::Spread) || (auto && mode.is_paginated()) {
            vs.viewer.fit.set(FitMode::Width);
        }
    });
}
