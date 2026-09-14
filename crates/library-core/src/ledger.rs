//! The rescan ledger: what a scan of a watched folder DOES.
//!
//! This is the module the feature's edge cases live in. A rescan runs on every
//! window focus, so a wrong answer is not a crash — it is a book that comes back
//! after the reader deleted it, or a library that gains a duplicate every time the
//! window is clicked. [`diff_folder`] is pure, so the cases are testable.

use std::collections::{HashMap, HashSet};

use crate::book::{Book, Fingerprint, Origin, Row, book_rows, book_rows_mut, find_by_id};
use crate::folder::{Tombstone, WatchedFolder};
use crate::scan::FoundFile;
use crate::shelf::Shelf;

/// What the ledger needs to know about a book already in the library: its id, its
/// address, whether that address is known to be dead, and — for a book the library
/// copied — the address the copy was made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownBook {
    pub id: String,
    pub path: String,
    pub missing: bool,
    /// A linked row carries `None`: its [`Book::source`] is its own address, and the rule this feeds is about a copy and its original.
    pub source: Option<String>,
}

/// Derived from the book list on every scan rather than persisted: a derived index cannot go stale.
pub type Registry = HashMap<Fingerprint, KnownBook>;

/// The [`Registry`] for a row list, built from its BOOK rows: a link has no
/// fingerprint, so a scan cannot see one and never places one. Two rows CAN share a
/// fingerprint — the duplicates a reader asked to keep — and the first row wins.
pub fn registry_of(rows: &[Row]) -> Registry {
    let mut out = Registry::with_capacity(rows.len());
    let books: Vec<&Book> = book_rows(rows).collect();
    for book in books.iter().filter(|b| !b.independent) {
        out.entry(book.fp).or_insert_with(|| known_of(book));
    }
    for book in books.iter().filter(|b| b.independent) {
        out.entry(book.fp).or_insert_with(|| known_of(book));
    }
    out
}

