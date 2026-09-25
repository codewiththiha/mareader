//! Full-text search, Rust side: an in-process index over the document's
//! extracted text.
//!
//! The pdf.js worker can only extract text in the browser, so the engine hands
//! each page over via `bridge::extract_page_text` and everything after that —
//! lowercasing, occurrence matching, snippet building, result ordering —
//! happens here, on the wasm heap. The index is built LAZILY: the document's
//! first search pays the extraction, never the open flow — a build scales
//! with the book and lands on a wasm heap that only grows, so an eager build
//! would ratchet the footprint of every book nobody ever searched. A retained
//! index is ADOPTED when it was built for the same document: the open flow
//! scopes the index to the document's pdf.js content fingerprint
//! ([`scope_to_document`]), and a reopen of the same bytes skips the
//! extraction entirely. A full rebuild per open/close cycle churned the
//! worker and the wasm heap — and wasm memory only ever grows, so every
//! cycle ratcheted the footprint up. After the build, a query is a pure
//! in-Rust scan: no pdf.js round trip, no per-query extraction.
//!
//! Extraction is concurrent in bounded batches ([`SEARCH_PAGE_CONCURRENCY`]
//! pages in flight per turn), so the worker is never flooded and live renders
//! keep their share; between turns the builder falls back to Pending, so the
//! event loop (and the reader's renders) runs without a busy wait.

use std::cell::RefCell;

use futures::stream::{self, StreamExt};
use serde::Deserialize;

use pdf_core::search::{PageText, SearchIndex, SearchItem};
use reader_core::search::SearchResponse;

use super::{EngineError, require_pdf_reader, resolve};
use crate::bridge;

/// Pages extracted concurrently per turn while the index is built. Three is
/// enough to hide the per-page worker round trip without starving live
/// renders; deliberately a plain const, not a setting.
pub const SEARCH_PAGE_CONCURRENCY: usize = 3;

/// The identity of the document an index belongs to: its content fingerprint
/// (the path when the engine reports none) and the page count it covers.
#[derive(Clone, PartialEq, Eq)]
struct IndexKey {
    identity: String,
    num_pages: u32,
}

thread_local! {
    static INDEX: RefCell<SearchIndex> = RefCell::new(SearchIndex::new());
    /// The document the open flow scoped the index to, if any.
    static SCOPED: RefCell<Option<IndexKey>> = const { RefCell::new(None) };
    /// What INDEX holds: the key it was BUILT for and how many pages the
    /// build indexed. `None` until a build completes — a half-extracted
    /// index (an open abandoned mid-build) is never adopted.
    static BUILT: RefCell<Option<(IndexKey, u32)>> = const { RefCell::new(None) };
}

fn with<R>(f: impl FnOnce(&mut SearchIndex) -> R) -> R {
    INDEX.with(|i| f(&mut i.borrow_mut()))
}

/// Scope the retained index to the document being opened. The fingerprint is
/// the content identity pdf.js derived from these exact bytes — it survives a
/// rename and changes on an in-place edit, which neither a path nor a stat
/// can promise from inside the webview; the path is the fallback for engines
/// that report none. `num_pages` rides along so a key can never adopt an
/// index of a different length.
///
/// Called synchronously from the open flow BEFORE the route flips, so the
/// first search of the new book can never query the old book's text: a
/// retained index for any other document is dropped here and now. The
/// retained index deliberately SURVIVES a close — that is the whole point of
/// the cache — and one bounded index (the last book's text) is cheaper than
/// the wasm-heap ratchet a re-extraction pays on every reopen.
pub fn scope_to_document(fingerprint: Option<&str>, path: &str, num_pages: u32) {
    let identity = fingerprint.filter(|f| !f.is_empty()).unwrap_or(path);
    let scoped = (!identity.is_empty()).then(|| IndexKey {
        identity: identity.to_string(),
        num_pages,
    });
    SCOPED.with(|s| *s.borrow_mut() = scoped);
    if adopted_count(num_pages).is_none() {
        with(|i| i.clear());
        BUILT.with(|b| *b.borrow_mut() = None);
    }
}

