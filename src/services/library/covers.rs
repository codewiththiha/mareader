//! The shelf's covers, rendered away from the reader.
//!
//! A cover is page 1 of a book as a small JPEG, and the engine can render one from a
//! path with nothing open — which is what makes a cover at IMPORT time possible at
//! all.

use std::collections::HashSet;
use std::sync::Arc;

use leptos::prelude::*;

#[cfg(test)]
use reader_core::format::Format;

use crate::state::library::{CoverImage, CoverMap};
use crate::state::AppState;
use library_core::book::{Book, Row, book_rows};

/// Maximum number of persisted cover images.
pub const COVER_CAP: usize = 60;

pub fn prune_covers(rows: &[Row], covers: &mut CoverMap) {
    let books: Vec<&Book> = book_rows(rows).collect();
    let live: HashSet<&str> = books.iter().map(|b| b.path()).collect();
    covers.retain(|path, _| live.contains(path.as_str()));
    if covers.len() <= COVER_CAP {
        return;
    }
    let mut by_recency: Vec<(String, u64)> = books
        .iter()
        .map(|b| (b.path().to_string(), b.last_read_ms.max(b.added_ms)))
        .collect();
    by_recency.sort_by_key(|(_, stamp)| std::cmp::Reverse(*stamp));
    let keep: HashSet<&str> = by_recency
        .iter()
        .take(COVER_CAP)
        .map(|(path, _)| path.as_str())
        .collect();
    covers.retain(|path, _| keep.contains(path.as_str()));
}

#[cfg(test)]
fn wanted(rows: &[Row], covers: &CoverMap) -> Vec<String> {
    book_rows(rows)
        .filter(|b| b.format == Format::Pdf)
        .map(|b| b.path().to_string())
        .filter(|path| !covers.contains_key(path))
        .collect()
}

/// Missing covers use the format glyph until first open. Rendering a cover
/// must not instantiate a PDF document in the persistent library realm.
pub fn backfill_missing(state: AppState) { prune_now(state); }

pub(crate) fn prune_now(state: AppState) {
    state.library.books.with_untracked(|rows| {
        state
            .library
            .covers
            .update(|covers| prune_covers(rows, covers));
    });
}

pub fn file_cover(state: AppState, path: String, data_url: String, width: f64, height: f64) {
    state.library.covers.update(|covers| {
        covers.insert(
            path,
            Arc::new(CoverImage {
                data_url,
                width,
                height,
            }),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::book::{Book, Fingerprint, Origin};

    fn book(path: &str, format: Format) -> Row {
        Row::Book(book_value(path, format))
    }

    fn book_value(path: &str, format: Format) -> Book {
        let len = path.len() as u64;
        Book {
            fp: Fingerprint {
                size: len,
                mtime_ms: 0,
                head_hash: len as u32,
            },
            format,
            origin: Origin::Linked {
                src: path.to_string(),
            },
            ..library_core::testkit::book(path)
        }
    }

    fn cover() -> Arc<CoverImage> {
        Arc::new(CoverImage {
            data_url: "data:image/jpeg;base64,x".to_string(),
            width: 240.0,
            height: 320.0,
        })
    }

    #[test]
    fn only_pdfs_without_a_cover_are_worth_a_render() {
        let books = vec![
            book("/a/dune.pdf", Format::Pdf),
            book("/b/notes.md", Format::Markdown),
            book("/c/log.txt", Format::Text),
            book("/d/second.pdf", Format::Pdf),
        ];
        let asked = wanted(&books, &CoverMap::default());
        assert_eq!(asked, vec!["/a/dune.pdf".to_string(), "/d/second.pdf".to_string()]);
        let mut with_link = books;
        with_link.push(Row::link("l1".into(), "Dune".into(), "/a/dune.pdf".into(), 5));
        assert_eq!(
            wanted(&with_link, &CoverMap::default()),
            vec!["/a/dune.pdf".to_string(), "/d/second.pdf".to_string()]
        );
    }

    #[test]
    fn a_cover_the_shelf_already_has_is_not_rendered_twice() {
        let books = vec![book("/a/dune.pdf", Format::Pdf)];
        let mut covers = CoverMap::default();
        covers.insert("/a/dune.pdf".to_string(), cover());
        assert!(wanted(&books, &covers).is_empty(), "an open already filed this one");
    }

    fn book_read(path: &str, last_read: u64) -> Row {
        let len = path.len() as u64;
        Row::Book(Book {
            fp: Fingerprint {
                size: len,
                mtime_ms: last_read,
                head_hash: len as u32,
            },
            origin: Origin::Linked {
                src: path.to_string(),
            },
            added_ms: last_read,
            last_read_ms: last_read,
            ..library_core::testkit::book(path)
        })
    }

    #[test]
    fn a_cover_outlives_nothing_it_does_not_belong_to() {
        let books = vec![
            book_read("/a.pdf", 1),
            book_read("/b.pdf", 2),
            Row::link("l1".into(), "A".into(), "/a.pdf".into(), 3),
        ];
        let mut covers: CoverMap = [
            ("/a.pdf".to_string(), cover()),
            ("/b.pdf".to_string(), cover()),
            ("/gone.pdf".to_string(), cover()),
        ]
        .into_iter()
        .collect();
        prune_covers(&books, &mut covers);
        assert_eq!(covers.len(), 2, "a link keeps no art alive and holds none");
        assert!(!covers.contains_key("/gone.pdf"));
    }

    #[test]
    fn the_cap_keeps_the_most_recently_read() {
        let books: Vec<Row> = (0..(COVER_CAP + 5))
            .map(|i| book_read(&format!("/books/{i}.pdf"), i as u64))
            .collect();
        let mut covers: CoverMap = books
            .iter()
            .filter_map(Row::book)
            .map(|b| (b.path().to_string(), cover()))
            .collect();
        prune_covers(&books, &mut covers);
        assert_eq!(covers.len(), COVER_CAP);
        assert!(!covers.contains_key("/books/0.pdf"));
        assert!(!covers.contains_key("/books/4.pdf"));
        assert!(covers.contains_key("/books/5.pdf"));
    }
}
