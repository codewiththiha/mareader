//! The page cutter: which blocks land on which page.
//!
//! The cut is block-granular — a paragraph is never split across a page —
//! and it runs over BLOCK HEIGHTS. Heights come from two places: a pure
//! ESTIMATE here (character counts against the column width, good enough
//! to seed the layout the instant a file opens) and the DOM's real
//! MEASUREMENT once the text has rendered. Both feed the same
//! [`paginate`], so the layout only ever re-cuts when real numbers arrive,
//! never twice for the same truth.
//!
//! Zoom never re-cuts: every length on the page scales by the same factor,
//! so the cut computed at scale 1 is exactly the cut at any scale.

use crate::block::{BlockKind, TextBlock};

/// The inputs of the height ESTIMATE, all at scale 1.
#[derive(Debug, Clone, Copy)]
pub struct BlockMetrics {
    /// The width the text flows in (the page's content width).
    pub content_width: f64,
    /// Body font size in px.
    pub font_size: f64,
    /// Line height, as a unitless multiple of the font size.
    pub line_height: f64,
    /// Space under a paragraph, in ems of the font size.
    pub paragraph_margin_em: f64,
    /// Average glyph advance as a fraction of the font size (proportional
    /// faces ≈ 0.5, monospace 0.6).
    pub char_width: f64,
}

impl BlockMetrics {
    /// Height of one line, in px.
    fn line_height_px(&self) -> f64 {
        self.font_size * self.line_height
    }

    /// How many glyphs fit on a line, at least one.
    fn chars_per_line(&self) -> f64 {
        (self.content_width / (self.font_size * self.char_width).max(0.001)).max(1.0)
    }
}

/// The estimated height of one block at scale 1.
///
/// Text blocks honour their hard line breaks (each source line wraps on
/// its own); Markdown blocks flow as one run — markup characters overstate
/// the rendered length slightly, which the 0.85 factor takes back. An
/// estimate is a SEED: the measurement pass replaces it with the real
/// number as soon as the block has rendered once.
///
/// A continuation chunk (the tail of a paragraph `subdivide` cut) carries
/// no paragraph space — the paragraph's one share of it belongs to its
/// first chunk — so its estimate skips the margin term to match the render.
pub fn estimate_block_height(block: &TextBlock, m: &BlockMetrics) -> f64 {
    let per_line = m.chars_per_line();
    let lines: f64 = match block.kind {
        BlockKind::Text => block
            .text
            .split('\n')
            .map(|line| {
                let chars = line.chars().count().max(1) as f64;
                (chars / per_line).ceil().max(1.0)
            })
            .sum(),
        BlockKind::Markdown => {
            let chars = block.text.chars().count() as f64 * 0.85;
            (chars / per_line).ceil().max(1.0)
        }
    };
    let margin = if block.continuation { 0.0 } else { m.paragraph_margin_em };
    lines * m.line_height_px() + margin * m.font_size
}

/// Estimates for a whole document, in block order.
pub fn estimate_heights(blocks: &[TextBlock], m: &BlockMetrics) -> Vec<f64> {
    blocks.iter().map(|b| estimate_block_height(b, m)).collect()
}

/// One page of the cut: the blocks `[start, start + count)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageCut {
    pub start: usize,
    pub count: usize,
}

impl PageCut {
    /// One-past-the-last block of the page.
    pub fn end(&self) -> usize {
        self.start + self.count
    }
}

/// Greedily pack block heights into pages of `content_height`.
///
/// Blocks are never split; a block taller than a page gets the page to
/// itself and overflows it (the renderer clips, which is the honest
/// reading of a paragraph no page can hold). An empty document still cuts
/// to one blank page — a document is never zero pages.
pub fn paginate(heights: &[f64], content_height: f64) -> Vec<PageCut> {
    if heights.is_empty() {
        return vec![PageCut { start: 0, count: 0 }];
    }
    let mut cuts = Vec::new();
    let mut start = 0usize;
    let mut used = 0.0f64;
    for (index, &height) in heights.iter().enumerate() {
        let fits = used + height <= content_height + 1e-9;
        if !fits && used > 0.0 {
            cuts.push(PageCut { start, count: index - start });
            start = index;
            used = 0.0;
        }
        used += height;
        // A block taller than the page: it owns this page, overflowing it.
        if height > content_height + 1e-9 {
            cuts.push(PageCut { start, count: index + 1 - start });
            start = index + 1;
            used = 0.0;
        }
    }
    if start < heights.len() {
        cuts.push(PageCut { start, count: heights.len() - start });
    }
    cuts
}

/// The 0-based page each block landed on (parallel to the block list).
pub fn block_page_index(cuts: &[PageCut], block_count: usize) -> Vec<u32> {
    let mut map = vec![0u32; block_count];
    for (page, cut) in cuts.iter().enumerate() {
        let end = cut.end().min(block_count);
        for slot in &mut map[cut.start..end] {
            *slot = page as u32;
        }
    }
    map
}

