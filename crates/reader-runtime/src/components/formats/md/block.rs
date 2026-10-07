//! Markdown: one top-level construct, rendered.

use leptos::prelude::*;
use leptos_md::{MarkdownOptions, render_markdown_with_options};

use reflow_core::block::TextBlock;

use crate::components::formats::txt::TxtBlockView;

/// The options every Markdown block renders with.
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
        // `tx-md` is the stylesheet's handle on the construct.
        Ok(rendered) => view! { <div class="tx-md">{rendered}</div> }.into_any(),
        // A block the renderer refuses still deserves its words.
        Err(_) => view! { <TxtBlockView block=block /> }.into_any(),
    }
}
