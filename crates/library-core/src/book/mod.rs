//! One book: its identity, where its bytes live, and where the reader left off.
//!
//! This file is the schema — [`Fingerprint`], [`Origin`], [`Book`] and the [`Row`]
//! a library's list actually holds — and the rules that act on rows live in the
//! modules beside it, one file per question:
//!
//! | module | the question |
//! | --- | --- |
//! | [`read`] | where the reader left off, and how a read is written down |
//! | [`naming`] | what a book is called, and what to call the second one |
//! | [`merge`] | how two books that turn out to be one fold back into one |
//! | [`check`] | what a walk's measurement does to the rows it lands on |
//! | [`query`] | looking a row up by id, by address, or by either |
//! | [`sanitize`] | making a persisted list of rows internally valid |
//!
//! Everything is re-exported here, so a caller says `book::find_row` and not which
//! file of this module it happens to live in: the split is this crate's business
//! and the address is the library's.
//!
//! A book is the library's row and the reader's resume point at once — the
//! page, the page count and the reflowable stream fraction used to live on a
//! separate "recent books" record, which meant the same path was written down
//! twice and the two copies drifted (import a folder and a book had an
//! address with no resume point; open it and the resume point had no shelf).
//! One record ends that: [`Book`] carries both, and opening a document is an
//! update to the book rather than an insert into a second list.

use serde::{Deserialize, Serialize};

use reader_core::format::Format;

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

/// Storage guard on the library's size. This is NOT a "recent books" cap — a
/// library is a collection, and trimming it to twenty would drop books the
/// user imported on purpose. It is the guard that keeps the persisted blob
/// (`blob::LibraryBlob`, one localStorage key) inside a browser's quota:
/// two thousand rows of paths and titles is a few hundred kilobytes. Past it
/// the least-recently-read books go, which is the only eviction order that
/// cannot drop something the reader is in the middle of.
pub const BOOKS_CAP: usize = 2000;

/// A book's content identity: what a rescan compares a file on disk against.
///
/// `size` + `mtime_ms` is what a move preserves, so a file dragged to another
/// folder inside a watched tree still resolves to the book it already is —
/// that is the [`ledger::ScanAction::Relink`](crate::ledger::ScanAction) case.
/// `head_hash` is the cheap disambiguator for the collision that matters (two
/// different books of exactly the same length, touched in the same
/// millisecond): FNV-1a over the first 8 KiB, see [`crate::hash`].
///
/// Deliberately NOT a whole-file hash. A library is scanned on every window
/// focus, and reading 2 GB of PDFs to answer "is this the book I placed last
/// week" is not a trade worth making; the first 8 KiB of a PDF is its header
/// and the start of its first object, which is where two books differ most.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub size: u64,
    /// Modification time in milliseconds since the Unix epoch. Files whose
    /// stamp predates the epoch (or a filesystem that reports none) land on 0
    /// rather than on a wrapped value — [`crate::hash`] owns that conversion.
    pub mtime_ms: u64,
    pub head_hash: u32,
}

impl Fingerprint {
    /// The fingerprint of a file's first bytes and metadata. One call site per
    /// side of the wire: the shell's folder walk and the shell's copy, so a
    /// linked book and its stored twin agree.
    pub fn of(size: u64, mtime_ms: u64, head: &[u8]) -> Self {
        Self {
            size,
            mtime_ms,
            head_hash: crate::hash::head_hash(head),
        }
    }

    /// A stand-in for a book the library knows by address only: a row migrated
    /// from the `v1` schema, which stored a path and a resume point and
    /// measured nothing. Derived from the address so two different books can
    /// never share one, and stamped with `mtime_ms == 0` so it reads as what
    /// it is — not a measurement.
    ///
    /// Replaced by [`Fingerprint::of`] the first time the file is checked or
    /// read; [`Book::fp_pending`] says which books are still waiting for that.
    pub fn placeholder(path: &str) -> Self {
        let bytes = path.as_bytes();
        Self {
            size: bytes.len() as u64,
            mtime_ms: 0,
            head_hash: crate::hash::head_hash(bytes),
        }
    }
}

