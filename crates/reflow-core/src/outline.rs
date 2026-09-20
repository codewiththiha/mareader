//! Format-independent block headings projected onto a live page cut.
use reader_core::outline::{OutlineNode, clamp_depth};

/// One heading, pointing at the block it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockHeading {
    /// The heading's text, markers and emphasis noise stripped.
    pub title: String,
    /// ATX level, 1 for `#`.
    pub level: u32,
    /// Index into the document's block list — the block whose first line this
    /// is. Stable for the whole session, because a block is never re-cut once
    /// it exists.
    pub block_index: usize,
}

/// Project the heading list onto the live page cut.
///
/// `block_to_page` is the 0-based page of every block, straight out of
/// [`crate::pager::block_page_index`]; a heading whose block the cut does
/// not know (a re-parse racing a re-cut) lands on the first page rather than
/// pointing nowhere. Depths are capped by [`clamp_depth`] because the panel
/// indents by level, and the outline is stored flattened in document order —
/// the same shape a PDF's tree arrives in.
pub fn headings_to_nodes(headings: &[BlockHeading], block_to_page: &[u32]) -> Vec<OutlineNode> {
    headings
        .iter()
        .map(|h| {
            let page = block_to_page.get(h.block_index).copied().unwrap_or(0) + 1;
            OutlineNode {
                title: h.title.clone(),
                page,
                depth: clamp_depth(h.level.saturating_sub(1)),
            }
        })
        .collect()
}

