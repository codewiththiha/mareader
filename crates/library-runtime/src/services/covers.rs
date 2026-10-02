//! The shelf's covers, rendered away from the shelf.
//!
//! A cover is page 1 of a book as a small JPEG. The library carries no engine
//! to render one, in either deployment: a hosted session ASKS the Shell
//! across the boundary (`ShellApi::bake_cover`, answered by the `coverBaked`
//! command — the Shell bakes in a pdf.js-only frame of its own), and the
//! unhosted session, which has no Shell, bakes nothing (its covers arrive
//! from the reader's open pipeline, which files one on every first open).
//! The queue is a request/response drain, one path in flight, with one
//! retry per path.

use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

use leptos::prelude::*;

use library_core::book::{Book, Row, book_rows};
use reader_core::format::Format;
use runtime_contract::boundary::ShellApi;
use runtime_contract::covers::{CoverImage, CoverMap};

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

thread_local! {
    static QUEUE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static DRAINING: RefCell<bool> = const { RefCell::new(false) };
    static DIRTY: RefCell<bool> = const { RefCell::new(false) };
    /// Requests whose answer (a `coverBaked` command) has not come back yet.
    /// Guards against a path being queued twice while its bake is in flight.
    static PENDING: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// One retry each: a cover can fail for a reason that is true for a second — a file still being copied, a worker still warming up — but a queue that re-attempts a genuinely unrenderable file forever never drains.
    static RETRIES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
}

/// Forget every in-flight bake and queued path: called when a session
/// starts. The ledger is module-level and a frame now outlives its sessions
/// (the Shell recycles frames), so a session that died mid-bake would
/// otherwise leave `DRAINING` set and its path `PENDING` — and the next
/// session's backfill would wait forever on an answer addressed to a shelf
/// that no longer exists.
pub fn reset_ledger() {
    QUEUE.with(|queue| queue.borrow_mut().clear());
    DRAINING.with(|draining| *draining.borrow_mut() = false);
    DIRTY.with(|dirty| *dirty.borrow_mut() = false);
    PENDING.with(|pending| pending.borrow_mut().clear());
    RETRIES.with(|retries| retries.borrow_mut().clear());
}

fn wanted(rows: &[Row], covers: &CoverMap) -> Vec<String> {
    book_rows(rows)
        .filter(|b| b.format == Format::Pdf)
        .map(|b| b.path().to_string())
        .filter(|path| !covers.contains_key(path))
        .collect()
}

pub fn backfill_missing(state: crate::context::LibraryContext) {
    // No Shell, no baker: an unhosted session has nobody to ask, and a queue
    // it started would only sit at its first path forever.
    if matches!(state.api, crate::context::ApiHandle::Standalone) {
        return;
    }
    RETRIES.with(|retries| retries.borrow_mut().clear());
    let wanted = state.library.books.with_untracked(|rows| {
        state
            .library
            .covers
            .with_untracked(|covers| wanted(rows, covers))
    });
    if wanted.is_empty() {
        return;
    }
    QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        for path in wanted {
            if !queue.contains(&path) {
                queue.push(path);
            }
        }
    });
    let start = DRAINING.with(|draining| {
        let mut draining = draining.borrow_mut();
        if *draining {
            false
        } else {
            *draining = true;
            true
        }
    });
    if start {
        drain(state);
    }
}

pub(crate) fn prune_now(state: crate::context::LibraryContext) {
    state.library.books.with_untracked(|rows| {
        state
            .library
            .covers
            .update(|covers| prune_covers(rows, covers));
    });
}

pub fn file_cover(
    state: crate::context::LibraryContext,
    path: String,
    data_url: String,
    width: f64,
    height: f64,
) {
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
    DIRTY.with(|dirty| *dirty.borrow_mut() = true);
}

/// Whether a cover was filed since the last save, clearing the flag.
fn take_dirty() -> bool {
    DIRTY.with(|dirty| std::mem::take(&mut *dirty.borrow_mut()))
}

fn drain(state: crate::context::LibraryContext) {
    let next = QUEUE.with(|queue| queue.borrow_mut().pop());
    let Some(path) = next else {
        DRAINING.with(|draining| *draining.borrow_mut() = false);
        // Pruned HERE rather than after every insert: a sixty-cover backfill was sixty full
        // recency sorts, and the queue running dry is exactly the moment the cap is worth
        // enforcing — the covers that will compete for it have all landed.
        prune_now(state);
        if take_dirty() {
            crate::services::persist_covers(state.library);
        }
        return;
    };
    let have = state
        .library
        .covers
        .with_untracked(|covers| covers.contains_key(&path));
    let in_flight = PENDING.with(|pending| pending.borrow().contains(&path));
    if have || in_flight {
        drain(state);
        return;
    }
    PENDING.with(|pending| {
        pending.borrow_mut().insert(path.clone());
    });
    // The request crosses the boundary, the answer comes back as
    // `coverBaked` into this session (a stale generation is dropped
    // Shell-side), and [`on_baked`] moves the queue on. The frame carries the
    // round trip over its port — the boundary asks, the answer lands, one
    // retry policy. Off the frame (the unhosted api, the host test lane)
    // the ask goes nowhere and the path stays PENDING: exactly a hosted
    // `bakeCover` whose answer never comes, which is what lets the host tests
    // exercise the queue/retry policy without a baker.
    state.api.bake_cover(&path);
}

