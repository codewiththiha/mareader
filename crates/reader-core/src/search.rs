//! The reader's search model, the scan both pipelines run, and the
//! scroll maths.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Which occurrence of the query, in which block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BlockHit {
    pub block: u32,
    /// Which occurrence of the query inside that block, counting from zero.
    pub occurrence: u32,
}

/// One occurrence of the query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchMatch {
    /// 1-based page holding this occurrence.
    pub page: u32,
    /// Ordinal of this occurrence within its page, in reading order.
    pub index: u32,
    /// Snippet of surrounding text for the results list, shared.
    pub text: Arc<str>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Where the hit sits in a reflowable document; `None` for a PDF.
    #[serde(default)]
    pub block_hit: Option<BlockHit>,
}

/// `{ok:true, query, total, matches:[…]}` — engine.search().
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub total: u32,
    pub matches: Vec<SearchMatch>,
}

/// Characters of context each side of a hit in a snippet.
pub const SNIPPET_RADIUS: usize = 32;

/// Every occurrence of `needle` in `haystack`, as character spans;
/// case-insensitive, non-overlapping, by needle length.
pub fn occurrence_spans(haystack: &str, folded: &str, needle: &str) -> Vec<(usize, usize)> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    // ASCII: folding is one character for one, so byte offsets are
    // character offsets.
    if haystack.is_ascii() && folded.is_ascii() && needle.is_ascii() {
        let needle = needle.to_ascii_lowercase();
        let mut out = Vec::new();
        let mut at = 0;
        while let Some(found) = folded[at..].find(&needle) {
            let start = at + found;
            out.push((start, start + needle.len()));
            at = start + needle.len();
        }
        return out;
    }
    let lowered = needle.to_lowercase();
    let (text, want) = if folded.chars().count() == haystack.chars().count() {
        (folded, lowered.as_str())
    } else {
        (haystack, needle)
    };
    let chars: Vec<char> = text.chars().collect();
    let want: Vec<char> = want.chars().collect();
    if want.len() > chars.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at + want.len() <= chars.len() {
        if chars[at..at + want.len()] == want[..] {
            out.push((at, at + want.len()));
            at += want.len();
        } else {
            at += 1;
        }
    }
    out
}

/// The context window around a hit for one results-list row.
pub fn snippet(text: &str, start: usize, end: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let from = start.saturating_sub(SNIPPET_RADIUS).min(chars.len());
    let to = (end + SNIPPET_RADIUS).min(chars.len()).max(from);
    let mut out: String = chars[from..to]
        .iter()
        .map(|&c| if c == '\n' { ' ' } else { c })
        .collect();
    if from > 0 {
        out.insert(0, '…');
    }
    if to < chars.len() {
        out.push('…');
    }
    out
}

/// Next active-result index with wrap-around.
pub fn next_search_index(len: usize, active: Option<usize>, dir: i32) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match active {
        Some(i) if dir > 0 => (i + 1) % len,
        Some(i) if dir == 0 => i, // dir == 0 is a no-op: stay put
        Some(i) => (i + len - 1) % len,
        None if dir > 0 => 0,
        None if dir == 0 => return None, // dir == 0 with nothing active: no movement
        None => len - 1,
    })
}

/// Fraction of the reading area left above a match.
const MATCH_VIEW_BIAS: f64 = 0.35;

