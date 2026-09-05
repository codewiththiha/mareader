//! What kind of construct a block holds.
//!
//! One rule runs through this crate: a block may be *cut* only if cutting it
//! cannot change what the renderer opens. That is a question about the
//! construct, not about the text's length, so the answer is settled here — the
//! fence-aware splitter in `reflow-core` produced the blocks, and this module
//! says which of them have structure of their own.
//!
//! The check is deliberately conservative, and the conservatism is cheap: a
//! false "not prose" costs one tighter page pack, while a false "prose" costs
//! a broken construct. So anything that could open a block-level construct
//! disqualifies a line, and a block is prose only when every one of its lines
//! is.

use reflow_core::block::{BlockKind, TextBlock};

/// The top-level constructs a reader distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkdownConstruct {
    /// An ATX (`#`) or setext (`===`) heading.
    Heading,
    /// A fenced or indented code block.
    Code,
    /// A bullet or ordered list, including task lists.
    List,
    /// A GFM pipe table.
    Table,
    /// A block quote.
    Quote,
    /// A thematic break (`---`, `***`, `___`).
    Rule,
    /// A raw HTML block. The reader refuses to render it, so it may not be cut
    /// into pieces that render as prose either.
    Html,
    /// Running prose — the only construct that may be split across a cut.
    Prose,
}

/// The construct `line` opens, or `None` when the line carries no block marker.
///
/// Only the first line can open a construct (the splitter already cut the
/// block on blank lines), so this answers per line and [`classify`] folds a
/// block's lines into one verdict. The scan is a MARKER sniff, deliberately
/// coarser than CommonMark: a seventh `#` is not a heading and is still not
/// prose, because the only thing this answers is whether a line break may be
/// cut, and every syntax the reader does not model refuses the cut.
fn construct_of_line(line: &str) -> Option<MarkdownConstruct> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return None;
    }
    // An indented code block (4+ columns) is code, not prose. A tab counts as
    // the four columns it advances to, so the check is on the indent's width
    // rather than on the bytes in front of the text.
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
                // `-ish`, `*not a bullet`: no marker follows the character, so
                // the line is prose and must stay cuttable.
                return None;
            }
        }
        _ if is_ordered_item(trimmed) => MarkdownConstruct::List,
        _ => return None,
    };
    Some(found)
}

/// The block's construct: the first marker it carries, or prose.
pub fn classify(block: &TextBlock) -> MarkdownConstruct {
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

/// Whether the block is running prose and may therefore be cut at its line
/// boundaries. A block whose lines are all prose has no structure for a split
/// to break; anything else keeps its render whole.
pub fn is_prose_block(block: &TextBlock, _lines: &[&str]) -> bool {
    block.kind == BlockKind::Text || classify(block) == MarkdownConstruct::Prose
}

/// Whether one Markdown line is running prose — no construct opens on it, and
/// it says something. The check is deliberately conservative: anything that
/// could begin a block-level construct disqualifies the whole block, so a false
/// "not prose" only costs a tighter page pack, never a broken render. A blank
/// line is refused on the same logic: the splitter never leaves one inside a
/// block, so a block that carries one is not in a state to be cut.
pub fn is_prose_line(line: &str) -> bool {
    !line.trim().is_empty() && construct_of_line(line).is_none()
}

/// How many columns the line is indented by, counting a tab as four — which is
/// how Markdown measures the indent that opens an indented code block, and why
/// one tab is enough here.
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
    let Some(first) = chars.next() else { return false };
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

/// One ATX heading line: its level (1–6) and its title with the markers and
/// the emphasis noise stripped. `None` for anything else, including a seventh
/// `#` (not a heading in CommonMark) and an empty one.
pub fn heading_of_line(line: &str) -> Option<(u32, String)> {
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
        assert_eq!(classify(&md("| a | b |\n|---|---|")), MarkdownConstruct::Table);
        assert_eq!(classify(&md("> quoted")), MarkdownConstruct::Quote);
        assert_eq!(classify(&md("---")), MarkdownConstruct::Rule);
        assert_eq!(classify(&md("Just a sentence.")), MarkdownConstruct::Prose);
        // The FIRST marker wins, which is what a list that opens with a
        // paragraph continuation line still reads as.
        assert_eq!(classify(&md("- a\nmore text")), MarkdownConstruct::List);
    }

    #[test]
    fn the_marker_sniff_is_coarser_than_commonmark_on_purpose() {
        // Seven hashes is not a heading, but it is still not prose either: the
        // only question asked here is whether the block may be cut.
        assert_eq!(classify(&md("####### too deep")), MarkdownConstruct::Heading);
        // A setext underline makes the whole block the heading it ends, and a
        // raw HTML block is refused on the first line of its tag.
        assert_eq!(classify(&md("Title\n=====")), MarkdownConstruct::Heading);
        assert_eq!(classify(&md("<details>\n<summary>more</summary>\n</details>")), MarkdownConstruct::Html);
        assert_eq!(heading_of_line("####### too deep"), None);
        assert_eq!(heading_of_line("#  Spaced  #"), Some((1, "Spaced".into())));
        assert_eq!(heading_of_line("## **Bold title**"), Some((2, "Bold title".into())));
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
        // A plain-text block is always splittable: its hard breaks are the cut
        // points, whatever the bytes look like.
        let plain = TextBlock::new(BlockKind::Text, "```\ncode");
        assert!(is_prose_block(&plain, &[]));
    }

    #[test]
    fn prose_line_agrees_with_the_classifier() {
        assert!(is_prose_line("a plain line"));
        assert!(!is_prose_line("- a"));
        assert!(!is_prose_line("# h"));
        assert!(!is_prose_line("\tcode"));
        // An empty line is not prose either: the splitter never leaves one
        // inside a block, and refusing it is the safe answer if it ever does.
        assert!(!is_prose_line(""));
    }
}