/// How the app holds a book's bytes.
///
/// The library was path-addressed before this existed — `open_path` hands the
/// raw path to the pdf.js bridge and the reflowable formats read it through
/// the shell's `read_file_text`, so nothing was ever copied into the app.
/// [`Origin::Linked`] is that mode, and it stays the default. [`Origin::Stored`]
/// is the new one: a copy inside the app's own store, so a book survives the
/// folder it came from being renamed, moved or deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Origin {
    /// Read in place: the book IS this path. The app never moves, renames or
    /// deletes it, and a vanished path is a [`Book::missing`] book with a
    /// Relink affordance — never a silent removal.
    Linked { src: String },
    /// Copied into the app's store. `src` is provenance: what the copy was
    /// made from, kept so a Relink can offer to copy again from a source that
    /// is still there. `None` for a book whose source was already gone.
    Stored { src: Option<String>, store: String },
}

impl Origin {
    /// The path the reader opens: the source for a linked book, the store copy
    /// for a stored one. This is the address every other layer speaks — the
    /// cover cache keys on it, the resume point is looked up by it, and the
    /// shell's `read_file_*` gate is asked about it.
    pub fn path(&self) -> &str {
        match self {
            Origin::Linked { src } => src,
            Origin::Stored { store, .. } => store,
        }
    }

    /// Where the bytes came from, when the app knows. Differs from [`path`]
    /// only for a stored book, and is what a relink-to-source offer reads.
    pub fn source(&self) -> Option<&str> {
        match self {
            Origin::Linked { src } => Some(src),
            Origin::Stored { src, .. } => src.as_deref(),
        }
    }

    /// True when the app owns the bytes and may delete them with the book.
    pub fn is_stored(&self) -> bool {
        matches!(self, Origin::Stored { .. })
    }

    /// Whether this is the library's own copy OF `path` — a stored book whose
    /// recorded provenance is that address.
    ///
    /// The question the conflict sheet's two dissolving answers ask before they
    /// write a folder's moved-out log, and the reason it is a method: the log is
    /// only honest when the survivor really is a copy of the file the dissolving
    /// row read. A merge of two books that merely rhyme by name writes no log,
    /// because there the file's own book is exactly what a later import should
    /// bring back — and a caller that spelled the match out would have to get
    /// the linked arm right too, where the answer is always no.
    pub fn is_store_copy_of(&self, path: &str) -> bool {
        matches!(self, Origin::Stored { src: Some(src), .. } if src == path)
    }
}

/// One book on a shelf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    /// Stable identity, independent of the address: a book keeps its id
    /// through a relink, a rename and a move between shelves. This is the
    /// drag payload and the shelf member.
    pub id: String,
    /// Content identity — what a rescan matches against. See [`Fingerprint`].
    pub fp: Fingerprint,
    /// The display name: the reader's own when they renamed the book
    /// ([`Book::title_locked`]), else the one captured at open time (a
    /// trustworthy `/Title`, else the file stem). `None` until one of the two
    /// happens, which is why [`Book::title`] falls back to the stem rather
    /// than storing a guess.
    #[serde(default)]
    pub title: Option<String>,
    /// Author, when the document or the import supplied one.
    #[serde(default)]
    pub author: Option<String>,
    pub format: Format,
    pub origin: Origin,
    /// When the book joined the library, in milliseconds since the epoch —
    /// the "Date added" sort's key.
    #[serde(default)]
    pub added_ms: u64,
    /// When the reader last opened it. `0` for a book imported but never read,
    /// which sorts last rather than first.
    #[serde(default)]
    pub last_read_ms: u64,
    /// 1-based page the reader last reached — the resume point.
    #[serde(default = "default_page")]
    pub page: u32,
    /// Total page count, for the "page X of Y" hint. 0 when unknown.
    #[serde(default)]
    pub num_pages: u32,
    /// Fractional position (0..=1) inside the continuous reading of a
    /// reflowable document. Written only while stream mode is live; `None`
    /// everywhere else, where the page above is the whole truth.
    #[serde(default)]
    pub fraction: Option<f64>,
    /// The address no longer resolves. Set by a path check, never by a scan:
    /// a missing book keeps its row and its shelf membership so a Relink can
    /// heal it, and is only dropped when the reader says so.
    #[serde(default)]
    pub missing: bool,
    /// The fingerprint is a placeholder derived from the address, not a
    /// measurement of the file — the mark a migrated book carries until the
    /// first path check replaces it. A rescan must not run while any book
    /// carries one (see [`crate::blob::LibraryBlob::awaiting_check`]): real
    /// fingerprints compared against placeholders match nothing, so a folder
    /// holding books the reader already has would add them all again.
    #[serde(default)]
    pub fp_pending: bool,
    /// This row is a book of its own rather than a second name for the file it
    /// reads — the mark the app's conflict sheet puts on a copy the reader
    /// answered *as new* for.
    ///
    /// Two rows of one address are normally twins: the reading position is a
    /// fact about the FILE, so a read and a path check write every row at the
    /// address, and the highlights and the cover are keyed by the address
    /// itself. An independent row opts out of the first half of that — its
    /// resume point is its own, and its marks are its own — every row's
    /// highlights are keyed by its id, so a removal of either row cannot take
    /// the other's with it. It does NOT opt out of the address's fate: whether
    /// the file resolves is still a fact about the file, so [`apply_check`]
    /// writes every row at the address whatever this says.
    ///
    #[serde(default)]
    pub independent: bool,
    /// The title is the reader's own words rather than the document's: a name
    /// typed into the shelf's rename, or one the conflict sheet minted and the
    /// reader accepted. [`crate::book::sanitize`] reads it as a promise — the
    /// rule that drops a title shaped like a filename exists to heal download
    /// debris a document supplied, and a name a person chose is not debris,
    /// whatever it looks like. A title the open pipeline captured from the
    /// document leaves the flag off and keeps answering to the rule.
    #[serde(default)]
    pub title_locked: bool,
}

