//! The chapter tree a PDF carries, flattened by the engine then trimmed here.

use reader_core::outline::{OutlineNode, clamp_depth};
use serde::{Deserialize, Serialize};

/// One flattened chapter as the engine reports it; `page` is 1-based.
pub struct OutlineEntry {
    pub title: String,
    pub page: u32,
    #[serde(default)]
    pub depth: u32,
}

/// The engine's entries as the reader's outline: unresolved pages dropped.
pub fn to_nodes(entries: Vec<OutlineEntry>, page_count: u32) -> Vec<OutlineNode> {
    entries
        .into_iter()
        .filter(|entry| entry.page >= 1)
        .map(|entry| OutlineNode {
            title: entry.title.trim().to_string(),
            page: if page_count == 0 {
                1
            } else {
                entry.page.min(page_count)
            },
            depth: clamp_depth(entry.depth),
        })
        .filter(|node| !node.title.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, page: u32, depth: u32) -> OutlineEntry {
        OutlineEntry {
            title: title.into(),
            page,
            depth,
        }
    }

    #[test]
    fn entries_become_nodes_in_order() {
        let nodes = to_nodes(vec![entry("One", 1, 0), entry("One.a", 2, 1)], 10);
        assert_eq!(nodes.len(), 2);
        assert_eq!(
            (nodes[1].title.as_str(), nodes[1].page, nodes[1].depth),
            ("One.a", 2, 1)
        );
    }

    #[test]
    fn an_unresolved_destination_and_a_blank_title_are_dropped() {
        let nodes = to_nodes(
            vec![
                entry("Nowhere", 0, 0),
                entry("  ", 3, 0),
                entry("Real", 4, 0),
            ],
            9,
        );
        assert_eq!(
            nodes.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(),
            ["Real"]
        );
    }

    #[test]
    fn pages_clamp_to_the_book_that_actually_opened() {
        // Never jump past the last sheet of a shorter file.
        let nodes = to_nodes(vec![entry("Late", 900, 0)], 12);
        assert_eq!(nodes[0].page, 12);
        // A book with no pages keeps every entry on page 1.
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
