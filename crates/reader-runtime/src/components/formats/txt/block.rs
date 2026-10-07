//! Plain text: a block shown exactly as written, hard breaks included.

use leptos::prelude::*;

use reflow_core::block::TextBlock;

#[component]
pub fn TxtBlockView(
    /// The block whose source is shown verbatim.
    block: TextBlock,
) -> impl IntoView {
    view! { <div class="tx-plain">{block.text}</div> }
}
