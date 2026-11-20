//! The library: books by id, their reading states, and what changed.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ai_pages::AiPage;
use crate::book::{Book, FileStamp, Format, Location, Origin, Position, State};
use crate::clock::{Clock, Field, Stamp};
use crate::extra::Extra;
use crate::folder::{Folder, Tombstone};
use crate::id::{AiPageId, BookId, ContentId, DeviceId, FolderId, LinkId, MarkId, ShelfId};
use crate::link::Link;
use crate::marks::Mark;
use crate::shelves::{Membership, Shelf};
use crate::tracked::Tracked;

mod parts;

pub(crate) use parts::Parts;

/// What a measured file is: its content and its size and time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measure {
    pub content: ContentId,
    pub stamp: FileStamp,
}

/// A file the reader opened, measured, with what the document says.
#[derive(Clone, Debug, PartialEq)]
pub struct Opened {
    pub path: PathBuf,
    pub measure: Measure,
    pub format: Format,
    pub title: String,
    /// The document's own author, if it names one.
    pub author: Option<String>,
}

/// What must reach disk since the last save: one row or file each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dirty {
    pub books: BTreeSet<BookId>,
    pub shelves: BTreeSet<ShelfId>,
    pub members: BTreeSet<(ShelfId, BookId)>,
    /// Gone ones included: their rows go too.
    pub folders: BTreeSet<FolderId>,
    /// Folder rows from before ledgers, adopted and now to delete.
    pub retired: BTreeSet<FolderId>,
    pub links: BTreeSet<LinkId>,
    /// `state/<id>.json`: one book's position each.
    pub states: BTreeSet<BookId>,
    pub marks: BTreeSet<MarkId>,
    pub ai_pages: BTreeSet<AiPageId>,
    /// A book's data was erased: its old words must leave the disk too.
    pub erased: bool,
}

impl Dirty {
    pub fn is_empty(&self) -> bool {
        self.store_is_clean() && self.states.is_empty()
    }

