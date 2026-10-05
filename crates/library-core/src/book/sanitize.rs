//! Make a persisted list of rows internally valid, idempotently.
//!
//! The blob is written by whatever build ran last and read by whatever build
//! runs next, so an arriving list may hold a link naming no book, two rows
//! sharing an id, or more rows than the cap allows.

use super::{BOOKS_CAP, Origin, Row, drop_dangling_links, stem_of};

/// Drop rows with no id, books with no address and links with no name or
/// target; dedupe by id (first wins — the reader's own order); clamp the resume
/// point; drop links whose book is gone; trim to [`BOOKS_CAP`].
///
/// Dedupe is by id, not content identity: two rows may share one file's
/// fingerprint when the reader asked to keep both.
pub fn sanitize(rows: &mut Vec<Row>) {
    let mut seen = std::collections::HashSet::new();
    rows.retain(|r| {
        if r.id().trim().is_empty() || !seen.insert(r.id().to_string()) {
            return false;
        }
        match r {
            Row::Book(b) => !b.path().trim().is_empty(),
            // A link with no name cannot be labeled; one with no target
            // cannot be clicked.
            Row::Link { name, target, .. } => !name.trim().is_empty() && !target.trim().is_empty(),
        }
    });
    for row in rows.iter_mut() {
        let Some(b) = row.as_book_mut() else {
            continue;
        };
        b.page = b.page.max(1);
        b.fraction = b.fraction.filter(|f| (0.0..=1.0).contains(f));
        // A title that is really a filename (the download name a PDF carries
        // in its metadata) is not a title: drop it and let the address stem
        // show. This also heals rows stored before the rule existed; names the
        // duplicate namer minted survive via the filename policy's
        // trailing-counter exemption.
        if !b.title_locked
            && b.title
                .as_deref()
                .is_some_and(|t| !reader_core::filename::is_usable_title(t))
        {
            b.title = None;
        }
        // A stored book whose title is its store address's stem is a burn-in
        // from the seeding the open pipeline used to do; dropping it lets the
        // source's stem show through `Book::title`'s fallback.
        if !b.title_locked
            && let Origin::Stored { store, .. } = &b.origin
            && b.title.as_deref() == Some(stem_of(store).as_str())
        {
            b.title = None;
        }
    }
    drop_dangling_links(rows);
    if rows.len() <= BOOKS_CAP {
        return;
    }
    // Trim by least-recently-read, not by position: the list order is the
    // reader's own arrangement. Removed back-to-front so earlier indices stay
    // valid.
    let mut by_age: Vec<usize> = (0..rows.len()).collect();
    by_age.sort_by_key(|&i| recency(&rows[i]));
    let mut evict: Vec<usize> = by_age.into_iter().take(rows.len() - BOOKS_CAP).collect();
    evict.sort_unstable_by(|a, b| b.cmp(a));
    for i in evict {
        rows.remove(i);
    }
    // An evicted book can be the one a link pointed at.
    drop_dangling_links(rows);
}

/// Eviction key: a book's last read, a link's creation. Neither a link nor an
/// unopened book has a reading of its own, so both sit at the bottom of a list
/// that has to lose rows.
fn recency(row: &Row) -> u64 {
    match row {
        Row::Book(b) => b.last_read_ms,
        Row::Link { added_ms, .. } => *added_ms,
    }
}
