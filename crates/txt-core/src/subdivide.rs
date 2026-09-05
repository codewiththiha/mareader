//! Splitting tall paragraphs so a page can be filled.
//!
//! A block is the paginator's atom: it is never cut mid-render. So a block
//! that is taller than the leftover space on a page is pushed whole to the
//! next one, and a 40-line paragraph in a fixed 5-line column would leave a
//! blank band on every page. Cutting the paragraph into line-bounded chunks
//! first is what lets the cutter pack tight, and because the chunks carry no
//! meaning of their own, nothing downstream has to know it happened.

use reflow_core::block::{SPLIT_MAX_LINES, TextBlock, subdivide_with};

/// Cut oversized plain-text blocks into line-bounded chunks.
///
/// Every plain-text block may be cut: its hard breaks are the natural cut
/// points, so a chunk renders exactly as the lines it holds did. The chunk
/// after the first is flagged a continuation, which is what keeps one
/// paragraph owning exactly one paragraph space however far the cut spreads.
///
/// `max_lines` of 0 disables the pass (the block list comes back untouched).
fn subdivide_with_budget(blocks: Vec<TextBlock>, max_lines: usize) -> Vec<TextBlock> {
    subdivide_with(blocks, max_lines, |_, _| true)
}

/// The default budget: [`SPLIT_MAX_LINES`] lines per chunk.
pub fn subdivide_paragraphs(blocks: Vec<TextBlock>) -> Vec<TextBlock> {
    subdivide_with_budget(blocks, SPLIT_MAX_LINES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reflow_core::block::BlockKind;

    #[test]
    fn a_tall_paragraph_becomes_line_bounded_chunks() {
        let source = (1..=7).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
        let out = subdivide_with_budget(vec![TextBlock::new(BlockKind::Text, source)], 3);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].text, "line 7");
        assert!(out[2].continuation);
    }

    #[test]
    fn the_default_budget_leaves_everything_a_page_fits_alone() {
        let short = TextBlock::new(BlockKind::Text, "one\ntwo\nthree\nfour\nfive");
        let long = TextBlock::new(BlockKind::Text, "l\n".repeat(6).trim());
        let out = subdivide_paragraphs(vec![short.clone(), long]);
        assert_eq!(out[0], short);
        assert_eq!(out[1].text, "l\nl\nl\nl\nl");
        assert_eq!(out[2].text, "l");
    }
}