    /// Nothing for the store: only positions changed.
    pub fn store_is_clean(&self) -> bool {
        self.books.is_empty()
            && self.shelves.is_empty()
            && self.members.is_empty()
            && self.folders.is_empty()
            && self.retired.is_empty()
            && self.links.is_empty()
            && self.marks.is_empty()
            && self.ai_pages.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct Library {
    pub(crate) clock: Clock,
    pub(crate) books: Tracked<BookId, Book>,
    pub(crate) states: BTreeMap<BookId, State>,
    pub(crate) folders: Tracked<FolderId, Folder>,
    pub(crate) links: Tracked<LinkId, Link>,
    pub(crate) shelves: Tracked<ShelfId, Shelf>,
    pub(crate) members: Tracked<(ShelfId, BookId), Membership>,
    pub(crate) marks: Tracked<MarkId, Mark>,
    pub(crate) ai_pages: Tracked<AiPageId, AiPage>,
    /// Each location's path to its book; rebuilt, never saved.
    pub(crate) at: HashMap<PathBuf, BookId>,
    /// Positions to write; the maps track their own changes.
    pub(crate) states_dirty: BTreeSet<BookId>,
    /// Store copies no book reads any more, for the shell to delete.
    pub(crate) swept: Vec<PathBuf>,
    /// Old folder rows the next save deletes; see `legacy`.
    pub(crate) retired: BTreeSet<FolderId>,
    /// Changes whenever a book, shelf or folder does; never repeats.
    revision: u64,
    /// The top level's last key, once looked up; see `next_key`.
    pub(crate) home_end: Option<Option<String>>,
    /// Data erased since the last save; see [`Dirty::erased`].
    pub(crate) erased: bool,
}

/// Unique across libraries, so a loaded one never reuses a revision.
fn next_revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Library {
    /// An empty library written by `device`.
    pub fn new(device: DeviceId) -> Self {
        Self::from_parts(Parts::new(Clock::new(device)))
    }

    /// A library as it was saved; only adopted old rows need writing.
    pub(crate) fn from_parts(parts: Parts) -> Self {
        let mut library = Self {
            clock: parts.clock,
            books: Tracked::new(parts.books),
            states: parts.states,
            folders: Tracked::new(parts.folders),
            links: Tracked::new(parts.links),
            shelves: Tracked::new(parts.shelves),
            members: Tracked::new(parts.members),
            marks: Tracked::new(parts.marks),
            ai_pages: Tracked::new(parts.ai_pages),
            at: HashMap::new(),
            states_dirty: BTreeSet::new(),
            swept: Vec::new(),
            retired: BTreeSet::new(),
            revision: next_revision(),
            home_end: None,
            erased: false,
        };
        crate::legacy::adopt(&mut library, parts.legacy);
        library.index();
        library
    }

    pub(crate) fn index(&mut self) {
        self.at.clear();
        // A twin shares its original's files; paths name the original.
        let originals = self
            .books
            .values()
            .filter(|b| b.alive() && b.twin_of.value.is_none());
        for book in originals {
            for location in &book.locations {
                self.at.insert(location.path.clone(), book.id);
            }
        }
    }

    pub fn device(&self) -> DeviceId {
        self.clock.device()
    }

    /// Every book not removed, in no particular order.
    pub fn books(&self) -> impl Iterator<Item = &Book> {
        self.books.values().filter(|b| b.alive())
    }

    pub fn len(&self) -> usize {
        self.books().count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn book(&self, id: BookId) -> Option<&Book> {
        self.books.get(&id).filter(|b| b.alive())
    }

    /// The book with a location at `path`, if any.
    pub fn book_at(&self, path: &Path) -> Option<BookId> {
        self.at.get(path).copied()
    }

    pub fn position(&self, id: BookId) -> Option<Position> {
        self.states.get(&id).map(|s| s.position.value.clone())
    }

    /// The books, last opened first; ties by title.
    pub fn recent(&self) -> Vec<&Book> {
        let mut books: Vec<&Book> = self.books().collect();
        books.sort_by(|a, b| {
            let newer = b.opened.value.cmp(&a.opened.value);
            newer.then_with(|| a.title.value.cmp(&b.title.value))
        });
        books
    }

    /// Records an open: the book of that content or path, else a new one.
    pub fn record_open(&mut self, opened: Opened, wall_ms: u64) -> BookId {
        self.record_open_as(opened, wall_ms, None)
    }

    /// As [`Self::record_open`]; a twin of that book read is the one opened.
    pub fn record_open_as(&mut self, opened: Opened, wall_ms: u64, twin: Option<BookId>) -> BookId {
        let at = self.clock.tick(wall_ms);
        let content = opened.measure.content;
        let original = |b: &&Book| b.content == content && b.shared();
        let by_content = self.books().find(original).map(|b| b.id);
        // The book at the file wins while its bytes match: its own copy.
        let same = |id: &BookId| self.book(*id).is_some_and(|b| b.content == content);
        let here = self.book_at(&opened.path).filter(same);
        // A file edited in place is the same book with new content.
        let id = here.or(by_content).or_else(|| self.book_at(&opened.path));
        let twin_of = |t: &BookId| {
            self.book(*t)
                .is_some_and(|b| id.is_some() && b.twin_of.value == id)
        };
        let reader = twin.filter(twin_of);
        let id = match id.and_then(|id| self.books.get_mut(&id)) {
            Some(book) => {
                book.content = content;
                book.format = opened.format;
                if reader.is_none() {
                    book.opened = Field::new(wall_ms, at);
                }
                // Filled once; a reader's own edit outlasts reopening.
                if book.author.value.is_none() && opened.author.is_some() {
                    book.author = Field::new(opened.author.clone(), at);
                }
                book.id
            }
            None => self.add(&opened, wall_ms, at, true),
        };
        // Opening the library's copy leaves it the library's copy.
        let stored = (self.books.get(&id).map(|b| &b.locations))
            .and_then(|ls| ls.iter().find(|l| l.path == opened.path))
            .filter(|l| l.origin == Origin::Store)
            .map(|l| l.from.clone());
        let (origin, from) = match stored {
            Some(from) => (Origin::Store, from),
            None => (self.origin_of(&opened.path), None),
        };
        self.place_from(id, &opened.path, opened.measure.stamp, origin, from);
        self.mirror(id);
        if let Some(book) = reader.and_then(|t| self.books.get_mut(&t)) {
            book.opened = Field::new(wall_ms, at);
        }
        self.bump();
        id
    }

    /// A new book, or a removed one back; found unopened, it reads 0.
    pub(crate) fn add(
        &mut self,
        opened: &Opened,
        wall_ms: u64,
        at: Stamp,
        opening: bool,
    ) -> BookId {
        let id = BookId::original(opened.measure.content);
        let last = if opening { wall_ms } else { 0 };
        // A new book joins the top level at the end.
        let key = match self.books.contains_key(&id) {
            true => String::new(),
            false => self.next_key(None),
        };
        let book = self.books.entry(id).or_insert_with(|| Book {
            id,
            content: opened.measure.content,
            format: opened.format,
            title: Field::new(opened.title.clone(), at),
            author: Field::new(opened.author.clone(), at),
            added: wall_ms,
            opened: Field::new(last, at),
            key: Field::new(key, at),
            locations: Vec::new(),
            removed: None,
            restored: None,
            twin_of: Field::default(),
            independent: false,
            design: Field::default(),
            forgot: None,
            extra: Extra::default(),
        });
        if !book.alive() {
            // Removed with its data: it returns as a book never seen.
            if book.forgot.is_some() && book.forgot >= book.removed {
                book.title = Field::new(opened.title.clone(), at);
                book.author = Field::new(opened.author.clone(), at);
                book.opened = Field::new(0, at);
                book.added = wall_ms;
            }
            book.restored = Some(at);
            if opening {
                book.opened = Field::new(wall_ms, at);
            }
            // Back on purpose: folders no longer leave it out.
            let content = opened.measure.content;
            let stoned = |s: &Tombstone| !s.moved && s.content == content;
            let holding = self
                .folders
                .values_mut_where(|f| f.ignored.iter().any(stoned));
            for folder in holding {
                folder.ignored.retain(|s| !stoned(s));
            }
        }
        id
    }

    /// Gives `original`'s twins its files, content and format again.
    pub(crate) fn mirror(&mut self, original: BookId) {
        let Some(book) = self.books.get(&original) else {
            return;
        };
        let (content, format) = (book.content, book.format);
        let locations = book.locations.clone();
        let twins = self
            .books
            .values_mut_where(|b| b.twin_of.value == Some(original));
        for twin in twins {
            twin.content = content;
            twin.format = format;
            twin.locations.clone_from(&locations);
        }
    }

    /// Mirrors each original with twins that changed since the last save.
    pub(crate) fn mirror_changed(&mut self) {
        let originals = self.books.values().filter_map(|b| b.twin_of.value);
        let originals: BTreeSet<BookId> = originals.collect();
        let changed = self.books.changed();
        let changed: Vec<BookId> = originals.intersection(changed).copied().collect();
        for original in changed {
            self.mirror(original);
        }
    }

    /// Makes the path the book's newest location, and only its own.
    pub(crate) fn place(&mut self, id: BookId, path: &Path, stamp: FileStamp, origin: Origin) {
        self.place_from(id, path, stamp, origin, None);
    }

    /// As [`Self::place`]; a store copy remembers the file it came from.
    pub(crate) fn place_from(
        &mut self,
        id: BookId,
        path: &Path,
        stamp: FileStamp,
        origin: Origin,
        from: Option<PathBuf>,
    ) {
        if let Some(old) = self.at.insert(path.to_path_buf(), id)
            && old != id
            && let Some(other) = self.books.get_mut(&old)
        {
            other.locations.retain(|l| l.path != path);
        }
        let Some(book) = self.books.get_mut(&id) else {
            return;
        };
        book.locations.retain(|l| l.path != path);
        let location = Location {
            path: path.to_path_buf(),
            origin,
            stamp,
            from,
            present: true,
        };
        book.locations.insert(0, location);
    }

    /// Writes down where the reader is; false if nothing changed.
    pub fn record_position(&mut self, id: BookId, position: Position, wall_ms: u64) -> bool {
        if self.book(id).is_none() || self.position(id).as_ref() == Some(&position) {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        let field = Field::new(position, at);
        let state = self
            .states
            .entry(id)
            .or_insert_with(|| State::new(field.clone()));
        state.position = field;
        self.states_dirty.insert(id);
        true
    }

    /// Marks each location there or not; done after loading.
    pub fn check_present(&mut self, exists: impl Fn(&Path) -> bool) {
        for book in self.books.quiet_values_mut() {
            for location in &mut book.locations {
                location.present = exists(&location.path);
            }
        }
        self.bump();
    }

    /// Copies in `store` that an open marked as opened, the library's again.
    pub fn reclaim_store(&mut self, store: &Path) -> usize {
        // Only `store/<content>/<name>`, the shape copies are made in.
        let stray = |l: &Location| {
            l.origin == Origin::Opened && l.path.parent().and_then(Path::parent) == Some(store)
        };
        let mut fixed = 0;
        for book in (self.books).values_mut_where(|b| b.locations.iter().any(stray)) {
            for location in book.locations.iter_mut().filter(|l| stray(l)) {
                location.origin = Origin::Store;
                fixed += 1;
            }
        }
        if fixed > 0 {
            self.mirror_changed();
            self.bump();
        }
        fixed
    }

    /// Store copies freed by removals and replacements, to delete now.
    pub fn take_swept(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.swept)
    }

    /// Changes whenever books, shelves or folders do, not positions.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// A change worth showing, seen by [`Self::revision`].
    pub(crate) fn bump(&mut self) {
        self.revision = next_revision();
    }

    /// What changed since the last call, now to be written.
    pub fn take_dirty(&mut self) -> Dirty {
        Dirty {
            books: self.books.take_changed(),
            shelves: self.shelves.take_changed(),
            members: self.members.take_changed(),
            folders: self.folders.take_changed(),
            retired: std::mem::take(&mut self.retired),
            links: self.links.take_changed(),
            states: std::mem::take(&mut self.states_dirty),
            marks: self.marks.take_changed(),
            ai_pages: self.ai_pages.take_changed(),
            erased: std::mem::take(&mut self.erased),
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.books.has_changes()
            || self.shelves.has_changes()
            || self.members.has_changes()
            || self.folders.has_changes()
            || self.links.has_changes()
            || self.marks.has_changes()
            || self.ai_pages.has_changes()
            || !self.states_dirty.is_empty()
            || !self.retired.is_empty()
    }

    /// Marks every row, so the next save writes them all; not positions.
    pub(crate) fn mark_all(&mut self) {
        self.books.mark_all();
        self.shelves.mark_all();
        self.members.mark_all();
        self.folders.mark_all();
        self.links.mark_all();
        self.marks.mark_all();
        self.ai_pages.mark_all();
    }
}