fn default_page() -> u32 {
    1
}

/// One row of the library's list: a [`Book`], or a [`Row::Link`] that points at
/// one.
///
/// The list is what a shelf holds and what the "All" level renders, and it
/// used to be a list of books — which is why a second copy of one file had to
/// be a second BOOK, with a fingerprint, a resume point, a path check and a
/// cover of its own, and every rule about twins had to be written down twice
/// over. A link is the row that is not a book: it has a name and a target and
/// nothing else, so it is invisible to every content check the library runs —
/// a fingerprint scan, a path check, a folder rescan, a name collision — and
/// clicking it takes the reader to the book it points at, wherever that is
/// filed.
///
/// Tagged on the wire (`{"kind":"book"…}` / `{"kind":"link"…}`) rather than
/// flattened into one struct with optional halves, because a link with a
/// fingerprint of zero and a book with an empty target are both rows a reader
/// could never have meant and a parser should refuse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Row {
    /// A book: an address, a fingerprint, a resume point.
    Book(Book),
    /// A pointer at a book. Not a book: no fingerprint, no page, no address of
    /// its own and no cover, and nothing in the library measures it, checks it
    /// or collides with it. Its name is the name it shows, which is the
    /// target's own at the moment the link was made — a link is a row a reader
    /// can recognise, not a live view of a title that may be renamed later.
    #[serde(rename_all = "camelCase")]
    Link {
        id: String,
        name: String,
        /// The id of the book row this points at.
        target: String,
        #[serde(default)]
        added_ms: u64,
    },
}

impl Row {
    /// A link row. The id is minted by the caller, like every other row's.
    pub fn link(id: String, name: String, target: String, added_ms: u64) -> Self {
        Row::Link {
            id,
            name,
            target,
            added_ms,
        }
    }

    /// The row's identity. Shelves hold these, a drag payload carries one, and
    /// a removal names one — for a link exactly as for a book.
    pub fn id(&self) -> &str {
        match self {
            Row::Book(b) => &b.id,
            Row::Link { id, .. } => id,
        }
    }

    /// The name the shelf shows, and the name a collision compares. A book's
    /// is its own title or the stem of its address ([`Book::title`], never
    /// empty); a link's is the name it was made with.
    pub fn display_name(&self) -> String {
        match self {
            Row::Book(b) => b.title(),
            Row::Link { name, .. } => name.clone(),
        }
    }

    pub fn is_link(&self) -> bool {
        matches!(self, Row::Link { .. })
    }

    /// The book this row is, or `None` for a link — the one question every
    /// content rule in the library asks before it looks at a row.
    pub fn book(&self) -> Option<&Book> {
        match self {
            Row::Book(b) => Some(b),
            Row::Link { .. } => None,
        }
    }

    /// The same, for a write.
    pub fn as_book_mut(&mut self) -> Option<&mut Book> {
        match self {
            Row::Book(b) => Some(b),
            Row::Link { .. } => None,
        }
    }

    /// Content identity, and the reason a link can never enter a content
    /// check: it has none.
    pub fn fp(&self) -> Option<Fingerprint> {
        match self {
            Row::Book(b) => Some(b.fp),
            Row::Link { .. } => None,
        }
    }

    /// The id of the book a link points at.
    pub fn target(&self) -> Option<&str> {
        match self {
            Row::Link { target, .. } => Some(target),
            Row::Book(_) => None,
        }
    }

    /// When the row joined the library — the "Date added" sort's key, which a
    /// link has and a book has.
    pub fn added_ms(&self) -> u64 {
        match self {
            Row::Book(b) => b.added_ms,
            Row::Link { added_ms, .. } => *added_ms,
        }
    }
}

