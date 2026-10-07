//! The reflowable document's outline, projected from its page cut.

use std::sync::Arc;

use leptos::prelude::*;

/// Keep the sidebar's chapter tree in step with the reflowable page cut.
pub fn reflow_outline(state: crate::context::ReaderContext) {
    Effect::new(move |_| {
        // The format is tracked first and decides participation.
        if !state.reader.format().is_reflowable() {
            return;
        }
        let reflow = state.reader.document.content.reflow;
        let headings = reflow.headings.get();
        let block_page = reflow.block_page.get();
        let nodes = md_core::headings_to_nodes(headings.as_slice(), block_page.as_slice());

        // Guarded: a `.set()` always notifies.
        let outline = state.reader.document.outline;
        let same = outline.with_untracked(|current| current.as_slice() == nodes.as_slice());
        if !same {
            outline.set(Arc::new(nodes));
        }
    });
}
