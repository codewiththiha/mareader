//! What a shelf actually holds: whether its books are on disk, stored copies,
//! or a mix of both. The governance badge (FolderMode) says where a shelf
//! *should* read from; this says what it *does* hold, which is what fixes the
//! duplicate-inside-read-at-place bug — a duplicated folder's books are stored
//! even though its parent seat still says "On disk".

use crate::book::{find_row, Book, Row};

use super::Shelf;

/// The composition of a shelf's direct members. Empty means no book rows to
/// count (only shelf links, or nothing at all); the caller falls back to the
/// governance badge in that case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    Empty,
    OnDisk,
    Stored,
    Mixed,
}

impl ContentKind {
    /// Short badge text the plate shows. Empty has none.
    pub fn badge(self) -> Option<&'static str> {
        match self {
            ContentKind::Empty => None,
            ContentKind::OnDisk => Some("On disk"),
            ContentKind::Stored => Some("Copied"),
            ContentKind::Mixed => Some("Mixed"),
        }
    }

    /// Tooltip the badge shows on hover. One sentence per kind, matching the
    /// existing folder-mode tooltips.
    pub fn tooltip(self) -> Option<&'static str> {
        match self {
            ContentKind::Empty => None,
            ContentKind::OnDisk => Some(
                "These books stay in their folder on disk; the library only remembers where they are.",
            ),
            ContentKind::Stored => Some(
                "Every book here is a copy the library keeps — the folder on disk can go.",
            ),
            ContentKind::Mixed => Some(
                "Some books are on disk and some are copies the library keeps.",
            ),
        }
    }

    /// Whether this kind is mixed.
    pub fn is_mixed(self) -> bool {
        matches!(self, ContentKind::Mixed)
    }
}

/// The book a member id names: a book row directly, a book link through its
/// target, and nothing for a shelf link — a pointer at a level is not content.
fn member_book<'a>(rows: &'a [Row], member_id: &str) -> Option<&'a Book> {
    match find_row(rows, member_id)? {
        Row::Book(b) => Some(b),
        Row::Link { target, .. } => {
            if crate::id::is_shelf(target) {
                return None;
            }
            match find_row(rows, target) {
                Some(Row::Book(b)) => Some(b),
                _ => None,
            }
        }
    }
}

/// Count a shelf's direct book members and report whether they are on disk,
/// stored, mixed, or absent.
///
/// Shelf links (Row::Link at a shelf id) are ignored — they are pointers, not
/// books. A book link (Row::Link at a book id) is resolved to its target book
/// so a shelf that holds a link at a stored book counts as stored.
fn content_kind(rows: &[Row], shelf: &Shelf) -> ContentKind {
    content_kind_for_ids(rows, &shelf.books)
}

/// Same as [`content_kind`] but over an explicit list of member ids; the
/// classification both walks share is [`member_book`].
fn content_kind_for_ids(rows: &[Row], member_ids: &[String]) -> ContentKind {
    let mut on_disk = 0usize;
    let mut stored = 0usize;

    for member_id in member_ids {
        let Some(book) = member_book(rows, member_id) else {
            continue;
        };
        if book.origin.is_stored() {
            stored += 1;
        } else {
            on_disk += 1;
        }
    }

    match (on_disk, stored) {
        (0, 0) => ContentKind::Empty,
        (_, 0) => ContentKind::OnDisk,
        (0, _) => ContentKind::Stored,
        _ => ContentKind::Mixed,
    }
}

/// Count a shelf and all shelves below it. Used for the badge when a folder
/// has no direct books but its children do — e.g. a duplicated folder tree
/// whose root is empty and whose leaves hold stored copies. If any level in
/// the subtree is mixed, the whole tree is mixed.
fn content_kind_recursive(rows: &[Row], shelves: &[Shelf], root_id: &str) -> ContentKind {
    let mut ids = vec![root_id.to_string()];
    let mut seen = std::collections::HashSet::new();
    seen.insert(root_id.to_string());
    let mut cursor = 0usize;
    while cursor < ids.len() {
        let current = ids[cursor].clone();
        cursor += 1;
        for child in crate::shelf::children_of(shelves, Some(current.as_str())) {
            if seen.insert(child.id.clone()) {
                ids.push(child.id.clone());
            }
        }
    }

    let mut on_disk = 0usize;
    let mut stored = 0usize;

    for shelf_id in ids {
        let Some(shelf) = crate::shelf::find(shelves, &shelf_id) else {
            continue;
        };
        for member_id in &shelf.books {
            let Some(book) = member_book(rows, member_id) else {
                continue;
            };
            if book.origin.is_stored() {
                stored += 1;
            } else {
                on_disk += 1;
            }
            if on_disk > 0 && stored > 0 {
                return ContentKind::Mixed;
            }
        }
    }

    match (on_disk, stored) {
        (0, 0) => ContentKind::Empty,
        (_, 0) => ContentKind::OnDisk,
        (0, _) => ContentKind::Stored,
        _ => ContentKind::Mixed,
    }
}

