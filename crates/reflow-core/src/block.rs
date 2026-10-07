//! The layout atom of a reflowable document, and the shared cutting rules.

/// Which renderer a block belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Text,
    Markdown,
}

/// One layout atom of a text document.
#[derive(Debug, Clone, PartialEq)]
pub struct TextBlock {
    pub kind: BlockKind,
    /// The block's source text, internal newlines kept.
    pub text: String,
    /// True when cut out of a longer block: carries no paragraph space.
    pub continuation: bool,
}

impl TextBlock {
    pub fn new(kind: BlockKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            continuation: false,
        }
    }

    /// The block's source lines, as the splittability predicates and the
    /// chunker both want them.
    pub fn lines(&self) -> Vec<&str> {
        self.text.split('\n').collect()
    }

    /// The block's first line, trimmed — what a classifier looks at.
    pub fn first_line(&self) -> &str {
        match self.text.find('\n') {
            Some(end) => self.text[..end].trim(),
            None => self.text.trim(),
        }
    }
}

/// The most source lines a splittable block keeps after splitting: five.
pub const SPLIT_MAX_LINES: usize = 5;

/// Cut a normalised source on blank lines; `fence_aware` keeps a fence whole.
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
                // The closer ENDS the block: what follows opens a fresh one.
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

/// Whether a trimmed line opens a fenced code block.
pub fn is_fence_open(trimmed: &str) -> bool {
    !fence_marker_of(trimmed).is_empty()
}

/// The fence marker a line opens with (``` or ~~~), or "" for none.
fn fence_marker_of(trimmed: &str) -> &'static str {
    if trimmed.starts_with("```") {
        "```"
    } else if trimmed.starts_with("~~~") {
        "~~~"
    } else {
        ""
    }
}

/// One shared fence state machine, so every Markdown scanner answers alike.
pub struct FenceTracker {
    marker: &'static str,
}

impl FenceTracker {
    pub fn inside(&self) -> bool {
        !self.marker.is_empty()
    }

    /// Feed one line; returns whether it is fence syntax.
    pub fn feed(&mut self, trimmed: &str) -> bool {
        if self.inside() {
            // A closing fence: the same marker char, nothing else on the line.
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

/// Cut oversized blocks into line-bounded chunks, so a page never goes half
/// empty.
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
        // Without it, the blank line inside the sample cuts it in half.
        assert!(
            !fence_free
                .iter()
                .any(|b| b.text.starts_with("```rust") && b.text.contains("println")),
            "the fence-free split kept the sample whole: {fence_free:?}"
        );
        // With it the sample is one block, and the next line its own.
        assert_eq!(fenced.len(), 3, "{fenced:?}");
        assert!(fenced[1].text.starts_with("```rust"));
        assert!(fenced[1].text.ends_with("```"));
        assert!(fenced[1].text.contains("\n\n"));
        assert_eq!(fenced[2].text, "after");
    }

    #[test]
    fn an_unclosed_fence_still_yields_one_block() {
        let blocks = split_blocks(
            "text\n\n```\ncode line\n\nmore code",
            BlockKind::Markdown,
            true,
        );
        assert_eq!(blocks.len(), 2);
        assert!(blocks[1].text.contains("more code"));
    }

    #[test]
    fn a_fence_marker_only_closes_a_fence() {
        // An info string after an opener is noise, not a close.
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
        assert_eq!(
            out.iter()
                .map(|b| b.text.clone())
                .collect::<Vec<_>>()
                .join("\n"),
            source
        );
        assert!(!out[0].text.ends_with('\n'));
        // A predicate refusing every block leaves the list untouched.
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
