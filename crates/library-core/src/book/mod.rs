//! The book schema: [`Fingerprint`], [`Origin`], [`Book`] and the [`Row`] a
//! library list holds. The rules that act on rows live one module per question:
//! [`read`], [`merge`], [`check`], [`sanitize`], [`naming`] and [`query`].

use serde::{Deserialize, Serialize};

use document_core::format::Format;

pub mod check;
pub mod merge;
pub mod naming;
pub mod query;
pub mod read;
pub mod sanitize;

pub use check::{add_book, apply_check, drop_dangling_links, drop_dead_shelf_links, remove_row};
pub use merge::{fold_books, further_point};
pub use naming::{duplicate_title, stem_of};
pub use query::{find_book_mut, find_by_id, find_by_path, index_by_id, resume_point};
pub use read::{ReadPoint, record_read, record_read_row, rows_for_read};
pub use sanitize::{sanitize};

/// Storage guard on the library's size, not a "recent books" cap: it keeps the
/// persisted blob inside the browser's quota. Past it, the least-recently-read
/// books go.
pub const BOOKS_CAP: usize = 2000;

/// A book's content identity, compared against files on disk during a rescan.
/// `size` + `mtime_ms` survive a move; `head_hash` separates two different
/// books with the same length and stamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub size: u64,
    pub mtime_ms: u64,
    pub head_hash: u32,
}

impl Fingerprint {
    pub fn of(size: u64, mtime_ms: u64, head: &[u8]) -> Self {
        Self {
            size,
            mtime_ms,
            head_hash: crate::hash::head_hash(head),
        }
    }

    /// Stand-in fingerprint for a book known by address only (a row migrated
    /// from the `v1` schema, which measured nothing). Derived from the address
    /// so two books never share one; `mtime_ms == 0` marks it as a placeholder.
    pub fn placeholder(path: &str) -> Self {
        let bytes = path.as_bytes();
        Self {
            size: bytes.len() as u64,
            mtime_ms: 0,
            head_hash: crate::hash::head_hash(bytes),
        }
    }
}

/// How the app holds a book's bytes: [`Origin::Linked`] is the path itself
/// (the default), [`Origin::Stored`] is a copy inside the app's own store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Origin {
    /// The book is the path; the app never moves, renames or deletes it. A
    /// vanished path is a missing book with a Relink affordance.
    Linked { src: String },
    /// A copy in the store. `src` is provenance — what the copy was made from,
    /// kept so Relink can copy again while the source still exists.
    Stored { src: Option<String>, store: String },
}

impl Origin {
    /// The address every other layer speaks: cover cache keys, resume-point
    /// lookups and the shell's `read_file_*` gate all use it.
    pub fn path(&self) -> &str {
        match self {
            Origin::Linked { src } => src,
            Origin::Stored { store, .. } => store,
        }
    }

    pub fn source(&self) -> Option<&str> {
        match self {
            Origin::Linked { src } => Some(src),
            Origin::Stored { src, .. } => src.as_deref(),
        }
    }

    pub fn is_stored(&self) -> bool {
        matches!(self, Origin::Stored { .. })
    }

    /// Whether this is the library's own copy of `path`. A folder's moved-out
    /// log is only honest when the survivor really is a copy of the file the
    /// dissolving row read.
    pub fn is_store_copy_of(&self, path: &str) -> bool {
        matches!(self, Origin::Stored { src: Some(src), .. } if src == path)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    /// Stable through a relink, a rename and a move between shelves; it is the
    /// drag payload and the shelf member.
    pub id: String,
    pub fp: Fingerprint,
    /// The reader's rename ([`Book::title_locked`]), else the title captured at
    /// open time. `None` until one of the two happens, which is why
    /// [`Book::title`] falls back to the stem rather than storing a guess.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    pub format: Format,
    pub origin: Origin,
    /// The "Date added" sort's key.
    #[serde(default)]
    pub added_ms: u64,
    /// `0` for a book imported but never read, which sorts last rather than first.
    #[serde(default)]
    pub last_read_ms: u64,
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default)]
    pub num_pages: u32,
    /// Written only while stream mode is live; `None` elsewhere, where `page`
    /// is the whole truth.
    #[serde(default)]
    pub fraction: Option<f64>,
    /// Set by a path check, never by a scan: a missing book keeps its row and
    /// shelf membership so Relink can heal it.
    #[serde(default)]
    pub missing: bool,
    /// Placeholder mark a migrated book carries until the first path check
    /// replaces it. No rescan may run while one is set: a real fingerprint
    /// never matches a placeholder, so the folder would re-add books the reader
    /// already has.
    #[serde(default)]
    pub fp_pending: bool,
    /// Marks a copy the conflict sheet answered *as new*. Two rows of one
    /// address are otherwise twins (reads, path checks, highlights and covers
    /// are all keyed by the address); an independent row reads, resumes and
    /// moves on its own.
    #[serde(default)]
    pub independent: bool,
    /// A name the reader typed or accepted from the conflict sheet.
    /// [`crate::book::sanitize`] treats it as a promise: its rule that drops
    /// filename-shaped titles heals download debris, and a chosen name is not
    /// debris.
    #[serde(default)]
    pub title_locked: bool,
}

fn default_page() -> u32 {
    1
}

/// One row of the library's list: a [`Book`], or a [`Row::Link`] pointing at
/// one. A link is the row a second copy of one file would otherwise have been;
/// it has no fingerprint, address or resume point of its own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Row {
    Book(Book),
    /// A pointer at a book. Nothing measures it; its name is the target's at
    /// the moment the link was made.
    #[serde(rename_all = "camelCase")]
    Link {
        id: String,
        name: String,
        target: String,
        #[serde(default)]
        added_ms: u64,
    },
}

impl Row {
    pub fn link(id: String, name: String, target: String, added_ms: u64) -> Self {
        Row::Link {
            id,
            name,
            target,
            added_ms,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Row::Book(b) => &b.id,
            Row::Link { id, .. } => id,
        }
    }

    /// The name the shelf shows and a collision compares: a book's title or
    /// address stem (never empty), a link's made-with name.
    pub fn display_name(&self) -> String {
        match self {
            Row::Book(b) => b.title(),
            Row::Link { name, .. } => name.clone(),
        }
    }

