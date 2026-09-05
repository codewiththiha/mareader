//! The layout atom of a reflowable document, and the shared cutting rules.
//!
//! A block is one layout atom: a paragraph of plain text, or one top-level
//! Markdown construct (heading, paragraph, list, code fence, table, quote,
//! rule). Blocks are what the paginator packs into pages and what the vertical
//! reader streams, so every reflowable format shares this shape — while the
//! *parsing* of each format lives in its own crate (`txt-core`, `md-core`).
//!
//! Two rules are shared too, because both formats want them and neither owns
//! them: [`split_blocks`] cuts a normalised source into blocks (blank lines are
//! the boundary, with code fences as the one exception a Markdown parser has to
//! honour), and [`subdivide_with`] cuts oversized blocks on line boundaries so
//! the paginator can fill a page without splitting a construct's render. Each
//! format passes its own predicate for what may be cut — plain text cuts
//! anywhere, Markdown only in running prose.

/// Which renderer a block belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    /// Plain text: rendered verbatim, hard line breaks preserved.
    Text,
    /// Markdown source: one top-level construct, rendered as Markdown.
    Markdown,
}

/// One layout atom of a text document.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBlock {
    pub kind: BlockKind,
    /// The block's source text. Text blocks keep their internal newlines;
    /// Markdown blocks keep exactly the lines of their construct.
    pub text: String,
    /// True when [`subdivide_with`] cut this block out of the MIDDLE of a
    /// longer one. A continuation carries no paragraph space of its own (the
    /// whole paragraph owns exactly one), so a paragraph split across a page
    /// cut — or streamed as several chunks — still reads as one paragraph.
    pub continuation: bool,
}

impl TextBlock {
    /// A first-class (non-continuation) block.
    pub fn new(kind: BlockKind, text: impl Into<String>) -> Self {
        Self { kind, text: text.into(), continuation: false }
    }

    /// The block's source lines, as the splittability predicates and the
    /// chunker both want them.
    pub fn lines(&self) -> Vec<&str> {
        self.text.split('\n').collect()
    }

    /// The block's first line, trimmed — what a classifier looks at to decide
    /// what the block is (`#` opens a heading, a tick opens a fence). Never
    /// allocates, because every block is classified on the way to the screen.
    pub fn first_line(&self) -> &str {
        match self.text.find('\n') {
            Some(end) => self.text[..end].trim(),
            None => self.text.trim(),
        }
    }
}

/// The most source lines a splittable block keeps after [`subdivide_with`].
///
/// Five lines is the balance point: at the default typography a chunk
/// measures ≈150px of type, so the paginator can fill a page to within one
/// chunk of its bottom edge instead of pushing a whole tall paragraph over
/// and leaving a blank band behind — while a heading, a code fence, a list
/// or a table still never splits.
pub const SPLIT_MAX_LINES: usize = 5;

/// Cut a normalised source into blocks on blank lines.
///
/// `fence_aware` is the whole difference between the two formats' top-level
/// split: a Markdown fenced block keeps its interior blank lines (they are
/// content) and ends at its closing fence (the line under it starts a new
/// block), a plain-text file has no fences to honour. Otherwise this is
/// exactly CommonMark's top-level block boundary for the constructs a reader
/// cares about, and it keeps the pipeline free of a second Markdown
/// dependency — the RENDER still goes through the real parser, block by block.
pub fn split_blocks(text: &str, kind: BlockKind, fence_aware: bool) -> Vec<TextBlock> {
    let mut blocks = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut fences = FenceTracker::default();

    let flush = |blocks: &mut Vec<TextBlock>, current: &mut Vec<&str>| {
        if current.is_empty() {
            return;
        }
        let joined = current.join("\n");
        current.clear();
        if !joined.trim().is_empty() {
            blocks.push(TextBlock::new(kind, joined));
        }
    };

    for line in text.split('\n') {
        let trimmed = line.trim_start();
        if fence_aware && fences.feed(trimmed) {
            current.push(line);
            if !fences.inside() {
                // The closer ENDS the block: what follows opens a fresh one,
                // without waiting for a blank line. CommonMark is explicit
                // about it, and a paragraph glued under a code sample must not
                // be swallowed into the fence — it would inherit the fence's
                // "never cut" verdict and push a whole page over.
                flush(&mut blocks, &mut current);
            }
            continue;
        }
        if fence_aware && fences.inside() {
            current.push(line);
            continue;
        }
        if line.trim().is_empty() {
            flush(&mut blocks, &mut current);
        } else {
            current.push(line);
        }
    }
    flush(&mut blocks, &mut current);
    blocks
}

