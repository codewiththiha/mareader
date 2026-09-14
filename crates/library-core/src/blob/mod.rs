//! The persisted shape of the library, and the migration from the shape it
//! replaced.
//!
//! One key holds the whole thing — books, shelves, watched folders and the
//! view — because they are one invariant: a shelf member that names no book is
//! a hole in the grid, and a folder ledger that remembers a fingerprint no book
//! carries is a book that can never come back.

use serde::{Deserialize, Serialize};

use crate::book::{Row, book_rows};
use crate::folder::WatchedFolder;
use crate::shelf::{Shelf, ShelfKind};
use crate::view::LibraryView;

pub mod migrate;

/// The library's localStorage key. `v3` rather than a schema edit under `v2`:
/// the list changed from books to [`Row`]s, and a `v2` blob this build cannot
/// parse must not be overwritten by the default before
/// [`migrate::migrate_v2`] has had a look at it.
pub const LIBRARY_KEY: &str = "pdfreader.library.v3";

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryBlob {
    /// Every row, in the order the "All" shelf shows them. This list IS the All
    /// order — there is no separate shelf for it (see [`crate::shelf`]).
    #[serde(default)]
    pub books: Vec<Row>,
    #[serde(default)]
    pub shelves: Vec<Shelf>,
    #[serde(default)]
    pub folders: Vec<WatchedFolder>,
    #[serde(default)]
    pub view: LibraryView,
}

impl LibraryBlob {
    pub fn is_empty(&self) -> bool {
        self.books.is_empty()
    }

    /// True when some book still carries a placeholder fingerprint, i.e. the
    /// library has not been checked against the filesystem since it loaded. The
    /// frontend holds a folder rescan until this clears: scanning against
    /// unmeasured fingerprints would add a second copy of every migrated book.
    pub fn awaiting_check(&self) -> bool {
        book_rows(&self.books).any(|b| b.fp_pending)
    }
}

/// Make a persisted library internally valid: the per-list rules
/// ([`crate::book::sanitize`], [`crate::folder::sanitize`],
/// [`crate::view::sanitize`], [`crate::shelf::sanitize`]) plus the two that only
/// make sense across lists — a shelf member that names no book, and a folder
/// shelf whose folder is gone. Idempotent.
///
/// The shelves go LAST and the order is the point: the rules below can drop
/// shelves, and a member swept before that would be swept against a list that
/// still holds one.
pub fn sanitize(blob: &mut LibraryBlob) {
    crate::book::sanitize(&mut blob.books);
    crate::folder::sanitize(&mut blob.folders);
    crate::view::sanitize(&mut blob.view);

    // A blob could carry two rows with one id however valid its books look;
    // `book::sanitize` dedupes by id, so by here the shelves below resolve against
    // a list whose ids are unique — and whose fingerprints are NOT, because a
    // duplicate the reader kept is two honest rows of one file.
    let known: std::collections::HashSet<&str> = blob.books.iter().map(|r| r.id()).collect();
    for shelf in blob.shelves.iter_mut() {
        shelf.books.retain(|m| known.contains(m.as_str()));
    }

    // A folder shelf whose folder was removed has no rescan to refill it and no
    // watch dot to explain it, so it goes; a virtual shelf is the reader's own, and
    // so is one that left its tree with its books copied.
    let folders: std::collections::HashSet<&str> =
        blob.folders.iter().map(|f| f.id.as_str()).collect();
    blob.shelves.retain(|s| match &s.kind {
        ShelfKind::Virtual | ShelfKind::Departed => true,
        ShelfKind::Folder { folder_id, .. } => folders.contains(folder_id.as_str()),
    });

    crate::shelf::sanitize(&mut blob.shelves);

    // A link may point at a shelf as well as at a book, and the sweep above could
    // not ask which shelves survived: this one can, with both lists in hand.
    crate::book::drop_dead_shelf_links(&mut blob.books, &blob.shelves);
}

#[cfg(test)]
mod tests {
    use super::migrate::{BlobV2, RecentBook, migrate_v1, migrate_v2};
    use super::*;
    use crate::book::{Book, Fingerprint};
    use crate::folder::FolderOpts;
    use crate::tracking::TrackingTree;
    use std::collections::{BTreeMap, HashSet};

    /// The book a row holds. Every row a migration makes is a book.
    fn at(blob: &LibraryBlob, i: usize) -> &Book {
        blob.books[i].book().expect("a migrated row is a book")
    }

    fn at_mut(blob: &mut LibraryBlob, i: usize) -> &mut Book {
        blob.books[i].as_book_mut().expect("a migrated row is a book")
    }

    fn legacy(path: &str, page: u32, num: u32) -> RecentBook {
        RecentBook {
            path: path.to_string(),
            title: None,
            page,
            num_pages: num,
            fraction: None,
        }
    }

