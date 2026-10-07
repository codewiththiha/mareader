//! Resolving the chapter tree after the reader is up.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

/// Ask the engine for this document's chapter tree and file it when it lands.
pub(super) fn resolve(state: crate::context::ReaderContext, path: String, stamp: u64) {
    // The clamp needs the page count, read untracked.
    let page_count = state.reader.document.num_pages.get_untracked();
    // The pane's OWN session resolves its own document's tree.
    let pdf = state.pane.pdf();
    spawn_local(async move {
        let entries = pdf.outline().await.unwrap_or_default();
        // Two guards: the pane's generation and the path.
        if state.pane.owns_generation(stamp)
            && state.reader.document.path.get_untracked().as_deref() == Some(path.as_str())
        {
            state.reader.document.set_pdf_outline(entries, page_count);
        }
        // The pending flag clears even when the book changed.
        state.reader.document.outline_pending.set(false);
    });
}
