//! The chapter tree a Markdown file carries in its headings.
//!
//! A PDF's outline is a dictionary the file was authored with; a Markdown
//! document's is the `#` markers in its body. Once extracted they are the same
//! thing — [`reader_core::outline::OutlineNode`], which is what lets the
//! sidebar's outline panel, the active-entry memo and the floating chapter label
//! serve a Markdown book with no format branch in sight.
//!
//! The one real difference is *where a chapter points*. A Markdown heading has
//! no page number: the page depends on the reader's typography, their window and
//! the current page cut. So the extraction is keyed on the BLOCK the heading
//! starts in ([`MarkdownHeading::block_index`]), and [`headings_to_nodes`] turns
//! those into pages against the live block→page map. That is also why the reader
//! re-derives the tree after the measure column re-cuts the document: the
//! chapters follow the pagination instead of fighting it.

use reader_core::outline::{OutlineNode, clamp_depth};
use reflow_core::block::TextBlock;
use reflow_core::source::normalize;

use crate::ast::heading_of_line;

/// One heading, pointing at the block it opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownHeading {
    /// The heading's text, markers and emphasis noise stripped.
    pub title: String,
    /// ATX level, 1 for `#`.
    pub level: u32,
    /// Index into the document's block list — the block whose first line this
    /// is. Stable for the whole session, because a block is never re-cut once
    /// it exists.
    pub block_index: usize,
}

/// Scan a Markdown source for its headings, in document order.
///
/// The scan runs on the RAW file rather than on the parsed blocks, and counts
/// block boundaries exactly as [`reflow_core::block::split_blocks`] does (a
/// blank line closes a block, a fence keeps its interior blank lines): one
/// pass over the text, no allocation of block strings just to find a dozen
/// headings. Fence state is not decoration here — a `#` inside a code sample is
/// a shell prompt, not a chapter.
pub fn extract_headings(raw: &str) -> Vec<MarkdownHeading> {
    let text = normalize(raw);
    let mut headings = Vec::new();
    let mut in_fence = false;
    let mut fence_marker = "";
    // The block the current line belongs to, and how many blocks have opened.
    // Counting them here — rather than parsing the blocks and searching them —
    // is what keeps this one pass over the source.
    let mut current: Option<usize> = None;
    let mut opened = 0usize;
    for line in text.split('\n') {
        let trimmed = line.trim();
        if !in_fence && reflow_core::block::is_fence_open(trimmed) {
            // The fence line itself belongs to a block (that is how the
            // splitter sees it), so it opens one if the blank line before it
            // had closed the previous.
            if current.is_none() {
                current = Some(opened);
                opened += 1;
            }
            in_fence = true;
            fence_marker = reflow_core::block::fence_marker_of(trimmed);
            continue;
        }
        if in_fence {
            let marker_char = fence_marker.chars().next().unwrap_or('`');
            if trimmed.starts_with(fence_marker)
                && trimmed.trim_end_matches(marker_char).trim().is_empty()
            {
                in_fence = false;
                // The closer ends the block for the counter too, or the
                // heading under it would be attributed to the fenced block.
                current = None;
            }
            continue;
        }
        if in_fence {
            continue;
        }
        if trimmed.is_empty() {
            current = None;
            continue;
        }
        if current.is_none() {
            current = Some(opened);
            opened += 1;
        }
        if let Some((level, title)) = heading_of_line(trimmed) {
            headings.push(MarkdownHeading { title, level, block_index: current.unwrap_or(0) });
        }
    }
    headings
}

/// The headings of an ALREADY-PARSED document, keyed on its final blocks.
///
/// [`extract_headings`] is the cheap path for a caller that has the source; this
/// is the correct path for the open flow, because the blocks it returns have
/// been through `subdivide_prose` and a split shifts every index after it. Keyed
/// on the final list, an outline entry points at the block the interface will
/// actually paint, which is what lets the page number be derived from the live
/// cut rather than chased after it.
///
/// A heading is a block that is not a continuation and whose first line is an
/// ATX heading — so a `#` inside a fenced sample never joins the tree (a fence is
/// one block, and it opens with its ticks).
pub fn headings_of_blocks(blocks: &[TextBlock]) -> Vec<MarkdownHeading> {
    blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| !block.continuation)
        .filter_map(|(index, block)| {
            let (level, title) = heading_of_line(block.first_line())?;
            Some(MarkdownHeading { title, level, block_index: index })
        })
        .collect()
}