/// The book rows of a list, in order. Every content rule in the library walks
/// this rather than the list: a link has no fingerprint to compare, no address
/// to check and no resume point to write.
pub fn book_rows(rows: &[Row]) -> impl Iterator<Item = &Book> {
    rows.iter().filter_map(Row::book)
}

/// The same, for a write.
pub fn book_rows_mut(rows: &mut [Row]) -> impl Iterator<Item = &mut Book> {
    rows.iter_mut().filter_map(Row::as_book_mut)
}

/// The row an id names, whatever kind it is. What a shelf render, a drag and a
/// removal ask; [`find_by_id`] is the question a content rule asks instead.
pub fn find_row<'a>(rows: &'a [Row], id: &str) -> Option<&'a Row> {
    rows.iter().find(|r| r.id() == id)
}

/// The same, for a write.
pub fn find_row_mut<'a>(rows: &'a mut [Row], id: &str) -> Option<&'a mut Row> {
    rows.iter_mut().find(|r| r.id() == id)
}

impl Book {
    /// A book just joined to the library: its identity, its content identity,
    /// its pipeline, the address it is read from, and the moment it joined —
    /// everything else is the blank a reader fills in later.
    ///
    /// One constructor rather than a struct literal per importing path, so a
    /// joined book starts at page 1, with no title of its own and not missing,
    /// in every one of them. The paths that mint a row — a folder walk, a
    /// handful of loose files, a restore and a hand-open ([`record_read`]) —
    /// override only the fields they have an answer for: an open has measured
    /// nothing, so it stamps a [`Fingerprint::placeholder`] and the
    /// [`Book::fp_pending`] mark on top.
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

    /// The address this book is read from. See [`Origin::path`].
    pub fn path(&self) -> &str {
        self.origin.path()
    }

    /// The name to show: the document's own title, else the file stem, else
    /// the address. Never empty — a card with no name is a card the reader
    /// cannot tell from its neighbour.
    ///
    /// The stem a STORED book falls back to is its SOURCE's — the file the
    /// reader imported — and not its store address's: the store names every
    /// book's bytes `source.<ext>` inside the folder keyed by the id
    /// (`library_core::store`), so a fallback that read the address would call
    /// every imported book "source", which is a layout artifact rather than a
    /// name. A copy whose provenance is unknown has nothing better and keeps
    /// the address's own stem.
    pub fn title(&self) -> String {
        crate::text::display_or_stem(self.title.as_deref(), self.name_source())
    }

    /// The human-readable stem of this book's address — the file's own name
    /// without its extension, which is the name a book with no title of its
    /// own shows and the name a collision compares. [`stem_of`] over the same
    /// address [`Book::title`] falls back to.
    pub fn stem(&self) -> String {
        stem_of(self.name_source())
    }

    /// The address whose stem is this book's display fallback: the source the
    /// bytes came from for a stored book that knows one, its own address
    /// everywhere else. One spelling so [`Book::title`] and [`Book::stem`]
    /// cannot fall back to different names.
    fn name_source(&self) -> &str {
        match &self.origin {
            Origin::Stored { src: Some(src), .. } => src,
            origin => origin.path(),
        }
    }

    /// The author line, when there is one to show.
    pub fn author(&self) -> Option<String> {
        crate::text::non_blank(self.author.as_deref()).map(str::to_string)
    }

    /// Reading progress as a fraction of the document, when the page count is
    /// known. What the card's bar and the list row's percentage both draw.
    pub fn progress(&self) -> Option<f64> {
        if self.num_pages == 0 {
            return self.fraction.filter(|f| (0.0..=1.0).contains(f));
        }
        Some((self.page.min(self.num_pages) as f64 / self.num_pages as f64).clamp(0.0, 1.0))
    }

    /// Take a measurement of THIS book's own bytes as its identity.
    ///
    /// The copy half of the library's two measurements. A stored row is the
    /// library's own instance of a file, so its fingerprint is the copy's and
    /// the source address's stays free for whatever folder reads it — which is
    /// the whole of why a departure can leave a book on its shelf and still
    /// hand the OS file back to its folder's ledger.
    ///
    /// A copy that could not be weighed leaves [`Book::fp_pending`] set rather
    /// than wearing a fingerprint that is not its own: the startup sweep
    /// measures the store path and finishes the job, and every watched folder's
    /// rescan is held off until it does, because a real fingerprint compared
    /// against a placeholder matches nothing and would re-add the book.
    ///
    /// Deliberately does NOT clear [`Book::missing`]. "What is this instance"
    /// and "is the address there" are two questions, and this answers only the
    /// first; a caller that has just made the bytes its own says so itself.
    pub fn adopt_measurement(&mut self, measured: Option<Fingerprint>) {
        match measured {
            Some(fp) => {
                self.fp = fp;
                self.fp_pending = false;
            }
            None => self.fp_pending = true,
        }
    }