/// Scroll offset bringing a match into view, or `None` if visible.
pub fn scroll_to_reveal(
    match_top: f64,
    match_bot: f64,
    scroll_top: f64,
    viewport_h: f64,
    inset_top: f64,
    inset_bottom: f64,
    margin: f64,
) -> Option<f64> {
    // The genuinely readable band, in scroll coordinates.
    let view_top = scroll_top + inset_top + margin;
    let view_bot = scroll_top + viewport_h - inset_bottom - margin;
    // A viewport too small for the insets falls back to the match's top.
    if view_bot <= view_top || match_bot - match_top > view_bot - view_top {
        return Some((match_top - inset_top - margin).max(0.0));
    }
    if match_top >= view_top && match_bot <= view_bot {
        return None; // already comfortably visible — don't move
    }
    // Off-screen or clipped: place it at the bias line.
    let band = view_bot - view_top;
    let target = match_top - inset_top - margin - band * MATCH_VIEW_BIAS;
    Some(target.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cycling through results: wrap both ways, and a lone result is its
    /// own neighbour.
    #[test]
    fn cycles_and_wraps() {
        // (len, active, dir, expected)
        let cases: &[(usize, Option<usize>, i32, Option<usize>)] = &[
            (3, Some(2), 1, Some(0)),
            (3, Some(0), -1, Some(2)),
            (3, Some(1), 1, Some(2)),
            (3, Some(1), -1, Some(0)),
            (3, Some(0), 1, Some(1)),
            (3, None, 1, Some(0)),
            (3, None, -1, Some(2)),
            (1, Some(0), 1, Some(0)),
            (1, Some(0), -1, Some(0)),
            (1, None, 1, Some(0)),
            (1, None, -1, Some(0)),
        ];
        for &(len, active, dir, want) in cases {
            assert_eq!(
                next_search_index(len, active, dir),
                want,
                "len={len} active={active:?} dir={dir}"
            );
        }
    }

    /// No results means nothing to select, and `dir == 0` means stay put.
    #[test]
    fn empty_results_and_zero_direction() {
        assert_eq!(next_search_index(0, None, 1), None);
        assert_eq!(next_search_index(0, None, -1), None);
        assert_eq!(next_search_index(0, Some(0), 1), None);
        assert_eq!(next_search_index(3, Some(1), 0), Some(1));
        assert_eq!(next_search_index(1, Some(0), 0), Some(0));
        assert_eq!(next_search_index(3, None, 0), None);
    }

    /// A match already in the readable band does not move the view.
    #[test]
    fn visible_match_does_not_scroll() {
        // scroll 600, viewport 800, insets 48/56, margin 24: band 672..1320.
        assert_eq!(
            scroll_to_reveal(700.0, 720.0, 600.0, 800.0, 48.0, 56.0, 24.0),
            None
        );
        // Flush against each edge of the band is still "visible".
        assert_eq!(
            scroll_to_reveal(672.0, 700.0, 600.0, 800.0, 48.0, 56.0, 24.0),
            None
        );
        assert_eq!(
            scroll_to_reveal(1290.0, 1320.0, 600.0, 800.0, 48.0, 56.0, 24.0),
            None
        );
    }

    /// A match under the fold is brought to the bias line, never past 0.
    #[test]
    fn offscreen_match_scrolls_to_the_bias_line() {
        let band = 800.0 - 48.0 - 56.0 - 2.0 * 24.0; // 624
        // Far below the fold.
        let want = 5000.0 - 48.0 - 24.0 - band * MATCH_VIEW_BIAS;
        assert_eq!(
            scroll_to_reveal(5000.0, 5020.0, 600.0, 800.0, 48.0, 56.0, 24.0),
            Some(want)
        );
        // Above the band (scrolled past): comes back to the same bias line.
        let want_up = 100.0 - 48.0 - 24.0 - band * MATCH_VIEW_BIAS;
        assert_eq!(
            scroll_to_reveal(100.0, 120.0, 600.0, 800.0, 48.0, 56.0, 24.0),
            Some(want_up.max(0.0))
        );
        // Near the very top of the document: clamped, never negative.
        assert!(scroll_to_reveal(10.0, 30.0, 600.0, 800.0, 48.0, 56.0, 24.0).unwrap() >= 0.0);
    }

    /// A match partly clipped by the bottom edge counts as not visible.
    #[test]
    fn clipped_match_is_revealed() {
        // Band 672..1320; this straddles the bottom edge.
        assert!(scroll_to_reveal(1300.0, 1360.0, 600.0, 800.0, 48.0, 56.0, 24.0).is_some());
        // And this one is clipped by the top chrome.
        assert!(scroll_to_reveal(650.0, 690.0, 600.0, 800.0, 48.0, 56.0, 24.0).is_some());
    }

    /// The engine's JSON carries no `block_hit`; the field is optional.
    #[test]
    fn a_match_from_the_engine_deserializes_without_a_block_hit() {
        let json = r#"{"query":"dune","total":1,"matches":[
            {"page":4,"index":0,"text":"…the dune sea…","x":12.0,"y":80.5,"w":30.0,"h":9.0}
        ]}"#;
        let response: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.matches.len(), 1);
        assert_eq!(response.matches[0].block_hit, None);

        // And a reflowable match round-trips the half a PDF never sends.
        let hit = BlockHit {
            block: 17,
            occurrence: 2,
        };
        let json = serde_json::to_string(&hit).unwrap();
        assert_eq!(serde_json::from_str::<BlockHit>(&json).unwrap(), hit);
    }

    /// The scan both pipelines and the painter share: reading order, by
    /// character.
    #[test]
    fn occurrences_are_numbered_in_reading_order_without_overlapping() {
        let folded = |s: &str| s.to_lowercase();
        let spans = |hay: &str, needle: &str| occurrence_spans(hay, &folded(hay), needle);

        assert_eq!(spans("The Dune of Dune", "dune"), vec![(4, 8), (12, 16)]);
        // One hit, not two: the first consumes the characters the second would
        // have started on.
        assert_eq!(spans("aaa", "aa"), vec![(0, 2)]);
        assert_eq!(spans("ab\u{1F600}cd dune", "dune"), vec![(6, 10)]);
        assert_eq!(spans("héllo wörld", "HÉLLO"), vec![(0, 5)]);
        // Nothing to search for, nothing found — including a query of spaces.
        assert!(spans("anything", "").is_empty());
        assert!(spans("anything", "   ").is_empty());
        // A padded query still matches, at the trimmed needle's length.
        assert_eq!(spans("a target here", " target "), vec![(2, 8)]);
    }

    /// 'İ' folds to two characters, so the folded copy's offsets do not
    /// transfer.
    #[test]
    fn a_fold_that_changes_length_never_reports_a_moved_offset() {
        let text = "İstanbul dune";
        let folded = text.to_lowercase();
        assert_ne!(folded.chars().count(), text.chars().count());
        // The case-sensitive fallback still finds the exact hit, at the offset
        // the ORIGINAL text has.
        assert_eq!(occurrence_spans(text, &folded, "dune"), vec![(9, 13)]);
        // And it does not pretend to a case-insensitive one it cannot place.
        assert!(occurrence_spans(text, &folded, "DUNE").is_empty());
    }

    /// The window the results list shows: original casing, newlines folded.
    #[test]
    fn the_snippet_window_elides_only_the_edges_it_cuts() {
        let long = format!("{}target{}", "x".repeat(200), "y".repeat(200));
        let (start, end) = occurrence_spans(&long, &long.to_lowercase(), "target")[0];
        let s = snippet(&long, start, end);
        assert!(s.starts_with('…') && s.ends_with('…'), "{s}");
        assert_eq!(s.chars().count(), SNIPPET_RADIUS + 6 + SNIPPET_RADIUS + 2);

        // A hit at the very start has no left edge to elide.
        let head = format!("target{}", "y".repeat(200));
        let (start, end) = occurrence_spans(&head, &head.to_lowercase(), "target")[0];
        let s = snippet(&head, start, end);
        assert!(!s.starts_with('…'), "{s}");
        assert!(s.ends_with('…'));

        // The whole text fits: no ellipses, casing kept, newlines folded.
        let folded_newlines = "alpha\nbeta GAMMA delta";
        let (start, end) =
            occurrence_spans(folded_newlines, &folded_newlines.to_lowercase(), "gamma")[0];
        assert_eq!(
            snippet(folded_newlines, start, end),
            "alpha beta GAMMA delta"
        );
    }

    /// Degenerate geometry still produces a usable offset.
    #[test]
    fn degenerate_geometry_falls_back_to_top_alignment() {
        assert_eq!(
            scroll_to_reveal(2000.0, 4000.0, 0.0, 800.0, 48.0, 56.0, 24.0),
            Some(2000.0 - 48.0 - 24.0)
        );
        assert_eq!(
            scroll_to_reveal(2000.0, 2020.0, 0.0, 60.0, 48.0, 56.0, 24.0),
            Some(2000.0 - 48.0 - 24.0)
        );
    }
}