/// One bake answer for `path`: files the art and clears the retry, or
/// requeues once on the first failure and drops the path on the second —
/// then moves the queue on. Called from the session command surface
/// (`coverBaked`): one body, one retry policy.
pub fn on_baked(
    state: crate::context::LibraryContext,
    path: String,
    image: Option<runtime_contract::covers::CoverImage>,
) {
    let requested = PENDING.with(|pending| pending.borrow_mut().remove(&path));
    if !requested {
        // An answer this session never asked for (the request belonged to
        // the session before it in the same frame). The art is still good,
        // so file it — but this session's drain did not wait on it, and
        // moving the queue from here would start a second drain beside it.
        if let Some(image) = image {
            file_cover(state, path, image.data_url, image.width, image.height);
            if !DRAINING.with(|draining| *draining.borrow()) && take_dirty() {
                prune_now(state);
                crate::services::persist_covers(state.library);
            }
        }
        return;
    }
    match image {
        Some(image) => {
            RETRIES.with(|retries| retries.borrow_mut().remove(&path));
            file_cover(state, path, image.data_url, image.width, image.height);
        }
        None => {
            let first_failure = RETRIES.with(|retries| retries.borrow_mut().insert(path.clone()));
            if first_failure {
                QUEUE.with(|queue| queue.borrow_mut().push(path.clone()));
            }
        }
    }
    drain(state);
}

#[cfg(test)]
mod answer_tests {
    use super::*;

    #[test]
    fn a_success_files_the_art_and_clears_the_retry() {
        let state = crate::context::LibraryContext::default();
        // A cover belongs to a shelf row: the drain's dry-queue prune throws
        // out any art whose book is gone, so the success path is only
        // observable over a library that holds the book.
        state.library.books.update(|rows| {
            rows.push(Row::Book(Book::new(
                "a".to_string(),
                library_core::testkit::fingerprint(),
                Format::Pdf,
                library_core::book::Origin::Linked {
                    src: "/a.pdf".to_string(),
                },
                0,
            )));
        });
        RETRIES.with(|retries| retries.borrow_mut().insert("/a.pdf".to_string()));
        PENDING.with(|pending| pending.borrow_mut().insert("/a.pdf".to_string()));
        QUEUE.with(|queue| queue.borrow_mut().clear());
        on_baked(
            state,
            "/a.pdf".to_string(),
            Some(CoverImage {
                data_url: "data:image/jpeg;base64,x".to_string(),
                width: 240.0,
                height: 320.0,
            }),
        );
        assert!(
            state
                .library
                .covers
                .with_untracked(|covers| covers.contains_key("/a.pdf"))
        );
        assert!(RETRIES.with(|retries| !retries.borrow().contains("/a.pdf")));
        assert!(PENDING.with(|pending| !pending.borrow().contains("/a.pdf")));
    }

    #[test]
    fn a_new_session_starts_with_an_empty_ledger() {
        QUEUE.with(|queue| queue.borrow_mut().push("/q.pdf".to_string()));
        DRAINING.with(|draining| *draining.borrow_mut() = true);
        PENDING.with(|pending| pending.borrow_mut().insert("/p.pdf".to_string()));
        reset_ledger();
        assert!(QUEUE.with(|queue| queue.borrow().is_empty()));
        assert!(!DRAINING.with(|draining| *draining.borrow()));
        assert!(PENDING.with(|pending| pending.borrow().is_empty()));
    }

    #[test]
    fn an_unrequested_answer_files_the_art_without_moving_the_queue() {
        let state = crate::context::LibraryContext::default();
        reset_ledger();
        QUEUE.with(|queue| queue.borrow_mut().push("/next.pdf".to_string()));
        DRAINING.with(|draining| *draining.borrow_mut() = true);
        on_baked(
            state,
            "/old.pdf".to_string(),
            Some(CoverImage {
                data_url: "data:image/jpeg;base64,x".to_string(),
                width: 240.0,
                height: 320.0,
            }),
        );
        assert!(
            state
                .library
                .covers
                .with_untracked(|covers| covers.contains_key("/old.pdf"))
        );
        // The running drain still owns the queue: nothing was popped.
        assert_eq!(QUEUE.with(|queue| queue.borrow().len()), 1);
        assert!(PENDING.with(|pending| pending.borrow().is_empty()));
        reset_ledger();
    }

    #[test]
    fn the_first_failure_requeues_once_the_second_does_not() {
        let state = crate::context::LibraryContext::default();
        for (path, pre_seeded) in [("/first.pdf", false), ("/second.pdf", true)] {
            QUEUE.with(|queue| queue.borrow_mut().clear());
            RETRIES.with(|retries| {
                retries.borrow_mut().clear();
                if pre_seeded {
                    retries.borrow_mut().insert(path.to_string());
                }
            });
            PENDING.with(|pending| pending.borrow_mut().insert(path.to_string()));
            on_baked(state, path.to_string(), None);
            // A requeued path does not sit in the queue for long: the drain
            // that closes on_baked pops it straight back into PENDING to
            // re-issue the bake. Pending is the observable form of "requeued";
            // a path that already spent its retry is in neither set.
            let outstanding = PENDING.with(|pending| pending.borrow().contains(&path.to_string()));
            assert_eq!(outstanding, !pre_seeded, "{path}");
            let queued_again = QUEUE.with(|queue| queue.borrow().contains(&path.to_string()));
            assert!(!queued_again, "{path}");
        }
    }
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
        assert_eq!(
            asked,
            vec!["/a/dune.pdf".to_string(), "/d/second.pdf".to_string()]
        );
        let mut with_link = books;
        with_link.push(Row::link(
            "l1".into(),
            "Dune".into(),
            "/a/dune.pdf".into(),
            5,
        ));
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
        assert!(
            wanted(&books, &covers).is_empty(),
            "an open already filed this one"
        );
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
