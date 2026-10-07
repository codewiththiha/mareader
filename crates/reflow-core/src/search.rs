//! Search over a reflowable document: the shared scan over its blocks.

use reader_core::search::{occurrence_spans, snippet};

use crate::block::TextBlock;

/// One occurrence of the query.
#[derive(Debug, Clone, PartialEq)]
pub struct TextHit {
    pub block: usize,
    /// Which occurrence of the query this is inside its own block, from zero.
    pub occurrence: usize,
    /// Surrounding context for the results list, newlines folded to spaces.
    pub snippet: String,
}

/// Matches per query are capped, so a pathological haystack cannot swamp it.
const MAX_MATCHES: usize = 2000;

/// Every occurrence of `query`, in reading order; blank queries match nothing.
pub fn find_matches(blocks: &[TextBlock], query: &str) -> Vec<TextHit> {
    if query.trim().is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        // Folded once per block per query and handed to the scan.
        let folded = block.text.to_lowercase();
        let spans = occurrence_spans(&block.text, &folded, query);
        for (occurrence, (start, end)) in spans.into_iter().enumerate() {
            hits.push(TextHit {
                block: index,
                occurrence,
                snippet: snippet(&block.text, start, end),
            });
            if hits.len() >= MAX_MATCHES {
                return hits;
            }
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::BlockKind;

    fn block(text: &str) -> TextBlock {
        TextBlock::new(BlockKind::Text, text)
    }

    #[test]
    fn matching_is_case_insensitive_and_ordered() {
        let blocks = [
            block("The Dune of Dune"),
            block("no match here"),
            block("dune again"),
        ];
        let hits = find_matches(&blocks, "dune");
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].block, 0);
        assert_eq!(hits[1].block, 0);
        assert_eq!(hits[2].block, 2);
    }

    /// The ordinal a highlight painter pairs its own occurrences with.
    fn occurrences_are_numbered_within_their_own_block() {
        let blocks = [block("dune dune dune"), block("nothing"), block("dune")];
        let hits = find_matches(&blocks, "dune");
        let numbered: Vec<(usize, usize)> =
            hits.iter().map(|hit| (hit.block, hit.occurrence)).collect();
        assert_eq!(numbered, vec![(0, 0), (0, 1), (0, 2), (2, 0)]);
    }

    #[test]
    fn empty_and_blank_queries_match_nothing() {
        let blocks = [block("anything")];
        assert!(find_matches(&blocks, "").is_empty());
        assert!(find_matches(&blocks, "   ").is_empty());
    }

    #[test]
    fn snippets_keep_original_casing_and_fold_newlines() {
        let blocks = [block("alpha\nbeta GAMMA delta")];
        let hits = find_matches(&blocks, "gamma");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].snippet.contains("GAMMA"), "{}", hits[0].snippet);
        assert!(!hits[0].snippet.contains('\n'));
    }

    #[test]
    fn snippets_elide_only_the_edges_they_cut() {
        let long = format!("{}target{}", "x".repeat(200), "y".repeat(200));
        let blocks = [block(&long)];
        let hits = find_matches(&blocks, "target");
        let s = &hits[0].snippet;
        assert!(s.starts_with('…'), "{s}");
        assert!(s.ends_with('…'), "{s}");
        // A match at the very start elides only the right edge.
        let blocks = [block(&format!("target{}", "y".repeat(200)))];
        let s = &find_matches(&blocks, "target")[0].snippet;
        assert!(!s.starts_with('…'), "{s}");
        assert!(s.ends_with('…'));
    }

    #[test]
    fn unicode_does_not_break_the_window() {
        let blocks = [block("héllo wörld — héllo again")];
        let hits = find_matches(&blocks, "héllo");
        assert_eq!(hits.len(), 2);
        for hit in &hits {
            assert!(!hit.snippet.is_empty());
        }
    }

    #[test]
    fn the_match_list_is_capped() {
        let blocks = [block(&"a".repeat(MAX_MATCHES * 4))];
        let hits = find_matches(&blocks, "a");
        assert_eq!(hits.len(), MAX_MATCHES);
    }
}