/// The retained index's page count when it was built for exactly the scoped
/// document; `None` when anything disagrees or the index is empty.
fn adopted_count(num_pages: u32) -> Option<u32> {
    let scoped = SCOPED.with(|s| s.borrow().clone())?;
    if scoped.num_pages != num_pages {
        return None;
    }
    let (built, indexed) = BUILT.with(|b| b.borrow().clone())?;
    (built == scoped && indexed > 0 && with(|i| !i.is_empty())).then_some(indexed)
}

/// Remember what a finished build produced, so the next open of the same
/// document adopts it. A build whose page count no longer matches the scope
/// (a different document opened underneath the builder) records nothing.
fn record_build(num_pages: u32, indexed: u32) {
    let built = SCOPED
        .with(|s| s.borrow().clone())
        .filter(|key| key.num_pages == num_pages)
        .map(|key| (key, indexed));
    BUILT.with(|b| *b.borrow_mut() = built);
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
/// build the in-process index — unless the index retained from the last build
/// was made for this exact document, in which case the extraction is skipped
/// entirely. Returns the number of pages indexed; the caller usually ignores
/// it — the `{ok:true, count}` envelope shape is kept for the engine
/// contract. Unreadable pages are skipped, never fatal: a corrupted page must
/// not kill a search.
pub async fn build_search_index(num_pages: u32) -> Result<u32, EngineError> {
    require_pdf_reader()?;
    if let Some(indexed) = adopted_count(num_pages) {
        return Ok(indexed);
    }
    with(|i| i.clear());
    BUILT.with(|b| *b.borrow_mut() = None);
    if num_pages == 0 {
        record_build(num_pages, 0);
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
            with(|i| {
                i.add_page(PageText {
                    page: p.page,
                    items,
                })
            });
            indexed += 1;
        }
        cursor = end + 1;
    }
    record_build(num_pages, indexed);
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
        SCOPED.with(|s| *s.borrow_mut() = None);
        BUILT.with(|b| *b.borrow_mut() = None);
    }

    /// A finished build for one page of "moby", simulated without the engine:
    /// scope, extract, record — the three steps `build_search_index` runs.
    fn simulate_built(fingerprint: Option<&str>, path: &str, num_pages: u32) {
        scope_to_document(fingerprint, path, num_pages);
        with(|i| {
            i.add_page(PageText {
                page: 1,
                items: vec![item("moby")],
            })
        });
        record_build(num_pages, 1);
    }

    #[test]
    fn reopening_the_same_book_adopts_the_retained_index() {
        reset();
        simulate_built(Some("fp-a"), "/shelf/book.pdf", 1);
        scope_to_document(Some("fp-a"), "/shelf/book.pdf", 1);
        assert_eq!(adopted_count(1), Some(1));
        assert_eq!(with(|i| i.query("moby").total), 1);
    }

    #[test]
    fn a_different_book_drops_the_retained_index() {
        reset();
        simulate_built(Some("fp-a"), "/shelf/book.pdf", 1);
        scope_to_document(Some("fp-b"), "/shelf/other.pdf", 1);
        assert_eq!(adopted_count(1), None);
        assert!(with(|i| i.is_empty()));
    }

    #[test]
    fn without_a_fingerprint_the_path_is_the_identity() {
        reset();
        simulate_built(None, "/shelf/book.pdf", 1);
        scope_to_document(None, "/shelf/book.pdf", 1);
        assert_eq!(adopted_count(1), Some(1));
        // Same address, different length: the page count guards the key, so
        // an edited file at the old path never adopts the stale index.
        scope_to_document(None, "/shelf/book.pdf", 2);
        assert_eq!(adopted_count(2), None);
        assert!(with(|i| i.is_empty()));
    }
}
