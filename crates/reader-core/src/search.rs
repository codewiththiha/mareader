//! The reader's search model, the scan both pipelines run, and the maths the
//! search UI runs on.
//!
//! Both pipelines answer in this shape: the PDF side builds it from the
//! engine's page-text index (`pdf_core::search`), a reflowable document from
//! `reflow_core::search`, and the results list, the cycling and the scroll
//! reveal are the same code for either. That is why it lives here rather than
//! beside either parser.
//!
//! The SCAN lives here for the same reason, and a sharper one: a match carries
//! an ordinal — which occurrence of the query this is — and the thing that
//! paints a box over the hit counts occurrences again, independently, to find
//! which of its boxes that ordinal names. Two scanners are two chances to
//! disagree about what an occurrence is, so there is one ([`occurrence_spans`]),
//! and the snippet window next to it ([`snippet`]) for the same reason.
//!
//! The engine returns ONE ENTRY PER OCCURRENCE in document order, not one per
//! page: "next result" means the next match, which is usually still on the
//! current page. A match's rect is in scale-1 CSS px relative to its page's
//! top-left; the UI multiplies by the current scale to place it.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Which occurrence of the query, in which block, of a document with no fixed
/// page grid.
///
/// The two halves of a [`SearchMatch`] answer "where is this hit" for the two
/// kinds of document the reader opens. A page of pixels has a fixed grid, so a
/// box in page space IS an identity, and `x`/`y`/`w`/`h` carry it. A document the
/// reader lays out itself has no such grid — every typography knob re-cuts its
/// pages, and a stored box would point at whatever moved underneath — so its
/// answer is the block and the occurrence inside it, which is the same identity
/// its gloss marks keep.
///
/// The painter that covers a block's row re-finds the query in that row's
/// rendered text and numbers the occurrences in reading order, so this pair
/// names one box on screen without any geometry being exchanged — exactly the
/// deal the engine's text-layer painter makes with `page` + `index`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BlockHit {
    /// Index of the block the hit sits in, in document order.
    pub block: u32,
    /// Which occurrence of the query inside that block, counting from zero.
    pub occurrence: u32,
}

/// One occurrence of the query.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchMatch {
    /// 1-based page holding this occurrence.
    pub page: u32,
    /// Ordinal of this occurrence WITHIN its page, in reading order. The engine
    /// stamps the same number onto the highlight box it paints, so this pair
    /// names one box on screen without matching geometry.
    pub index: u32,
    /// Snippet of surrounding text, for the results list. Shared so a
    /// 500-hit query does not clone 500 independent `String`s of the same
    /// haystack windows.
    pub text: Arc<str>,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Where the hit sits in a document the reader lays out itself; `None` for a
    /// fixed-grid one, whose rect above is the whole answer. `#[serde(default)]`
    /// because the engine's response has never carried it and never will.
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

/// Characters of context on each side of a hit in a results-list snippet.
///
/// One number for both families, because one dropdown shows both: the row clips
/// at 80 characters anyway (`components/search/result_list.rs`), so a wider
/// window is text the reader never sees, and two windows are two shapes of the
/// same row.
pub const SNIPPET_RADIUS: usize = 32;

