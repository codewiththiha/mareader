//! The "recent books" library: which documents the reader has opened, where they
//! left off in each, and the persisted cover art for the shelf.
//!
//! Kept OUT of `Settings` on purpose. Reading position changes on every page
//! turn (and every scroll-row boundary in continuous mode), while `Settings`
//! is the appearance/zoom blob that repaints and re-serialises on every write.
//! Coupling the two would make each page turn re-run the appearance paint
//! effect and re-serialise the whole settings JSON. The library therefore lives
//! in its own signal and its own localStorage key, and written on its own
//! schedule: reading progress — the hot path, a write per page turn — is saved
//! on a debounce by `crate::effects::reader::reading_progress`, while the
//! moments that are the last thing before a teardown — a document closing, a
//! book leaving the shelf — write immediately through
//! `crate::storage::persist_library`, because a debounced save there is a save
//! that may never land.

use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::sync::Arc;

use leptos::prelude::RwSignal;

/// Hard cap on remembered books: enough for a long reading list without
/// pinning unbounded cover art / paths.
pub const RECENT_CAP: usize = 20;

/// One remembered book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentBook {
    /// Filesystem path (or URL) the document was opened from — the identity.
    pub path: String,
    /// Display name captured at open time (trustworthy `/Title`, else file stem).
    #[serde(default)]
    pub title: Option<String>,
    /// 1-based page the reader last reached — the resume point.
    #[serde(default = "default_page")]
    pub page: u32,
    /// Total page count (for the "page X of Y" hint). 0 when unknown.
    #[serde(default)]
    pub num_pages: u32,
    /// Fractional position (0..=1) inside the continuous reading of a
    /// reflowable document — where the stream was between the very top and
    /// the very bottom of the text. Written only while the stream mode is
    /// the live one; `None` everywhere else (a page is the whole truth
    /// there, and the page field above is the resume point).
    #[serde(default)]
    pub fraction: Option<f64>,
}

fn default_page() -> u32 {
    1
}

/// Persisted cover art for one book: the first page rendered to a small JPEG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

/// Insert `book` at the front (most-recent first), dropping any prior entry
/// with the same path so a re-opened book moves to the front instead of
/// duplicating, then trim to `RECENT_CAP`. Returns the path of a book evicted
/// past the cap (so its cached cover can be dropped too), if any.
pub fn upsert(recent: &mut Vec<RecentBook>, book: RecentBook) -> Option<String> {
    recent.retain(|b| b.path != book.path);
    recent.insert(0, book);
    if recent.len() > RECENT_CAP {
        let evicted = recent.get(RECENT_CAP).map(|b| b.path.clone());
        recent.truncate(RECENT_CAP);
        evicted
    } else {
        None
    }
}

/// Remove a book by path. Returns true when it was present.
pub fn remove(recent: &mut Vec<RecentBook>, path: &str) -> bool {
    let before = recent.len();
    recent.retain(|b| b.path != path);
    recent.len() != before
}

/// The saved page for `path`, if it is in the list.
pub fn find_page(recent: &[RecentBook], path: &str) -> Option<u32> {
    recent.iter().find(|b| b.path == path).map(|b| b.page)
}

/// The saved fractional stream position (see [`RecentBook::fraction`]),
/// for reflowable documents opening back into the continuous mode.
pub fn find_fraction(recent: &[RecentBook], path: &str) -> Option<f64> {
    recent
        .iter()
        .find(|b| b.path == path)
        .and_then(|b| b.fraction)
        .filter(|f| (0.0..=1.0).contains(f))
}

/// Make a persisted list internally valid: drop empty paths, dedupe by path
/// (first wins), clamp pages to >= 1, and trim to the cap. Idempotent.
pub fn sanitize(recent: &mut Vec<RecentBook>) {
    let mut seen = std::collections::HashSet::new();
    recent.retain(|b| !b.path.trim().is_empty() && seen.insert(b.path.clone()));
    recent.truncate(RECENT_CAP);
    for b in recent.iter_mut() {
        b.page = b.page.max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(path: &str, page: u32, num: u32) -> RecentBook {
        RecentBook {
            path: path.to_string(),
            title: None,
            page,
            num_pages: num,
            fraction: None,
        }
    }

    #[test]
    fn upsert_moves_reopened_books_to_the_front() {
        let mut v = vec![book("a", 1, 10), book("b", 1, 20)];
        upsert(&mut v, book("b", 7, 20));
        assert_eq!(v[0].path, "b");
        assert_eq!(v[0].page, 7);
        assert_eq!(v.len(), 2, "re-opening must not duplicate");
    }

    #[test]
    fn upsert_trims_past_the_cap_and_reports_the_eviction() {
        let mut v: Vec<RecentBook> = Vec::new();
        // Each upsert inserts at the FRONT, so after this loop the list is
        // [p19, p18, …, p0] — p0 is the OLDEST, sitting at the tail.
        for i in 0..RECENT_CAP {
            upsert(&mut v, book(&format!("p{i}"), 1, 10));
        }
        // Push one more: the tail (p0) is evicted.
        let evicted = upsert(&mut v, book("new", 1, 10));
        assert_eq!(evicted.as_deref(), Some("p0"));
        assert_eq!(v.len(), RECENT_CAP);
        assert_eq!(v[0].path, "new");
    }

    #[test]
    fn remove_drops_only_the_target() {
        let mut v = vec![book("a", 1, 10), book("b", 1, 20)];
        assert!(remove(&mut v, "a"));
        assert!(!remove(&mut v, "a"));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].path, "b");
    }

    #[test]
    fn find_page_returns_nothing_for_unknown_paths() {
        let v = vec![book("a", 42, 100)];
        assert_eq!(find_page(&v, "a"), Some(42));
        assert_eq!(find_page(&v, "zzz"), None);
    }

    #[test]
    fn sanitize_dedupes_and_clamps() {
        let mut v = vec![
            book("a", 1, 10),
            book("a", 99, 10), // duplicate path
            RecentBook { path: "  ".into(), title: None, page: 3, num_pages: 0, fraction: None }, // empty path
            RecentBook { path: "c".into(), title: None, page: 0, num_pages: 0, fraction: None }, // page 0
        ];
        sanitize(&mut v);
        let paths: Vec<&str> = v.iter().map(|b| b.path.as_str()).collect();
        assert_eq!(paths, vec!["a", "c"]);
        assert_eq!(v[0].page, 1, "first duplicate wins");
        assert_eq!(v[1].page, 1, "page 0 clamps to 1");
    }
}

/// The library domain: the recent-books shelf and the cover-art cache.
///
/// Covers are grouped WITH the books list on purpose: the recent-book cap
/// (`RECENT_CAP`) is only a real memory cap if covers are evicted together
/// with their books, and owning both in one struct makes that invariant
/// visible at the type level.
#[derive(Clone, Copy, Default)]
pub struct LibraryState {
    /// Recent books, most-recent first.
    pub books: RwSignal<Vec<RecentBook>>,
    /// Cover art (page-1 JPEG data URLs) keyed by path.
    pub covers: RwSignal<CoverMap>,
}

/// The cover-art cache: page-1 JPEG data URLs keyed by document path.
///
/// Behind an `Arc` because a cover is a base64 data URL — tens of kilobytes
/// of `String` each, a shelf's worth of them megabytes in total — and the map
/// is read out of a signal on every shelf render and cloned whole before
/// every save. Sharing the images makes those reads pointer copies; only the
/// (small) map spine is ever duplicated.
pub type CoverMap = HashMap<String, Arc<CoverImage>>;
