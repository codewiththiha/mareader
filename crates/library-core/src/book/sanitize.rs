//! Make a persisted list of rows internally valid, and idempotent about it.
//!
//! The blob is written by whatever build wrote it last and read by whatever build
//! reads it next, so a list that arrives may hold a member naming no book, two rows
//! sharing an id, or a row past the cap. Sanitizing is what makes the reader's own
//! guard enough: it runs once on load and again after every migration.

use super::{BOOKS_CAP, Origin, Row, drop_dangling_links, stem_of};

/// Make a persisted list internally valid: drop rows with no id, books with no
/// address and links with no name or no target, dedupe by id (first wins — the
/// reader's own order), clamp the resume point, drop the links whose book is
/// gone, and trim to [`BOOKS_CAP`] by least-recently-read. Idempotent.
///
/// By ID and not by content identity: two rows are allowed to share one
/// file's fingerprint when the reader asked to keep both — the library's
/// conflict sheet mints such duplicates under [`duplicate_title`] names — and
/// a sanitizer that deduped by fingerprint would silently undo a choice the
/// reader made. A blob carrying two rows with one ID is the corrupt case this
/// still heals. A link is not deduped against its book either: two links at
/// one book on two shelves are two pointers, and both work.
pub fn sanitize(rows: &mut Vec<Row>) {
    let mut seen = std::collections::HashSet::new();
    rows.retain(|r| {
        if r.id().trim().is_empty() || !seen.insert(r.id().to_string()) {
            return false;
        }
        match r {
            Row::Book(b) => !b.path().trim().is_empty(),
            // A link with no name is a row the shelf cannot label, and one
            // with no target is a row that cannot be clicked.
            Row::Link { name, target, .. } => {
                !name.trim().is_empty() && !target.trim().is_empty()
            }
        }
    });
    for row in rows.iter_mut() {
        let Some(b) = row.as_book_mut() else {
            continue;
        };
        b.page = b.page.max(1);
        b.fraction = b.fraction.filter(|f| (0.0..=1.0).contains(f));
        // A title that is really a filename — the download name a PDF carries
        // in its metadata — is not a title: drop it and let the stem of the
        // address show, which is the name the reader sees in their own file
        // manager. This is also the heal for rows stored before the rule
        // existed, on the load that first knows better. A name a duplicate
        // namer minted survives it, through the trailing-counter exemption in
        // `reader_core::filename` — and a name the READER chose survives it
        // outright (`title_locked`): the rule hunts debris a document
        // supplied, and what a person typed at the shelf's rename is not
        // debris whatever it looks like.
        if !b.title_locked && b.title.as_deref().is_some_and(|t| !reader_core::filename::is_usable_title(t)) {
            b.title = None;
        }
        // A stored book whose title IS its store address's stem is a burn-in
        // rather than a name: the open pipeline used to seed the shelf's record
        // with the stem of the address it opened, and for a stored book that
        // address is the store's own file — "source" for every copy this
        // layout stores, "<stem>_<id>" for one the flat store kept. Dropping it
        // lets the source's stem show again (`Book::title`'s fallback), which
        // is the name the reader imported. A locked title is the reader's and
        // stays: "source" is a name a person may genuinely choose.
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
    // Trim by least-recently-read rather than by position: the tail of the
    // list is the reader's own arrangement, and rearranging is not the same
    // promise as "I have not opened this in a year". Taken from the back so
    // the indices ahead of a removal stay the indices they were.
    let mut by_age: Vec<usize> = (0..rows.len()).collect();
    by_age.sort_by_key(|&i| recency(&rows[i]));
    let mut evict: Vec<usize> = by_age.into_iter().take(rows.len() - BOOKS_CAP).collect();
    evict.sort_unstable_by(|a, b| b.cmp(a));
    for i in evict {
        rows.remove(i);
    }
    // An evicted book can be the one a link was pointing at.
    drop_dangling_links(rows);
}

/// What the storage cap evicts by: when a book was last read, and when a link
/// was made. A link has no reading of its own, and neither has a book nobody
/// has opened, so both sit at the bottom of a list that has to lose rows —
/// which is the honest order for a cap that exists to keep a blob inside a
/// browser's quota.
fn recency(row: &Row) -> u64 {
    match row {
        Row::Book(b) => b.last_read_ms,
        Row::Link { added_ms, .. } => *added_ms,
    }
}