/// Every occurrence of `needle` in `haystack`, as character spans in reading
/// order: the one scan, called by the PDF's page-text index, by a reflowable
/// document's blocks, and by the layer that paints hits over a block's rendered
/// text.
///
/// `folded` is `haystack.to_lowercase()`. It is a parameter rather than a call
/// here because the hot caller already holds it: the PDF index folds every page's
/// text once when it builds and rescans it on every keystroke.
///
/// Matching is case-insensitive and non-overlapping, advancing by the needle's
/// length — `"aa"` in `"aaa"` is one hit, at 0 — which is what
/// `str::match_indices` does and what the engine's painter has always done. An
/// empty or whitespace-only needle matches nothing.
///
/// Case folding can change a string's LENGTH: 'İ' lowercases to two characters.
/// A span counted in the folded copy would then not be a span of the text the
/// reader is looking at, so when folding changed the character count the scan
/// runs over the ORIGINAL, case-sensitively. A missed hit is a smaller lie than
/// a box over characters nobody searched for.
pub fn occurrence_spans(haystack: &str, folded: &str, needle: &str) -> Vec<(usize, usize)> {
    let needle = needle.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    // ASCII is the common case and the cheap one: folding is one character for
    // one, so byte offsets are character offsets and neither side is copied.
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

/// The context window around a hit, for one results-list row: [`SNIPPET_RADIUS`]
/// characters either side of `[start, end)`, elided at the edges the window does
/// not reach, newlines folded to spaces because a row is one line.
///
/// The offsets are CHARACTERS — the spans [`occurrence_spans`] reports — so the
/// window needs no byte-boundary walking and reads the same in a document of
/// Latin prose and one of emoji. Casing is the original's: the scan runs over a
/// folded copy, the reader reads this.
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

/// Next active-result index with wrap-around. `dir > 0` forward, `dir < 0` back.
/// `active = None` → first (dir > 0) or last (dir < 0).
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

/// Fraction of the reading area to leave above a match when scrolling it into
/// view, so it lands in comfortable reading position rather than jammed against
/// the top edge.
const MATCH_VIEW_BIAS: f64 = 0.35;

/// Scroll offset that brings a match into view, or `None` if it already is.
///
/// WHY A RANGE AND NOT A PAGE TOP. Jumping to `page_top` put the match anywhere
/// on the page — a hit near the bottom of a tall page landed off-screen, and
/// the reader had to hunt for it. This targets the MATCH.
///
/// It is also deliberately lazy: while the match is comfortably inside the
/// reading area the view does not move at all, so stepping through several hits
/// on one screen highlights them in place instead of jerking the page for each.
///
/// Arguments are in the scroll container's coordinates: `match_top`/`match_bot`
/// are the match's edges within the column, `scroll_top` the current offset,
/// `viewport_h` the container height, and `inset_top`/`inset_bottom` the parts
/// of the container hidden behind the floating toolbar / covered by the search
/// bar. `margin` keeps the match clear of those edges.
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
    // A viewport too small for the insets (or a match taller than the band):
    // fall back to putting the match's top at the top of the readable area.
    if view_bot <= view_top || match_bot - match_top > view_bot - view_top {
        return Some((match_top - inset_top - margin).max(0.0));
    }
    if match_top >= view_top && match_bot <= view_bot {
        return None; // already comfortably visible — don't move
    }
    // Off-screen (or clipped): place it at the bias line, which reads better
    // than pinning it to whichever edge it left from.
    let band = view_bot - view_top;
    let target = match_top - inset_top - margin - band * MATCH_VIEW_BIAS;
    Some(target.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cycling through results: wrap in both directions, start at the first or
    /// last when nothing is active, and treat a single result as its own
    /// neighbour. `dir` carries only a sign, so a larger stride behaves the same.
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
            assert_eq!(next_search_index(len, active, dir), want, "len={len} active={active:?} dir={dir}");
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

    /// A match already sitting in the readable band does not move the view.
    /// This is what lets several hits on one screen light up one after another
    /// without the page twitching.
    #[test]
    fn visible_match_does_not_scroll() {
        // scroll 600, viewport 800, top inset 48, bottom inset 56, margin 24
        // => readable band spans 672..1320 in scroll coordinates.
        assert_eq!(scroll_to_reveal(700.0, 720.0, 600.0, 800.0, 48.0, 56.0, 24.0), None);
        // Flush against each edge of the band is still "visible".
        assert_eq!(scroll_to_reveal(672.0, 700.0, 600.0, 800.0, 48.0, 56.0, 24.0), None);
        assert_eq!(scroll_to_reveal(1290.0, 1320.0, 600.0, 800.0, 48.0, 56.0, 24.0), None);
    }

    /// A match under the fold, or hidden behind the top chrome, is brought to
    /// the bias line — NOT to the edge it left from, and never past 0.
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

    /// A match partly clipped by the bottom edge counts as not visible: the
    /// bug being fixed is precisely "the hit is on this page but off-screen".
    #[test]
    fn clipped_match_is_revealed() {
        // Band is 672..1320; this straddles the bottom edge, so the reader can
        // only see part of the hit.
        assert!(scroll_to_reveal(1300.0, 1360.0, 600.0, 800.0, 48.0, 56.0, 24.0).is_some());
        // And this one is clipped by the top chrome.
        assert!(scroll_to_reveal(650.0, 690.0, 600.0, 800.0, 48.0, 56.0, 24.0).is_some());
    }

    /// The engine's JSON has no `block_hit` in it, and a search response that
    /// failed to deserialize would take the whole feature down — so the field is
    /// optional on the wire and reads as `None` for a PDF.
    #[test]
    fn a_match_from_the_engine_deserializes_without_a_block_hit() {
        let json = r#"{"query":"dune","total":1,"matches":[
            {"page":4,"index":0,"text":"…the dune sea…","x":12.0,"y":80.5,"w":30.0,"h":9.0}
        ]}"#;
        let response: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.matches.len(), 1);
        assert_eq!(response.matches[0].block_hit, None);

        // And a reflowable match round-trips the half a PDF never sends.
        let hit = BlockHit { block: 17, occurrence: 2 };
        let json = serde_json::to_string(&hit).unwrap();
        assert_eq!(serde_json::from_str::<BlockHit>(&json).unwrap(), hit);
    }

    /// The scan both pipelines and the highlight painter share: reading order,
    /// non-overlapping, and counting CHARACTERS — an emoji is one of them, so a
    /// hit after it starts where the reader would count, not where its bytes are.
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

    /// 'İ' folds to two characters, so the folded copy's offsets are not the
    /// original's. The scan refuses to guess: it drops back to a case-sensitive
    /// read of the text the reader actually sees.
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

    /// The window the results list shows: original casing, newlines folded, and
    /// an ellipsis only on the edges it actually cut.
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
        assert_eq!(snippet(folded_newlines, start, end), "alpha beta GAMMA delta");
    }

    /// Degenerate geometry must still produce a usable offset rather than
    /// panicking or returning None: a match taller than the band, and a
    /// viewport smaller than its own insets.
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
