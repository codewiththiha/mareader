//! The one place the reader decides HOW a block is painted.
//!
//! Both reflowable formats lay their blocks out through the same machinery —
//! the same page host, the same stream, the same measure column — and at the end
//! of that machinery sits a single question: is this block's text literal, or is
//! it Markdown? Answering it used to mean a `match block.kind` inside one shared
//! block view, which put format names in the common code and made a third format
//! an edit to files that had no business knowing about it. Now the dispatch is
//! here: the layout asks for a [`BlockView`], the host hands it a
//! [`BlockRender`] read off the document's format, and only this file names the
//! two formats.
//!
//! What is shared stays here — the block wrapper, its continuation rule, and the
//! search-hit layer an addressable row carries — so the two views differ only in
//! what they put INSIDE the box, and a page and a measure column can never
//! disagree about spacing or about what is painted over it.

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
    /// Verbatim, hard line breaks preserved.
    Plain,
    /// One top-level construct, rendered as Markdown.
    Markdown,
}

impl BlockRender {
    /// The renderer a document's blocks are painted with.
    ///
    /// Derived from the DOCUMENT's format rather than from a per-block `kind`
    /// tag: a block's kind used to be the dispatch key, which let one file mix
    /// renderers mid-page by accident. One document, one renderer is the rule —
    /// and it is also what keeps a `.txt` that happens to contain `#` showing
    /// those characters, because a reader who opened it as text asked for bytes.
    pub fn of_format(format: Format) -> Self {
        match format {
            Format::Markdown => BlockRender::Markdown,
            Format::Text | Format::Pdf => BlockRender::Plain,
        }
    }
}

#[component]
pub fn BlockView(
    /// Reader state, for the search-hit layer an addressable row carries.
    ///
    /// Only such a row uses it: the measure column renders every block a second
    /// time, publishes no index, and so paints no hits — which is what keeps a
    /// query from doubling the boxes a document shows and, more importantly,
    /// from putting absolutely positioned children inside the element whose
    /// height is being measured.
    state: ReaderState,
    /// The block to paint.
    block: TextBlock,
    /// Which format's view paints inside the wrapper.
    render: BlockRender,
    /// The block's index in the document, published as `data-block-index`.
    ///
    /// This is the one handle a gloss mark has on the DOM: a reflowable mark
    /// remembers a block and a character range rather than a rect, and
    /// projecting it back to pixels means finding the element that renders
    /// that block (see `crate::components::ai::reflow_anchor`). Absent for the
    /// measure column, which renders every block a second time and must never
    /// answer for one.
    #[prop(optional)]
    index: Option<usize>,
) -> impl IntoView {
    let content = match render {
        BlockRender::Plain => view! { <TxtBlockView block=block.clone() /> }.into_any(),
        BlockRender::Markdown => view! { <MdBlockView block=block.clone() /> }.into_any(),
    };
    // A row that can be looked up is a row a search hit can be painted over:
    // the layer positions itself against the row's own box, so it belongs
    // inside it, and it walks the row's text — which the measure column's twin
    // must never do.
    let hits = match index {
        Some(row) => view! { <BlockSearchHits state=state block=row /> }.into_any(),
        None => ().into_any(),
    };
    // The tail of a split paragraph drops its paragraph space (see
    // `reflow_core::block::subdivide_with`); the class does exactly that, for
    // both formats, from here.
    let class = if block.continuation { "tx-block tx-cont" } else { "tx-block" };
    // The id is the lookup half of the pair the attribute is the identity half
    // of: `data-block-index` is what the engine's selection tracker walks up to
    // (and what tells a measure-column twin from a real row, since it gets no
    // index at all), the id is what the gloss projection resolves a mark's
    // block with, per mark per frame. See
    // [`crate::components::viewer::page_host::block_row_id`].
    let id = index.map(crate::components::viewer::page_host::block_row_id);
    view! {
        <div class=class id=id data-block-index=index>
            {content}
            {hits}
        </div>
    }
}