    fn shelf(id: &str, name: &str, kind: ShelfKind, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: name.to_string(),
            kind,
            books: books.iter().map(|b| b.to_string()).collect(),
            parent: None,
            manual_parent: false,
        }
    }

    #[test]
    fn an_empty_library_is_the_default_one() {
        let blob = LibraryBlob::default();
        assert!(blob.is_empty());
        assert!(blob.shelves.is_empty() && blob.folders.is_empty());
        assert_eq!(blob.view, LibraryView::default());
        assert!(!blob.awaiting_check(), "nothing to check is nothing pending");
    }

    #[test]
    fn a_migration_keeps_the_order_and_the_resume_point() {
        let v1 = vec![
            legacy("/books/dune.pdf", 42, 400),
            legacy("/books/notes.md", 1, 0),
        ];
        let blob = migrate_v1(v1, 1_000);
        assert_eq!(blob.books.len(), 2);
        assert_eq!(at(&blob, 0).path(), "/books/dune.pdf");
        assert_eq!(at(&blob, 0).page, 42);
        assert_eq!(at(&blob, 0).num_pages, 400);
        assert_eq!(at(&blob, 1).format, reader_core::format::Format::Markdown);
        assert!(book_rows(&blob.books).all(|b| !b.origin.is_stored()));
        assert!(blob.shelves.is_empty(), "All is the row list, not a shelf");
        assert_ne!(blob.books[0].id(), blob.books[1].id());
    }

    #[test]
    fn a_migrated_book_says_so_until_it_has_been_measured() {
        let mut blob = migrate_v1(
            vec![legacy("/books/dune.pdf", 1, 1), legacy("/books/notes.md", 1, 1)],
            5,
        );
        assert!(blob.awaiting_check());
        assert!(book_rows(&blob.books).all(|b| b.fp_pending));
        // The placeholder is derived from the address, so two migrated books never
        // collide on it: the ledger's fingerprint index is first-wins, and a migration
        // that collapsed every row onto one stamp would hide all of them but one.
        assert_ne!(
            Fingerprint::placeholder("/a.pdf"),
            Fingerprint::placeholder("/b.pdf")
        );
        crate::book::sanitize(&mut blob.books);
        assert_eq!(blob.books.len(), 2, "both rows survive the dedupe");
        at_mut(&mut blob, 0).fp = Fingerprint::of(1024, 99, b"%PDF-1.7");
        at_mut(&mut blob, 0).fp_pending = false;
        assert!(blob.awaiting_check(), "the second book is still a placeholder");
        at_mut(&mut blob, 1).fp_pending = false;
        assert!(!blob.awaiting_check());
    }

    #[test]
    fn a_migration_drops_rows_with_no_address_and_clamps_the_page() {
        let blob = migrate_v1(
            vec![legacy("", 1, 1), legacy("   ", 1, 1), legacy("/a.pdf", 0, 9)],
            1,
        );
        assert_eq!(blob.books.len(), 1);
        assert_eq!(at(&blob, 0).page, 1);
    }

    #[test]
    fn an_impossible_fraction_does_not_survive_the_migration() {
        let mut row = legacy("/notes.md", 1, 0);
        row.fraction = Some(1.4);
        let blob = migrate_v1(vec![row], 1);
        assert_eq!(at(&blob, 0).fraction, None);
    }

    #[test]
    fn a_v1_blob_parses_under_its_own_names() {
        let rows: Vec<RecentBook> = serde_json::from_str(
            r#"[{"path":"/a.pdf","title":"A","page":3,"numPages":9,"fraction":0.5}]"#,
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].num_pages, 9);
        assert_eq!(rows[0].fraction, Some(0.5));
        let older: Vec<RecentBook> = serde_json::from_str(r#"[{"path":"/a.pdf"}]"#).unwrap();
        assert_eq!(older[0].page, 1);
        assert_eq!(older[0].title, None);
    }

    #[test]
    fn the_whole_library_round_trips_through_one_key() {
        let blob = LibraryBlob {
            books: migrate_v1(vec![legacy("/books/dune.pdf", 4, 40)], 1).books,
            shelves: vec![shelf("s1", "Sci-fi", ShelfKind::Virtual, &[])],
            folders: vec![WatchedFolder {
                id: "f1".into(),
                root: "/books".into(),
                opts: FolderOpts::default(),
                placed: HashSet::new(),
                ignored: Vec::new(),
                shelf_map: BTreeMap::new(),
                last_seen: Vec::new(),
                scanned_ms: 0,
                tracking: TrackingTree::default(),
            }],
            view: LibraryView::default(),
        };
        let json = serde_json::to_string(&blob).unwrap();
        let back: LibraryBlob = serde_json::from_str(&json).unwrap();
        assert_eq!(back, blob);
    }

    #[test]
    fn a_v2_library_becomes_a_library_of_rows() {
        let v1 = migrate_v1(vec![legacy("/books/dune.pdf", 42, 400)], 1_000);
        let legacy_blob = BlobV2 {
            books: v1.books.iter().filter_map(|r| r.book().cloned()).collect(),
            shelves: vec![shelf("s1", "Sci-fi", ShelfKind::Virtual, &[])],
            folders: Vec::new(),
            view: LibraryView::default(),
        };
        let blob = migrate_v2(legacy_blob.clone());
        assert_eq!(blob.books.len(), 1);
        assert!(blob.books[0].book().is_some());
        assert_eq!(at(&blob, 0).page, 42);
        assert_eq!(blob.shelves, legacy_blob.shelves);
        let json = serde_json::to_string(&blob).unwrap();
        assert!(json.contains("\"kind\":\"book\""), "{json}");
        let back: LibraryBlob = serde_json::from_str(&json).unwrap();
        assert_eq!(back, blob);
    }

    #[test]
    fn a_link_travels_in_the_blob_and_goes_with_its_book() {
        let mut blob = migrate_v1(vec![legacy("/books/dune.pdf", 1, 1)], 1);
        let target = blob.books[0].id().to_string();
        blob.books.push(Row::link("l1".into(), "Dune".into(), target.clone(), 9));
        blob.shelves = vec![shelf("s1", "One", ShelfKind::Virtual, &["l1"])];
        let json = serde_json::to_string(&blob).unwrap();
        assert!(json.contains("\"kind\":\"link\""), "{json}");
        assert!(json.contains("\"target\":\""), "{json}");
        let mut back: LibraryBlob = serde_json::from_str(&json).unwrap();
        assert_eq!(back.books.len(), 2, "a link is a row the blob carries");
        assert_eq!(back.books[1].target(), Some(target.as_str()));

        // A link at nothing is the one failure mode a link has, so the load that
        // finds it drops the row and the shelf member that named it.
        back.books.remove(0);
        let mut orphan = back;
        sanitize(&mut orphan);
        assert!(orphan.books.is_empty());
        assert!(orphan.shelves[0].books.is_empty());
    }

    #[test]
    fn a_blob_without_the_newer_halves_still_loads() {
        let blob: LibraryBlob = serde_json::from_str(r#"{"books":[]}"#).unwrap();
        assert_eq!(blob, LibraryBlob::default());
        let blob: LibraryBlob = serde_json::from_str("{}").unwrap();
        assert!(blob.is_empty());
    }

    #[test]
    fn a_shelf_member_that_names_no_book_is_dropped() {
        let mut blob = LibraryBlob {
            books: migrate_v1(vec![legacy("/books/dune.pdf", 1, 1)], 1).books,
            shelves: Vec::new(),
            ..LibraryBlob::default()
        };
        let kept = blob.books[0].id().to_string();
        blob.shelves = vec![shelf(
            "s1",
            "Sci-fi",
            ShelfKind::Virtual,
            &[kept.as_str(), "gone"],
        )];
        sanitize(&mut blob);
        assert_eq!(blob.shelves[0].books, vec![kept]);
    }

    #[test]
    fn a_folder_shelf_goes_with_its_folder_but_a_virtual_one_stays() {
        let mut blob = LibraryBlob {
            shelves: vec![
                shelf(
                    "s1",
                    "Books",
                    ShelfKind::Folder {
                        folder_id: "gone".into(),
                        rel: None,
                    },
                    &[],
                ),
                shelf("s2", "Mine", ShelfKind::Virtual, &[]),
            ],
            ..LibraryBlob::default()
        };
        sanitize(&mut blob);
        let ids: Vec<&str> = blob.shelves.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["s2"]);
    }

    #[test]
    fn a_shelf_nested_in_a_stale_folder_shelf_survives_at_the_root() {
        let mut blob = LibraryBlob {
            shelves: vec![
                shelf(
                    "s1",
                    "Books",
                    ShelfKind::Folder {
                        folder_id: "gone".into(),
                        rel: None,
                    },
                    &[],
                ),
                Shelf {
                    parent: Some("s1".into()),
                    ..shelf("s2", "Mine", ShelfKind::Virtual, &[])
                },
            ],
            ..LibraryBlob::default()
        };
        sanitize(&mut blob);
        assert_eq!(blob.shelves.len(), 1);
        assert_eq!(blob.shelves[0].id, "s2");
        assert_eq!(
            blob.shelves[0].parent, None,
            "a parent that went with its folder is not a parent any more"
        );
    }

    #[test]
    fn two_rows_sharing_an_id_leave_one_book_and_one_member() {
        let mut blob = migrate_v1(vec![legacy("/a.pdf", 1, 1), legacy("/b.pdf", 1, 1)], 1);
        let shared = blob.books[0].id().to_string();
        at_mut(&mut blob, 1).id = shared.clone();
        blob.shelves = vec![shelf(
            "s1",
            "One",
            ShelfKind::Virtual,
            &[shared.as_str(), shared.as_str()],
        )];
        sanitize(&mut blob);
        assert_eq!(blob.books.len(), 1);
        assert_eq!(blob.shelves[0].books, vec![shared]);
    }

    #[test]
    fn sanitize_is_idempotent() {
        let mut blob = LibraryBlob {
            books: migrate_v1(vec![legacy("/a.pdf", 1, 1)], 1).books,
            shelves: vec![shelf("s1", "One", ShelfKind::Virtual, &["nope"])],
            ..LibraryBlob::default()
        };
        sanitize(&mut blob);
        let once = blob.clone();
        sanitize(&mut blob);
        assert_eq!(blob, once);
    }
}
