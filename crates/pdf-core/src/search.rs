//! Serde types for search results returned by engine.search(), plus the pure
//! navigation/scroll maths the search UI runs on.
//!
//! The engine returns ONE ENTRY PER OCCURRENCE in document order, not one per
//! page: "next result" means the next match, which is usually still on the
//! current page. A match's rect is in scale-1 CSS px relative to its page's
//! top-left; the UI multiplies by the current scale to place it.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

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
}

/// `{ok:true, query, total, matches:[…]}` — engine.search().
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub total: u32,
    pub matches: Vec<SearchMatch>,
}

// ---------------------------------------------------------------------------
// The in-process search index
// ---------------------------------------------------------------------------
//
// The engine (pdf.js) can only extract text in the browser; everything after
// that — lowercasing, occurrence matching, snippet building — is string
// work that belongs here, off the JS heap. The engine hands pages over as
// [`PageText`] (geometry already normalised to scale-1 CSS px), [`SearchIndex`]
// stores them per page, and `query` scans the stored strings per keystroke
// with no pdf.js round trip at all.
//
// The matching mirrors what the old JS `search()` did, so results, order and
// highlight geometry stay byte-identical for ASCII documents: per item, scan
// the lowercased text for the query, interpolate the match rect across the
// item rect, and build the snippet with the same ±25/±30-character window.
// `index` is the 0-based ordinal of the occurrence WITHIN its page, in
// reading order — the same number the engine stamps on highlight boxes, so
// `setActiveMatch` keeps naming one box without matching geometry.

/// One extracted text run of a page, with its scale-1 rect.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchItem {
    /// The original glyph string (what snippets read naturally).
    pub text: String,
    /// Lowercased copy of `text`, computed once at index build so every
    /// query scans strings instead of allocating a fresh lowercase per item.
    pub lower: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl SearchItem {
    /// Build an index item, precomputing the lowercase copy.
    pub fn new(text: impl Into<String>, x: f64, y: f64, w: f64, h: f64) -> Self {
        let text = text.into();
        Self {
            lower: text.to_lowercase(),
            text,
            x,
            y,
            w,
            h,
        }
    }
}

/// One page's extracted text, as the engine hands it over.
#[derive(Debug, Clone, PartialEq)]
pub struct PageText {
    /// 1-based page number.
    pub page: u32,
    pub items: Vec<SearchItem>,
}

/// The document's full-text index: every extracted page, in document order.
///
/// Pages can arrive OUT of order (the builder extracts concurrently), so the
/// index keeps them keyed by page and walks them sorted at query time.
#[derive(Debug, Default)]
pub struct SearchIndex {
    pages: std::collections::BTreeMap<u32, PageText>,
}