    /// Make this row the library's own copy of the file it reads.
    ///
    /// The write half of a departure, and the one rule both conversions ride —
    /// one book leaving the ground that made it, and a whole shelf of them
    /// leaving when a folder is re-imported as copies. Everything the reader put
    /// into the row travels with it:
    ///
    ///   * the visible name moves into [`Book::title`], because the store file is
    ///     named after the row's id and a shelf reading "b1c2d3" is a shelf that
    ///     renamed the book;
    ///   * the COPY's measurement becomes the identity, so the original
    ///     fingerprint is left free for whatever folder still reads the source
    ///     file — which is the whole of what a departure is for;
    ///   * [`Book::missing`] clears, because an address that stopped resolving is
    ///     no longer this row's problem once the bytes are the app's own. A row
    ///     left missing through a conversion would be a card offering to find a
    ///     file the library already holds.
    ///
    /// The id, the format and the resume point ride the row untouched: a
    /// conversion changes where the bytes live, not what the reader was reading.
    /// A measurement that failed leaves [`Book::fp_pending`] set rather than
    /// blocking the conversion — the startup sweep measures the store path and
    /// finishes the job.
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

    /// The address resolved, and this is what was at it.
    ///
    /// The heal half, and the one every path check and every folder walk goes
    /// through: the row's identity becomes the measurement, the placeholder
    /// mark goes, and the address is not missing. A row migrated from the `v1`
    /// schema carries a [`Fingerprint::placeholder`] that no measurement ever
    /// matched, so this is also what stops a rescan from adding a second copy of
    /// a book the library has always had — see [`apply_check`].
    pub fn heal(&mut self, fp: Fingerprint) {
        self.fp = fp;
        self.fp_pending = false;
        self.missing = false;
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The rows a list of books makes: the library's list holds rows, and a
    /// link is a row too — the tests that build one say so.
    fn rows(books: impl IntoIterator<Item = Book>) -> Vec<Row> {
        books.into_iter().map(Row::Book).collect()
    }

    /// The book a row holds. Every row these tests build is a book unless the
    /// test is about a link.
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
        // A reflowable stream has no pages: its fraction is the truth.
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
        // A blank author is no author at all.
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
        // The order the reader arranged is untouched.
        assert_eq!(at(&books, 0).id, "a");
        assert_eq!(at(&books, 1).page, 42);
        assert_eq!(at(&books, 1).last_read_ms, 500);
        assert_eq!(at(&books, 1).title.as_deref(), Some("Two"));
        // A read is not a measurement: the fingerprint is whatever the last
        // path check found, and this book has already been checked.
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
        // The reader proved the file opens and nothing more, so the identity is
        // a placeholder until the path check that follows measures it.
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
        // The reader hands over whatever the document said; the library is the
        // thing that has to stay valid, so the clamping happens on the way in
        // rather than at every read of the row.
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
        // A second pass over an unchanged file changes nothing, so a startup
        // check of a healthy library writes no state at all.
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
        // The second pass is not news.
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
        // A duplicate the reader chose to keep is a second ROW, not a second
        // file: the address is read, checked and resumed as one, and a heal
        // that reached only the first row would leave its twin holding a
        // watched folder's rescan off forever.
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
        // The duplicate's own name is a value, not a gap: the shared read
        // fills the first row's title and leaves the second row's alone.
        assert_eq!(at(&books, 0).title.as_deref(), Some("Dune"));
        assert_eq!(at(&books, 1).title.as_deref(), Some("dune_1"));
        assert_eq!(
            apply_check(&mut books, &check("/books/dune.pdf", true, 20, 2, 8)).len(),
            2,
            "one measurement heals every row at the address"
        );
        assert!(book_rows(&books).all(|b| !b.fp_pending && b.fp == fp(20, 2, 8)));
    }

    // -------------------------------------------------------------------
    // A book of its own: what independence takes and what it leaves.
    // -------------------------------------------------------------------

    fn private(id: &str, path: &str) -> Book {
        Book {
            independent: true,
            ..linked(id, path)
        }
    }

