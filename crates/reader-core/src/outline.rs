//! The document's chapter tree, in the shape the reader displays.
//!
//! One node type for every format, which is the point: the outline panel, the
//! floating label and the reveal-on-page-turn memo all take a
//! `Vec<OutlineNode>` and never ask whether the chapters came from a PDF's
//! `/Outlines` dictionary (resolved through the engine, in
//! `pdf_core::outline`) or from a Markdown file's `#` headings (derived while
//! the blocks are paginated, in `md_core::outline`). A format that can name
//! where its sections start can fill the sidebar.
//!
//! The nodes are stored flattened, in document order, with the nesting depth
//! as a field — the panel indents rather than recurses, and a page change only
//! needs the last entry whose page is at or before it.

/// One chapter of the open document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineNode {
    /// The chapter's own title, as the document spells it.
    pub title: String,
    /// The 1-based page the chapter starts on.
    pub page: u32,
    /// Nesting level, 0 for a top-level chapter.
    pub depth: u32,
}

impl OutlineNode {
    pub fn new(title: impl Into<String>, page: u32, depth: u32) -> Self {
        Self { title: title.into(), page, depth }
    }
}

/// The deepest level the panel will indent. Deeper headings still appear, at
/// the cap — a chapter the reader can see is worth more than a clean tree.
const MAX_OUTLINE_DEPTH: u32 = 5;

/// Clamp a raw depth into the range the panel draws. A negative-looking depth
/// (an outline that numbers its levels from 1) is normalised by the caller
/// that knows its own convention; this only guards the far end.
pub fn clamp_depth(depth: u32) -> u32 {
    depth.min(MAX_OUTLINE_DEPTH)
}

/// The entry the reader is currently inside, for the sidebar to highlight and
/// the floating label to show.
///
/// A chapter owns every page from its own up to (but not including) the next
/// chapter that starts later, so the answer is the LAST entry whose page is at
/// or before `page`. Ties matter: a chapter and its first section can open on the
/// same page, and the later — i.e. more specific — one wins.
///
/// `None` before the first chapter's page: a cover or a preface belongs to no
/// section, and highlighting chapter 1 there would be a lie.
///
/// Both producers sort their output by page — a PDF flattens in document order
/// (`loader.ts:flattenOutline`), a Markdown outline is read off the blocks in
/// order — so one binary search answers it. A malformed file that flattens out of
/// order takes the linear path, which stays correct. A malformed file can reach
/// that path, so it is handled rather than asserted: an outline is document
/// input, and document input gets no panic.
pub fn active_entry(nodes: &[OutlineNode], page: u32) -> Option<usize> {
    if !nodes.is_sorted_by_key(|node| node.page) {
        return nodes.iter().rposition(|node| node.page <= page);
    }
    // `checked_sub` so a page before the first entry yields `None` rather than
    // underflowing the index.
    nodes.partition_point(|node| node.page <= page).checked_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(title: &str, page: u32, depth: u32) -> OutlineNode {
        OutlineNode::new(title, page, depth)
    }

    #[test]
    fn the_last_chapter_at_or_before_the_page_wins() {
        let nodes = [node("One", 1, 0), node("Two", 10, 0), node("Two.a", 12, 1)];
        assert_eq!(active_entry(&nodes, 1), Some(0));
        assert_eq!(active_entry(&nodes, 11), Some(1));
        assert_eq!(active_entry(&nodes, 40), Some(2));
    }

    #[test]
    fn before_the_first_chapter_and_with_no_outline_there_is_nothing_to_highlight() {
        assert_eq!(active_entry(&[node("Later", 5, 0)], 1), None);
        assert_eq!(active_entry(&[], 7), None);
    }

    #[test]
    fn a_tie_on_a_page_resolves_to_the_deeper_entry() {
        let nodes = [
            node("Chapter 1", 3, 0),
            node("1.1 Intro", 3, 1),
            node("1.2 Details", 9, 1),
            node("Chapter 2", 20, 0),
        ];
        assert_eq!(active_entry(&nodes, 3), Some(1));
        assert_eq!(active_entry(&nodes, 8), Some(1));
        assert_eq!(active_entry(&nodes, 9), Some(2));
        // The last entry owns everything to the end of the document.
        assert_eq!(active_entry(&nodes, 999), Some(3));
    }

    #[test]
    fn an_out_of_order_outline_still_answers_correctly() {
        let nodes = [node("Late", 30, 0), node("First", 2, 0), node("Middle", 9, 0)];
        assert_eq!(active_entry(&nodes, 2), Some(1));
        assert_eq!(active_entry(&nodes, 25), Some(2));
        assert_eq!(active_entry(&nodes, 1), None);
    }

    #[test]
    fn depth_caps_without_dropping_the_entry() {
        assert_eq!(clamp_depth(0), 0);
        assert_eq!(clamp_depth(MAX_OUTLINE_DEPTH), MAX_OUTLINE_DEPTH);
        assert_eq!(clamp_depth(99), MAX_OUTLINE_DEPTH);
    }
}
