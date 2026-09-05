//! The chapter tree a PDF carries, and how it becomes the reader's outline.
//!
//! A PDF's `/Outlines` dictionary is a tree of destinations, and resolving what
//! each one points at is a round trip through the pdf.js worker — seconds for a
//! textbook, and the reason the reader asks for the tree only after page 1 is
//! on screen. By the time it answers, the engine has already flattened it in
//! document order (see `resolveOutline` in `public/pdfEngine.ts`), so what
//! arrives is [`OutlineEntry`]: a title, a 1-based page, and the depth it sat at
//! in the tree.
//!
//! [`to_nodes`] is the last PDF-shaped step. From here the reader holds
//! `reader_core::outline::OutlineNode`s, exactly the type a Markdown document's
//! headings become, and nothing downstream asks which kind of file it came from.

use reader_core::outline::{OutlineNode, clamp_depth};
use serde::{Deserialize, Serialize};

/// One flattened chapter, as the engine reports it.
///
/// CONTRACT: the field names are the wire shape `pdfEngine.js` resolves them
/// to; `page` is 1-based (0 means the destination did not resolve).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlineEntry {
    pub title: String,
    pub page: u32,
    /// Nesting depth in the source tree, 0 for a top-level chapter.
    #[serde(default)]
    pub depth: u32,
}

/// The engine's entries as the reader's outline.
///
/// Two rules are enforced here rather than trusted from the file: a chapter
/// whose destination never resolved (`page` 0) cannot be jumped to and is
/// dropped, and every title is trimmed because an outline is the one place a
/// document's whitespace shows up as a UI bug. Depths are clamped to what the
/// panel indents.
pub fn to_nodes(entries: Vec<OutlineEntry>, page_count: u32) -> Vec<OutlineNode> {
    entries
        .into_iter()
        .filter(|entry| entry.page >= 1)
        .map(|entry| OutlineNode {
            title: entry.title.trim().to_string(),
            page: if page_count == 0 { 1 } else { entry.page.min(page_count) },
            depth: clamp_depth(entry.depth),
        })
        .filter(|node| !node.title.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, page: u32, depth: u32) -> OutlineEntry {
        OutlineEntry { title: title.into(), page, depth }
    }

    #[test]
    fn entries_become_nodes_in_order() {
        let nodes = to_nodes(vec![entry("One", 1, 0), entry("One.a", 2, 1)], 10);
        assert_eq!(nodes.len(), 2);
        assert_eq!((nodes[1].title.as_str(), nodes[1].page, nodes[1].depth), ("One.a", 2, 1));
    }

    #[test]
    fn an_unresolved_destination_and_a_blank_title_are_dropped() {
        let nodes = to_nodes(vec![entry("Nowhere", 0, 0), entry("  ", 3, 0), entry("Real", 4, 0)], 9);
        assert_eq!(nodes.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["Real"]);
    }

    #[test]
    fn pages_clamp_to_the_book_that_actually_opened() {
        // A file whose outline was authored against a different page count must
        // never jump past the last sheet.
        let nodes = to_nodes(vec![entry("Late", 900, 0)], 12);
        assert_eq!(nodes[0].page, 12);
        // A book with no pages at all keeps every entry on page 1 rather than
        // producing a jump to a page that does not exist.
        let empty = to_nodes(vec![entry("Late", 900, 0)], 0);
        assert_eq!(empty[0].page, 1);
    }

    #[test]
    fn the_wire_shape_is_camel_case() {
        let json = serde_json::to_string(&entry("One", 2, 1)).unwrap();
        assert_eq!(json, "{\"title\":\"One\",\"page\":2,\"depth\":1}");
        // `depth` is optional on the wire: a flat list is all level 0.
        let flat: OutlineEntry = serde_json::from_str("{\"title\":\"T\",\"page\":7}").unwrap();
        assert_eq!((flat.title.as_str(), flat.page, flat.depth), ("T", 7, 0));
    }
}
