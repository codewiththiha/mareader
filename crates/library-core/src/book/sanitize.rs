//! Make a persisted list of rows internally valid, idempotently.

use super::{BOOKS_CAP, Origin, Row, drop_dangling_links, stem_of};

/// Drop invalid rows, dedupe by id, clamp the point, trim to the cap.
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
        // A filename-shaped title is not a title.
        if !b.title_locked
            && b.title
                .as_deref()
                .is_some_and(|t| !reader_core::filename::is_usable_title(t))
        {
            b.title = None;
        }
        // A store-address stem title is a burn-in; drop it.
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
    // Trim by least-recently-read, back to front.
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

/// Eviction key: last read, or a link's creation.
fn recency(row: &Row) -> u64 {
    match row {
        Row::Book(b) => b.last_read_ms,
        Row::Link { added_ms, .. } => *added_ms,
    }
}
