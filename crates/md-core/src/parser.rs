//! A Markdown file into its top-level blocks.

use reflow_core::block::{BlockKind, SPLIT_MAX_LINES, TextBlock, split_blocks, subdivide_with};
use reflow_core::source::normalize;

/// A Markdown file into its top-level blocks: blank lines separate blocks
/// EXCEPT inside a fenced code block, where a blank line is content.
///
/// Fence awareness is the one rule that cannot be skipped — without it a
/// paragraph's worth of code with a blank line in the middle becomes two
/// blocks, and the renderer then closes and reopens every construct around it.
pub fn parse_markdown(raw: &str) -> Vec<TextBlock> {
    let text = normalize(raw);
    split_blocks(&text, BlockKind::Markdown, true)
}

/// Cut oversized PROSE blocks into line-bounded chunks, leaving every
/// construct with structure of its own whole.
///
/// A split inside a paragraph falls on a soft break, so the two chunks render
/// exactly as the one paragraph did and only the second loses its paragraph
/// space. A split inside a list, a fence, a table or a quote does not: those
/// are re-opened by the renderer as their own construct, and a list that
/// restarts its numbering mid-page is a worse reader experience than a page
/// with a short band at the bottom. So the predicate is the classifier.
pub fn subdivide_prose(blocks: Vec<TextBlock>) -> Vec<TextBlock> {
    subdivide_with(blocks, SPLIT_MAX_LINES, crate::ast::is_prose_block)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_splits_on_blank_lines() {
        let blocks = parse_markdown("# Title\n\nSome prose.\n\n- a\n- b\n");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].text, "# Title");
        assert_eq!(blocks[1].text, "Some prose.");
        assert_eq!(blocks[2].text, "- a\n- b");
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Markdown));
    }

    #[test]
    fn an_empty_or_blank_file_has_no_blocks() {
        assert!(parse_markdown("").is_empty());
        assert!(parse_markdown("\n\n   \n").is_empty());
    }

    #[test]
    fn prose_splits_and_structured_constructs_do_not() {
        // Every sample is padded past the five-line budget, so being over it is
        // common to all of them: the only thing that can keep a construct whole
        // is the predicate that says it is not prose.
        fn over_the_budget(head: &str) -> String {
            let mut text = head.to_string();
            while text.lines().count() < SPLIT_MAX_LINES * 2 + 2 {
                text.push_str("\na filler line of running prose");
            }
            text
        }
        let prose = over_the_budget("an opening line of running prose");
        let out = subdivide_prose(vec![TextBlock::new(BlockKind::Markdown, prose.clone())]);
        assert_eq!(out.len(), 3);
        assert!(!out[0].continuation);
        assert!(out[1].continuation && out[2].continuation);
        // Cut on line boundaries, losing nothing: the chunks are the source.
        let rejoined = out
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(rejoined, prose);

        for structured in [
            "```rs\ncode\n```",
            "- a\n- b\n- c",
            "# heading\n\ntext",
            "| a | b |\n|---|---|\n| 1 | 2 |",
            "> quoted\n> lines",
            "1. one\n2. two",
        ] {
            let block = TextBlock::new(BlockKind::Markdown, over_the_budget(structured));
            let out = subdivide_prose(vec![block.clone()]);
            assert_eq!(out, vec![block], "{structured}");
        }
    }
}
