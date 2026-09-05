//! Resolving the chapter tree, after the reader is already up.
//!
//! The outline lands when it lands. Flattening a chapter tree resolves every
//! destination through the pdf.js worker — a per-entry round trip that
//! textbook outlines pay in seconds — and none of it is needed to paint the
//! first page. Asking for it here keeps `open` fast and the reader mounts the
//! moment page 1 is known; the panel shows a resolving state
//! (`outline_pending`) and fills when the tree is back.
//!
//! This is the PDF tail. A reflowable document's chapters are already in its
//! text, so there is nothing to resolve and nothing to wait for: its outline is
//! projected from the block list by `effects::reader::reflow_outline`, and the open
//! flow files it without a hop through here.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use pdf_engine::api as engine;

use crate::services::document::session;
use crate::state::AppState;

/// Ask the engine for this document's chapter tree and file it when it lands.
pub(super) fn resolve(state: AppState, path: String, stamp: u64) {
    // The clamp needs the page count, and the count is the open flow's answer,
    // already published by the time this runs (outline resolution starts after
    // `Ready`). Read untracked: this is a one-shot write, not a subscription.
    let page_count = state.reader.document.num_pages.get_untracked();
    spawn_local(async move {
        let entries = engine::outline().await.unwrap_or_default();
        // Two guards, and both earn their place: the stamp rules out a
        // superseded attempt on the SAME path (close-and-reopen), the path
        // rules out a tree that resolved for a different book.
        if session::owns(stamp)
            && state.reader.document.path.get_untracked().as_deref() == Some(path.as_str())
        {
            state.reader.document.set_pdf_outline(entries, page_count);
        }
        // The pending flag clears even when the book changed under the
        // lookup: a pending state that outlives its document would pin the
        // panel on "resolving".
        state.reader.document.outline_pending.set(false);
    });
}
