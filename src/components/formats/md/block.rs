//! Markdown: one top-level construct, rendered.
//!
//! A block's text goes through `leptos-md` (pulldown-cmark) as its own
//! document, so a heading never shares a layout atom with the paragraph under it
//! — which is what lets the paginator treat each block as one unpackable unit.
//!
//! Typography does not live here on purpose: every style the settings control
//! (font, size, spacing, justification) is inherited from the page host's inline
//! style, so a block renders identically on a page and in the measure column, at
//! any scale, with no props for it.

use leptos::prelude::*;
use leptos_md::{MarkdownOptions, render_markdown_with_options};

use reflow_core::block::TextBlock;

use crate::components::formats::txt::TxtBlockView;

/// The options every Markdown block renders with.
///
/// GFM is on (tables and task lists are everyday Markdown), links open in a
/// new tab, and raw HTML is REFUSED: these are the reader's own files, but
/// a Markdown document is still untrusted input, and there is nothing a
/// reading view needs from inline `<script>` or `<style>`. Code blocks get
/// no theme classes — the reader's stylesheet owns the look, and keeps it
/// in every tint.
fn markdown_options() -> MarkdownOptions {
    MarkdownOptions::new()
        .with_gfm(true)
        .with_language_classes(true)
        .with_new_tab_links(true)
        .with_allow_raw_html(false)
        .without_code_theme()
}

#[component]
pub fn MdBlockView(
    /// The block whose Markdown source becomes one construct.
    block: TextBlock,
) -> impl IntoView {
    match render_markdown_with_options(&block.text, markdown_options()) {
        // The `tx-md` wrapper is the stylesheet's handle on the rendered
        // construct: one block, one wrapper, so the rules can never leak past
        // their construct.
        Ok(rendered) => view! { <div class="tx-md">{rendered}</div> }.into_any(),
        // A block the renderer refuses still deserves its words.
        Err(_) => view! { <TxtBlockView block=block /> }.into_any(),
    }
}