/// The row the library already holds for this content, when it holds one: not "is
/// this NAME on the level I am dropping onto", which is [`crate::conflict::collide`],
/// but "are these BYTES already a book somewhere in the library".
pub fn existing_for(rows: &[Row], fp: Fingerprint) -> Option<ExistingContent> {
    if fp.mtime_ms == 0 {
        return None;
    }
    // Bound rather than chained: the registry is a temporary, and borrowing out of one in the same expression drops it.
    let registry = registry_of(rows);
    let known = registry.get(&fp)?;
    if known.path.is_empty() {
        return None;
    }
    Some(ExistingContent {
        row_id: known.id.clone(),
        missing: known.missing,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingContent {
    pub row_id: String,
    pub missing: bool,
}

fn known_of(book: &crate::book::Book) -> KnownBook {
    KnownBook {
        id: book.id.clone(),
        path: book.path().to_string(),
        missing: book.missing,
        // A stored row's provenance, and nothing for a linked one: a link's source IS its address, so carrying it would make every linked book look like a copy of itself.
        source: match &book.origin {
            Origin::Stored { src, .. } => src.clone(),
            Origin::Linked { .. } => None,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanAction {
    /// A book the library does not have. The caller mints the row and records the fingerprint in [`crate::folder::WatchedFolder::placed`].
    Add(FoundFile),
    /// A known book whose address moved on disk: rewrite the address, and leave the id, the resume point and every shelf membership alone.
    Relink { book_id: String, to: String },
    /// The common case by far — a rescan of an unchanged folder is a list of these.
    Skip,
}

/// Decide what a folder's scan does, file by file, in the order the walk produced it. Pure: it reads the folder's ledger and the global registry and answers with one [`ScanAction`] per found file.
pub fn diff_folder(folder: &WatchedFolder, registry: &Registry, found: &[FoundFile]) -> Vec<ScanAction> {
    let mut out = Vec::with_capacity(found.len());
    for file in found {
        out.push(decide(folder, registry, file));
    }
    out
}

/// Split out of [`diff_folder`] so a test can name the case it is asserting instead of building a whole walk for it.
pub fn decide(folder: &WatchedFolder, registry: &Registry, file: &FoundFile) -> ScanAction {
    // The tombstone wins over everything, including a fingerprint this folder has never seen: the reader removed this file, and it is still on disk and still admitted.
    if folder.is_ignored(&file.fp) {
        return ScanAction::Skip;
    }
    match registry.get(&file.fp) {
        None => {
            // Unknown content this folder placed before, with the book row gone (a storage trim, or a hand-edited blob): do not resurrect it.
            if folder.placed.contains(&file.fp) {
                ScanAction::Skip
            } else if folder.tracks_rung(&folder.shelf_key(file)) {
                ScanAction::Add(file.clone())
            } else {
                // A rung nobody watches adds nothing: a subfolder the reader turned off under a
                // watched root is off for the walk too. An EXPLICIT import answers through
                // [`decide_import`] instead — the reader asking for this ground by name.
                ScanAction::Skip
            }
        }
        Some(known) => known_action(folder, known, file),
    }
}

/// A KNOWN fingerprint is a skip at its own address and a relink when the address
/// moved. What the two tables disagree about is the unknown and the removed.
fn known_action(folder: &WatchedFolder, known: &KnownBook, file: &FoundFile) -> ScanAction {
    if known.path == file.path {
        return ScanAction::Skip;
    }
    // The row the registry named is the library's own COPY of the file this walk is
    // standing on: the address moved because there are two addresses, not because a book
    // went anywhere, so both tables stay quiet.
    if known.source.as_deref() == Some(file.path.as_str()) {
        return ScanAction::Skip;
    }
    // The address moved: this folder placed the book, or the book is already known to be missing.
    if folder.placed.contains(&file.fp) || known.missing {
        ScanAction::Relink {
            book_id: known.id.clone(),
            to: file.path.clone(),
        }
    } else {
        ScanAction::Skip
    }
}

/// One row of an explicit import's table: the same questions as [`decide`], with the
/// two rows a previous removal owns answered the other way. A tombstone is lifted
/// rather than honoured. The lift happens when the book lands ([`restore_deleted`]).
pub fn decide_import(folder: &WatchedFolder, registry: &Registry, file: &FoundFile) -> ScanAction {
    match registry.get(&file.fp) {
        None => ScanAction::Add(file.clone()),
        Some(known) => {
            let action = known_action(folder, known, file);
            // The one row `diff_folder` and this table answer differently. A rescan stays quiet
            // about content another folder placed; an explicit import is a reader asking for THIS
            // folder, and a byte-identical copy of a book another folder holds is still a file
            // this folder has.
            match action {
                ScanAction::Skip if known.path != file.path => ScanAction::Add(file.clone()),
                other => other,
            }
        }
    }
}

/// [`diff_folder`] for a run the reader asked for by name.
pub fn diff_import(folder: &WatchedFolder, registry: &Registry, found: &[FoundFile]) -> Vec<ScanAction> {
    let mut out = Vec::with_capacity(found.len());
    for file in found {
        out.push(decide_import(folder, registry, file));
    }
    out
}

/// The addresses an explicit COPIES run owes a book of its own: the found files the
/// library already reads in place. The three conditions are the whole of what separates
/// "a second instance the reader just asked for" from "the instance the library
/// already has".
pub fn copy_over_paths(found: &[FoundFile], registry: &Registry, rows: &[Row]) -> HashSet<String> {
    found
        .iter()
        .filter(|file| {
            registry.get(&file.fp).is_some_and(|known| {
                find_by_id(rows, &known.id).is_some_and(|b| {
                    matches!(b.origin, Origin::Linked { .. }) && b.path() == file.path
                })
            })
        })
        .map(|file| file.path.clone())
        .collect()
}

/// The files an UNBOUND copies run owes a book of its own: one per fingerprint, first
/// found wins. "Unbound" is the copies run that must not touch a standing tree's
/// ledger — a copies import of the ground a read-at-place tree still reads, whose *as
/// new* answer is a second shelf of the library's own rather than a rewrite of it.
pub fn unbound_copies(found: &[FoundFile], registry: &Registry, rows: &[Row]) -> Vec<FoundFile> {
    let copy_paths = copy_over_paths(found, registry, rows);
    let mut seen: HashSet<Fingerprint> = HashSet::new();
    let mut out = Vec::new();
    for file in found {
        let owed = match registry.get(&file.fp) {
            None => true,
            Some(known) => {
                if known.source.as_deref() == Some(file.path.as_str()) {
                    false
                } else if known.path == file.path {
                    copy_paths.contains(&file.path)
                } else {
                    !known.missing
                }
            }
        };
        if owed && seen.insert(file.fp) {
            out.push(file.clone());
        }
    }
    out
}

/// What a *replace* of that tree puts through the removal's sweep first, so the copies that land come back in the names the shelves showed.
pub fn linked_rows_of(rows: &[Row], placed: &HashSet<Fingerprint>) -> Vec<String> {
    book_rows(rows)
        .filter(|b| matches!(b.origin, Origin::Linked { .. }) && placed.contains(&b.fp))
        .map(|b| b.id.clone())
        .collect()
}

/// Drop the relinks that would point a book at an address another row reads. Two rows
/// can hold one fingerprint and [`registry_of`] is first-wins, so a walk of the OTHER
/// folder would rewrite the first one's address out from under it.
pub fn keep_healable_relinks(relinks: &mut Vec<(String, String)>, rows: &[Row]) {
    relinks.retain(|(_, to)| !book_rows(rows).any(|b| b.path() == to.as_str()));
}

/// Record a deliberate removal, so the next rescan stays quiet about the file. Only the
/// folders that PLACED the book take the tombstone: removing a book the reader added by
/// hand must not poison a watched folder that happens to contain the same file.
pub fn tombstone(folders: &mut [WatchedFolder], entry: &Tombstone) {
    for folder in folders.iter_mut() {
        if folder.placed.contains(&entry.fp) && !folder.is_ignored(&entry.fp) {
            folder.ignored.push(entry.clone());
        }
    }
}

/// Drop the tombstones whose books came back: a fingerprint can rejoin the library by
/// any route, and a tombstone left behind for a book that exists is a restore row
/// offering something the reader already has. A MOVED-OUT log is kept — its two jobs
/// both outlive the copy.
pub fn prune_tombstones(folder: &mut WatchedFolder, registry: &Registry) {
    folder
        .ignored
        .retain(|entry| entry.moved || !registry.contains_key(&entry.fp));
}

/// A restore measures the file before it promises anything, and a removal that stays put when the measurement fails is the difference between "that file is gone" and a book quietly lost.
pub fn find_tombstone<'a>(folder: &'a WatchedFolder, fp: &Fingerprint) -> Option<&'a Tombstone> {
    folder.ignored.iter().find(|entry| &entry.fp == fp)
}

/// Does NOT touch `placed`: the import that follows marks the placement when the book
/// lands, and marking it here would leave a fingerprint the ledger skips with no book
/// behind it.
pub fn restore_deleted(folder: &mut WatchedFolder, fp: &Fingerprint) -> Option<Tombstone> {
    let at = folder.ignored.iter().position(|entry| &entry.fp == fp)?;
    Some(folder.ignored.remove(at))
}

/// Borrowed rather than cloned: the menu that asks these opens on a click, and copying
/// a whole library to answer one question is a frame drop. Duplicates resolve to the
/// first, the rule [`registry_of`] gives.
pub fn index_by_fp(rows: &[Row]) -> HashMap<Fingerprint, &Book> {
    let mut out = HashMap::with_capacity(rows.len());
    for book in book_rows(rows) {
        out.entry(book.fp).or_insert(book);
    }
    out
}

/// A book this folder could give the reader back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovered {
    /// Removed by the reader, and still not in the library. The file may or may
    /// not still be on disk — a restore measures before it promises.
    Deleted(Tombstone),
    /// Still in the library, still inside this folder on disk, but no longer on
    /// any shelf this folder owns: the reader moved it somewhere else in the app.
    /// Nothing is wrong, and nothing is re-imported — the offer is to show it here
    /// as well, or to go and look at where it went.
    Moved {
        book_id: String,
        /// The name the shelf shows for it (`Book::title` — the document's own
        /// title when it had one, else the stem of the file the reader knows:
        /// a stored book's SOURCE, never the store's own `source.pdf`).
        title: Option<String>,
        path: String,
        /// The first shelf the book is on, by name, for the "now in Fiction" half
        /// of the row. `None` for a book that is in the library and on no shelf.
        home_shelf: Option<String>,
    },
}

/// What this folder's import menu can offer to give back.
///
/// Pure and synchronous, and that is the design: the menu opens on a click and
/// answers from the last scan's `last_seen` plus the folder's tombstones, so it
/// costs a walk over two short lists rather than a walk over a directory tree.
/// A restore re-measures the one file it is about to import, which is where the
/// freshness actually matters.
///
/// Takes the shelf list rather than a membership callback: which shelves a
/// book is on and which of them the folder owns are both questions
/// [`crate::shelf`] already answers, and a caller that spelled either out
/// would be spelling out a rule this crate owns. "First membership" is shelf
/// order, which is what makes the row's home a deterministic answer rather
/// than whichever a map happened to yield.
pub fn recoverables(
    folder: &WatchedFolder,
    books_by_fp: &HashMap<Fingerprint, &Book>,
    shelves: &[Shelf],
) -> Vec<Recovered> {
    let mut out = Vec::new();

    // Removed books first: a row that offers something back is worth more than a
    // row that offers to show you something you already have.
    for entry in &folder.ignored {
        if books_by_fp.contains_key(&entry.fp) {
            continue;
        }
        // A moved-out log is not a removal: the library still holds the book, and the way back is an import of the file, which spends the log.
        if entry.moved {
            continue;
        }
        out.push(Recovered::Deleted(entry.clone()));
    }

    let owned_by_folder = |shelf: &Shelf| shelf.kind.folder_id() == Some(folder.id.as_str());
    for (fp, path) in &folder.last_seen {
        let Some(book) = books_by_fp.get(fp) else {
            continue;
        };
        // The card already offers a relink; listing it here too would be a second door that does not know the address is bad.
        if book.missing {
            continue;
        }
        let on = crate::shelf::containing(shelves, &book.id);
        if on.iter().any(|shelf| owned_by_folder(shelf)) {
            continue;
        }
        out.push(Recovered::Moved {
            book_id: book.id.clone(),
            // The DISPLAY name: for a stored book the menu's fallback reads `path`, which is the store's own `source.pdf`.
            title: Some(book.title()),
            path: path.clone(),
            home_shelf: on.first().map(|shelf| shelf.name.clone()),
        });
    }
    out
}

/// Apply a `Relink`: rewrite the address, clear `missing`, keep everything else.
/// A stored book does NOT take the new address — its bytes are the app's own copy,
/// and a source file moving is provenance — so only its recorded source moves.
pub fn relink(rows: &mut [Row], book_id: &str, to: &str) -> bool {
    // A link is never relinked and never the row a relink names: it has no address to move.
    let Some(book) = book_rows_mut(rows).find(|b| b.id == book_id) else {
        return false;
    };
    match &mut book.origin {
        crate::book::Origin::Linked { src } => {
            *src = to.to_string();
            book.missing = false;
        }
        crate::book::Origin::Stored { src, .. } => {
            *src = Some(to.to_string());
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Book, Origin};
    use crate::folder::{FolderOpts, Tombstone};
    use crate::tracking::TrackingTree;
    use reader_core::format::Format;
    use std::collections::{BTreeMap, HashSet};

    /// A test's own reading of [`ScanAction`] rather than a method on it: nothing in the
    /// app asks the question, and an accessor only tests call is an API that says it is
    /// load-bearing when it is not.
    fn changes(action: &ScanAction) -> bool {
        !matches!(action, ScanAction::Skip)
    }

    fn fp(n: u32) -> Fingerprint {
        crate::testkit::fp_n(n)
    }

    #[test]
    fn content_the_library_holds_is_a_question_before_it_is_a_second_copy() {
        // A folder walk has always asked it, through the registry, and a loose file dropped on the library never did.
        let rows = vec![crate::testkit::row_at("b1", "/books/dune.pdf")];
        let held = existing_for(&rows, fp(1)).expect("the library holds this content");
        assert_eq!(held.row_id, "b1");
        assert!(!held.missing);
        assert_eq!(existing_for(&rows, fp(2)), None);
        // A link has no fingerprint, so it is never an answer: a pointer is not a copy of a file.
        let with_link = vec![
            crate::testkit::row_at("b1", "/books/dune.pdf"),
            crate::testkit::link("l1", "Dune", "b1"),
        ];
        assert_eq!(
            existing_for(&with_link, fp(1)).map(|e| e.row_id).as_deref(),
            Some("b1")
        );
    }

    #[test]
    fn an_unmeasured_fingerprint_matches_nothing() {
        // A placeholder is derived from the address and stamped `mtime_ms == 0`: matching a
        // real measurement against one would either miss every book or claim one it does not.
        let pending = vec![Row::Book(Book {
            fp: Fingerprint::placeholder("/books/dune.pdf"),
            fp_pending: true,
            ..crate::testkit::book("b1")
        })];
        assert_eq!(existing_for(&pending, Fingerprint::placeholder("/books/dune.pdf")), None);
        assert_eq!(existing_for(&pending, fp(1)), None);
        // A measured row answers, which is what makes the guard about the fingerprint rather than about the row.
        let measured = vec![crate::testkit::row_at("b1", "/books/dune.pdf")];
        assert!(existing_for(&measured, fp(1)).is_some());
    }

    #[test]
    fn a_book_whose_address_died_is_still_the_book_the_library_holds() {
        // `missing` rides the answer because it changes what the question means: a row that
        // resolves is an offer to open it, and one that does not is an offer to find the file
        // again rather than to add a second copy.
        let gone = vec![Row::Book(Book {
            missing: true,
            ..crate::testkit::book_at("b1", "/books/dune.pdf")
        })];
        let held = existing_for(&gone, fp(1)).expect("the content is still held");
        assert_eq!(held.row_id, "b1");
        assert!(held.missing);
    }

    #[test]
    fn a_duplicate_the_reader_kept_is_still_one_content() {
        // Two honest rows of one file share a fingerprint, and the registry is first-wins: the answer names a real row.
        let twins = vec![
            crate::testkit::row_at("b1", "/books/dune.pdf"),
            Row::Book(Book {
                independent: true,
                ..crate::testkit::book_at("b2", "/books/dune.pdf")
            }),
        ];
        let held = existing_for(&twins, fp(1)).expect("one content");
        assert_eq!(held.row_id, "b1", "the shared row answers, not the private one");
    }

    fn file(n: u32, path: &str) -> FoundFile {
        FoundFile {
            path: path.to_string(),
            rel: path.trim_start_matches("/books/").to_string(),
            ext: "pdf".into(),
            size: u64::from(n),
            fp: fp(n),
        }
    }

    fn folder(placed: &[u32], ignored: &[u32]) -> WatchedFolder {
        WatchedFolder {
            id: "f1".into(),
            root: "/books".into(),
            opts: FolderOpts::default(),
            placed: placed.iter().copied().map(fp).collect::<HashSet<_>>(),
            ignored: ignored.iter().copied().map(stone).collect(),
            shelf_map: BTreeMap::new(),
            last_seen: Vec::new(),
            scanned_ms: 0,
            // The ledger's tables answer for a folder the walk is ON — which since tracking became a tree means a tree that tracks from its root.
            tracking: TrackingTree::tracking_root(),
        }
    }

    fn stone(n: u32) -> Tombstone {
        Tombstone {
            fp: fp(n),
            title: Some(format!("Book {n}")),
            format: reader_core::format::Format::Pdf,
            last_path: format!("/books/{n}.pdf"),
            shelf_id: None,
            removed_ms: 5,
            moved: false,
            returned_row: None,
        }
    }

    fn registry(rows: &[(u32, &str, &str, bool)]) -> Registry {
        rows.iter()
            .map(|(n, id, path, missing)| {
                (
                    fp(*n),
                    KnownBook {
                        id: (*id).to_string(),
                        path: (*path).to_string(),
                        missing: *missing,
                        source: None,
                    },
                )
            })
            .collect()
    }

    /// A registry whose rows are the library's own COPIES: each entry carries the address its bytes were made from.
    fn copied_registry(rows: &[(u32, &str, &str, &str)]) -> Registry {
        rows.iter()
            .map(|(n, id, store, source)| {
                (
                    fp(*n),
                    KnownBook {
                        id: (*id).to_string(),
                        path: (*store).to_string(),
                        missing: false,
                        source: Some((*source).to_string()),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn an_unknown_fingerprint_is_added() {
        let f = folder(&[], &[]);
        assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Add(file(1, "/books/a.pdf")));
    }

    /// The tracking tree's quiet half: a rung turned off under a watched root adds nothing
    /// on a RESCAN, while an explicit import of the same ground still adds what it finds.
    #[test]
    fn a_rung_nobody_watches_adds_nothing_on_a_rescan() {
        let mut f = folder(&[], &[]);
        f.set_tracking("Fiction", false);
        let reg = registry(&[]);
        assert_eq!(
            decide(&f, &reg, &file(1, "/books/a.pdf")),
            ScanAction::Add(file(1, "/books/a.pdf"))
        );
        assert_eq!(
            decide(&f, &reg, &file(4, "/books/Poetry/d.pdf")),
            ScanAction::Add(file(4, "/books/Poetry/d.pdf"))
        );
        assert_eq!(decide(&f, &reg, &file(2, "/books/Fiction/b.pdf")), ScanAction::Skip);
        assert_eq!(decide(&f, &reg, &file(3, "/books/Fiction/SciFi/c.pdf")), ScanAction::Skip);
        assert_eq!(
            decide_import(&f, &reg, &file(2, "/books/Fiction/b.pdf")),
            ScanAction::Add(file(2, "/books/Fiction/b.pdf"))
        );
    }

    #[test]
    fn a_known_book_at_its_own_address_is_skipped() {
        let f = folder(&[1], &[]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
        assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
    }

    #[test]
    fn a_known_book_at_a_new_address_is_relinked() {
        let f = folder(&[1], &[]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
        assert_eq!(
            decide(&f, &r, &file(1, "/books/moved/a.pdf")),
            ScanAction::Relink {
                book_id: "b1".into(),
                to: "/books/moved/a.pdf".into()
            }
        );
    }

    #[test]
    fn a_missing_book_is_relinked_by_a_folder_that_never_placed_it() {
        let f = folder(&[], &[]);
        let r = registry(&[(1, "b1", "/gone/a.pdf", true)]);
        assert!(matches!(
            decide(&f, &r, &file(1, "/books/a.pdf")),
            ScanAction::Relink { .. }
        ));
    }

    #[test]
    fn a_second_folder_never_relinks_a_book_it_did_not_place() {
        let f = folder(&[], &[]);
        let r = registry(&[(1, "b1", "/other/a.pdf", false)]);
        assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
    }

    /// Row 6 — the rule the whole ledger exists for: a book the reader moved off this folder's shelf is still in the library, and a rescan must not put it back.
    #[test]
    fn a_book_moved_off_the_folder_shelf_is_never_re_added() {
        let f = folder(&[1], &[]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
        assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
        let f = folder(&[1], &[]);
        assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Skip);
    }

    #[test]
    fn a_removed_book_stays_removed() {
        let f = folder(&[], &[1]);
        assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Skip);
        let f = folder(&[1], &[1]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
        assert_eq!(decide(&f, &r, &file(1, "/books/elsewhere.pdf")), ScanAction::Skip);
    }

    /// A removal is an answer to "should this come back on its own", not to "the reader is asking for it again".
    #[test]
    fn an_explicit_import_overrides_the_removals_that_wrote_the_tombstones() {
        let f = folder(&[], &[1]);
        assert_eq!(
            decide_import(&f, &registry(&[]), &file(1, "/books/a.pdf")),
            ScanAction::Add(file(1, "/books/a.pdf"))
        );
        let f = folder(&[1], &[]);
        assert_eq!(
            decide_import(&f, &registry(&[]), &file(1, "/books/a.pdf")),
            ScanAction::Add(file(1, "/books/a.pdf"))
        );
    }

    /// The import table keeps the rows that are about THIS folder: a book the library
    /// already holds at this address is still a Skip, and at another address this folder
    /// placed it is still a Relink. The row it no longer keeps is the one that made a
    /// second folder's identical copy invisible.
    #[test]
    fn an_explicit_import_duplicates_nothing_this_folder_placed() {
        let f = folder(&[1], &[]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false)]);
        assert_eq!(decide_import(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
        assert_eq!(
            decide_import(&f, &r, &file(1, "/books/moved/a.pdf")),
            ScanAction::Relink {
                book_id: "b1".into(),
                to: "/books/moved/a.pdf".into()
            }
        );
        let f = folder(&[], &[]);
        let r = registry(&[(1, "b1", "/other/a.pdf", false)]);
        assert_eq!(
            decide_import(&f, &r, &file(1, "/books/a.pdf")),
            ScanAction::Add(file(1, "/books/a.pdf"))
        );
        assert_eq!(decide(&f, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
        let placed = folder(&[1], &[]);
        let relink = ScanAction::Relink {
            book_id: "b1".into(),
            to: "/books/a.pdf".into(),
        };
        assert_eq!(decide_import(&placed, &r, &file(1, "/books/a.pdf")), relink);
    }

    /// The exception both tables carry: the row the registry named is the library's own
    /// COPY of the file the walk is standing on. The two addresses are not a move — the copy
    /// is in the store, its source is here — so there is no provenance to rewrite.
    #[test]
    fn a_copy_never_relinks_its_own_source() {
        let f = folder(&[1], &[]);
        let r = copied_registry(&[(1, "b1", "/store/b1.pdf", "/books/a.pdf")]);
        assert_eq!(
            decide(&f, &r, &file(1, "/books/a.pdf")),
            ScanAction::Skip,
            "a rescan of the copy's own source is quiet, however the folder placed it"
        );
        let stranger = folder(&[], &[]);
        assert_eq!(decide(&stranger, &r, &file(1, "/books/a.pdf")), ScanAction::Skip);
        let other = copied_registry(&[(1, "b1", "/store/b1.pdf", "/books/elsewhere.pdf")]);
        assert_eq!(
            decide(&f, &other, &file(1, "/books/moved.pdf")),
            ScanAction::Relink {
                book_id: "b1".into(),
                to: "/books/moved.pdf".into()
            }
        );
        let mut dead = copied_registry(&[(1, "b1", "/store/b1.pdf", "/books/gone.pdf")]);
        dead.get_mut(&fp(1)).expect("a row").missing = true;
        assert_eq!(
            decide(&stranger, &dead, &file(1, "/books/a.pdf")),
            ScanAction::Relink {
                book_id: "b1".into(),
                to: "/books/a.pdf".into()
            }
        );
    }

    /// The one case where the two tables part company over a copy: an import is a reader
    /// asking for THIS file, and the copy is not it, so the quiet answer becomes the
    /// import's own `Add`.
    #[test]
    fn an_explicit_import_of_a_copy_source_asks_for_the_files_own_book() {
        let f = folder(&[1], &[]);
        let r = copied_registry(&[(1, "b1", "/store/b1.pdf", "/books/a.pdf")]);
        assert_eq!(
            decide_import(&f, &r, &file(1, "/books/a.pdf")),
            ScanAction::Add(file(1, "/books/a.pdf")),
            "the file is not the copy, so the import owes it a book of its own"
        );
        let stranger = folder(&[], &[]);
        assert_eq!(
            decide_import(&stranger, &r, &file(1, "/books/a.pdf")),
            ScanAction::Add(file(1, "/books/a.pdf"))
        );
    }

    // A linked book has no second address: its source IS its path, so the copy's exception can never swallow a move heal.
    #[test]
    fn a_linked_book_is_never_a_copy_of_itself() {
        let rows = vec![Row::Book(Book::new(
            "b1".into(),
            fp(1),
            Format::Markdown,
            Origin::Linked {
                src: "/books/a.pdf".into(),
            },
            0,
        ))];
        let r = registry_of(&rows);
        assert_eq!(r.get(&fp(1)).and_then(|k| k.source.clone()), None);
        let f = folder(&[1], &[]);
        assert_eq!(
            decide(&f, &r, &file(1, "/books/moved/a.pdf")),
            ScanAction::Relink {
                book_id: "b1".into(),
                to: "/books/moved/a.pdf".into()
            },
            "the file moved inside the tree, and the heal is what a rescan owes it"
        );
    }

    #[test]
    fn the_registry_carries_a_copy_provenance() {
        let rows = vec![Row::Book(Book::new(
            "b1".into(),
            fp(1),
            Format::Markdown,
            Origin::Stored {
                src: Some("/books/a.pdf".into()),
                store: "/store/b1.pdf".into(),
            },
            0,
        ))];
        let known = registry_of(&rows).get(&fp(1)).cloned().expect("a row");
        assert_eq!(known.path, "/store/b1.pdf", "the address is the store's");
        assert_eq!(known.source.as_deref(), Some("/books/a.pdf"), "and the source is the file's");
        let rows = vec![Row::Book(Book::new(
            "b2".into(),
            fp(2),
            Format::Markdown,
            Origin::Stored {
                src: None,
                store: "/store/b2.pdf".into(),
            },
            0,
        ))];
        assert_eq!(registry_of(&rows).get(&fp(2)).and_then(|k| k.source.clone()), None);
    }

    #[test]
    fn a_whole_walk_answers_in_order_and_only_changes_are_changes() {
        let f = folder(&[2], &[3]);
        let r = registry(&[(2, "b2", "/books/b.pdf", false)]);
        let walk = vec![
            file(1, "/books/a.pdf"),
            file(2, "/books/b.pdf"),
            file(3, "/books/c.pdf"),
            file(4, "/books/sub/d.pdf"),
        ];
        let actions = diff_folder(&f, &r, &walk);
        assert_eq!(actions.len(), walk.len());
        assert!(changes(&actions[0]));
        assert!(!changes(&actions[1]));
        assert!(!changes(&actions[2]));
        assert!(changes(&actions[3]));
        assert_eq!(actions.iter().filter(|a| changes(a)).count(), 2);
    }

    #[test]
    fn an_unchanged_folder_costs_one_state_write_nothing() {
        let f = folder(&[1, 2], &[]);
        let r = registry(&[(1, "b1", "/books/a.pdf", false), (2, "b2", "/books/b.pdf", false)]);
        let walk = vec![file(1, "/books/a.pdf"), file(2, "/books/b.pdf")];
        assert!(diff_folder(&f, &r, &walk).iter().all(|a| !changes(a)));
    }

    #[test]
    fn a_copies_run_copies_over_every_file_the_library_reads_in_place() {
        let linked = |id: &str, path: &str, n: u32| {
            Row::Book(Book::new(
                id.into(),
                fp(n),
                Format::Markdown,
                Origin::Linked { src: path.into() },
                0,
            ))
        };
        let stored = |id: &str, path: &str, n: u32| {
            Row::Book(Book::new(
                id.into(),
                fp(n),
                Format::Markdown,
                Origin::Stored {
                    src: Some(path.into()),
                    store: format!("/store/{id}.md"),
                },
                0,
            ))
        };
        let rows = vec![
            linked("b1", "/one/a.md", 1),   // read in place
            stored("b2", "/one/b.md", 2),   // already the library's own copy
            linked("b3", "/one/c.md", 3),   // read in place by ANOTHER folder's tree
        ];
        let found = vec![
            file(1, "/one/a.md"),
            file(2, "/one/b.md"),
            file(3, "/one/c.md"),
            file(4, "/one/d.md"), // content the library does not hold
        ];
        let registry = registry_of(&rows);
        let paths = copy_over_paths(&found, &registry, &rows);
        let mut paths = paths.into_iter().collect::<Vec<_>>();
        paths.sort();
        assert_eq!(
            paths,
            vec!["/one/a.md".to_string(), "/one/c.md".to_string()],
            "which folder placed the linked row is nobody's question: a copies run              owes a book of its own for every file the library reads in place. A              stored row is already a copy, and a file the library does not hold is              an ordinary add."
        );
    }

    /// The unbound copies run's own table: what the bound explicit run owes, minus everything a ledger would have answered.
    #[test]
    fn an_unbound_copies_run_owes_every_file_but_the_copy_the_library_made() {
        let linked = |id: &str, path: &str, n: u32| {
            Row::Book(Book::new(
                id.into(),
                fp(n),
                Format::Markdown,
                Origin::Linked { src: path.into() },
                0,
            ))
        };
        let stored = |id: &str, path: &str, n: u32| {
            Row::Book(Book::new(
                id.into(),
                fp(n),
                Format::Markdown,
                Origin::Stored {
                    src: Some(path.into()),
                    store: format!("/store/{id}.md"),
                },
                0,
            ))
        };
        let rows = vec![
            linked("b1", "/one/a.md", 1), // a tree reads in place: owed a copy
            stored("b2", "/one/b.md", 2), // the library's own copy of this very file
            linked("b3", "/one/c.md", 3), // read in place: owed a copy
            Row::Book(Book {
                missing: true,
                ..Book::new(
                    "b9".into(),
                    fp(9),
                    Format::Markdown,
                    Origin::Linked { src: "/gone/x.md".into() },
                    0,
                )
            }),
        ];
        let found = vec![
            file(1, "/one/a.md"),
            file(2, "/one/b.md"),
            file(3, "/one/c.md"),
            file(4, "/one/d.md"), // content nobody holds: an ordinary add
            file(1, "/one/a-copy.md"), // a second file of the first one's bytes
            file(9, "/one/x.md"), // a missing book's content, at a new address
        ];
        let registry = registry_of(&rows);
        let owed = unbound_copies(&found, &registry, &rows);
        let paths: Vec<&str> = owed.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["/one/a.md", "/one/c.md", "/one/d.md"],
            "the copy the library already made is not owed a second, one book \
             per fingerprint inside the walk, and a missing book's heal is the \
             walk that owns the row's rather than this run's"
        );
    }

    #[test]
    fn a_namesake_at_another_address_is_not_a_second_instance() {
        let rows = vec![Row::Book(Book::new(
            "b1".into(),
            fp(1),
            Format::Markdown,
            Origin::Linked {
                src: "/elsewhere/a.md".into(),
            },
            0,
        ))];
        let found = vec![file(1, "/one/a.md")];
        assert!(
            copy_over_paths(&found, &registry_of(&rows), &rows).is_empty(),
            "the row the ledger named is not the row at this address"
        );
    }

    #[test]
    fn the_replace_list_is_the_linked_rows_the_ledger_answers_for() {
        let rows = vec![
            Row::Book(Book::new(
                "b1".into(),
                fp(1),
                Format::Markdown,
                Origin::Linked {
                    src: "/one/a.md".into(),
                },
                0,
            )),
            Row::Book(Book::new(
                "b2".into(),
                fp(2),
                Format::Markdown,
                Origin::Stored {
                    src: Some("/one/b.md".into()),
                    store: "/store/b2.md".into(),
                },
                0,
            )),
            Row::Book(Book::new(
                "b3".into(),
                fp(3),
                Format::Markdown,
                Origin::Linked {
                    src: "/one/c.md".into(),
                },
                0,
            )),
        ];
        let placed: HashSet<Fingerprint> = [fp(1), fp(2), fp(3)].into_iter().collect();
        assert_eq!(
            linked_rows_of(&rows, &placed),
            vec!["b1".to_string(), "b3".to_string()],
            "a stored book is already the library's own and never converts"
        );
    }

    #[test]
    fn a_relink_onto_an_address_a_row_already_reads_is_dropped() {
        // Two rows, one fingerprint, and the registry is first-wins: a walk of the OTHER folder would rewrite b1's address out from under it.
        let rows = vec![
            Row::Book(Book::new(
                "b1".into(),
                fp(1),
                Format::Markdown,
                Origin::Linked {
                    src: "/one/a.md".into(),
                },
                0,
            )),
            Row::Book(Book::new(
                "b2".into(),
                fp(2),
                Format::Markdown,
                Origin::Linked {
                    src: "/one/gone.md".into(),
                },
                0,
            )),
        ];
        let mut relinks = vec![
            ("b1".to_string(), "/one/a.md".to_string()),
            ("b2".to_string(), "/one/moved.md".to_string()),
        ];
        keep_healable_relinks(&mut relinks, &rows);
        assert_eq!(
            relinks,
            vec![("b2".to_string(), "/one/moved.md".to_string())],
            "the heal that moves nobody stays, and the one that would steal an address goes"
        );
    }

    #[test]
    fn only_the_folders_that_placed_a_book_take_its_tombstone() {
        let mut folders = vec![folder(&[1], &[]), folder(&[2], &[]), folder(&[], &[])];
        folders[1].id = "f2".into();
        folders[2].id = "f3".into();
        tombstone(&mut folders, &stone(1));
        assert!(folders[0].is_ignored(&fp(1)));
        assert!(folders[1].ignored.is_empty());
        assert!(folders[2].ignored.is_empty());
        tombstone(&mut folders, &stone(9));
        assert!(folders.iter().all(|f| f.ignored.len() <= 1));
    }

    #[test]
    fn placing_a_file_is_what_makes_the_next_scan_skip_it() {
        let mut f = folder(&[], &[]);
        f.mark_placed(fp(1));
        assert_eq!(decide(&f, &registry(&[]), &file(1, "/books/a.pdf")), ScanAction::Skip);
    }

    fn book(id: &str, origin: Origin, missing: bool) -> Row {
        Row::Book(book_value(id, origin, missing))
    }

    fn book_value(id: &str, origin: Origin, missing: bool) -> Book {
        Book {
            fp: fp(1),
            title: Some("Dune".into()),
            origin,
            added_ms: 1,
            last_read_ms: 5,
            page: 42,
            num_pages: 400,
            missing,
            ..crate::testkit::book(id)
        }
    }

    fn at(rows: &[Row], i: usize) -> &Book {
        rows[i].book().expect("a book row")
    }

    #[test]
    fn a_relink_moves_the_address_and_keeps_everything_else() {
        let mut books = vec![book("b1", Origin::Linked { src: "/gone/a.pdf".into() }, true)];
        assert!(relink(&mut books, "b1", "/books/a.pdf"));
        assert_eq!(at(&books, 0).path(), "/books/a.pdf");
        assert!(!at(&books, 0).missing);
        assert_eq!(at(&books, 0).page, 42, "the resume point is the reader's, not the scan's");
        assert_eq!(at(&books, 0).title.as_deref(), Some("Dune"));
        assert_eq!(books[0].id(), "b1");
        assert!(!relink(&mut books, "zzz", "/x"));
    }

    #[test]
    fn a_relink_never_repoints_a_stored_book_at_the_source() {
        // The store copy is what the reader opens; a source file moving is provenance.
        let mut books = vec![book(
            "b1",
            Origin::Stored {
                src: Some("/gone/a.pdf".into()),
                store: "/app/store/pdf/a_b1.pdf".into(),
            },
            false,
        )];
        assert!(relink(&mut books, "b1", "/downloads/a.pdf"));
        assert_eq!(at(&books, 0).path(), "/app/store/pdf/a_b1.pdf");
        assert_eq!(at(&books, 0).origin.source(), Some("/downloads/a.pdf"));
    }


    fn sized_book(id: &str, n: u32, path: &str, missing: bool) -> Row {
        Row::Book(Book {
            fp: fp(n),
            origin: Origin::Linked {
                src: path.to_string(),
            },
            missing,
            ..book_value(id, Origin::Linked { src: path.to_string() }, false)
        })
    }

    fn vshelf(id: &str, name: &str, books: &[&str]) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: name.to_string(),
            kind: crate::shelf::ShelfKind::Virtual,
            books: books.iter().map(|b| b.to_string()).collect(),
            parent: None,
            manual_parent: false,
        }
    }

    fn fshelf(id: &str, name: &str, books: &[&str]) -> Shelf {
        Shelf {
            kind: crate::shelf::ShelfKind::Folder {
                folder_id: "f1".to_string(),
                rel: None,
            },
            ..vshelf(id, name, books)
        }
    }

    #[test]
    fn a_removed_book_is_offered_back_with_enough_to_recognise_it() {
        let f = folder(&[1], &[2]);
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let out = recoverables(&f, &index, &[fshelf("s1", "Books", &[])]);
        assert_eq!(out.len(), 1);
        match &out[0] {
            Recovered::Deleted(entry) => {
                assert_eq!(entry.fp, fp(2));
                assert_eq!(entry.label(), "Book 2");
                assert_eq!(entry.last_path, "/books/2.pdf");
            }
            other => panic!("expected a removal, got {other:?}"),
        }
    }

    #[test]
    fn a_moved_out_log_is_not_offered_back_as_a_removal() {
        // The book a moved-out log belongs to is still in the library as its own stored copy: the way back is an import of the file, which spends the log.
        let mut f = folder(&[1], &[2]);
        f.ignored.push(Tombstone {
            moved: true,
            returned_row: Some("b9".into()),
            ..stone(3)
        });
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let out = recoverables(&f, &index, &[]);
        assert_eq!(out.len(), 1, "only the real removal is offered back");
        match &out[0] {
            Recovered::Deleted(entry) => assert_eq!(entry.fp, fp(2)),
            other => panic!("expected a removal, got {other:?}"),
        }
    }

    #[test]
    fn a_book_that_came_back_by_another_route_is_not_a_recovery() {
        let f = folder(&[1], &[1]);
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        assert!(recoverables(&f, &index, &[]).is_empty());
    }

    #[test]
    fn pruning_drops_the_tombstone_of_a_book_that_returned() {
        let mut f = folder(&[1], &[1, 2]);
        let reg = registry(&[(1, "b1", "/books/1.pdf", false)]);
        prune_tombstones(&mut f, &reg);
        let left: Vec<Fingerprint> = f.ignored.iter().map(|t| t.fp).collect();
        assert_eq!(left, vec![fp(2)], "only the book that is really gone stays");
    }

    /// A moved-out log outlives the copy that carries its fingerprint: it keeps a rescan
    /// quiet about a file the library answered for once, and it is what an import of that
    /// file spends.
    #[test]
    fn pruning_keeps_a_moved_out_log_whatever_the_registry_says() {
        let mut f = folder(&[1, 2], &[]);
        f.ignored.push(Tombstone {
            moved: true,
            ..stone(1)
        });
        f.ignored.push(stone(2));
        let reg = registry(&[(1, "b1", "/store/b1.pdf", false), (2, "b2", "/books/2.pdf", false)]);
        prune_tombstones(&mut f, &reg);
        let left: Vec<Fingerprint> = f.ignored.iter().map(|t| t.fp).collect();
        assert_eq!(
            left,
            vec![fp(1)],
            "the copy's own fingerprint in the registry is the log's reason to stand, not to go"
        );
        assert!(f.ignored[0].moved, "and the log that stands is the moved-out one");
    }

    /// The log's spend is a restore's landing: after it, the file has a linked book again and the folder needs no log.
    #[test]
    fn a_moved_out_log_is_spent_by_the_restore_that_brings_the_book_back() {
        let mut f = folder(&[1], &[]);
        f.ignored.push(Tombstone {
            moved: true,
            ..stone(1)
        });
        let reg = registry(&[(1, "b1", "/store/b1.pdf", false)]);
        prune_tombstones(&mut f, &reg);
        assert_eq!(f.ignored.len(), 1, "a scan leaves it standing");
        assert!(restore_deleted(&mut f, &fp(1)).is_some(), "a restore takes it");
        prune_tombstones(&mut f, &reg);
        assert!(f.ignored.is_empty(), "and nothing puts it back");
    }

    #[test]
    fn a_book_moved_off_every_folder_shelf_is_offered_as_a_move() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let shelves = [
            fshelf("s1", "Books", &[]),
            vshelf("s9", "Fiction", &["b1"]),
        ];
        assert_eq!(
            recoverables(&f, &index, &shelves),
            vec![Recovered::Moved {
                book_id: "b1".into(),
                title: Some("Dune".into()),
                path: "/books/1.pdf".into(),
                home_shelf: Some("Fiction".into()),
            }]
        );
    }

    #[test]
    fn a_book_still_on_one_of_the_folders_shelves_is_not_a_move() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let here = [fshelf("s1", "Books", &["b1"])];
        assert!(recoverables(&f, &index, &here).is_empty());
    }

    #[test]
    fn a_book_on_a_shelf_the_folder_does_not_own_is_a_move() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let shelves = [
            fshelf("s1", "Books", &[]),
            fshelf("s2", "Sci-fi", &[]),
            vshelf("s9", "Fiction", &["b1"]),
        ];
        let out = recoverables(&f, &index, &shelves);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_missing_book_is_a_relink_and_not_a_move() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", true)];
        let index = index_by_fp(&books);
        let shelves = [
            fshelf("s1", "Books", &[]),
            vshelf("s9", "Fiction", &["b1"]),
        ];
        assert!(recoverables(&f, &index, &shelves).is_empty());
    }

    #[test]
    fn a_file_no_longer_in_the_tree_is_neither_a_move_nor_a_removal() {
        let mut f = folder(&[1, 2], &[]);
        f.last_seen = vec![(fp(2), "/books/2.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        assert!(recoverables(&f, &index, &[fshelf("s1", "Books", &[])]).is_empty());
    }

    #[test]
    fn removals_are_listed_before_moves() {
        let mut f = folder(&[1, 2], &[2]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let shelves = [
            fshelf("s1", "Books", &[]),
            vshelf("s9", "Fiction", &["b1"]),
        ];
        let out = recoverables(&f, &index, &shelves);
        assert_eq!(out.len(), 2);
        assert!(matches!(out[0], Recovered::Deleted(_)));
        assert!(matches!(out[1], Recovered::Moved { .. }));
    }

    #[test]
    fn a_home_shelf_is_the_first_one_in_shelf_order() {
        // Deterministic rather than whichever a map happened to yield, because the row's label is a sentence.
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let shelves = [
            fshelf("s1", "Books", &[]),
            vshelf("s8", "Fiction", &["b1"]),
            vshelf("s3", "Classics", &["b1"]),
        ];
        let out = recoverables(&f, &index, &shelves);
        match &out[0] {
            Recovered::Moved { home_shelf, .. } => {
                assert_eq!(home_shelf.as_deref(), Some("Fiction"))
            }
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn a_book_on_no_shelf_at_all_has_no_home_to_name() {
        let mut f = folder(&[1], &[]);
        f.last_seen = vec![(fp(1), "/books/1.pdf".to_string())];
        let books = vec![sized_book("b1", 1, "/books/1.pdf", false)];
        let index = index_by_fp(&books);
        let out = recoverables(&f, &index, &[fshelf("s1", "Books", &[])]);
        match &out[0] {
            Recovered::Moved { home_shelf, .. } => assert_eq!(home_shelf, &None),
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn a_restore_takes_the_tombstone_and_leaves_the_placement_to_the_import() {
        let mut f = folder(&[1], &[2]);
        assert!(find_tombstone(&f, &fp(2)).is_some());
        assert!(find_tombstone(&f, &fp(9)).is_none());
        // Peeking must not consume: a removal that stays put when the measurement fails is the difference between "that file is gone" and a lost book.
        assert!(find_tombstone(&f, &fp(2)).is_some());
        let taken = restore_deleted(&mut f, &fp(2)).expect("present");
        assert_eq!(taken.fp, fp(2));
        assert!(!f.is_ignored(&fp(2)));
        assert!(
            !f.placed.contains(&fp(2)),
            "the import marks the placement, not the restore"
        );
        assert!(restore_deleted(&mut f, &fp(2)).is_none());
    }

    #[test]
    fn a_tombstone_is_the_record_a_restore_row_needs() {
        let b = book_value(
            "b1",
            Origin::Linked {
                src: "/books/dune.pdf".into(),
            },
            false,
        );
        let entry = Tombstone::of(&b, Some("s2".into()), 999);
        assert_eq!(entry.fp, b.fp);
        assert_eq!(entry.title.as_deref(), Some("Dune"));
        assert_eq!(entry.format, reader_core::format::Format::Pdf);
        assert_eq!(entry.last_path, "/books/dune.pdf");
        assert_eq!(entry.shelf_id.as_deref(), Some("s2"));
        assert_eq!(entry.removed_ms, 999);
        assert_eq!(entry.label(), "Dune");
    }

    #[test]
    fn a_tombstone_of_a_stored_book_names_the_file_it_came_from() {
        // The log labels itself with the name the SHELF showed, never the store's own "source.pdf".
        let mut b = book_value(
            "b1",
            Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/Library/items/b1/source.pdf".into(),
            },
            false,
        );
        b.title = None;
        assert_eq!(Tombstone::of(&b, None, 1).label(), "dune");
    }

    #[test]
    fn a_tombstone_of_a_book_never_opened_labels_itself_from_the_file() {
        let mut b = book_value(
            "b1",
            Origin::Linked {
                src: "/books/rust-book.pdf".into(),
            },
            false,
        );
        b.title = None;
        assert_eq!(Tombstone::of(&b, None, 1).label(), "rust-book");
    }

    #[test]
    fn a_tombstone_crosses_the_wire_with_its_camel_case_names() {
        let entry = stone(3);
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"lastPath\""), "{json}");
        assert!(json.contains("\"removedMs\""), "{json}");
        assert!(json.contains("\"shelfId\""), "{json}");
        assert!(!json.contains('_'), "{json}");
        let back: Tombstone = serde_json::from_str(&json).unwrap();
        assert_eq!(back, entry);
        let older: Tombstone = serde_json::from_str(
            r#"{"fp":{"size":1,"mtimeMs":1,"headHash":1},"format":"pdf",
                "lastPath":"/a.pdf","removedMs":2}"#,
        )
        .unwrap();
        assert_eq!(older.shelf_id, None);
        assert_eq!(older.title, None);
        assert!(!older.moved);
        assert_eq!(older.returned_row, None);
        let moved_stone = Tombstone {
            moved: true,
            returned_row: Some("b7".into()),
            ..stone(3)
        };
        let json = serde_json::to_string(&moved_stone).unwrap();
        assert!(json.contains("\"moved\":true"), "{json}");
        assert!(json.contains("\"returnedRow\":\"b7\""), "{json}");
        let back: Tombstone = serde_json::from_str(&json).unwrap();
        assert_eq!(back, moved_stone);
    }

    #[test]
    fn the_last_scan_is_remembered_only_for_books_this_folder_placed() {
        let mut f = folder(&[1], &[]);
        let found = vec![file(1, "/books/1.pdf"), file(2, "/books/2.pdf")];
        f.record_seen(&found);
        assert_eq!(f.last_seen, vec![(fp(1), "/books/1.pdf".to_string())]);
        // A second scan REPLACES the first: the menu answers "where is it now", not "where has it ever been".
        f.record_seen(&[]);
        assert!(f.last_seen.is_empty());
    }

    #[test]
    fn a_scan_that_changed_nothing_still_refreshes_what_was_seen() {
        // The reason `record_seen` is not behind the "did anything change" check: a book moved out between two quiet scans is what the menu has to see.
        let mut f = folder(&[1], &[]);
        f.record_seen(&[file(1, "/books/1.pdf")]);
        f.record_seen(&[file(1, "/books/moved/1.pdf")]);
        assert_eq!(f.last_seen, vec![(fp(1), "/books/moved/1.pdf".to_string())]);
    }

    #[test]
    fn a_link_is_invisible_to_a_scan() {
        // A pointer is not a copy of a file: no fingerprint for a walk to match, no address to relink, nothing a folder could place.
        let rows = vec![
            Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
            book("b1", Origin::Linked { src: "/books/a.pdf".into() }, false),
        ];
        let r = registry_of(&rows);
        assert_eq!(r.len(), 1, "the link is not in it");
        assert_eq!(r[&fp(1)].id, "b1");
        assert_eq!(index_by_fp(&rows).len(), 1);
        let mut rows = rows;
        assert!(!relink(&mut rows, "l1", "/somewhere/a.pdf"));
        assert!(relink(&mut rows, "b1", "/moved/a.pdf"));
        assert_eq!(at(&rows, 1).path(), "/moved/a.pdf");
    }

    #[test]
    fn the_registry_is_built_from_the_books_and_first_row_wins() {
        let books = vec![
            book("b1", Origin::Linked { src: "/books/a.pdf".into() }, false),
            Row::Book(Book {
                id: "dup".into(),
                ..book_value("b1", Origin::Linked { src: "/books/a.pdf".into() }, false)
            }),
        ];
        let r = registry_of(&books);
        assert_eq!(r.len(), 1);
        assert_eq!(r[&fp(1)].id, "b1");
        assert_eq!(r[&fp(1)].path, "/books/a.pdf");
        assert!(!r[&fp(1)].missing);
    }
}