/// The first block of 1-based `page`, clamped into the document — the
/// index a page jump scrolls to. A page beyond the last cut lands on the
/// last page; page 0 lands on the first.
pub fn first_block_of_page(cuts: &[PageCut], page: u32) -> usize {
    let index = page.saturating_sub(1) as usize;
    cuts.get(index).or_else(|| cuts.last()).map_or(0, |cut| cut.start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockKind;

    fn metrics() -> BlockMetrics {
        BlockMetrics {
            content_width: 650.0,
            font_size: 17.0,
            line_height: 1.7,
            paragraph_margin_em: 1.0,
            char_width: 0.5,
        }
    }

    fn block(text: &str) -> TextBlock {
        TextBlock::new(BlockKind::Text, text)
    }

    fn continuation(text: &str) -> TextBlock {
        TextBlock { kind: BlockKind::Text, text: text.to_string(), continuation: true }
    }

    #[test]
    fn estimates_scale_with_length_and_breaks() {
        let m = metrics();
        let short = estimate_block_height(&block("hello"), &m);
        let two_lines = estimate_block_height(&block(&"x".repeat(200)), &m);
        assert!(two_lines > short * 1.5, "{two_lines} vs {short}");
        // A hard break costs at least one extra line.
        let broken = estimate_block_height(&block("a\nb"), &m);
        let joined = estimate_block_height(&block("a b"), &m);
        assert!(broken > joined);
        // Every block carries its paragraph margin.
        assert!(short > m.paragraph_margin_em * m.font_size);
    }

    #[test]
    fn markdown_markup_is_discounted() {
        let m = metrics();
        let md = TextBlock::new(
            BlockKind::Markdown,
            "**bold** and *italic* and `code` and [links](http://example.com)",
        );
        let plain = TextBlock::new(BlockKind::Text, md.text.clone());
        assert!(estimate_block_height(&md, &m) <= estimate_block_height(&plain, &m));
    }

    #[test]
    fn a_continuation_chunk_carries_no_paragraph_space() {
        let m = metrics();
        let first = estimate_block_height(&block("same text"), &m);
        let tail = estimate_block_height(&continuation("same text"), &m);
        let margin = m.paragraph_margin_em * m.font_size;
        assert!((first - tail - margin).abs() < 1e-9, "{first} vs {tail}");
    }

    #[test]
    fn paginate_packs_greedily_and_never_splits_a_block() {
        // Page holds 100: 60+60 cannot share, but 60+40 fills the page
        // exactly — and a page packed to capacity is full, not overfull.
        let cuts = paginate(&[60.0, 60.0, 40.0, 40.0], 100.0);
        assert_eq!(cuts.len(), 3);
        assert_eq!(cuts[0], PageCut { start: 0, count: 1 });
        assert_eq!(cuts[1], PageCut { start: 1, count: 2 });
        assert_eq!(cuts[2], PageCut { start: 3, count: 1 });
    }

    #[test]
    fn an_oversized_block_gets_its_own_page() {
        let cuts = paginate(&[30.0, 250.0, 30.0], 100.0);
        assert_eq!(cuts.len(), 3);
        assert_eq!(cuts[1], PageCut { start: 1, count: 1 });
        // The neighbours are not swallowed into the overflow page.
        assert_eq!(cuts[2], PageCut { start: 2, count: 1 });
    }

    #[test]
    fn an_empty_document_is_one_blank_page() {
        assert_eq!(paginate(&[], 100.0), vec![PageCut { start: 0, count: 0 }]);
    }

    #[test]
    fn everything_fits_on_one_page_when_it_fits() {
        let cuts = paginate(&[10.0, 20.0, 30.0], 1000.0);
        assert_eq!(cuts, vec![PageCut { start: 0, count: 3 }]);
    }

    #[test]
    fn the_block_map_and_first_block_agree_with_the_cut() {
        let cuts = paginate(&[60.0, 60.0, 40.0, 40.0], 100.0);
        let map = block_page_index(&cuts, 4);
        assert_eq!(map, vec![0, 1, 1, 2]);
        assert_eq!(first_block_of_page(&cuts, 1), 0);
        assert_eq!(first_block_of_page(&cuts, 2), 1);
        assert_eq!(first_block_of_page(&cuts, 3), 3);
        // Out-of-range pages clamp into the document.
        assert_eq!(first_block_of_page(&cuts, 99), 3);
        assert_eq!(first_block_of_page(&cuts, 0), 0);
    }

    #[test]
    fn the_cut_covers_every_block_exactly_once() {
        let heights: Vec<f64> = (1..=50).map(|i| (i % 7) as f64 * 9.0 + 4.0).collect();
        let cuts = paginate(&heights, 60.0);
        let mut covered = Vec::new();
        for cut in &cuts {
            for i in cut.start..cut.end() {
                covered.push(i);
            }
        }
        assert_eq!(covered, (0..50).collect::<Vec<_>>());
    }
}
