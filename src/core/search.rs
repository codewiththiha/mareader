//! Serde types for search results returned by engine.search().
//!
//! `matches` rects are in scale-1 CSS px relative to the page's top-left; the UI
//! multiplies by the current scale to compute highlight placement / scroll offsets.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub page: u32,
    pub text: String,
    pub matches: Vec<Rect>,
}

/// `{ok:true, query, total, results:[{page, text, matches}]}` — engine.search().
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub total: u32,
    pub results: Vec<SearchResult>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_results_return_none() {
        assert_eq!(next_search_index(0, None, 1), None);
        assert_eq!(next_search_index(0, None, -1), None);
        assert_eq!(next_search_index(0, Some(0), 1), None);
    }

    #[test]
    fn single_result_wraps_to_itself() {
        assert_eq!(next_search_index(1, Some(0), 1), Some(0));
        assert_eq!(next_search_index(1, Some(0), -1), Some(0));
        assert_eq!(next_search_index(1, None, 1), Some(0));
        assert_eq!(next_search_index(1, None, -1), Some(0));
    }

    #[test]
    fn forward_and_back_wrap_around() {
        assert_eq!(next_search_index(3, Some(2), 1), Some(0));
        assert_eq!(next_search_index(3, Some(0), -1), Some(2));
        assert_eq!(next_search_index(3, Some(1), 1), Some(2));
        assert_eq!(next_search_index(3, Some(1), -1), Some(0));
        // larger strides behave the same (dir only carries sign)
        assert_eq!(next_search_index(3, Some(0), 1), Some(1));
    }

    #[test]
    fn none_active_starts_at_first_or_last() {
        assert_eq!(next_search_index(3, None, 1), Some(0));
        assert_eq!(next_search_index(3, None, -1), Some(2));
    }

    #[test]
    fn zero_dir_is_a_noop() {
        // dir == 0 means neither forward nor back: stay put.
        assert_eq!(next_search_index(3, Some(1), 0), Some(1));
        assert_eq!(next_search_index(1, Some(0), 0), Some(0));
        // nothing active and no direction: no movement.
        assert_eq!(next_search_index(3, None, 0), None);
    }
}