    pub fn is_link(&self) -> bool {
        matches!(self, Row::Link { .. })
    }

    pub fn book(&self) -> Option<&Book> {
        match self {
            Row::Book(b) => Some(b),
            Row::Link { .. } => None,
        }
    }

    pub fn as_book_mut(&mut self) -> Option<&mut Book> {
        match self {
            Row::Book(b) => Some(b),
            Row::Link { .. } => None,
        }
    }

    pub fn fp(&self) -> Option<Fingerprint> {
        match self {
            Row::Book(b) => Some(b.fp),
            Row::Link { .. } => None,
        }
    }

    pub fn target(&self) -> Option<&str> {
        match self {
            Row::Link { target, .. } => Some(target),
            Row::Book(_) => None,
        }
    }

    pub fn added_ms(&self) -> u64 {
        match self {
            Row::Book(b) => b.added_ms,
            Row::Link { added_ms, .. } => *added_ms,
        }
    }
}

/// What every content rule walks: a link has no fingerprint to compare, no
/// address to check and no resume point to write.
pub fn book_rows(rows: &[Row]) -> impl Iterator<Item = &Book> {
    rows.iter().filter_map(Row::book)
}

pub fn book_rows_mut(rows: &mut [Row]) -> impl Iterator<Item = &mut Book> {
    rows.iter_mut().filter_map(Row::as_book_mut)
}

pub fn find_row<'a>(rows: &'a [Row], id: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.id() == id)
}

pub fn find_row_mut<'a>(rows: &'a mut [Row], id: &str) -> Option<&'a mut Row> {
    rows.iter_mut().find(|r| r.id() == id)
}

impl Book {
    /// One constructor for every importing path, so a joined book always
    /// starts at page 1, with no title of its own and not missing.
    pub fn new(id: String, fp: Fingerprint, format: Format, origin: Origin, added_ms: u64) -> Self {
        Self {
            id,
            fp,
            title: None,
            author: None,
            format,
            origin,
            added_ms,
            last_read_ms: 0,
            page: 1,
            num_pages: 0,
            fraction: None,
            missing: false,
            fp_pending: false,
            independent: false,
            title_locked: false,
        }
    }

    pub fn path(&self) -> &str {
        self.origin.path()
    }

    /// The name to show: the document's title, else the file stem, else the
    /// address — never empty. A stored book falls back to its source's stem,
    /// since store bytes are always named `source.<ext>`.
    pub fn title(&self) -> String {
        crate::text::display_or_stem(self.title.as_deref(), self.name_source())
    }

    /// The stem of this book's address: the name an untitled book shows and a
    /// collision compares.
    pub fn stem(&self) -> String {
        stem_of(self.name_source())
    }

    /// One spelling so [`Book::title`] and [`Book::stem`] cannot fall back to
    /// different names.
    fn name_source(&self) -> &str {
        match &self.origin {
            Origin::Stored { src: Some(src), .. } => src,
            origin => origin.path(),
        }
    }

    pub fn author(&self) -> Option<String> {
        crate::text::non_blank(self.author.as_deref()).map(str::to_string)
    }

    pub fn progress(&self) -> Option<f64> {
        if self.num_pages == 0 {
            return self.fraction.filter(|f| (0.0..=1.0).contains(f));
        }
        Some((self.page.min(self.num_pages) as f64 / self.num_pages as f64).clamp(0.0, 1.0))
    }

    /// Take a measurement of this book's own bytes as its identity. A stored
    /// row's fingerprint is the copy's, which leaves the source address free
    /// for whatever folder reads it — a departure can leave the book on its
    /// shelf and still hand the OS file back to the folder's ledger.
    pub fn adopt_measurement(&mut self, measured: Option<Fingerprint>) {
        match measured {
            Some(fp) => {
                self.fp = fp;
                self.fp_pending = false;
            }
            None => self.fp_pending = true,
        }
    }

    /// Make this row the library's own copy of the file it reads. Everything
    /// the reader put into the row travels with it; the store name is minted
    /// from the row's id rather than its title.
    pub fn become_stored(&mut self, src: &str, store: String, measured: Option<Fingerprint>) {
        if self.title.is_none() {
            self.title = Some(crate::text::display_or_stem(None, src));
        }
        self.origin = Origin::Stored {
            src: Some(src.to_string()),
            store,
        };
        self.adopt_measurement(measured);
        self.missing = false;
    }

