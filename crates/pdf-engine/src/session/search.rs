//! Full-text search: each session's index over its document's extracted text.
use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};

use futures::stream::{self, StreamExt};
use serde::Deserialize;

use pdf_core::search::{PageText, SearchIndex, SearchItem};
use reader_core::search::SearchResponse;

use super::{PdfSession, no_session};
use crate::api::{self, EngineError};

/// Pages extracted concurrently per turn while the index is built.
pub const SEARCH_PAGE_CONCURRENCY: usize = 3;

/// The identity of the document an index belongs to.
#[derive(Clone, PartialEq, Eq, Debug)]
struct IndexKey {
    identity: String,
    num_pages: u32,
}

/// One session's search state.
#[derive(Default)]
pub(crate) struct SearchState {
    index: SearchIndex,
    /// The document the session's open scoped the index to.
    scoped: Option<IndexKey>,
    /// What `index` holds: the key it was built for, and the page count.
    built: Option<(IndexKey, u32)>,
}

thread_local! {
    /// The last disposed session's finished index (see the module docs).
    static RETAINED: RefCell<Option<SearchState>> = const { RefCell::new(None) };
}

impl SearchState {
    /// Scope the index to the document being opened.
    pub(crate) fn scope(&mut self, fingerprint: Option<&str>, path: &str, num_pages: u32) {
        let identity = fingerprint.filter(|f| !f.is_empty()).unwrap_or(path);
        let scoped = (!identity.is_empty()).then(|| IndexKey {
            identity: identity.to_string(),
            num_pages,
        });
        self.scoped = scoped.clone();
        if self.adopted_count(num_pages).is_some() {
            return;
        }
        // A retained index for this exact document is adopted, others dropped.
        let retained = RETAINED.with(|r| r.borrow_mut().take());
        match retained {
            Some(r) if scoped.is_some() && r.built.as_ref().map(|(k, _)| k) == scoped.as_ref() => {
                self.index = r.index;
                self.built = r.built;
            }
            _ => {
                self.index.clear();
                self.built = None;
            }
        }
    }

    /// The index's page count when built for this scoped document.
    fn adopted_count(&self, num_pages: u32) -> Option<u32> {
        let scoped = self.scoped.as_ref()?;
        if scoped.num_pages != num_pages {
            return None;
        }
        let (built, indexed) = self.built.as_ref()?;
        (built == scoped && *indexed > 0 && !self.index.is_empty()).then_some(*indexed)
    }

    /// Remember what a finished build produced.
    fn record_build(&mut self, num_pages: u32, indexed: u32) {
        self.built = self
            .scoped
            .clone()
            .filter(|key| key.num_pages == num_pages)
            .map(|key| (key, indexed));
    }

    pub(crate) fn query(&self, query: &str) -> SearchResponse {
        self.index.query(query)
    }
}

/// A disposed session's index, kept only when worth adopting.
pub(crate) fn retain(state: SearchState) {
    if state.built.as_ref().is_some_and(|(_, n)| *n > 0) && !state.index.is_empty() {
        RETAINED.with(|r| *r.borrow_mut() = Some(state));
    }
}

/// Drop the realm's retained index: a text document opened.
pub fn drop_retained_search() {
    RETAINED.with(|r| *r.borrow_mut() = None);
}

/// `{ok:true, page, items}`: engine.extractPageText.
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

/// In-flight search index builds across every session.
static BUILD_ACTIVE: AtomicU32 = AtomicU32::new(0);

pub(crate) fn search_build_active() -> u32 {
    BUILD_ACTIVE.load(Ordering::Relaxed)
}

struct BuildActiveGuard;

