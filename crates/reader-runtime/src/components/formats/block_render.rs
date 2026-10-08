//! The one place the reader decides how a block is painted.

use leptos::prelude::*;

use reader_core::format::Format;
use reflow_core::block::TextBlock;

use super::md::MdBlockView;
use super::reflow::BlockSearchHits;
use super::txt::TxtBlockView;
use crate::state::ReaderState;

/// Which format's renderer a block gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockRender {
    Plain,
    Markdown,
}

impl BlockRender {
    /// The renderer a document's blocks are painted with.
    pub fn of_format(format: Format) -> Self {
        match format {
            Format::Markdown => BlockRender::Markdown,
            Format::Text | Format::Pdf => BlockRender::Plain,
        }
    }
}

#[component]
pub fn BlockView(
    /// Reader state, for an addressable row's search-hit layer.
    state: ReaderState,
    block: TextBlock,
    /// Which format's view paints inside the wrapper.
    render: BlockRender,
    /// The block's index, published as `data-block-index`.
    #[prop(optional)]
    index: Option<usize>,
) -> impl IntoView {
    let content = match render {
        BlockRender::Plain => view! { <TxtBlockView block=block.clone() /> }.into_any(),
        BlockRender::Markdown => view! { <MdBlockView block=block.clone() /> }.into_any(),
    };
    // A lookup-able row is one a search hit can be painted over.
    let hits = match index {
        Some(row) => view! {
            <BlockSearchHits state=state block=row />
            <crate::components::cefr::reflow::BlockCefrMarks state=state block=row />
        }
        .into_any(),
        None => ().into_any(),
    };
    // A split paragraph's tail drops its paragraph space.
    let class = if block.continuation {
        "tx-block tx-cont"
    } else {
        "tx-block"
    };
    // The id is the lookup half of the attribute pair.
    let id = index.map(crate::components::viewer::page_host::block_row_id);
    view! {
        <div class=class id=id data-block-index=index>
            {content}
            {hits}
        </div>
    }
}