/// The badge rule every folder surface reads: what the shelf ITSELF holds
/// decides the badge, and a shelf holding no books falls back to what its
/// subtree holds, so a folder whose leaves carry the copies still says so at
/// its root. A missing shelf is an empty one.
pub fn badge_kind(rows: &[Row], shelves: &[Shelf], shelf_id: &str) -> ContentKind {
    let direct = crate::shelf::find(shelves, shelf_id)
        .map(|shelf| content_kind(rows, shelf))
        .unwrap_or(ContentKind::Empty);
    if direct != ContentKind::Empty {
        direct
    } else {
        content_kind_recursive(rows, shelves, shelf_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Book, Fingerprint, Origin};
    use crate::testkit;

    fn linked(id: &str, path: &str) -> Row {
        Row::Book(Book {
            id: id.to_string(),
            fp: Fingerprint { size: 1, mtime_ms: 1, head_hash: 1 },
            title: None,
            author: None,
            format: reader_core::format::Format::Pdf,
            origin: Origin::Linked { src: path.to_string() },
            added_ms: 1,
            last_read_ms: 1,
            page: 1,
            num_pages: 0,
            fraction: None,
            missing: false,
            fp_pending: false,
            independent: false,
            title_locked: false,
        })
    }

    fn stored(id: &str, src: &str) -> Row {
        Row::Book(Book {
            id: id.to_string(),
            fp: Fingerprint { size: 1, mtime_ms: 1, head_hash: 1 },
            title: None,
            author: None,
            format: reader_core::format::Format::Pdf,
            origin: Origin::Stored {
                src: Some(src.to_string()),
                store: format!("/store/{id}.pdf"),
            },
            added_ms: 1,
            last_read_ms: 1,
            page: 1,
            num_pages: 0,
            fraction: None,
            missing: false,
            fp_pending: false,
            independent: false,
            title_locked: false,
        })
    }

    fn shelf(id: &str, members: &[&str]) -> Shelf {
        testkit::shelf(id, id, members, None)
    }

    #[test]
    fn empty_shelf_has_no_content() {
        let rows: Vec<Row> = vec![];
        let s = shelf("s1", &[]);
        assert_eq!(content_kind(&rows, &s), ContentKind::Empty);
        assert_eq!(ContentKind::Empty.badge(), None);
    }

    #[test]
    fn only_linked_books_are_on_disk() {
        let rows = vec![linked("b1", "/a.pdf"), linked("b2", "/b.pdf")];
        let s = shelf("s1", &["b1", "b2"]);
        assert_eq!(content_kind(&rows, &s), ContentKind::OnDisk);
        assert_eq!(ContentKind::OnDisk.badge(), Some("On disk"));
    }

    #[test]
    fn only_stored_books_are_copied() {
        let rows = vec![stored("b1", "/a.pdf"), stored("b2", "/b.pdf")];
        let s = shelf("s1", &["b1", "b2"]);
        assert_eq!(content_kind(&rows, &s), ContentKind::Stored);
        assert_eq!(ContentKind::Stored.badge(), Some("Copied"));
    }

    #[test]
    fn mixed_books_report_mixed() {
        let rows = vec![linked("b1", "/a.pdf"), stored("b2", "/b.pdf")];
        let s = shelf("s1", &["b1", "b2"]);
        assert_eq!(content_kind(&rows, &s), ContentKind::Mixed);
        assert_eq!(ContentKind::Mixed.badge(), Some("Mixed"));
        assert!(ContentKind::Mixed.tooltip().unwrap().contains("Some books"));
    }

    #[test]
    fn shelf_links_are_ignored() {
        let rows = vec![
            linked("b1", "/a.pdf"),
            Row::link("l1".into(), "Shelf".into(), "s2".into(), 1),
        ];
        let s = shelf("s1", &["b1", "l1"]);
        assert_eq!(content_kind(&rows, &s), ContentKind::OnDisk);
        let s2 = shelf("s2", &["l1"]);
        assert_eq!(content_kind(&rows, &s2), ContentKind::Empty);
    }

    #[test]
    fn book_links_resolve_to_their_target() {
        let rows = vec![
            stored("b1", "/a.pdf"),
            Row::link("l1".into(), "Link".into(), "b1".into(), 1),
        ];
        let s = shelf("s1", &["l1"]);
        assert_eq!(content_kind(&rows, &s), ContentKind::Stored);
    }

    #[test]
    fn duplicated_folder_inside_read_at_place_is_stored_not_on_disk() {
        // The bug: a folder shelf inside a read-at-place tree is duplicated.
        // The duplicate is virtual, its parent is the original's parent (still
        // inside the tree), so governance says "On disk", but its members are
        // stored copies.
        let rows = vec![stored("dup1", "/a.pdf"), stored("dup2", "/b.pdf")];
        let s = shelf("dup_shelf", &["dup1", "dup2"]);
        // Content says stored, so badge must be Copied, not On disk.
        assert_eq!(content_kind(&rows, &s), ContentKind::Stored);
    }

    #[test]
    fn the_badge_reads_the_shelf_itself_first() {
        let rows = vec![linked("b1", "/a.pdf"), stored("b2", "/b.pdf")];
        let root = shelf("s1", &["b1"]);
        let kid = testkit::shelf("s2", "s2", &["b2"], Some("s1"));
        let shelves = vec![root, kid];
        // The shelf's own member decides the badge even when its subtree holds
        // something else: what the folder holds is what it says.
        assert_eq!(badge_kind(&rows, &shelves, "s1"), ContentKind::OnDisk);
        assert_eq!(badge_kind(&rows, &shelves, "s2"), ContentKind::Stored);
    }

    #[test]
    fn an_empty_shelf_badge_falls_back_to_its_subtree() {
        let rows = vec![stored("b1", "/a.pdf"), stored("b2", "/b.pdf")];
        let root = shelf("s1", &[]);
        let kid = testkit::shelf("s2", "s2", &["b1", "b2"], Some("s1"));
        let shelves = vec![root, kid];
        // The duplicated-tree shape: a root holding nothing whose leaves are
        // stored copies still reads as Copied.
        assert_eq!(badge_kind(&rows, &shelves, "s1"), ContentKind::Stored);
    }

    #[test]
    fn a_shelf_nobody_holds_is_empty() {
        let rows: Vec<Row> = vec![];
        assert_eq!(badge_kind(&rows, &[], "gone"), ContentKind::Empty);
    }
}