    #[test]
    fn two_rows_of_one_file_are_two_books_with_two_ids() {
        // The marks are keyed by row id, so "removing one of the two takes
        // nothing from the other" is a property of the storage rather than of a
        // key a private row had to be given. What the crate owes is that the two
        // rows really are two ids, and that a shared one and a private one at the
        // same address do not resolve to each other.
        let shared = linked("a", "/books/dune.pdf");
        let own = private("b", "/books/dune.pdf");
        assert_eq!(shared.id, "a");
        assert_eq!(own.id, "b");
        assert_ne!(shared.id, own.id, "two rows, two keys, two mark lists");
        assert!(own.independent && !shared.independent);
        // An import resolving this address never lands on the private row, which
        // is what keeps a reader's own book off a shelf the question never
        // mentioned — `add_book`'s rule rather than the address lookup's, since
        // `find_by_path` answers with whichever row the list holds first.
        // `shared` is the arrival: same content as the private row, so a dedupe
        // that did not skip private rows would resolve onto it and file the
        // reader's own book on a shelf the import never mentioned.
        let mut list = rows([own.clone()]);
        assert_eq!(add_book(&mut list, shared.clone()), "a", "a private row holds nothing back");
        assert_eq!(list.len(), 2, "so the arrival joins as a book of its own");
        // A shared row beside the private one is the answer instead, whichever
        // order the list happens to hold them in.
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
        // Two rows of one file share their position — unless one of them is a
        // book of its own, whose position is the reader's answer for THAT book
        // and is not a fact about the file.
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
        // A path check is the address's fate rather than the reader's, so it
        // still writes every row: a private book of a file that resolved is
        // not missing, and one of a file that did not is.
        assert_eq!(
            apply_check(&mut books, &check("/books/dune.pdf", true, 20, 2, 8)).len(),
            2
        );
        assert!(book_rows(&books).all(|b| !b.missing && b.fp == fp(20, 2, 8)));
    }

    #[test]
    fn an_open_that_names_no_row_treats_the_address_as_one_book() {
        // An address whose rows are all books of their own, opened by a route
        // that carries no id — a drop, an "open with". Independence is an
        // answer to "which row did the reader mean", and an open that cannot
        // ask it falls back to the address, which is the rule it has always
        // followed. The alternative is a second row minted for a file the
        // library already holds.
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
        // Naming one of them is the question the address cannot answer, and
        // the answer is that book alone.
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
        // No row named: every shared row at the address, and the private one
        // is not a twin of anything.
        assert_eq!(rows_for_read(&books, None, "/books/dune.pdf"), vec![0, 2]);
        // A shared row named: its twins, not just itself — the id says which
        // book the reader opened, and a shared book's position is not its own.
        assert_eq!(rows_for_read(&books, Some("c"), "/books/dune.pdf"), vec![0, 2]);
        // A private row named: itself, and nothing else at the address.
        assert_eq!(rows_for_read(&books, Some("b"), "/books/dune.pdf"), vec![1]);
        // A row of another address is no row of this one, and a row that went
        // is no row at all: the address answers either way.
        assert_eq!(rows_for_read(&books, Some("d"), "/books/dune.pdf"), vec![0, 2]);
        assert_eq!(rows_for_read(&books, Some("zzz"), "/books/dune.pdf"), vec![0, 2]);
        assert!(rows_for_read(&books, None, "/books/nothing.pdf").is_empty());
    }

    #[test]
    fn a_read_that_names_its_row_writes_that_row() {
        let mut books = rows([linked("a", "/books/dune.pdf"), private("b", "/books/dune.pdf")]);
        let point = ReadPoint { page: 240, num_pages: 400, fraction: None };
        // Naming a shared row is the address's rule: every shared row moves.
        assert!(record_read_row(&mut books, "a", "/books/dune.pdf", None, None, point, 9).is_none());
        assert_eq!(at(&books, 0).page, 240);
        assert_eq!(at(&books, 1).page, 1, "the private row is not a twin of it");
        // Naming the private row moves that row and nothing else.
        let further = ReadPoint { page: 380, num_pages: 400, fraction: None };
        assert!(record_read_row(&mut books, "b", "/books/dune.pdf", None, None, further, 11).is_none());
        assert_eq!(at(&books, 1).page, 380);
        assert_eq!(at(&books, 1).last_read_ms, 11);
        assert_eq!(at(&books, 0).page, 240, "the shared row keeps the read it was given");
        // A row that went while the document was open falls back to the
        // address, rather than dropping the read or minting a row.
        assert!(record_read_row(&mut books, "zzz", "/books/dune.pdf", None, None, point, 12).is_none());
        assert_eq!(at(&books, 0).page, 240);
        assert_eq!(books.len(), 2);
        // And an address nothing holds still gains a book, whichever way in.
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
        // Named: that row's own truth, clamped the way the address's lookups
        // clamp it.
        assert_eq!(resume_point(&books, Some("b"), "/books/dune.pdf"), (240, Some(0.5)));
        assert_eq!(resume_point(&books, Some("a"), "/books/dune.pdf"), (12, None));
        // Not named: the address's first row, which is the rule every open
        // that arrives as a path has always followed.
        assert_eq!(resume_point(&books, None, "/books/dune.pdf"), (12, None));
        assert_eq!(resume_point(&books, Some("zzz"), "/books/dune.pdf"), (12, None));
        assert_eq!(resume_point(&books, None, "/books/nope.pdf"), (1, None));
    }

