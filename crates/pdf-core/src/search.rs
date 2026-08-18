//! Serde types for search results returned by engine.search(), plus the pure
//! navigation/scroll maths the search UI runs on.
//!
//! The engine returns ONE ENTRY PER OCCURRENCE in document order, not one per
//! page: "next result" means the next match, which is usually still on the
//! current page. A match's rect is in scale-1 CSS px relative to its page's
//! top-left; the UI multiplies by the current scale to place it.

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
    /// Snippet of surrounding text, for the results list.
    pub text: String,
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