    /// The heal every path check and folder walk goes through: the row's
    /// identity becomes the measurement, the placeholder mark goes, and the
    /// address is not missing. A `v1` placeholder never matched a measurement,
    /// so this is also what stops a rescan from adding a second copy.
    pub fn heal(&mut self, fp: Fingerprint) {
        self.fp = fp;
        self.fp_pending = false;
        self.missing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(books: impl IntoIterator<Item = Book>) -> Vec<Row> {
        books.into_iter().map(Row::Book).collect()
    }

    fn at(rows: &[Row], i: usize) -> &Book {
        rows[i].book().expect("a book row")
    }

    fn at_mut(rows: &mut [Row], i: usize) -> &mut Book {
        rows[i].as_book_mut().expect("a book row")
    }

    fn fp(size: u64, mtime: u64, head: u32) -> Fingerprint {
        Fingerprint {
            size,
            mtime_ms: mtime,
            head_hash: head,
        }
    }

    fn linked(id: &str, path: &str) -> Book {
        Book {
            id: id.to_string(),
            fp: fp(10, 1, 7),
            title: None,
            author: None,
            format: Format::Pdf,
            origin: Origin::Linked {
                src: path.to_string(),
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
        }
    }

    #[test]
    fn a_linked_book_is_the_path_it_was_opened_from() {
        let b = linked("a", "/books/dune.pdf");
        assert_eq!(b.path(), "/books/dune.pdf");
        assert_eq!(b.origin.source(), Some("/books/dune.pdf"));
        assert!(!b.origin.is_stored());
    }

    #[test]
    fn a_stored_book_opens_from_the_store_and_remembers_its_source() {
        let b = Book {
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/Library/pdf/dune_1a2b3c4d.pdf".into(),
            },
            ..linked("a", "/ignored")
        };
        assert_eq!(b.path(), "/app/Library/pdf/dune_1a2b3c4d.pdf");
        assert_eq!(b.origin.source(), Some("/downloads/dune.pdf"));
        assert!(b.origin.is_stored());
    }

    #[test]
    fn the_title_falls_back_to_the_stem_and_never_to_nothing() {
        let mut b = linked("a", "/books/Dune.pdf");
        assert_eq!(b.title(), "Dune");
        b.title = Some("  ".into());
        assert_eq!(b.title(), "Dune", "a blank title is no title");
        b.origin = Origin::Linked { src: "/".into() };
        assert_eq!(b.title(), "/", "the address is the last resort");
    }

    #[test]
    fn progress_is_a_page_fraction_or_the_stream_fraction() {
        let mut b = linked("a", "/books/dune.pdf");
        assert_eq!(b.progress(), None, "an unknown page count is no progress");
        b.num_pages = 200;
        b.page = 50;
        assert_eq!(b.progress(), Some(0.25));
        b.page = 900;
        assert_eq!(b.progress(), Some(1.0), "past the end clamps");
        let mut s = linked("s", "/notes.md");
        s.fraction = Some(0.4);
        assert_eq!(s.progress(), Some(0.4));
        s.fraction = Some(1.7);
        assert_eq!(s.progress(), None, "an out-of-range fraction is dropped");
    }

    #[test]
    fn an_author_fills_a_gap_and_never_overwrites_one() {
        let mut books = rows([linked("a", "/books/one.pdf")]);
        record_read(
            &mut books,
            "/books/one.pdf",
            None,
            Some("Frank Herbert".into()),
            ReadPoint::fresh(),
            1,
        );
        assert_eq!(at(&books, 0).author.as_deref(), Some("Frank Herbert"));
        record_read(
            &mut books,
            "/books/one.pdf",
            None,
            Some("Somebody Else".into()),
            ReadPoint::fresh(),
            2,
        );
        assert_eq!(
            at(&books, 0).author.as_deref(),
            Some("Frank Herbert"),
            "the first author the document gave is the one the shelf keeps"
        );
        let mut books = rows([linked("b", "/books/two.pdf")]);
        record_read(
            &mut books,
            "/books/two.pdf",
            None,
            Some("   ".into()),
            ReadPoint::fresh(),
            1,
        );
        assert_eq!(at(&books, 0).author, None);
    }

    #[test]
    fn reading_a_known_book_updates_it_in_place() {
        let mut books = rows([linked("a", "/books/one.pdf"), linked("b", "/books/two.pdf")]);
        let created = record_read(
            &mut books,
            "/books/two.pdf",
            Some("Two".into()),
            None,
            ReadPoint { page: 42, num_pages: 100, fraction: None },
            500,
        );
        assert!(created.is_none(), "an existing book is not created again");
        assert_eq!(books.len(), 2);
        assert_eq!(at(&books, 0).id, "a");
        assert_eq!(at(&books, 1).page, 42);
        assert_eq!(at(&books, 1).last_read_ms, 500);
        assert_eq!(at(&books, 1).title.as_deref(), Some("Two"));
        assert_eq!(at(&books, 1).fp, fp(10, 1, 7));
        assert!(!at(&books, 1).fp_pending);
    }

    #[test]
    fn reading_an_unknown_book_creates_a_linked_one_at_the_front() {
        let mut books = rows([linked("a", "/books/one.pdf")]);
        let created = record_read(
            &mut books,
            "/books/new.md",
            None,
            None,
            ReadPoint::fresh(),
            600,
        )
        .expect("a new book");
        assert_eq!(books.len(), 2);
        assert_eq!(at(&books, 0).id, created.id);
        assert_eq!(created.format, Format::Markdown);
        assert_eq!(created.path(), "/books/new.md");
        assert!(!created.origin.is_stored());
        // Opening proved the file exists and nothing more; the next path check measures it.
        assert!(created.fp_pending);
        assert_eq!(created.fp, Fingerprint::placeholder("/books/new.md"));
    }

    #[test]
    fn a_title_only_ever_fills_a_gap() {
        let mut books = rows([Book {
            title: Some("Named by the document".into()),
            ..linked("a", "/books/one.pdf")
        }]);
        record_read(
            &mut books,
            "/books/one.pdf",
            Some("one".into()),
            None,
            ReadPoint { page: 3, num_pages: 10, fraction: None },
            10,
        );
        assert_eq!(at(&books, 0).title.as_deref(), Some("Named by the document"));
    }

    #[test]
    fn a_resume_point_is_settled_before_it_is_written() {
        // The reader hands over whatever the document said; the library keeps it valid.
        let mut books = rows([linked("a", "/books/one.pdf")]);
        record_read(
            &mut books,
            "/books/one.pdf",
            None,
            None,
            ReadPoint { page: 0, num_pages: 10, fraction: Some(1.4) },
            1,
        );
        assert_eq!(at(&books, 0).page, 1);
        assert_eq!(at(&books, 0).fraction, None);
        assert_eq!(ReadPoint::fresh(), ReadPoint { page: 1, num_pages: 0, fraction: None });
    }

    #[test]
    fn a_path_check_is_what_measures_a_book() {
        let mut books = rows([Book {
            fp_pending: true,
            ..linked("a", "/books/one.pdf")
        }]);
        let touched = apply_check(
            &mut books,
            &check("/books/one.pdf", true, 20, 2, 8),
        );
        assert_eq!(touched, vec!["a".to_string()]);
        assert_eq!(at(&books, 0).fp, fp(20, 2, 8));
        assert!(!at(&books, 0).fp_pending, "the measurement replaces the placeholder");
        assert!(apply_check(&mut books, &check("/books/one.pdf", true, 20, 2, 8)).is_empty());
    }

    #[test]
    fn a_path_that_does_not_resolve_marks_the_book_missing_and_keeps_it() {
        let mut books = rows([Book {
            page: 42,
            num_pages: 100,
            ..linked("a", "/books/one.pdf")
        }]);
        assert_eq!(
            apply_check(&mut books, &check("/books/one.pdf", false, 0, 0, 0)),
            vec!["a".to_string()]
        );
        assert!(at(&books, 0).missing);
        assert!(!at(&books, 0).fp_pending, "a check that ran is not a check still owed");
        assert_eq!(books.len(), 1, "a missing book is not a removed one");
        assert_eq!(at(&books, 0).page, 42, "the resume point survives the address dying");
        assert_eq!(at(&books, 0).fp, fp(10, 1, 7), "and so does the last known identity");
        assert!(apply_check(&mut books, &check("/books/one.pdf", false, 0, 0, 0)).is_empty());
    }

    #[test]
    fn a_check_for_an_address_the_library_does_not_hold_does_nothing() {
        let mut books = rows([linked("a", "/books/one.pdf")]);
        assert!(apply_check(&mut books, &check("/books/other.pdf", true, 1, 1, 1)).is_empty());
        assert!(!at(&books, 0).missing);
    }

    #[test]
    fn two_rows_of_one_file_share_its_reading_truth() {
        // A kept duplicate is a second row, not a second file: reads, checks
        // and heals reach every row at the address, or a twin's placeholder
        // would hold a folder's rescan off forever.
        let mut books = rows([
            Book {
                fp_pending: true,
                ..linked("a", "/books/dune.pdf")
            },
            Book {
                id: "b".into(),
                title: Some("dune_1".into()),
                fp_pending: true,
                ..linked("a", "/books/dune.pdf")
            },
        ]);
        assert!(record_read(
            &mut books,
            "/books/dune.pdf",
            Some("Dune".into()),
            None,
            ReadPoint { page: 90, num_pages: 400, fraction: None },
            700,
        )
        .is_none());
        for book in book_rows(&books) {
            assert_eq!(book.page, 90);
            assert_eq!(book.last_read_ms, 700);
        }
        assert_eq!(at(&books, 0).title.as_deref(), Some("Dune"));
        assert_eq!(at(&books, 1).title.as_deref(), Some("dune_1"));
        assert_eq!(
            apply_check(&mut books, &check("/books/dune.pdf", true, 20, 2, 8)).len(),
            2,
            "one measurement heals every row at the address"
        );
        assert!(book_rows(&books).all(|b| !b.fp_pending && b.fp == fp(20, 2, 8)));
    }

    fn private(id: &str, path: &str) -> Book {
        Book {
            independent: true,
            ..linked(id, path)
        }
    }

    #[test]
    fn two_rows_of_one_file_are_two_books_with_two_ids() {
        // Marks are keyed by row id; the crate owes that the two rows are two
        // ids and never resolve to each other.
        let shared = linked("a", "/books/dune.pdf");
        let own = private("b", "/books/dune.pdf");
        assert_eq!(shared.id, "a");
        assert_eq!(own.id, "b");
        assert_ne!(shared.id, own.id, "two rows, two keys, two mark lists");
        assert!(own.independent && !shared.independent);
        // An import resolving this address must never land on the private row.
        let mut list = rows([own.clone()]);
        assert_eq!(add_book(&mut list, shared.clone()), "a", "a private row holds nothing back");
        assert_eq!(list.len(), 2, "so the arrival joins as a book of its own");
        for order in [
            rows([shared.clone(), own.clone()]),
            rows([own.clone(), shared.clone()]),
        ] {
            let mut both = order;
            assert_eq!(
                add_book(&mut both, Book { fp: fp(10, 1, 7), ..linked("new", "/books/dune.pdf") }),
                "a",
                "a shared row at the address is the one an import resolves to"
            );
        }
    }

    #[test]
    fn a_shared_read_leaves_a_private_book_where_it_was() {
        // Two rows of one file share their position — unless one is independent.
        let mut books = rows([linked("a", "/books/dune.pdf"), private("b", "/books/dune.pdf")]);
        at_mut(&mut books, 1).page = 240;
        record_read(
            &mut books,
            "/books/dune.pdf",
            Some("Dune".into()),
            None,
            ReadPoint { page: 90, num_pages: 400, fraction: None },
            700,
        );
        assert_eq!(at(&books, 0).page, 90, "the shared row moves");
        assert_eq!(at(&books, 1).page, 240, "and the private one does not");
        assert_eq!(at(&books, 0).last_read_ms, 700);
        assert_eq!(at(&books, 1).last_read_ms, 1, "its stamp is its own too");
        // A path check is the address's fate, so it writes every row.
        assert_eq!(
            apply_check(&mut books, &check("/books/dune.pdf", true, 20, 2, 8)).len(),
            2
        );
        assert!(book_rows(&books).all(|b| !b.missing && b.fp == fp(20, 2, 8)));
    }

    #[test]
    fn an_open_that_names_no_row_treats_the_address_as_one_book() {
        // An open that cannot ask which row was meant falls back to the address.
        let mut books = rows([private("a", "/books/dune.pdf"), private("b", "/books/dune.pdf")]);
        assert_eq!(rows_for_read(&books, None, "/books/dune.pdf"), vec![0, 1]);
        assert!(record_read(
            &mut books,
            "/books/dune.pdf",
            Some("Dune".into()),
            None,
            ReadPoint { page: 12, num_pages: 400, fraction: None },
            5,
        )
        .is_none());
        assert_eq!(books.len(), 2, "nothing was minted for a file already held");
        assert!(book_rows(&books).all(|b| b.page == 12));
        assert_eq!(rows_for_read(&books, Some("b"), "/books/dune.pdf"), vec![1]);
    }

    #[test]
    fn the_rows_a_read_belongs_to_are_the_rows_it_writes() {
        let books = rows([
            linked("a", "/books/dune.pdf"),
            private("b", "/books/dune.pdf"),
            linked("c", "/books/dune.pdf"),
            linked("d", "/books/other.pdf"),
        ]);
        assert_eq!(rows_for_read(&books, None, "/books/dune.pdf"), vec![0, 2]);
        assert_eq!(rows_for_read(&books, Some("c"), "/books/dune.pdf"), vec![0, 2]);
        assert_eq!(rows_for_read(&books, Some("b"), "/books/dune.pdf"), vec![1]);
        assert_eq!(rows_for_read(&books, Some("d"), "/books/dune.pdf"), vec![0, 2]);
        assert_eq!(rows_for_read(&books, Some("zzz"), "/books/dune.pdf"), vec![0, 2]);
        assert!(rows_for_read(&books, None, "/books/nothing.pdf").is_empty());
    }

    #[test]
    fn a_read_that_names_its_row_writes_that_row() {
        let mut books = rows([linked("a", "/books/dune.pdf"), private("b", "/books/dune.pdf")]);
        let point = ReadPoint { page: 240, num_pages: 400, fraction: None };
        assert!(record_read_row(&mut books, "a", "/books/dune.pdf", None, None, point, 9).is_none());
        assert_eq!(at(&books, 0).page, 240);
        assert_eq!(at(&books, 1).page, 1, "the private row is not a twin of it");
        let further = ReadPoint { page: 380, num_pages: 400, fraction: None };
        assert!(record_read_row(&mut books, "b", "/books/dune.pdf", None, None, further, 11).is_none());
        assert_eq!(at(&books, 1).page, 380);
        assert_eq!(at(&books, 1).last_read_ms, 11);
        assert_eq!(at(&books, 0).page, 240, "the shared row keeps the read it was given");
        assert!(record_read_row(&mut books, "zzz", "/books/dune.pdf", None, None, point, 12).is_none());
        assert_eq!(at(&books, 0).page, 240);
        assert_eq!(books.len(), 2);
        let created = record_read_row(
            &mut books,
            "zzz",
            "/books/other.pdf",
            Some("Other".into()),
            None,
            ReadPoint::fresh(),
            13,
        );
        assert_eq!(created.and_then(|b| b.title).as_deref(), Some("Other"));
        assert_eq!(books.len(), 3);
    }

    #[test]
    fn the_resume_point_follows_the_row_the_reader_named() {
        let mut shared = linked("a", "/books/dune.pdf");
        shared.page = 12;
        let mut own = private("b", "/books/dune.pdf");
        own.page = 240;
        own.fraction = Some(0.5);
        let books = rows([shared, own]);
        assert_eq!(resume_point(&books, Some("b"), "/books/dune.pdf"), (240, Some(0.5)));
        assert_eq!(resume_point(&books, Some("a"), "/books/dune.pdf"), (12, None));
        assert_eq!(resume_point(&books, None, "/books/dune.pdf"), (12, None));
        assert_eq!(resume_point(&books, Some("zzz"), "/books/dune.pdf"), (12, None));
        assert_eq!(resume_point(&books, None, "/books/nope.pdf"), (1, None));
    }

    #[test]
    fn an_import_resolves_to_a_shared_row_and_never_to_a_private_one() {
        // An import must not resolve to a private book.
        let mut books = rows([private("a", "/one/dune.pdf")]);
        let arrival = Book {
            origin: Origin::Linked { src: "/two/dune.pdf".into() },
            ..linked("new", "/two/dune.pdf")
        };
        assert_eq!(add_book(&mut books, arrival), "new", "a private row holds nothing back");
        assert_eq!(books.len(), 2);
        let again = linked("new2", "/three/dune.pdf");
        assert_eq!(add_book(&mut books, again), "new");
        assert_eq!(books.len(), 2);
    }

    #[test]
    fn a_scan_names_the_shared_row_and_only_falls_back_to_a_private_one() {
        let books = rows([private("a", "/one/dune.pdf"), linked("b", "/two/dune.pdf")]);
        let registry = crate::ledger::registry_of(&books);
        assert_eq!(
            registry.get(&fp(10, 1, 7)).map(|k| k.id.as_str()),
            Some("b"),
            "the file moved, so the shared row is the one that follows it"
        );
        let only = rows([private("a", "/one/dune.pdf")]);
        assert_eq!(
            crate::ledger::registry_of(&only).get(&fp(10, 1, 7)).map(|k| k.id.as_str()),
            Some("a")
        );
    }

    #[test]
    fn a_fold_takes_the_further_place_and_fills_the_gaps() {
        let mut keep = linked("keep", "/books/dune.pdf");
        keep.page = 12;
        keep.num_pages = 300;
        keep.added_ms = 500;
        keep.last_read_ms = 900;
        let mut gone = linked("gone", "/copies/dune.pdf");
        gone.page = 240;
        gone.title = Some("Dune".into());
        gone.author = Some("Frank Herbert".into());
        gone.added_ms = 300;
        gone.last_read_ms = 700;

        fold_books(&mut keep, &gone);
        assert_eq!(keep.page, 240, "a merge never sends a reader backwards");
        assert_eq!(keep.num_pages, 300, "the count survives from whichever row knew it");
        assert_eq!(keep.title.as_deref(), Some("Dune"), "a name fills a gap");
        assert_eq!(keep.author.as_deref(), Some("Frank Herbert"));
        assert_eq!(keep.added_ms, 300, "the book joined when it first joined");
        assert_eq!(keep.last_read_ms, 900, "and was read as recently as it was");
        assert_eq!(keep.id, "keep");
        assert_eq!(keep.path(), "/books/dune.pdf");
        let mut keep2 = linked("keep", "/books/dune.pdf");
        keep2.page = 12;
        keep2.title = Some("Mine".into());
        fold_books(&mut keep2, &gone);
        assert_eq!(keep2.page, 240);
        assert_eq!(keep2.title.as_deref(), Some("Mine"), "and never overwrites a name");
    }

    #[test]
    fn a_page_tie_goes_to_the_deeper_stream_fraction() {
        let a = ReadPoint { page: 10, num_pages: 0, fraction: Some(0.4) };
        let b = ReadPoint { page: 10, num_pages: 0, fraction: Some(0.7) };
        assert_eq!(further_point(a, b), b);
        assert_eq!(further_point(b, a), b);
        let plain = ReadPoint { page: 10, num_pages: 0, fraction: None };
        assert_eq!(further_point(plain, a), a);
        assert_eq!(further_point(a, plain), a);
        assert_eq!(further_point(a, a), a);
    }

    #[test]
    fn a_fold_measures_and_unloses_an_address() {
        // A placeholder yields to a measurement; an address is dead only when both rows say so.
        let mut pending = linked("a", "/gone/dune.pdf");
        pending.fp = Fingerprint::placeholder("/gone/dune.pdf");
        pending.fp_pending = true;
        pending.missing = true;
        let measured = linked("b", "/books/dune.pdf");
        fold_books(&mut pending, &measured);
        assert!(!pending.fp_pending, "the merged row has been weighed");
        assert_eq!(pending.fp, measured.fp);
        assert!(!pending.missing);
        let mut both = linked("a", "/gone/dune.pdf");
        both.fp_pending = true;
        both.missing = true;
        let mut also = linked("b", "/gone/dune.pdf");
        also.fp_pending = true;
        also.missing = true;
        fold_books(&mut both, &also);
        assert!(both.fp_pending && both.missing);
        // added_ms == 0 means never, not the epoch.
        let mut never = linked("a", "/one.pdf");
        never.added_ms = 0;
        let joined = {
            let mut b = linked("b", "/two.pdf");
            b.added_ms = 40;
            b
        };
        fold_books(&mut never, &joined);
        assert_eq!(never.added_ms, 40);
    }

    #[test]
    fn a_relink_heals_a_missing_book() {
        let mut books = rows([Book {
            missing: true,
            ..linked("a", "/gone/one.pdf")
        }]);
        assert!(crate::ledger::relink(&mut books, "a", "/books/one.pdf"));
        assert!(!at(&books, 0).missing);
        assert_eq!(at(&books, 0).path(), "/books/one.pdf");
    }

    fn check(path: &str, exists: bool, size: u64, mtime: u64, head: u32) -> crate::wire::PathCheck {
        crate::wire::PathCheck {
            path: path.to_string(),
            exists,
            size,
            mtime_ms: mtime,
            head_hash: head,
        }
    }

    #[test]
    fn the_same_content_is_the_same_book_whatever_its_address() {
        let mut books = rows([linked("a", "/books/one.pdf")]);
        let twin = Book {
            id: "twin".into(),
            origin: Origin::Linked {
                src: "/other/one.pdf".into(),
            },
            ..linked("a", "/books/one.pdf")
        };
        assert_eq!(add_book(&mut books, twin), "a");
        assert_eq!(books.len(), 1, "a second address never makes a second book");
    }

    #[test]
    fn different_content_is_a_different_book() {
        let mut books = rows([linked("a", "/books/one.pdf")]);
        let other = Book {
            fp: fp(11, 1, 7),
            ..linked("b", "/books/one.pdf")
        };
        assert_eq!(add_book(&mut books, other), "b");
        assert_eq!(books.len(), 2);
    }

    #[test]
    fn an_import_files_in_the_order_the_walk_produced() {
        // Inserting each file at the front would reverse a whole folder.
        let mut books: Vec<Row> = Vec::new();
        for name in ["a", "b", "c", "d"] {
            let book = Book {
                fp: fp(name.as_bytes()[0] as u64, 1, 1),
                ..linked(name, &format!("/books/{name}.pdf"))
            };
            add_book(&mut books, book);
        }
        let ids: Vec<&str> = books.iter().map(Row::id).collect();
        assert_eq!(ids, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn removing_returns_the_book_so_the_caller_can_finish_the_job() {
        let mut books = rows([linked("a", "/books/one.pdf"), linked("b", "/books/two.pdf")]);
        let gone = remove_row(&mut books, "a").expect("present");
        assert_eq!(gone.book().expect("a book row").path(), "/books/one.pdf");
        assert_eq!(books.len(), 1);
        assert!(remove_row(&mut books, "zzz").is_none());
    }

    #[test]
    fn the_resume_point_is_looked_up_by_address() {
        let books = rows([Book {
            page: 42,
            num_pages: 100,
            fraction: Some(0.5),
            ..linked("a", "/books/one.pdf")
        }]);
        assert_eq!(resume_point(&books, None, "/books/one.pdf"), (42, Some(0.5)));
        assert_eq!(resume_point(&books, None, "/books/zzz.pdf"), (1, None));
        assert_eq!(find_by_path(&books, "/books/one.pdf").map(|b| b.id.as_str()), Some("a"));
    }

    #[test]
    fn sanitize_dedupes_by_id_and_clamps_the_resume() {
        let mut books = rows([
            Book {
                page: 0,
                ..linked("a", "/books/one.pdf")
            },
            Book {
                origin: Origin::Linked {
                    src: "/copies/one.pdf".into(),
                },
                ..linked("a", "/books/one.pdf")
            },
            // Same content under two ids is a duplicate the reader chose to keep; a fingerprint dedupe would take it back.
            Book {
                id: "dup".into(),
                title: Some("one_1".into()),
                ..linked("dup", "/books/one.pdf")
            },
            Book {
                id: "  ".into(),
                ..linked("blank", "/books/three.pdf")
            },
            Book {
                fp: fp(99, 9, 9),
                fraction: Some(2.0),
                ..linked("c", "/books/four.md")
            },
        ]);
        sanitize(&mut books);
        let ids: Vec<&str> = books.iter().map(Row::id).collect();
        assert_eq!(ids, vec!["a", "dup", "c"]);
        assert_eq!(at(&books, 0).page, 1, "page 0 clamps to 1");
        assert_eq!(at(&books, 1).title.as_deref(), Some("one_1"), "a duplicate keeps the name it was minted with");
        assert_eq!(at(&books, 2).fraction, None, "an impossible fraction is dropped");
    }

    #[test]
    fn a_title_the_reader_chose_survives_the_debris_rule() {
        // The title rule hunts document-supplied debris; a reader-typed name is locked.
        let mut books = rows([
            Book {
                title: Some("0321894073.pdf".into()),
                ..linked("doc", "/books/one.pdf")
            },
            Book {
                id: "ren".into(),
                title: Some("my_report_final.pdf".into()),
                title_locked: true,
                ..linked("ren", "/books/two.pdf")
            },
        ]);
        sanitize(&mut books);
        assert_eq!(
            at(&books, 0).title, None,
            "a document's download debris still goes"
        );
        assert_eq!(
            at(&books, 1).title.as_deref(),
            Some("my_report_final.pdf"),
            "the reader's own name stays, whatever it looks like"
        );
    }

    #[test]
    fn a_stored_book_is_named_by_the_file_it_came_from() {
        // Store bytes are named `source.pdf`, so the store address's stem is a layout artifact.
        let stored = Book {
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/Library/items/b1/source.pdf".into(),
            },
            ..linked("b1", "/app/Library/items/b1/source.pdf")
        };
        assert_eq!(stored.title(), "dune");
        assert_eq!(stored.stem(), "dune");
        let orphan = Book {
            origin: Origin::Stored {
                src: None,
                store: "/app/Library/items/b2/source.pdf".into(),
            },
            ..linked("b2", "/app/Library/items/b2/source.pdf")
        };
        assert_eq!(orphan.title(), "source");
        let named = Book {
            title: Some("Dune".into()),
            ..stored
        };
        assert_eq!(named.title(), "Dune");
    }

    #[test]
    fn a_store_stem_burnt_into_a_title_is_healed_and_a_readers_own_is_not() {
        // A stored book's open-address stem is the store's own "source": a
        // burn-in the load drops.
        let mut books = rows([
            Book {
                title: Some("source".into()),
                origin: Origin::Stored {
                    src: Some("/downloads/dune.pdf".into()),
                    store: "/app/Library/items/b1/source.pdf".into(),
                },
                ..linked("b1", "/app/Library/items/b1/source.pdf")
            },
            Book {
                title: Some("source".into()),
                title_locked: true,
                origin: Origin::Stored {
                    src: Some("/downloads/foundation.pdf".into()),
                    store: "/app/Library/items/b2/source.pdf".into(),
                },
                ..linked("b2", "/app/Library/items/b2/source.pdf")
            },
        ]);
        sanitize(&mut books);
        assert_eq!(at(&books, 0).title, None, "the burn-in goes");
        assert_eq!(at(&books, 0).title(), "dune", "and the source's stem shows");
        assert_eq!(
            at(&books, 1).title.as_deref(),
            Some("source"),
            "the reader's own name stays, however plain"
        );
    }

    #[test]
    fn a_link_to_a_shelf_survives_the_row_sweep() {
        // A shelf id is never a book id; which shelves exist is the shelf sweep's question.
        let mut books = rows([linked("a", "/books/one.pdf")]);
        books.push(Row::link("l1".into(), "Books".into(), "s1".into(), 1));
        books.push(Row::link("l2".into(), "Gone".into(), "zz".into(), 1));
        sanitize(&mut books);
        let ids: Vec<&str> = books.iter().map(Row::id).collect();
        assert_eq!(ids, vec!["a", "l1"], "the book link at a gone book goes; the shelf link stays");
    }

    #[test]
    fn a_link_to_a_shelf_goes_with_the_shelf() {
        let mut books = rows([linked("a", "/books/one.pdf")]);
        books.push(Row::link("l1".into(), "Books".into(), "s1".into(), 1));
        books.push(Row::link("l2".into(), "Comics".into(), "s2".into(), 1));
        let shelves = vec![crate::shelf::Shelf {
            id: "s1".into(),
            name: "Books".into(),
            kind: Default::default(),
            books: Vec::new(),
            parent: None,
            manual_parent: false,
        }];
        drop_dead_shelf_links(&mut books, &shelves);
        let ids: Vec<&str> = books.iter().map(Row::id).collect();
        assert_eq!(ids, vec!["a", "l1"], "only the pointer at a shelf that is gone goes");
    }

    #[test]
    fn a_duplicate_is_named_by_the_first_free_counter() {
        let in_use: std::collections::HashSet<String> =
            ["Dune", "Dune_1", "Neuromancer"].iter().map(|s| s.to_string()).collect();
        assert_eq!(duplicate_title("Dune", &in_use), "Dune_2");
        assert_eq!(duplicate_title("Neuromancer", &in_use), "Neuromancer_1");
        // Duplicating a duplicate steps instead of stacking.
        assert_eq!(duplicate_title("Dune_1", &in_use), "Dune_2");
        let stepped: std::collections::HashSet<String> = ["Dune", "Dune_1", "Dune_2"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(duplicate_title("Dune_2", &stepped), "Dune_3");
        let gaps: std::collections::HashSet<String> =
            ["Dune", "Dune_2"].iter().map(|s| s.to_string()).collect();
        assert_eq!(duplicate_title("Dune", &gaps), "Dune_1");
        assert_eq!(duplicate_title("  ", &std::collections::HashSet::new()), "Book_1");
        // The minted name survives the sanitizer via the exemption in `document_core::filename`.
        assert!(document_core::filename::is_usable_title("Dune_1"));
        assert!(document_core::filename::is_usable_title(&duplicate_title("dune", &in_use)));
    }

    #[test]
    fn duplicate_titles_count_up_1_2_3() {
        let mut in_use: std::collections::HashSet<String> =
            ["Dune"].iter().map(|s| s.to_string()).collect();
        let mut minted = Vec::new();
        for expected in ["Dune_1", "Dune_2", "Dune_3"] {
            let next = duplicate_title("Dune", &in_use);
            assert_eq!(next, expected);
            in_use.insert(next.clone());
            minted.push(next);
        }
        assert_eq!(minted, vec!["Dune_1", "Dune_2", "Dune_3"]);
        assert_eq!(duplicate_title("Dune_2", &in_use), "Dune_4");
    }

    #[test]
    fn the_storage_cap_evicts_the_least_recently_read() {
        let mut books: Vec<Row> = (0..(BOOKS_CAP + 2))
            .map(|i| {
                Row::Book(Book {
                    last_read_ms: i as u64,
                    ..linked(&format!("b{i}"), &format!("/books/{i}.pdf"))
                })
            })
            .collect();
        sanitize(&mut books);
        assert_eq!(books.len(), BOOKS_CAP);
        assert!(
            book_rows(&books).all(|b| b.last_read_ms >= 2),
            "the two never-read books go first"
        );
    }

    #[test]
    fn a_book_survives_a_round_trip_through_storage() {
        let book = Book {
            id: "b1".into(),
            fp: fp(1024, 1_700_000_000_000, 0xdead_beef),
            title: Some("Dune".into()),
            author: Some("Frank Herbert".into()),
            format: Format::Pdf,
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/store/pdf/dune_b1.pdf".into(),
            },
            added_ms: 5,
            last_read_ms: 9,
            page: 12,
            num_pages: 400,
            fraction: None,
            missing: true,
            fp_pending: false,
            independent: true,
            title_locked: false,
        };
        let json = serde_json::to_string(&book).unwrap();
        assert!(json.contains("\"kind\":\"stored\""), "{json}");
        assert!(json.contains("\"format\":\"pdf\""), "{json}");
        let back: Book = serde_json::from_str(&json).unwrap();
        assert_eq!(back, book);
    }

    #[test]
    fn a_blob_from_before_a_field_existed_still_loads() {
        // Every field has a default, so a row from an older build loads rather than dropping the library.
        let b: Book = serde_json::from_str(
            r#"{"id":"b1","fp":{"size":1,"mtimeMs":2,"headHash":3},
                "format":"markdown","origin":{"kind":"linked","src":"/n.md"}}"#,
        )
        .unwrap();
        assert_eq!(b.page, 1);
        assert!(!b.missing);
        assert_eq!(b.title, None);
        assert_eq!(b.path(), "/n.md");
    }

    #[test]
    fn the_mutable_book_lookup_steps_over_a_link() {
        let mut list = vec![
            Row::link("l1".into(), "Dune".into(), "b1".into(), 1),
            Row::Book(linked("b1", "/a.pdf")),
        ];
        // A writer of a book's facts must not reach a link row.
        assert!(find_row_mut(&mut list, "l1").is_some());
        assert!(find_book_mut(&mut list, "l1").is_none());
        find_book_mut(&mut list, "b1").unwrap().missing = true;
        assert!(find_by_id(&list, "b1").is_some_and(|b| b.missing));
        assert!(find_book_mut(&mut list, "gone").is_none());
    }

    #[test]
    fn a_stored_book_knows_the_address_it_was_copied_from() {
        let copy = Book::new(
            "b1".into(),
            fp(1, 1, 1),
            Format::Pdf,
            Origin::Stored {
                src: Some("/src/a.pdf".into()),
                store: "/store/b1.pdf".into(),
            },
            1,
        );
        assert!(copy.origin.is_store_copy_of("/src/a.pdf"));
        assert!(!copy.origin.is_store_copy_of("/store/b1.pdf"), "the copy is not its own provenance");
        assert!(!copy.origin.is_store_copy_of("/src/other.pdf"));
        // A linked book is the address, and a copy whose source is gone has no provenance to match.
        assert!(!linked("b2", "/src/a.pdf").origin.is_store_copy_of("/src/a.pdf"));
        let orphan = Book::new(
            "b3".into(),
            fp(1, 1, 1),
            Format::Pdf,
            Origin::Stored {
                src: None,
                store: "/store/b3.pdf".into(),
            },
            1,
        );
        assert!(!orphan.origin.is_store_copy_of("/src/a.pdf"));
    }

    #[test]
    fn adopting_a_measurement_makes_the_copy_the_identity() {
        let mut b = linked("b1", "/src/a.pdf");
        b.fp_pending = true;
        b.adopt_measurement(Some(fp(99, 5, 3)));
        assert_eq!(b.fp, fp(99, 5, 3));
        assert!(!b.fp_pending);
        // Adoption answers "what is this instance", not "is the address there";
        // callers that made the bytes their own clear `missing` themselves.
        assert!(!b.missing);
    }

    #[test]
    fn a_copy_that_could_not_be_weighed_stays_pending() {
        // A fingerprint nobody measured would be a guess every later rescan
        // trusts; a guess that matched nothing is a book added twice.
        let mut b = linked("b1", "/src/a.pdf");
        b.adopt_measurement(None);
        assert!(b.fp_pending);
        assert_eq!(b.fp, fp(10, 1, 7), "the identity it had is left alone");
    }

    #[test]
    fn a_departure_keeps_the_name_the_shelf_showed() {
        // The store file is named after the row's id, so an untitled row would
        // read as "b1c2d3" the moment it left.
        let mut b = linked("b1", "/src/Dune.pdf");
        b.become_stored("/src/Dune.pdf", "/store/b1.pdf".into(), Some(fp(9, 9, 9)));
        assert_eq!(b.title.as_deref(), Some("Dune"));
        assert_eq!(
            b.origin,
            Origin::Stored {
                src: Some("/src/Dune.pdf".into()),
                store: "/store/b1.pdf".into()
            }
        );
        assert_eq!(b.fp, fp(9, 9, 9), "the copy's own measurement is the identity");
        assert!(!b.fp_pending);
        assert!(!b.missing);
        // The opened address is the copy's now and the source is provenance,
        // leaving the original fingerprint free for the folder that reads it.
        assert_eq!(b.path(), "/store/b1.pdf");
        assert_eq!(b.origin.source(), Some("/src/Dune.pdf"));
    }

    #[test]
    fn a_title_the_document_gave_survives_a_departure() {
        let mut b = linked("b1", "/src/a.pdf");
        b.title = Some("Dune".to_string());
        b.become_stored("/src/a.pdf", "/store/b1.pdf".into(), None);
        assert_eq!(b.title.as_deref(), Some("Dune"), "a name is not a gap");
        assert!(b.fp_pending, "a copy nobody weighed is still owed its first check");
    }

    #[test]
    fn healing_an_address_brings_a_missing_book_back() {
        // The first walk that finds the file replaces the migrated row's
        // placeholder; until then every watched folder's rescan is held off.
        let mut b = linked("b1", "/src/a.pdf");
        b.missing = true;
        b.fp_pending = true;
        b.heal(fp(40, 9, 2));
        assert_eq!(b.fp, fp(40, 9, 2));
        assert!(!b.missing);
        assert!(!b.fp_pending);
    }
}