    #[test]
    fn an_import_resolves_to_a_shared_row_and_never_to_a_private_one() {
        // A private book is not the library's answer to "the book for this
        // content": an import that resolved to it would file the reader's own
        // book on a shelf the question never mentioned.
        let mut books = rows([private("a", "/one/dune.pdf")]);
        let arrival = Book {
            origin: Origin::Linked { src: "/two/dune.pdf".into() },
            ..linked("new", "/two/dune.pdf")
        };
        assert_eq!(add_book(&mut books, arrival), "new", "a private row holds nothing back");
        assert_eq!(books.len(), 2);
        // A shared row beside it is the one an import resolves to.
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
        // A content only a private book holds is still held: a scan that could
        // not see it would add a second row for a file already on the shelf.
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
        // The survivor's own identity is what every membership and every
        // storage key already names, so a fold leaves all of it alone.
        assert_eq!(keep.id, "keep");
        assert_eq!(keep.path(), "/books/dune.pdf");
        // And the other way round the point still wins, because which row the
        // caller named is the caller's choice and not a coin toss per field.
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
        // A fraction beats no fraction on the same page, and a full tie keeps
        // the first — the survivor's own point.
        let plain = ReadPoint { page: 10, num_pages: 0, fraction: None };
        assert_eq!(further_point(plain, a), a);
        assert_eq!(further_point(a, plain), a);
        assert_eq!(further_point(a, a), a);
    }

    #[test]
    fn a_fold_measures_and_unloses_an_address() {
        // A placeholder yields to a measurement, and an address is dead only
        // when both rows say so — one live copy is a book that opens.
        let mut pending = linked("a", "/gone/dune.pdf");
        pending.fp = Fingerprint::placeholder("/gone/dune.pdf");
        pending.fp_pending = true;
        pending.missing = true;
        let measured = linked("b", "/books/dune.pdf");
        fold_books(&mut pending, &measured);
        assert!(!pending.fp_pending, "the merged row has been weighed");
        assert_eq!(pending.fp, measured.fp);
        assert!(!pending.missing);
        // Two placeholders stay one: nothing has been measured, so the
        // survivor's stands until the check that follows.
        let mut both = linked("a", "/gone/dune.pdf");
        both.fp_pending = true;
        both.missing = true;
        let mut also = linked("b", "/gone/dune.pdf");
        also.fp_pending = true;
        also.missing = true;
        fold_books(&mut both, &also);
        assert!(both.fp_pending && both.missing);
        // And a join stamp of zero is "never", not the epoch.
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
        // One insert at the front per file would reverse a whole folder, which is
        // the difference between a shelf that reads like the folder and one that
        // reads like its mirror.
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
            // Same id twice: one book, whichever address the second row wore.
            Book {
                origin: Origin::Linked {
                    src: "/copies/one.pdf".into(),
                },
                ..linked("a", "/books/one.pdf")
            },
            // Same CONTENT twice under two ids: the duplicate a reader chose
            // to keep, which a fingerprint dedupe would silently take back.
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
        // The rule that drops a filename-shaped title hunts what a document
        // supplied; a name the reader typed at the shelf's rename wears the
        // lock and is nobody's debris — however much it looks like a file.
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
        // The store names every book's bytes `source.pdf` inside the folder its
        // id keys, so the address's own stem is a layout artifact: the display
        // fallback reads the SOURCE the copy was made from.
        let stored = Book {
            origin: Origin::Stored {
                src: Some("/downloads/dune.pdf".into()),
                store: "/app/Library/items/b1/source.pdf".into(),
            },
            ..linked("b1", "/app/Library/items/b1/source.pdf")
        };
        assert_eq!(stored.title(), "dune");
        assert_eq!(stored.stem(), "dune");
        // A copy whose provenance is unknown has nothing better than its own
        // address, and keeps it.
        let orphan = Book {
            origin: Origin::Stored {
                src: None,
                store: "/app/Library/items/b2/source.pdf".into(),
            },
            ..linked("b2", "/app/Library/items/b2/source.pdf")
        };
        assert_eq!(orphan.title(), "source");
        // And a title of its own outranks every fallback.
        let named = Book {
            title: Some("Dune".into()),
            ..stored
        };
        assert_eq!(named.title(), "Dune");
    }