/// Whether a trimmed line opens a fenced code block (``` or ~~~, optionally
/// followed by an info string).
pub fn is_fence_open(trimmed: &str) -> bool {
    !fence_marker_of(trimmed).is_empty()
}

/// The fence marker a line opens with (``` or ~~~), or "" for none.
pub fn fence_marker_of(trimmed: &str) -> &'static str {
    if trimmed.starts_with("```") {
        "```"
    } else if trimmed.starts_with("~~~") {
        "~~~"
    } else {
        ""
    }
}

/// One shared fence state machine — the open/close rules
/// [`split_blocks`] follows, extracted so every scanner over Markdown lines
/// (the block splitter, the outline's heading scan, the metadata reader's
/// title fallback) answers the fence question identically instead of each
/// carrying its own copy of the rules.
///
/// A fence OPENS on ``` or ~~~ (three or more, optionally followed by an
/// info string) when no fence is open, and CLOSES on a line that is nothing
/// but the SAME marker characters (possibly longer, possibly spaced). An
/// opener-looking line inside a fence is info-string noise, not a close —
/// the splitter's own rule, which a plain startswith toggle gets wrong.
#[derive(Debug, Clone, Copy, Default)]
pub struct FenceTracker {
    /// The marker of the open fence (``` or ~~~), "" while outside.
    marker: &'static str,
}

impl FenceTracker {
    /// Whether the scan is currently inside a fenced block.
    pub fn inside(&self) -> bool {
        !self.marker.is_empty()
    }

    /// Feed one line, trimmed however the caller trims it (the rules only
    /// look at the line's own bytes). Returns whether the line is fence
    /// syntax — an opener or a closer — which the caller treats as
    /// structure, never as content. After a `true` return,
    /// [`inside`](Self::inside) says whether the fence just opened or just
    /// closed.
    pub fn feed(&mut self, trimmed: &str) -> bool {
        if self.inside() {
            // A closing fence: the same marker char, nothing on the line
            // but the fence itself (it may be longer than the opener).
            let marker_char = self.marker.chars().next().unwrap_or('`');
            let closes = trimmed.starts_with(self.marker)
                && trimmed.trim_end_matches(marker_char).trim().is_empty();
            if closes {
                self.marker = "";
            }
            closes
        } else if is_fence_open(trimmed) {
            self.marker = fence_marker_of(trimmed);
            true
        } else {
            false
        }
    }
}

