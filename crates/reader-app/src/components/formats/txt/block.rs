//! Plain text: a block shown exactly as it was written.
//!
//! The whole renderer is one `<div>` and `white-space: pre-wrap` in the
//! stylesheet — the format's only promise is that the bytes a reader chose are
//! the bytes they see, hard line breaks included. That is what makes fixed-line
//! prose, ASCII tables and code-ish notes read as authored, and it is why this
//! view has no options to configure.

use leptos::prelude::*;

use reflow_core::block::TextBlock;

#[component]
pub fn TxtBlockView(
    /// The block whose source is shown verbatim.
    block: TextBlock,
) -> impl IntoView {
    view! { <div class="tx-plain">{block.text}</div> }
}
