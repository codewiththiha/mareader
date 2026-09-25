//! What an overlay painted over a document re-derives on.
//!
//! A watcher re-derives its box on scroll, zoom and page change; a layer that
//! paints over the type — a gloss stroke, a search hit — has to re-derive on
//! everything that can move a block without moving the scroller. These are the
//! fingerprints for that, as `Signal<u64>` so an unchanged re-measure notifies
//! nobody.

use std::hash::Hash;

use leptos::prelude::*;

use crate::state::ReaderState;
use app_ui::epoch::epoch_signal;

/// What a layer painted over a page re-derives on.
///
/// A PDF's strokes are host-local at a fixed scale, so scale is the only thing
/// that moves them. A reflowable document's blocks are laid out by the browser:
/// the page cut, the typography and the scroll position all move a stroke, so
/// all three are in the fingerprint. It is a `u64` rather than the values
/// themselves so an unchanged re-measure notifies nothing.
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

/// An `invalidate` input for a watcher that has nothing beyond scroll, zoom and
/// the page number to react to — which is every PDF, since a page is fixed
/// pixels and cannot move under a mark.
pub fn no_invalidation() -> Signal<u64> {
    Signal::derive(|| 0u64)
}

/// The `invalidate` input a reflowable document needs: the page cut's
/// generation, the geometry it was cut with, and the stream's extent.
///
/// A re-cut moves blocks between pages, so a mark's page and its pixels both
/// change without anything scrolling or zooming. This is the signal that makes
/// the card and the Explain pill notice.
///
/// The typography is deliberately NOT read here. Every knob that moves type
/// moves the cut (the measurement pipeline re-publishes it), so the cut's generation
/// and `geometry` already cover it — and a knob that moves neither (the ink
/// dial, the column's alignment) cannot move a mark either. Reading settings
/// instead would re-derive every stroke on a colour change for nothing.
///
/// It is a FINGERPRINT rather than the vectors themselves: `Signal<u64>`
/// notifies only when the value differs, so a re-measure that re-cut nothing
/// costs one hash and wakes nobody.
///
/// The cut enters it as its GENERATION counter, not as its contents. This
/// derive re-runs on every scroll frame of a reflowable document (the stroke
/// layer reads it through [`layer_refresh`], which tracks scroll), and hashing
/// the split meant walking every page boundary of the open book once per frame
/// — hundreds of them in a long novel, to conclude, on almost every frame, that
/// nothing had moved. One counter read says the same thing. The counter's
/// granularity is coarser by design: it bumps on a re-publish that changed
/// nothing, and the cost of that is one wake of the consumers, which is less
/// than the hash it replaced.
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
        // The stream re-lays its blocks when the reading column's width moves
        // (a window resize, a page-margin change) without the page cut moving.
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
