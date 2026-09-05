//! Cutting a plain-text file into paragraphs.

use reflow_core::block::{BlockKind, TextBlock, split_blocks};
use reflow_core::source::normalize;

/// A plain-text file into blocks: runs of non-empty lines, separated by blank
/// lines. Internal single newlines are KEPT — the renderer preserves them
/// (`pre-wrap`), which is what makes fixed-line prose and code-ish notes read
/// as authored.
///
/// No fence awareness, on purpose: a fence is Markdown syntax, and inside a
/// plain-text file a blank line inside an indented code sample is a paragraph
/// boundary like any other.
pub fn parse_plain_text(raw: &str) -> Vec<TextBlock> {
    let text = normalize(raw);
    split_blocks(&text, BlockKind::Text, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraphs_split_on_blank_lines_only() {
        let blocks = parse_plain_text("line one\nline two\n\nsecond para\n\n\nthird");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].text, "line one\nline two");
        assert_eq!(blocks[1].text, "second para");
        assert_eq!(blocks[2].text, "third");
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Text));
    }

    #[test]
    fn an_empty_or_blank_file_has_no_blocks() {
        assert!(parse_plain_text("").is_empty());
        assert!(parse_plain_text("  \n\n  \n").is_empty());
    }

    #[test]
    fn markup_looking_lines_stay_verbatim() {
        // Plain text is not Markdown: a heading marker is two characters, and
        // a fence is a line of backticks. The one paragraph they sit in stays
        // one paragraph, blank lines aside.
        let blocks = parse_plain_text("# not a heading\n```\nfenced?");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "# not a heading\n```\nfenced?");
    }
}
