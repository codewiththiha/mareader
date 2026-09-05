//! The reflowable document's outline, projected from its own page cut.
//!
//! A PDF's chapters are addresses written into the file; a Markdown document's
//! are `#` markers whose page depends on the reader's typography, their window
//! and the current cut. So this effect does not resolve anything — it takes the
//! headings the open flow found (block indices, in `ReflowContent::headings`) and
//! maps them through the live block→page table, which is the same table the pages
//! are drawn from.
//!
//! That is why the outline of a text document MOVES while you read it rather than
//! going stale: a re-measure that re-cuts the pages republishes `block_page`, and
//! the chapters follow the pagination instead of fighting it. It is also why no
//! `outline_pending` handshake is needed here — there is no async lookup to wait
//! on; the tree is complete the frame the cut is.
//!
//! The write is guarded, because a Leptos `.set()` always notifies: a re-cut that
//! leaves every heading on the page it was already on must not re-render the
//! sidebar panel, the floating label, or the reveal effect.
//!
//! A PDF is not this effect's document, and it says so by returning before it
//! reads anything: its tree is the engine's answer, filed by
//! `services::document::open::outline`. Had this effect written an empty tree for
//! it instead, the sidebar would flash empty on the way from a Markdown book to a
//! PDF — the one moment the two outlines are both in the state.

use std::sync::Arc;

use leptos::prelude::*;

use crate::state::AppState;

/// Keep the sidebar's chapter tree in step with the reflowable page cut.
pub fn reflow_outline(state: AppState) {
    Effect::new(move |_| {
        // The format is tracked FIRST and decides participation. Returning for a
        // PDF is not a lost subscription: opening a Markdown file flips `format`
        // (the open flow writes it before it publishes `Ready`), which re-runs
        // this effect, and from there the two reads below are live.
        if !state.reader.format().is_reflowable() {
            return;
        }
        let reflow = state.reader.document.content.reflow;
        let headings = reflow.headings.get();
        let block_page = reflow.block_page.get();
        let nodes = md_core::headings_to_nodes(headings.as_slice(), block_page.as_slice());

        // Guarded, because a `.set()` always notifies and a re-cut usually leaves
        // the chapters where they were.
        let outline = state.reader.document.outline;
        let same = outline.with_untracked(|current| current.as_slice() == nodes.as_slice());
        if !same {
            outline.set(Arc::new(nodes));
        }
    });
}