    #[test]
    fn a_store_stem_burnt_into_a_title_is_healed_and_a_readers_own_is_not() {
        // The open pipeline used to seed the shelf's record with the stem of
        // the address it opened, which for a stored book is the store's own
        // "source": a burn-in the load drops, so the source's stem shows
        // again. A title the reader typed is locked, and "source" is a name a
        // person may genuinely choose.
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
        // The row sweep can only ask the row list, and a shelf id is never a
        // book id: a folder link is kept, and which shelves still exist is the
        // shelf sweep's question, not this one's.
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
        // The industry's counter: the first number nobody wears.
        assert_eq!(duplicate_title("Dune", &in_use), "Dune_2");
        assert_eq!(duplicate_title("Neuromancer", &in_use), "Neuromancer_1");
        // Duplicating a duplicate steps instead of stacking: the counter is
        // not part of the name.
        assert_eq!(duplicate_title("Dune_1", &in_use), "Dune_2");
        let stepped: std::collections::HashSet<String> = ["Dune", "Dune_1", "Dune_2"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(duplicate_title("Dune_2", &stepped), "Dune_3");
        // Gaps are filled...
        let gaps: std::collections::HashSet<String> =
            ["Dune", "Dune_2"].iter().map(|s| s.to_string()).collect();
        assert_eq!(duplicate_title("Dune", &gaps), "Dune_1");
        // ...and a blank base still gets a name a shelf can show.
        assert_eq!(duplicate_title("  ", &std::collections::HashSet::new()), "Book_1");
        // The minted name survives the sanitizer's title rule — the exemption
        // in `reader_core::filename` is what makes the round trip honest.
        assert!(reader_core::filename::is_usable_title("Dune_1"));
        assert!(reader_core::filename::is_usable_title(&duplicate_title("dune", &in_use)));
    }

    #[test]
    fn duplicate_titles_count_up_1_2_3() {
        // Three of one file in a row: each duplicate takes the next free
        // counter, the way a file manager copies the same name three times.
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
        // And a duplicate OF a duplicate keeps counting on the same stem
        // rather than stacking counters.
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
        // Every field carries a default, so a row written by an older build
        // (or one hand-edited) loads rather than dropping the library.
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
        // A link is a row and `find_row_mut` answers it; a writer of a book's
        // facts must not be able to reach one this way.
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
        // A linked book IS the address rather than a copy of it, and a copy
        // whose source is already gone has no provenance to match.
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
        // An adoption answers "what is this instance", not "is an address
        // there": the two callers that also clear `missing` are the two that
        // have just made the bytes their own, and they say so themselves.
        assert!(!b.missing);
    }

    #[test]
    fn a_copy_that_could_not_be_weighed_stays_pending() {
        // A placeholder the startup sweep finishes is honest. A fingerprint
        // nobody measured would be a guess every later rescan trusts, and a
        // guess that matched nothing is a book the library adds twice.
        let mut b = linked("b1", "/src/a.pdf");
        b.adopt_measurement(None);
        assert!(b.fp_pending);
        assert_eq!(b.fp, fp(10, 1, 7), "the identity it had is left alone");
    }

    #[test]
    fn a_departure_keeps_the_name_the_shelf_showed() {
        // The store file is named after the row's id, so a row with no title of
        // its own would start reading as "b1c2d3" the moment it left.
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
        // The address the reader opens is the copy's now, and the source is
        // provenance — which is what leaves the original fingerprint free for
        // the folder that still reads it.
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
        // A migrated row carries a placeholder, and the first walk that finds
        // the file is what replaces it: until then every watched folder's
        // rescan is held off, because a real fingerprint matches no placeholder.
        let mut b = linked("b1", "/src/a.pdf");
        b.missing = true;
        b.fp_pending = true;
        b.heal(fp(40, 9, 2));
        assert_eq!(b.fp, fp(40, 9, 2));
        assert!(!b.missing);
        assert!(!b.fp_pending);
    }
}
