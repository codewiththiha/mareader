//! Full-text search, Rust side: an in-process index over the document's
//! extracted text.
//!
//! The pdf.js worker can only extract text in the browser, so the engine (JS)
//! hands each page over via `bridge::extract_page_text` and everything after
//! that — lowercasing, occurrence matching, snippet building, result ordering
//! — happens here, on the wasm heap. The index is rebuilt on document open
//! (and lazily by the search effect if it isn't), after which a query is a
//! pure in-Rust scan: no pdf.js round trip, no per-query text extraction.
//!
//! Extraction is concurrent in bounded batches: [`SEARCH_PAGE_CONCURRENCY`]
//! pages in flight per turn, so the pdf.js worker is never flooded and live
//! page renders keep their share of it. Between turns the builder's future
//! falls back to Pending awaiting the next pdf.js round trip, so the event
//! loop (and the reader's own renders) runs between turns without a busy
//! wait — a textbook-sized index build stays responsive.

use std::cell::RefCell;

use futures::stream::{self, StreamExt};
use serde::Deserialize;

use pdf_core::search::{PageText, SearchIndex, SearchItem, SearchResponse};

use super::{require_pdf_reader, resolve, EngineError};
use crate::bridge;

/// Pages extracted concurrently per turn while the index is built. Three is
/// enough to hide the per-page worker round trip without starving live
/// renders (the caller reads "3 pages per turn" as the feel of the build);
/// this is deliberately a plain const, not a setting.
pub const SEARCH_PAGE_CONCURRENCY: usize = 3;

thread_local! {
    static INDEX: RefCell<SearchIndex> = RefCell::new(SearchIndex::new());
}

fn with<R>(f: impl FnOnce(&mut SearchIndex) -> R) -> R {
    INDEX.with(|i| f(&mut i.borrow_mut()))
}

/// `{ok:true, page, items:[{str,x,y,w,h}]}` — engine.extractPageText. The
/// items are already normalised to scale-1 CSS px relative to the page's
/// top-left, so this is the one payload shape the search module parses.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PageTextPayload {
    page: u32,
    items: Vec<ItemPayload>,
}

#[derive(Debug, Deserialize)]
struct ItemPayload {
    #[serde(rename = "str")]
    text: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

/// Extract every page (concurrently, [`SEARCH_PAGE_CONCURRENCY`] per turn)
/// and build the in-process index. Returns the number of pages indexed; the
/// caller usually ignores it, the `{ok:true, count}` envelope shape is kept
/// for the engine contract.
///
/// Unreadable pages are skipped, never fatal — the old streaming search did
/// the same (a corrupted page must not kill a search).
pub async fn build_search_index(num_pages: u32) -> Result<u32, EngineError> {
    require_pdf_reader()?;
    with(|i| i.clear());
    if num_pages == 0 {
        return Ok(0);
    }

    // One TURN = [`SEARCH_PAGE_CONCURRENCY`] pages extracted concurrently,
    // then the builder yields to the event loop before the next turn. The
    // whole document never floods the pdf.js worker, and between turns the
    // reader's own renders get the main thread (buffer_unordered over the
    // whole stream would also cap in-flight work, but never let the UI run
    // until the LAST page's promise settled).
    let mut indexed = 0u32;
    let mut cursor = 1u32;
    while cursor <= num_pages {
        let end = (cursor + SEARCH_PAGE_CONCURRENCY as u32 - 1).min(num_pages);
        let batch: Vec<u32> = (cursor..=end).collect();
        let extracted: Vec<Option<PageTextPayload>> = stream::iter(batch)
            .map(|page| async move {
                let value = bridge::extract_page_text(page).await;
                resolve::<PageTextPayload>(value, "extractPageText").ok()
            })
            .buffer_unordered(SEARCH_PAGE_CONCURRENCY)
            .collect()
            .await;

        for p in extracted.into_iter().flatten() {
            let items = p
                .items
                .into_iter()
                .map(|it| SearchItem::new(it.text, it.x, it.y, it.w, it.h))
                .collect();
            with(|i| i.add_page(PageText { page: p.page, items }));
            indexed += 1;
        }
        cursor = end + 1;
    }
    Ok(indexed)
}

/// Query the in-process index. No engine round trip: the whole response is
/// computed from the extracted text this crate already holds.
///
/// After the query, the active query is published to the engine's text layers
/// (`setSearchContext`) so already-mounted pages repaint their highlight
/// boxes — they paint from the DOM text layer, not from the match list, so
/// without this the results list would fill while the page stayed unmarked.
pub async fn search(query: &str) -> Result<SearchResponse, EngineError> {
    let response = with(|i| i.query(query));
    bridge::set_search_context(query);
    Ok(response)
}

/// Emphasise occurrence `index` of `page` as the current match (`index < 0`
/// clears the marker without touching the other highlights).
pub fn set_active_match(page: u32, index: i32) {
    bridge::set_active_match(page, index);
}

/// Drop every highlight and the active match, and clear the query context.
pub fn clear_highlights() {
    if !super::guard_pdf_reader() {
        return;
    }
    bridge::clear_highlights();
}

/// Forget the current document's index (teardown path).
pub fn clear_index() {
    with(|i| i.clear());
}
