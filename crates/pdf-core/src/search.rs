//! The PDF page-text index: the engine extracts, this crate searches.

use std::sync::Arc;

use reader_core::search::{SearchMatch, SearchResponse, occurrence_spans, snippet};

/// One extracted text run of a page, with its scale-1 rect.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchItem {
    /// The original glyph string (what snippets read naturally).
    pub text: String,
    /// Lowercased copy of `text`, computed once at index build.
    pub lower: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl SearchItem {
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

/// The document's full-text index: every extracted page, keyed by page.
#[derive(Debug, Default)]
pub struct SearchIndex {
    pages: std::collections::BTreeMap<u32, PageText>,
}

impl SearchIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// Add or replace one page; replacement keeps re-extraction idempotent.
    pub fn add_page(&mut self, page: PageText) {
        self.pages.insert(page.page, page);
    }

    pub fn clear(&mut self) {
        self.pages.clear();
    }

    /// Run `query` against the index, in document order.
    pub fn query(&self, query: &str) -> SearchResponse {
        let mut matches = Vec::new();
        if query.trim().is_empty() {
            return SearchResponse {
                query: query.to_string(),
                total: 0,
                matches,
            };
        }
        for page in self.pages.values() {
            let mut ord = 0u32;
            for item in &page.items {
                // A zero-width run has no rectangle to highlight; skip it.
                if item.w <= 0.0 {
                    continue;
                }
                // One rect per run: a hit's box is its proportional slice.
                let chars = item.text.chars().count().max(1) as f64;
                for (start, end) in occurrence_spans(&item.text, &item.lower, query) {
                    let span = (end - start).max(1) as f64;
                    matches.push(SearchMatch {
                        page: page.page,
                        index: ord,
                        text: Arc::<str>::from(snippet(&item.text, start, end)),
                        x: item.x + item.w * start as f64 / chars,
                        y: item.y,
                        w: (item.w * span / chars).max(1.0),
                        h: item.h,
                        // A page of pixels answers with the rect above.
                        block_hit: None,
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
    use reader_core::search::SNIPPET_RADIUS;

    fn item(text: &str, x: f64, w: f64) -> SearchItem {
        SearchItem::new(text, x, 100.0, w, 12.0)
    }

    fn page(n: u32, items: Vec<SearchItem>) -> PageText {
        PageText { page: n, items }
    }

    #[test]
    fn query_finds_every_occurrence_across_items_and_pages() {
        let mut index = SearchIndex::new();
        index.add_page(page(
            1,
            vec![
                item("The quick brown fox", 0.0, 100.0),
                item("jumps over the fox", 10.0, 90.0),
            ],
        ));
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
        assert!(
            (resp.matches[0].x - 25.0).abs() < 1e-9,
            "x = {}",
            resp.matches[0].x
        );
        assert!(
            (resp.matches[0].w - 50.0).abs() < 1e-9,
            "w = {}",
            resp.matches[0].w
        );
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
        // The shared window: SNIPPET_RADIUS characters either side of the hit.
        let core: String = s.chars().filter(|c| *c != '…').collect();
        assert_eq!(
            core.chars().count(),
            SNIPPET_RADIUS * 2 + 6,
            "core len {}",
            core.chars().count()
        );
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
        // "aaaa"/"aa": the scan advances by the needle, so 2 matches, not 3.
        let mut index = SearchIndex::new();
        index.add_page(page(1, vec![item("aaaa", 0.0, 100.0)]));
        let resp = index.query("aa");
        assert_eq!(resp.total, 2);
    }
}