/// Project the heading list onto the live page cut.
///
/// `block_to_page` is the 0-based page of every block, straight out of
/// [`reflow_core::pager::block_page_index`]; a heading whose block the cut does
/// not know (a re-parse racing a re-cut) lands on the first page rather than
/// pointing nowhere. Depths are capped by [`clamp_depth`] because the panel
/// indents by level, and the outline is stored flattened in document order —
/// the same shape a PDF's tree arrives in.
pub fn headings_to_nodes(headings: &[MarkdownHeading], block_to_page: &[u32]) -> Vec<OutlineNode> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{parse_markdown, subdivide_prose};
    use reflow_core::block::{BlockKind, TextBlock, split_blocks};

    fn blocks_of(md: &str) -> Vec<TextBlock> {
        split_blocks(&normalize(md), BlockKind::Markdown, true)
    }

    #[test]
    fn headings_are_found_in_document_order_with_their_block() {
        let md = "# Dune\n\nSome prose.\n\n## Part One\n\nMore prose.\n\n###### Deep\n";
        let headings = extract_headings(md);
        assert_eq!(
            headings.iter().map(|h| (h.title.as_str(), h.level, h.block_index)).collect::<Vec<_>>(),
            vec![("Dune", 1, 0), ("Part One", 2, 2), ("Deep", 6, 4)]
        );
        // Every recorded block index really is the heading's block.
        let blocks = blocks_of(md);
        for h in &headings {
            assert!(
                blocks[h.block_index].text.starts_with('#'),
                "block {} is {:?}",
                h.block_index,
                blocks[h.block_index].text
            );
        }
    }

    #[test]
    fn a_hash_inside_a_code_sample_is_not_a_chapter() {
        let md = "# Real\n\n```sh\n$ cargo run\n# a comment, not a heading\n```\n\n## Second\n";
        let headings = extract_headings(md);
        assert_eq!(headings.iter().map(|h| h.title.as_str()).collect::<Vec<_>>(), ["Real", "Second"]);
    }

    #[test]
    fn an_unclosed_fence_hides_the_rest_of_the_file() {
        let headings = extract_headings("# A\n\n```\n# B\n# C");
        assert_eq!(headings.len(), 1);
    }

    #[test]
    fn blank_lines_and_leading_noise_shift_nothing() {
        let md = "\n\n# One\n\n\n\n# Two\n";
        let headings = extract_headings(md);
        assert_eq!(headings.len(), 2);
        let blocks = blocks_of(md);
        for h in &headings {
            assert!(blocks[h.block_index].text.starts_with("# "), "{h:?}");
        }
    }

    #[test]
    fn empty_and_headingless_files_have_no_outline() {
        assert!(extract_headings("").is_empty());
        assert!(extract_headings("just prose\nstill prose").is_empty());
        assert!(extract_headings("#\n\n##   \n").is_empty());
    }

    #[test]
    fn block_headings_survive_a_prose_split_that_moves_the_indices() {
        // A long prose paragraph before a heading is subdivided for the page
        // pack, which pushes the heading's index. Reading the headings off the
        // FINAL blocks is what keeps the entry pointing at the right one; the
        // raw source scan, which counts the blocks as the splitter first cut
        // them, would land on the last chunk instead. The paragraph has to be
        // long in LINES for this to be a test at all: `subdivide_with` only
        // cuts a block taller than `SPLIT_MAX_LINES`, so one very long line
        // would subdivide into nothing and the indices would not move.
        let line = "word ".repeat(12).trim().to_string();
        let prose = (0..8).map(|_| line.as_str()).collect::<Vec<_>>().join("\n");
        let source = format!("{prose}\n\n## Chapter\n\ntext\n");
        let blocks = subdivide_prose(parse_markdown(&source));
        let found = headings_of_blocks(&blocks);
        assert_eq!(blocks.len(), 4, "{blocks:?}");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Chapter");
        assert_eq!(found[0].block_index, 2, "the split moved the heading");
        assert_eq!(blocks[found[0].block_index].first_line(), "## Chapter");
        assert_eq!(
            extract_headings(&source)[0].block_index,
            1,
            "the stale answer the final-blocks read exists to prevent"
        );
        // A `#` inside a fence stays out of the tree.
        let fenced = parse_markdown("# Real\n\n```\n# not a heading\n```\n");
        let found = headings_of_blocks(&fenced);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Real");
    }

    #[test]
    fn pages_come_from_the_cut_and_depths_from_the_level() {
        let headings = [
            MarkdownHeading { title: "One".into(), level: 1, block_index: 0 },
            MarkdownHeading { title: "Two".into(), level: 3, block_index: 2 },
            MarkdownHeading { title: "Gone".into(), level: 2, block_index: 99 },
        ];
        // Blocks 0-1 on page 0, blocks 2+ on page 1.
        let nodes = headings_to_nodes(&headings, &[0, 0, 1, 1]);
        assert_eq!(nodes[0].page, 1);
        assert_eq!(nodes[0].depth, 0);
        assert_eq!(nodes[1].page, 2);
        assert_eq!(nodes[1].depth, 2);
        // A heading the cut does not know lands on page 1, never nowhere.
        assert_eq!(nodes[2].page, 1);
    }
}
