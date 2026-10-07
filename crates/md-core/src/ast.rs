//! What kind of construct a block holds; only running prose may be cut.

use reflow_core::block::{BlockKind, TextBlock};

/// The top-level constructs a reader distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkdownConstruct {
    /// An ATX (`#`) or setext (`===`) heading.
    Heading,
    /// A fenced or indented code block.
    Code,
    /// A bullet or ordered list, including task lists.
    List,
    /// A GFM pipe table.
    Table,
    Quote,
    /// A thematic break (`---`, `***`, `___`).
    Rule,
    /// A raw HTML block: the reader refuses it, so it may not be cut.
    Html,
    /// Running prose — the only construct that may be split across a cut.
    Prose,
}

/// The construct `line` opens, or `None` when it carries no block marker.
fn construct_of_line(line: &str) -> Option<MarkdownConstruct> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    // An indented code block (4+ columns) is code, not prose.
    if indent_columns(line) >= 4 {
        return Some(MarkdownConstruct::Code);
    }
    // A fence, opening or closing: the marker `reflow-core`'s splitter tracks.
    if reflow_core::block::is_fence_open(trimmed) {
        return Some(MarkdownConstruct::Code);
    }
    let first = trimmed.chars().next()?;
    let found = match first {
        '#' => MarkdownConstruct::Heading,
        // A setext underline: the block it ends IS the heading.
        '=' => MarkdownConstruct::Heading,
        '>' => MarkdownConstruct::Quote,
        '|' => MarkdownConstruct::Table,
        '_' => MarkdownConstruct::Rule,
        '<' => MarkdownConstruct::Html,
        '-' | '*' | '+' => {
            if is_rule(trimmed) {
                MarkdownConstruct::Rule
            } else if is_bullet(trimmed) {
                MarkdownConstruct::List
            } else {
                // No marker follows: the line is prose and stays cuttable.
                return None;
            }
        }
        _ if is_ordered_item(trimmed) => MarkdownConstruct::List,
        _ => return None,
    };
    Some(found)
}

/// The block's construct: the first marker it carries, or prose.
fn classify(block: &TextBlock) -> MarkdownConstruct {
    if block.kind != BlockKind::Markdown {
        return MarkdownConstruct::Prose;
    }
    for line in block.text.split('\n') {
        if let Some(kind) = construct_of_line(line) {
            return kind;
        }
    }
    MarkdownConstruct::Prose
}

/// Whether the block is running prose and may be cut at its line boundaries.
pub(crate) fn is_prose_block(block: &TextBlock, _lines: &[&str]) -> bool {
    block.kind == BlockKind::Text || classify(block) == MarkdownConstruct::Prose
}

/// Columns the line is indented by, counting a tab as four.
fn indent_columns(line: &str) -> usize {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

/// `---`, `***` or `___`: three or more of one marker, nothing but spaces
/// between them.
fn is_rule(trimmed: &str) -> bool {
    let mut chars = trimmed.chars().filter(|c| !c.is_whitespace());
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, '-' | '*' | '_') {
        return false;
    }
    let mut count = 1;
    for c in chars {
        if c != first {
            return false;
        }
        count += 1;
        if count >= 3 {
            return true;
        }
    }
    false
}

/// A bullet: `-`, `*` or `+` followed by a space (or ending the line).
fn is_bullet(trimmed: &str) -> bool {
    let mut chars = trimmed.chars();
    match chars.next() {
        Some('-' | '*' | '+') => chars.next().is_none_or(|c| c == ' ' || c == '\t'),
        _ => false,
    }
}

/// An ordered list item: digits, then `.` or `)`.
fn is_ordered_item(trimmed: &str) -> bool {
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    digits > 0 && matches!(trimmed.chars().nth(digits), Some('.' | ')'))
}

/// One ATX heading line: its level (1-6) and marker-stripped title.
pub(crate) fn heading_of_line(line: &str) -> Option<(u32, String)> {
    let trimmed = line.trim();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    // The closing sequence (`## Title ##`) is optional syntax; drop it.
    let body = trimmed[level..].trim().trim_end_matches('#').trim();
    if body.is_empty() {
        return None;
    }
    Some((level as u32, body.trim_matches('*').trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn md(text: &str) -> TextBlock {
        TextBlock::new(BlockKind::Markdown, text)
    }

    #[test]
    fn every_construct_recognises_itself() {
        assert_eq!(classify(&md("# Heading")), MarkdownConstruct::Heading);
        assert_eq!(classify(&md("### Deep")), MarkdownConstruct::Heading);
        assert_eq!(classify(&md("```\ncode\n```")), MarkdownConstruct::Code);
        assert_eq!(classify(&md("    indented code")), MarkdownConstruct::Code);
        assert_eq!(classify(&md("- a\n- b")), MarkdownConstruct::List);
        assert_eq!(classify(&md("1. one\n2. two")), MarkdownConstruct::List);
        assert_eq!(classify(&md("- [x] done")), MarkdownConstruct::List);
        assert_eq!(
            classify(&md("| a | b |\n|---|---|")),
            MarkdownConstruct::Table
        );
        assert_eq!(classify(&md("> quoted")), MarkdownConstruct::Quote);
        assert_eq!(classify(&md("---")), MarkdownConstruct::Rule);
        assert_eq!(classify(&md("Just a sentence.")), MarkdownConstruct::Prose);
        // The FIRST marker wins, as a leading continuation line reads.
        assert_eq!(classify(&md("- a\nmore text")), MarkdownConstruct::List);
    }

    #[test]
    fn the_marker_sniff_is_coarser_than_commonmark_on_purpose() {
        // Seven hashes is not a heading, but still not prose: may it be cut?
        assert_eq!(
            classify(&md("####### too deep")),
            MarkdownConstruct::Heading
        );
        // A setext underline makes the block a heading; raw HTML is refused.
        assert_eq!(classify(&md("Title\n=====")), MarkdownConstruct::Heading);
        assert_eq!(
            classify(&md("<details>\n<summary>more</summary>\n</details>")),
            MarkdownConstruct::Html
        );
        assert_eq!(heading_of_line("####### too deep"), None);
        assert_eq!(heading_of_line("#  Spaced  #"), Some((1, "Spaced".into())));
        assert_eq!(
            heading_of_line("## **Bold title**"),
            Some((2, "Bold title".into()))
        );
        assert_eq!(heading_of_line("###"), None);
        assert_eq!(heading_of_line("not a heading"), None);
    }

    #[test]
    fn only_prose_is_splittable() {
        let prose = md("word word word\nword word word");
        assert!(is_prose_block(&prose, &[]));
        for structured in ["```rs\ncode", "- a", "> q", "| a |", "4. x", "    x"] {
            let block = md(structured);
            assert!(!is_prose_block(&block, &[]), "{structured}");
        }
        // Plain text is always splittable: its hard breaks are the cut points.
        let plain = TextBlock::new(BlockKind::Text, "```\ncode");
        assert!(is_prose_block(&plain, &[]));
    }
}
