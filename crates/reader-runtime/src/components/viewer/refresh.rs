//! What an overlay painted over a document re-derives on.

use std::hash::Hash;

use leptos::prelude::*;

use crate::state::ReaderState;
use app_ui::epoch::epoch_signal;

/// What a layer painted over a page re-derives on.
pub fn layer_refresh(state: ReaderState) -> Signal<u64> {
    epoch_signal(move |hasher| {
        state.viewer.zoom.display.get().to_bits().hash(hasher);
        if state.reflowable() {
            state.viewer.scroll_top.get().to_bits().hash(hasher);
            state.viewer.container_size.get().0.to_bits().hash(hasher);
            let _ = reflow_invalidation(state).get();
        }
    })
}

/// The `invalidate` input for a fixed-pixel PDF.
pub fn no_invalidation() -> Signal<u64> {
    Signal::derive(|| 0u64)
}

/// The reflowable `invalidate`: the page cut's generation plus the
/// geometry.
pub fn reflow_invalidation(state: ReaderState) -> Signal<u64> {
    epoch_signal(move |hasher| {
        // `ViewMode` is `Eq` but not `Hash`, and its discriminant is all a
        // fingerprint needs.
        (state.viewer.mode.get() as u8).hash(hasher);
        state
            .document
            .content
            .reflow
            .cut_generation
            .get()
            .hash(hasher);
        let geo = state.document.content.reflow.geometry.get();
        geo.content_width.to_bits().hash(hasher);
        geo.content_height.to_bits().hash(hasher);
        // The stream re-lays when the column width moves.
        state
            .document
            .content
            .reflow
            .stream_total
            .get()
            .to_bits()
            .hash(hasher);
        state.viewer.container_size.get().0.to_bits().hash(hasher);
    })
}
