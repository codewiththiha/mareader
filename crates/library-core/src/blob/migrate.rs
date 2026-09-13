//! The two shapes this library was persisted as before the one it is now, and
//! the migrations from each.
//!
//! Kept beside [`super::LibraryBlob`] rather than in the app, because a migration
//! is a rule about the library's shape and a rule is something a test can call:
//! `cargo test -p library-core` holds both of them to account, and a migration
//! only the browser could run is a migration nobody has ever seen fail.
//!
//! Both are one-way in effect and leave the key they read alone, so a reader who
//! downgrades still finds the library the build they downgraded to wrote. The
//! first save after a migrated load is what puts the new blob under its own key
//! (see `src/storage/mod.rs`).
//!
//! A `v1` row carries no measurement, so a migrated book gets a
//! [`Fingerprint::placeholder`] and the [`Book::fp_pending`] mark, and
//! [`super::LibraryBlob::awaiting_check`] holds every watched folder's rescan off
//! until the first path check replaces it — a real fingerprint compared against a
//! placeholder matches nothing, so a rescan that ran early would add a second copy
//! of every book already on the shelf.

use serde::{Deserialize, Serialize};

use crate::book::{Book, Fingerprint, Origin, Row};
use crate::folder::WatchedFolder;
use crate::shelf::Shelf;
use crate::view::LibraryView;

use super::LibraryBlob;

/// The key the previous schema lived under — one book per row and no links.
/// Read once, on a load that finds no `v3`, and left in place afterwards: a
/// downgrade should still see the library it wrote.
pub const V2_KEY: &str = "pdfreader.library.v2";

/// The key the schema before that one lived under. Read once, on a load that
/// finds neither `v3` nor `v2`.
pub const LEGACY_KEY: &str = "pdfreader.library.v1";

/// The `v2` library: the same shelves and folders, and one BOOK per row. Kept
/// beside [`LibraryBlob`] for the reason [`RecentBook`] is — a load that finds
/// no `v3` has to be able to read what the previous build wrote, and the shape
/// it wrote is a rule a test can hold.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobV2 {
    #[serde(default)]
    pub books: Vec<Book>,
    #[serde(default)]
    pub shelves: Vec<Shelf>,
    #[serde(default)]
    pub folders: Vec<WatchedFolder>,
    #[serde(default)]
    pub view: LibraryView,
}

/// Turn a `v2` library into this one: every book becomes a book ROW, and
/// nothing else moves. The order survives, the shelves keep their members —
/// the ids they name are the ids the rows carry — and a library that had no
/// links gains none, because a link is a thing a reader makes and no earlier
/// build could have made one.
pub fn migrate_v2(legacy: BlobV2) -> LibraryBlob {
    LibraryBlob {
        books: legacy.books.into_iter().map(Row::Book).collect(),
        shelves: legacy.shelves,
        folders: legacy.folders,
        view: legacy.view,
    }
}

/// The `v1` row: a path, a title and a resume point. Kept in this crate (not
/// in the app) because the migration is a rule about the library's shape, and
/// a rule is something a test can call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentBook {
    pub path: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default)]
    pub num_pages: u32,
    #[serde(default)]
    pub fraction: Option<f64>,
}

fn default_page() -> u32 {
    1
}

/// Turn a `v1` recent-books list into a library.
///
/// Every row becomes a [`Origin::Linked`] book — read in place was the only
/// mode the old build had, and a migration that quietly copied two gigabytes
/// of PDFs into an app store would be the worst possible surprise. The order
/// survives, so the shelf the reader had is the shelf they get.
///
/// A `v1` row carries no measurement, so its fingerprint is a placeholder
/// derived from the address ([`Fingerprint::placeholder`]) and the book is
/// marked [`Book::fp_pending`]. The first path check replaces it with the real
/// one; until then [`LibraryBlob::awaiting_check`] holds a rescan off, because
/// a scan comparing real fingerprints against placeholders would add a second
/// copy of every book already on the shelf.
pub fn migrate_v1(legacy: Vec<RecentBook>, now_ms: u64) -> LibraryBlob {
    let books: Vec<Row> = legacy
        .into_iter()
        .filter(|b| !b.path.trim().is_empty())
        .enumerate()
        .map(|(i, b)| {
            let id = crate::id::new_id(now_ms, i as u32);
            Book {
                fp: Fingerprint::placeholder(&b.path),
                format: reader_core::format::format_of(&b.path),
                origin: Origin::Linked { src: b.path },
                title: crate::text::non_blank(b.title.as_deref()).map(str::to_string),
                author: None,
                id,
                // A migrated book was opened, so it has been read — but the old
                // schema kept no stamp, and `now_ms` would put every book at
                // the top of a "Last read" sort. Zero sorts them together,
                // below anything read since, which is the honest answer.
                added_ms: 0,
                last_read_ms: 0,
                page: b.page.max(1),
                num_pages: b.num_pages,
                fraction: b.fraction.filter(|f| (0.0..=1.0).contains(f)),
                missing: false,
                fp_pending: true,
                independent: false,
                title_locked: false,
            }
        })
        .map(Row::Book)
        .collect();
    LibraryBlob {
        books,
        shelves: Vec::new(),
        folders: Vec::new(),
        view: LibraryView::default(),
    }
}

