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