/// Cut oversized blocks into line-bounded chunks, so the paginator never has
/// to choose between splitting a block's render and leaving a near-empty page
/// above it.
///
/// `splittable` is the format's own answer to "may this block be cut?": for
/// plain text it is every block (its hard breaks are the natural cut points),
/// for Markdown only running prose (a split there falls on a soft break, so
/// the two chunks render exactly as the one paragraph did, with the second
/// marked [`continuation`](TextBlock::continuation) so it carries no paragraph
/// space of its own). Constructs with structure of their own — headings,
/// fences, lists, tables, quotes — pass through whole.
///
/// Runs once, right after parsing: the split depends only on the source (line
/// count), never on the live typography, so block identities are stable for
/// the whole session however the settings move.
pub fn subdivide_with(
    blocks: Vec<TextBlock>,
    max_lines: usize,
    splittable: impl Fn(&TextBlock, &[&str]) -> bool,
) -> Vec<TextBlock> {
    if max_lines == 0 {
        return blocks;
    }
    let mut out = Vec::with_capacity(blocks.len());
    for block in blocks {
        let lines: Vec<&str> = block.lines();
        if lines.len() <= max_lines || !splittable(&block, &lines) {
            out.push(block);
            continue;
        }
        for (ordinal, chunk) in lines.chunks(max_lines).enumerate() {
            out.push(TextBlock {
                kind: block.kind,
                text: chunk.join("\n"),
                continuation: ordinal > 0,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::normalize;

    #[test]
    fn split_blocks_cuts_on_blank_lines() {
        let blocks = split_blocks("one\ntwo\n\nthree", BlockKind::Text, false);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "one\ntwo");
        assert_eq!(blocks[1].text, "three");
        assert!(blocks.iter().all(|b| b.kind == BlockKind::Text));
        // A run of blanks is one boundary, and no block is all whitespace.
        assert!(split_blocks("  \n\n  \n", BlockKind::Text, false).is_empty());
    }

    #[test]
    fn fence_awareness_keeps_a_code_fence_whole() {
        let md = "before\n\n```rust\nfn main() {\n\n    println!(\"hi\");\n}\n```\nafter";
        let fence_free = split_blocks(md, BlockKind::Markdown, false);
        let fenced = split_blocks(md, BlockKind::Markdown, true);
        // Without the fence rule the blank line inside the sample cuts it in
        // half, so nothing there holds the opener and the body together. Counting
        // blocks would not say: fence awareness joins the sample while the closer
        // ends it, which is a wash on length and everything on content.
        assert!(
            !fence_free
                .iter()
                .any(|b| b.text.starts_with("```rust") && b.text.contains("println")),
            "the fence-free split kept the sample whole: {fence_free:?}"
        );
        // With it: the sample is one block, and the line under the closing
        // fence is its own — a paragraph glued to a code sample is still a
        // paragraph.
        assert_eq!(fenced.len(), 3, "{fenced:?}");
        assert!(fenced[1].text.starts_with("```rust"));
        assert!(fenced[1].text.ends_with("```"));
        assert!(fenced[1].text.contains("\n\n"));
        assert_eq!(fenced[2].text, "after");
    }

    #[test]
    fn an_unclosed_fence_still_yields_one_block() {
        let blocks = split_blocks("text\n\n```\ncode line\n\nmore code", BlockKind::Markdown, true);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[1].text.contains("more code"));
    }

    #[test]
    fn a_fence_marker_only_closes_a_fence() {
        // ` ```rs ` after an opener is info-string noise, not a close; the
        // bare marker is.
        let md = "```\ncode\n```rs\nmore\n```\n";
        let blocks = split_blocks(md, BlockKind::Markdown, true);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].text.ends_with("```"));
    }

    #[test]
    fn subdivide_cuts_only_where_the_predicate_allows() {
        let source = "one\ntwo\nthree\nfour\nfive\nsix\nseven";
        let blocks = vec![TextBlock::new(BlockKind::Text, source)];
        let out = subdivide_with(blocks, 3, |_, _| true);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].text, "one\ntwo\nthree");
        assert!(!out[0].continuation);
        assert!(out[1].continuation);
        assert!(out[2].continuation);
        // Nothing is lost or doubled, and no chunk grows a trailing blank line.
        assert_eq!(out.iter().map(|b| b.text.clone()).collect::<Vec<_>>().join("\n"), source);
        assert!(!out[0].text.ends_with('\n'));
        // A predicate that refuses every block leaves the list untouched, and
        // a zero budget disables the pass outright.
        let same = vec![TextBlock::new(BlockKind::Text, source)];
        assert_eq!(subdivide_with(same.clone(), 5, |_, _| false), same);
        assert_eq!(subdivide_with(same.clone(), 0, |_, _| true), same);
    }

    #[test]
    fn a_block_under_the_budget_is_never_touched() {
        let blocks = vec![
            TextBlock::new(BlockKind::Text, "one\ntwo\nthree"),
            TextBlock::new(BlockKind::Markdown, "# Heading"),
        ];
        assert_eq!(subdivide_with(blocks.clone(), 5, |_, _| true), blocks);
    }

    #[test]
    fn normalize_is_the_precondition_of_both_splits() {
        // The splitter relies on there being exactly one LF per line break.
        assert_eq!(normalize("\u{feff}a\r\nb\rc\n"), "a\nb\nc\n");
    }
}