impl Drop for BuildActiveGuard {
    fn drop(&mut self) {
        BUILD_ACTIVE.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Extract every page of the session's document into ITS index.
pub(crate) async fn build(session: &PdfSession, num_pages: u32) -> Result<u32, EngineError> {
    if !session.is_live() {
        return Err(no_session());
    }
    api::require_pdf_reader()?;
    // The build can be mid-flight when a pane closes, so it is gauged.
    BUILD_ACTIVE.fetch_add(1, Ordering::Relaxed);
    let _build_guard = BuildActiveGuard;
    if let Some(indexed) = session.with_search(|s| s.adopted_count(num_pages)) {
        return Ok(indexed);
    }
    session.with_search(|s| {
        s.index.clear();
        s.built = None;
    });
    if num_pages == 0 {
        session.with_search(|s| s.record_build(num_pages, 0));
        return Ok(0);
    }

    // One TURN = [`SEARCH_PAGE_CONCURRENCY`] pages extracted concurrently,
    // then the builder yields to the event loop.
    let mut indexed = 0u32;
    let mut cursor = 1u32;
    while cursor <= num_pages {
        let end = (cursor + SEARCH_PAGE_CONCURRENCY as u32 - 1).min(num_pages);
        let batch: Vec<u32> = (cursor..=end).collect();
        let extracted: Vec<Option<PageTextPayload>> = stream::iter(batch)
            .map(|page| async move {
                let value = session.extract_page_text(page).await?;
                api::resolve::<PageTextPayload>(value, "extractPageText").ok()
            })
            .buffer_unordered(SEARCH_PAGE_CONCURRENCY)
            .collect()
            .await;
        if !session.is_live() {
            return Err(no_session());
        }
        for p in extracted.into_iter().flatten() {
            let items = p
                .items
                .into_iter()
                .map(|it| SearchItem::new(it.text, it.x, it.y, it.w, it.h))
                .collect();
            session.with_search(|s| {
                s.index.add_page(PageText {
                    page: p.page,
                    items,
                })
            });
            indexed += 1;
        }
        cursor = end + 1;
    }
    session.with_search(|s| s.record_build(num_pages, indexed));
    Ok(indexed)
}

// The scope and adopt rules are pure host logic.
#[cfg(test)]
mod tests {
    use super::*;

    fn item(word: &str) -> SearchItem {
        SearchItem::new(word, 0.0, 0.0, 1.0, 1.0)
    }

    /// A finished build for one page, simulated without the engine.
    fn built(fingerprint: Option<&str>, path: &str, num_pages: u32) -> SearchState {
        let mut s = SearchState::default();
        s.scope(fingerprint, path, num_pages);
        s.index.add_page(PageText {
            page: 1,
            items: vec![item("moby")],
        });
        s.record_build(num_pages, 1);
        s
    }

    #[test]
    fn reopening_the_same_book_adopts_the_retained_index() {
        drop_retained_search();
        retain(built(Some("fp-a"), "/shelf/book.pdf", 1));
        let mut next = SearchState::default();
        next.scope(Some("fp-a"), "/shelf/book.pdf", 1);
        assert_eq!(next.adopted_count(1), Some(1));
        assert_eq!(next.query("moby").total, 1);
        // Adopted means MOVED: the slot is empty again.
        assert!(RETAINED.with(|r| r.borrow().is_none()));
    }

    #[test]
    fn a_different_book_drops_the_retained_index() {
        drop_retained_search();
        retain(built(Some("fp-a"), "/shelf/book.pdf", 1));
        let mut next = SearchState::default();
        next.scope(Some("fp-b"), "/shelf/other.pdf", 1);
        assert_eq!(next.adopted_count(1), None);
        assert!(next.index.is_empty());
        assert!(RETAINED.with(|r| r.borrow().is_none()));
    }

    #[test]
    fn without_a_fingerprint_the_path_is_the_identity() {
        drop_retained_search();
        retain(built(None, "/shelf/book.pdf", 1));
        let mut next = SearchState::default();
        next.scope(None, "/shelf/book.pdf", 1);
        assert_eq!(next.adopted_count(1), Some(1));
        // Same address, different length: the page count guards the key.
        next.scope(None, "/shelf/book.pdf", 2);
        assert_eq!(next.adopted_count(2), None);
    }

    #[test]
    fn an_unfinished_build_is_never_retained() {
        drop_retained_search();
        let mut half = SearchState::default();
        half.scope(Some("fp-a"), "/shelf/book.pdf", 3);
        half.index.add_page(PageText {
            page: 1,
            items: vec![item("moby")],
        });
        retain(half);
        assert!(RETAINED.with(|r| r.borrow().is_none()));
    }

    #[test]
    fn two_sessions_hold_two_indexes() {
        drop_retained_search();
        let a = built(Some("fp-a"), "/a.pdf", 1);
        let mut b = SearchState::default();
        b.scope(Some("fp-b"), "/b.pdf", 1);
        b.index.add_page(PageText {
            page: 1,
            items: vec![item("ahab")],
        });
        b.record_build(1, 1);
        assert_eq!(a.query("moby").total, 1);
        assert_eq!(a.query("ahab").total, 0);
        assert_eq!(b.query("ahab").total, 1);
        assert_eq!(b.query("moby").total, 0);
    }
}
