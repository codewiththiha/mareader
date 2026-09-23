//! Full-text search, Rust side: an in-process index over the document's
//! extracted text.
//!
//! The pdf.js worker can only extract text in the browser, so the engine hands
//! each page over via `bridge::extract_page_text` and everything after that —
//! lowercasing, occurrence matching, snippet building, result ordering —
//! happens here, on the wasm heap. The index is built LAZILY: the document's
//! first search pays the extraction, never the open flow. The index dies with
//! the format instance ([`scope_to_document`] clears; it does not adopt), so a
//! reopen extracts into a new heap. After the build, a query is a pure
//! in-Rust scan: no pdf.js round trip, no per-query extraction.
//!
//! Extraction is concurrent in bounded batches ([`SEARCH_PAGE_CONCURRENCY`]
//! pages in flight per turn), so the worker is never flooded and live renders
//! keep their share; between turns the builder falls back to Pending, so the
//! event loop (and the reader's renders) runs without a busy wait.

use std::cell::RefCell;

use futures::stream::{StreamExt, self};
use serde::Deserialize;

use pdf_core::search::{PageText, SearchIndex, SearchItem};
use reader_core::search::SearchResponse;

use super::{EngineError, require_pdf_reader, resolve};
use crate::bridge;

/// Pages extracted concurrently per turn while the index is built. Three is
/// enough to hide the per-page worker round trip without starving live
/// renders; deliberately a plain const, not a setting.
pub const SEARCH_PAGE_CONCURRENCY: usize = 3;

thread_local! {
    static INDEX: RefCell<SearchIndex> = RefCell::new(SearchIndex::new());
}

fn with<R>(f: impl FnOnce(&mut SearchIndex) -> R) -> R {
    INDEX.with(|i| f(&mut i.borrow_mut()))
}

/// Drop whatever index this heap holds. The signature stays so the open flow
/// does not grow a second call. Nothing is adopted: the instance dies with
/// the book, and a reopen extracts into the next heap. The arguments are the
/// document the caller just opened; they are not a cache key.
pub fn scope_to_document(fingerprint: Option<&str>, path: &str, num_pages: u32) {
    let _ = (fingerprint, path, num_pages);
    clear_index();
}

pub(crate) fn clear_index() {
    with(|i| i.clear());
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

/// Extract every page (concurrently, [`SEARCH_PAGE_CONCURRENCY`] per turn) and
/// build the in-process index. Returns the number of pages indexed; the caller usually ignores
/// it — the `{ok:true, count}` envelope shape is kept for the engine
/// contract. Unreadable pages are skipped, never fatal: a corrupted page must
/// not kill a search.
pub async fn build_search_index(num_pages: u32) -> Result<u32, EngineError> {
    require_pdf_reader()?;
    with(|i| i.clear());
    if num_pages == 0 {
        return Ok(0);
    }

    // One TURN = [`SEARCH_PAGE_CONCURRENCY`] pages extracted concurrently,
    // then the builder yields to the event loop. The document never floods
    // the pdf.js worker, and between turns the reader's renders get the main
    // thread (buffer_unordered over the whole stream would cap in-flight work
    // but never let the UI run until the LAST page settled).
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
/// (`setSearchContext`) so mounted pages repaint their highlight boxes — they
/// paint from the DOM text layer, not from the match list, so without this the
/// results list would fill while the page stayed unmarked.
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

pub fn clear_highlights() {
    if !super::guard_pdf_reader() {
        return;
    }
    bridge::clear_highlights();
}

// The scope/adopt/drop rules are pure host logic — the extraction itself
// needs the engine, but which index a document gets does not.
#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::search::{PageText, SearchItem};

    fn item(word: &str) -> SearchItem {
        SearchItem::new(word, 0.0, 0.0, 1.0, 1.0)
    }

    fn reset() {
        with(|i| i.clear());
    }

    fn plant() {
        with(|i| i.add_page(PageText { page: 1, items: vec![item("moby")] }));
    }

    #[test]
    fn reopening_the_same_book_does_not_adopt() {
        reset();
        plant();
        scope_to_document(Some("fp-a"), "/shelf/book.pdf", 1);
        assert!(with(|i| i.is_empty()));
        assert_eq!(with(|i| i.query("moby").total), 0);
    }

    #[test]
    fn a_different_book_finds_nothing() {
        reset();
        plant();
        scope_to_document(Some("fp-b"), "/shelf/other.pdf", 1);
        assert!(with(|i| i.is_empty()));
    }

    #[test]
    fn a_path_is_not_a_cache_key() {
        reset();
        plant();
        scope_to_document(None, "/shelf/book.pdf", 1);
        assert!(with(|i| i.is_empty()));
        plant();
        scope_to_document(None, "/shelf/book.pdf", 2);
        assert!(with(|i| i.is_empty()));
    }
}