/// One query's worth of matched positions within a single item, safe against
/// non-UTF8-boundary slicing (ASCII is the byte==char fast path).
fn find_occurrences(hay: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    if hay.is_ascii() && needle.is_ascii() {
        // Byte offsets are char offsets here; mirrors JS `indexOf` loop.
        let mut out = Vec::new();
        let mut at = 0usize;
        while let Some(rel) = hay[at..].find(needle) {
            let pos = at + rel;
            out.push(pos);
            at = pos + needle.len();
        }
        return out;
    }
    // Non-ASCII: search on chars so offsets are character offsets and every
    // subsequent slice is a valid boundary.
    let hay: Vec<char> = hay.chars().collect();
    let needle: Vec<char> = needle.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == needle[..] {
            out.push(i);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

/// Surrounding-text snippet: up to 25 chars before and 30 after the match,
/// with ellipses on the clipped sides — the same window the JS built.
fn snippet(text: &str, qlen: usize, at: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let start = at.saturating_sub(25);
    let end = (at + qlen + 30).min(chars.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out
}

impl SearchIndex {
    /// Create an empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of pages indexed so far.
    pub fn page_count(&self) -> u32 {
        self.pages.len() as u32
    }

    /// True when nothing is indexed yet.
    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Add (or replace) one page's extracted text. Replacing keeps a
    /// re-extraction idempotent; the builder sends each page exactly once.
    pub fn add_page(&mut self, page: PageText) {
        self.pages.insert(page.page, page);
    }

    /// Drop everything (new document, rebuild).
    pub fn clear(&mut self) {
        self.pages.clear();
    }

    /// Run `query` against the index, returning every occurrence in document
    /// order. An empty index or empty query yields an empty response.
    pub fn query(&self, query: &str) -> SearchResponse {
        let q = query.to_lowercase();
        let qlen = q.chars().count();
        let mut matches = Vec::new();
        if qlen == 0 {
            return SearchResponse {
                query: query.to_string(),
                total: 0,
                matches,
            };
        }
        for page in self.pages.values() {
            let mut ord = 0u32;
            for item in &page.items {
                // A zero-width run has no rectangle to highlight — skip it
                // exactly like the JS search did.
                if item.w <= 0.0 {
                    continue;
                }
                let len = item.lower.chars().count().max(1);
                for at in find_occurrences(&item.lower, &q) {
                    let at_f = at as f64;
                    matches.push(SearchMatch {
                        page: page.page,
                        index: ord,
                        text: Arc::<str>::from(snippet(&item.text, qlen, at)),
                        x: item.x + (item.w * at_f) / len as f64,
                        y: item.y,
                        w: (item.w * qlen as f64 / len as f64).max(1.0),
                        h: item.h,
                    });
                    ord += 1;
                }
            }
        }
        SearchResponse {
            query: query.to_string(),
            total: matches.len() as u32,
            matches,
        }
    }
}

#[cfg(test)]
mod index_tests {
    use super::*;

    fn item(text: &str, x: f64, w: f64) -> SearchItem {
        SearchItem::new(text, x, 100.0, w, 12.0)
    }

    fn page(n: u32, items: Vec<SearchItem>) -> PageText {
        PageText { page: n, items }
    }

    #[test]
    fn query_finds_every_occurrence_across_items_and_pages() {
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("The quick brown fox", 0.0, 100.0), item("jumps over the fox", 10.0, 90.0)]));
        index.add_page(page(2, vec![item("A fox in a box", 20.0, 80.0)]));
        let resp = index.query("fox");
        assert_eq!(resp.total, 3);
        // Document order, even though page 2 was added before page 1 here.
        assert_eq!(resp.matches[0].page, 1);
        assert_eq!(resp.matches[1].page, 1);
        assert_eq!(resp.matches[2].page, 2);
        // Ordinal is per page, in reading order.
        assert_eq!((resp.matches[0].page, resp.matches[0].index), (1, 0));
        assert_eq!((resp.matches[1].page, resp.matches[1].index), (1, 1));
        assert_eq!((resp.matches[2].page, resp.matches[2].index), (2, 0));
    }

    #[test]
    fn matching_is_case_insensitive_and_echoes_the_query() {
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("Hello World", 0.0, 100.0)]));
        let resp = index.query("WORLD");
        assert_eq!(resp.total, 1);
        assert_eq!(resp.query, "WORLD");
        assert_eq!(resp.matches[0].text.as_ref(), "Hello World");
    }

    #[test]
    fn rect_is_interpolated_across_the_item() {
        let mut index = SearchIndex::new();
        // "abcd", 100 px wide: "bc" starts at char 1 of 4 → x = 25.
        index.add_page(page(1, vec![item("abcd", 0.0, 100.0)]));
        let resp = index.query("bc");
        assert_eq!(resp.total, 1);
        assert!((resp.matches[0].x - 25.0).abs() < 1e-9, "x = {}", resp.matches[0].x);
        assert!((resp.matches[0].w - 50.0).abs() < 1e-9, "w = {}", resp.matches[0].w);
    }

    #[test]
    fn snippet_truncates_with_ellipses() {
        let mut index = SearchIndex::new();
        let long = format!("{}NEEDLE{}", "a".repeat(40), "b".repeat(40));
        index.add_page(page(1, vec![item(&long, 0.0, 100.0)]));
        let resp = index.query("needle");
        assert_eq!(resp.total, 1);
        let s = &resp.matches[0].text;
        assert!(s.starts_with('…') && s.ends_with('…'), "snippet: {s}");
        assert_eq!(s.chars().filter(|c| *c == 'N').count(), 1);
        // Window: ≤25 before (incl. ellipsis) and ≤30 after (incl. ellipsis).
        let core: String = s.chars().filter(|c| *c != '…').collect();
        assert!(core.chars().count() <= 25 + 6 + 30, "core len {}", core.chars().count());
    }

    #[test]
    fn whole_text_snippet_has_no_ellipses() {
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("a needle here", 0.0, 100.0)]));
        let resp = index.query("needle");
        assert_eq!(resp.matches[0].text.as_ref(), "a needle here");
    }

    #[test]
    fn empty_query_and_empty_index_are_clean() {
        let mut index = SearchIndex::new();
        assert_eq!(index.query("").total, 0);
        assert_eq!(index.query("x").total, 0);
        index.add_page(page(1, vec![item("alpha", 0.0, 1.0)]));
        assert_eq!(index.query("x").total, 0);
        assert_eq!(index.query("alpha").total, 1);
        index.clear();
        assert!(index.is_empty());
        assert_eq!(index.page_count(), 0);
    }

    #[test]
    fn zero_width_items_contribute_no_matches() {
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("noise", 0.0, 0.0)]));
        assert_eq!(index.query("noise").total, 0);
    }

    #[test]
    fn non_ascii_matching_never_panics_and_finds_the_query() {
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("héllo wörld", 0.0, 100.0)]));
        let resp = index.query("wörld");
        assert_eq!(resp.total, 1);
        assert_eq!(resp.matches[0].text.as_ref(), "héllo wörld");
        // And the casefold path: 'É' lowercases to 'é', which IS in "héllo".
        let resp = index.query("É");
        assert_eq!(resp.total, 1);
        let resp = index.query("héllo");
        assert_eq!(resp.total, 1);
    }

    #[test]
    fn overlapping_occurrences_advance_by_query_length() {
        // "aaaa" with query "aa": JS indexOf advances by qlen → 2 matches,
        // not 3. The port must keep that behaviour.
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("aaaa", 0.0, 100.0)]));
        let resp = index.query("aa");
        assert_eq!(resp.total, 2);
    }
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
pub const MATCH_VIEW_BIAS: f64 = 0.35;

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
