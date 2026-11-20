//! Removing books, twins, renames and the store copies that go with them.

use std::collections::{BTreeSet, HashMap};
use std::hash::{BuildHasher, RandomState};
use std::path::PathBuf;

use crate::arrange::Item;
use crate::book::{Book, Design, Format, Origin, Position};
use crate::clock::{Field, Stamp};
use crate::extra::Extra;
use crate::folder::Tombstone;
use crate::id::{BookId, FolderId, LinkId, ShelfId};
use crate::library::Library;
use crate::link::LinkTo;
use crate::shelves::{Membership, clean};
use crate::view::Home;

/// What becomes of the reader's own work on a removed book.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Data {
    /// Marks, place and name wait for the file to come back.
    #[default]
    Keep,
    /// They go with the book: a return lands as a new book.
    Delete,
}

/// What removing some books will do, told before it's done.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Receipt {
    pub books: usize,
    /// Books with a reading position, kept should they return.
    pub positions: usize,
    /// One book's position, when it alone goes and was started.
    pub position: Option<Position>,
    /// Spots on shelves, forgotten.
    pub spots: usize,
    /// The shelves they are filed on, by name.
    pub filed: Vec<String>,
    /// Copies in the library's own folder, deleted.
    pub copies: usize,
    pub copies_size: u64,
    /// Books a folder placed: kept out of its future imports.
    pub hidden: usize,
    pub marks: usize,
    /// Saved AI page results.
    pub ai_pages: usize,
    /// Words studied in them.
    pub words: usize,
    /// Their formats, each once, and their measured size.
    pub formats: Vec<Format>,
    pub size: u64,
}

/// What a removal takes, the cascade counted in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Removing {
    pub books: Vec<BookId>,
    /// Deepest first, so each goes before the shelf it sits in.
    pub shelves: Vec<ShelfId>,
    /// Books on the asked shelves and below, and the shelves below.
    pub inside_books: usize,
    pub inside_shelves: usize,
    /// Books read in place the removed rungs take: copies, or the folder's.
    pub copies: Vec<BookId>,
}

impl Receipt {
    /// Whether the data switch means anything: marks or a place.
    pub fn has_data(&self) -> bool {
        self.marks > 0 || self.ai_pages > 0 || self.words > 0 || self.positions > 0
    }
}

/// What removing one book does to its files; nothing if a twin stays.
struct Fate {
    copies: Vec<PathBuf>,
    folders: BTreeSet<FolderId>,
    /// A twin stays with the files: no folder leaves them out.
    kept: bool,
}

impl Library {
    /// Each book's fate when all of `removing` go together.
    fn fates(&self, removing: &BTreeSet<BookId>) -> Vec<(BookId, Fate)> {
        // A family's living members that stay: their files stay too.
        let mut staying: HashMap<BookId, usize> = HashMap::new();
        for book in self.books().filter(|b| !removing.contains(&b.id)) {
            *staying.entry(book.original()).or_default() += 1;
        }
        let books = removing.iter().filter_map(|id| self.book(*id));
        let fate = |book: &Book| {
            let mut fate = Fate {
                copies: Vec::new(),
                folders: BTreeSet::new(),
                kept: staying.contains_key(&book.original()),
            };
            if fate.kept {
                return fate;
            }
            for location in &book.locations {
                let used = |b: &Book| {
                    !removing.contains(&b.id) && b.locations.iter().any(|l| l.path == location.path)
                };
                match location.origin {
                    Origin::Store if !self.books().any(used) => {
                        fate.copies.push(location.path.clone())
                    }
                    Origin::Folder(id) => {
                        fate.folders.insert(id);
                    }
                    _ => {}
                }
            }
            fate
        };
        books.map(|b| (b.id, fate(b))).collect()
    }

    fn living(&self, ids: &[BookId]) -> BTreeSet<BookId> {
        let ids = ids.iter().copied();
        ids.filter(|id| self.book(*id).is_some()).collect()
    }

    /// The marks a book holds, removed ones aside.
    fn marks_on(&self, id: BookId) -> impl Iterator<Item = &crate::marks::Mark> {
        self.marks
            .values()
            .filter(move |m| m.alive.value && m.book.value == id)
    }

    /// What removing `ids` would do, for the reader to confirm.
    pub fn receipt(&self, ids: &[BookId]) -> Receipt {
        let removing = self.living(ids);
        let mut receipt = Receipt::default();
        let started = |id: BookId| {
            self.position(id)
                .filter(|p| p.page > 0 || p.place.is_some() || p.spine.is_some())
        };
        for (id, fate) in self.fates(&removing) {
            receipt.books += 1;
            receipt.positions += usize::from(started(id).is_some());
            receipt.copies += usize::from(!fate.copies.is_empty());
            receipt.marks += self.marks_on(id).count();
            receipt.ai_pages += self.ai_pages(id).len();
            receipt.words += self.sightings(id).len();
            let book = self.book(id);
            let placed = |c| self.folders.values().any(|f| f.placed.contains(&c));
            let in_folder =
                !fate.kept && (!fate.folders.is_empty() || book.is_some_and(|b| placed(b.content)));
            receipt.hidden += usize::from(in_folder);
            if let Some(book) = book {
                if !receipt.formats.contains(&book.format) {
                    receipt.formats.push(book.format);
                }
                let size = book.home().map_or(0, |l| l.stamp.size);
                receipt.size += size;
                if !fate.copies.is_empty() {
                    receipt.copies_size += size;
                }
            }
        }
        if let [only] = removing.iter().copied().collect::<Vec<_>>().as_slice() {
            receipt.position = started(*only);
        }
        let spot = |m: &&Membership| m.alive.value && removing.contains(&m.book);
        let spots: Vec<ShelfId> = (self.members.values().filter(spot))
            .map(|m| m.shelf)
            .filter(|s| self.shelf(*s).is_some())
            .collect();
        receipt.spots = spots.len();
        for shelf in spots.iter().filter_map(|s| self.shelf(*s)) {
            if !receipt.filed.contains(&shelf.name.value) {
                receipt.filed.push(shelf.name.value.clone());
            }
        }
        receipt
    }

    /// What removing `books` and `shelves` takes; `cascade` takes the insides.
    pub fn removing(&self, books: &[BookId], shelves: &[ShelfId], cascade: bool) -> Removing {
        let asked: Vec<ShelfId> = (shelves.iter().copied())
            .filter(|s| self.shelf(*s).is_some())
            .collect();
        let below = self.subtree(&asked);
        let mut all = asked.clone();
        all.extend(&below);
        let inside = self.books_on(&all);
        let mut out = Removing {
            books: self.living(books).into_iter().collect(),
            shelves: if cascade { all } else { asked },
            inside_books: inside.len(),
            inside_shelves: below.len(),
            copies: Vec::new(),
        };
        if cascade {
            out.books
                .extend(inside.iter().filter(|b| !books.contains(b)));
        }
        out.shelves
            .sort_by_key(|s| std::cmp::Reverse(self.path_to(*s).len()));
        for &shelf in &out.shelves {
            for book in self.rung_books(shelf, false) {
                if !out.books.contains(&book) && !out.copies.contains(&book) {
                    out.copies.push(book);
                }
            }
        }
        out
    }

    /// Removes books, tombstoning folder ones; returns how many.
    /// Freed store copies wait in [`Self::take_swept`].
    pub fn remove_books(&mut self, ids: &[BookId], data: Data, wall_ms: u64) -> usize {
        self.purge(ids, data, true, wall_ms)
    }

    /// Takes rows out with their data and no tombstone, as an answer does.
    pub(crate) fn drop_books(&mut self, ids: &[BookId], wall_ms: u64) -> usize {
        self.purge(ids, Data::Delete, false, wall_ms)
    }

    pub(crate) fn purge(&mut self, ids: &[BookId], data: Data, stone: bool, wall_ms: u64) -> usize {
        let removing = self.living(ids);
        let at = self.clock.tick(wall_ms);
        let mut done = 0;
        for (id, fate) in self.fates(&removing) {
            if stone && !fate.kept {
                self.tombstone_book(id, false, wall_ms);
            }
            self.swept.extend(fate.copies);
            if data == Data::Delete {
                self.forget_data(id, at);
            }
            let spots = self
                .members
                .values_mut_where(|m| m.book == id && m.alive.value);
            for spot in spots {
                spot.alive = Field::new(false, at);
            }
            if let Some(book) = self.books.get_mut(&id) {
                book.removed = Some(at);
                done += 1;
            }
        }
        self.promote_twins(&removing, at);
        self.drop_dangling_links(at);
        self.index();
        self.bump();
        done
    }

    /// The removal, or a move-out, written into every folder that placed it.
    pub(crate) fn tombstone_book(&mut self, id: BookId, moved: bool, wall_ms: u64) {
        let Some(book) = self.book(id) else {
            return;
        };
        let content = book.content;
        let Some(placer) = self.folders.values().find(|f| f.placed.contains(&content)) else {
            return;
        };
        let rung_of = |s: &ShelfId| {
            self.shelf(*s)
                .and_then(|s| s.kind.value.rung().map(|(f, _)| f))
        };
        let home = self
            .shelves_of(id)
            .into_iter()
            .find(|s| rung_of(s) == Some(placer.id));
        let stone = Tombstone {
            content,
            book: id,
            title: book.title.value.clone(),
            format: book.format,
            last_path: book
                .home()
                .map(|l| l.from.clone().unwrap_or_else(|| l.path.clone()))
                .unwrap_or_default(),
            shelf: home,
            removed_ms: wall_ms,
            moved,
            returned: None,
            size: book.home().map_or(0, |l| l.stamp.size),
        };
        let mut folders = self.folder_list();
        crate::ledger::tombstone(&mut folders, &stone);
        for folder in folders {
            if self.folders.get(&folder.id) != Some(&folder) {
                self.folders.insert(folder.id, folder);
            }
        }
    }

    /// Marks, AI pages, studied words and place go; it returns as new.
    fn forget_data(&mut self, id: BookId, at: Stamp) {
        // Ones removed before go too, words and all: kept only for Undo.
        for mark in self.marks.values_mut_where(|m| m.book.value == id) {
            mark.alive = Field::new(false, at);
            mark.word.clear();
            mark.context.clear();
            mark.gloss = Field::new(None, at);
            mark.note = Field::new(None, at);
        }
        for page in self.ai_pages.values_mut_where(|p| p.book.value == id) {
            page.alive = Field::new(false, at);
            page.result.clear();
            page.source.clear();
        }
        self.forget_sightings(id, at);
        if let Some(state) = self.states.get_mut(&id) {
            state.position = Field::new(
                Position {
                    page: 0,
                    pages: 0,
                    place: None,
                    spine: None,
                },
                at,
            );
            state.unlocked = Field::new(None, at);
            self.states_dirty.insert(id);
        }
        if let Some(book) = self.books.get_mut(&id) {
            book.forgot = Some(at);
        }
        self.erased = true;
    }

    /// Whether a record that died at `died` was erased with its book.
    pub(crate) fn erased(&self, book: BookId, died: Stamp) -> bool {
        let forgot = self.books.get(&book).and_then(|b| b.forgot);
        forgot.is_some_and(|f| died <= f)
    }

    /// Links whose book or shelf went go with it.
    pub(crate) fn drop_dangling_links(&mut self, at: Stamp) {
        let gone: Vec<LinkId> = (self.links.values())
            .filter(|l| l.alive())
            .filter(|l| match l.target {
                LinkTo::Book(b) => self.book(b).is_none(),
                LinkTo::Shelf(s) => self.shelf(s).is_none(),
            })
            .map(|l| l.id)
            .collect();
        for id in gone {
            if let Some(link) = self.links.get_mut(&id) {
                link.removed = Some(at);
            }
        }
    }

    /// Removes links; what they point at stays.
    pub fn remove_links(&mut self, ids: &[LinkId], wall_ms: u64) -> usize {
        let at = self.clock.tick(wall_ms);
        let mut done = 0;
        for link in self
            .links
            .values_mut_where(|l| l.alive() && ids.contains(&l.id))
        {
            link.removed = Some(at);
            done += 1;
        }
        self.bump();
        done
    }

    /// Each removed original's first living twin takes its place.
    fn promote_twins(&mut self, removed: &BTreeSet<BookId>, at: Stamp) {
        let mut heirs: HashMap<BookId, BookId> = HashMap::new();
        for book in self.books() {
            if let Some(original) = book.twin_of.value.filter(|o| removed.contains(o)) {
                heirs.entry(original).or_insert(book.id);
            }
        }
        let heir_of = |b: &Book| b.twin_of.value.is_some_and(|o| heirs.contains_key(&o));
        for book in self.books.values_mut_where(heir_of) {
            let Some(&heir) = book.twin_of.value.and_then(|o| heirs.get(&o)) else {
                continue;
            };
            book.twin_of = Field::new((book.id != heir).then_some(heir), at);
        }
    }

    /// A separate book of the same files, with its own place and shelves.
    pub fn twin(&mut self, id: BookId, wall_ms: u64) -> Option<BookId> {
        let book = self.book(id)?;
        let original = self.book(book.original())?.clone();
        let title = self.copy_title(&book.title.value);
        let at = self.clock.tick(wall_ms);
        let twin = self.new_book_id(at);
        let book = Book {
            id: twin,
            title: Field::new(title, at),
            author: Field::new(original.author.value.clone(), at),
            added: wall_ms,
            opened: Field::new(0, at),
            removed: None,
            restored: None,
            twin_of: Field::new(Some(original.id), at),
            independent: false,
            design: Field::default(),
            forgot: None,
            extra: Extra::default(),
            ..original
        };
        self.books.insert(twin, book);
        // Right behind the book pointed at, on top and on each of its shelves.
        let shelves = self.shelves_of(id).into_iter().map(Some);
        for level in std::iter::once(None).chain(shelves) {
            if let Some(shelf) = level {
                self.file(twin, shelf, wall_ms);
            }
            let order = self.arranged(level, Home::Everything);
            let others = order.into_iter().filter(|i| *i != Item::Book(twin));
            let before = others.skip_while(|i| *i != Item::Book(id)).nth(1);
            self.arrange(level, &[Item::Book(twin)], before, wall_ms);
        }
        self.bump();
        Some(twin)
    }

    /// `Title (n)` with the lowest `n` from 2 no living book has.
    fn copy_title(&self, title: &str) -> String {
        let base = strip_number(title);
        let taken = |name: &str| self.books().any(|b| b.title.value == name);
        let mut n = 2_u32;
        loop {
            let name = format!("{base} ({n})");
            if !taken(&name) || n == u32::MAX {
                return name;
            }
            n += 1;
        }
    }

    pub(crate) fn new_book_id(&self, at: Stamp) -> BookId {
        let state = RandomState::new();
        let seed = (at.ms, at.counter, self.device().0, "book");
        let half = |salt: u8| u128::from(state.hash_one((seed, salt)));
        let mut id = BookId(half(1) << 64 | half(2));
        while self.books.contains_key(&id) {
            id = BookId(id.0.wrapping_add(1));
        }
        id
    }

    /// Renames a book; false if blank or the same.
    pub fn rename_book(&mut self, id: BookId, title: &str, wall_ms: u64) -> bool {
        let Some(title) = clean(title) else {
            return false;
        };
        if self.book(id).is_none_or(|b| b.title.value == title) {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        if let Some(book) = self.books.get_mut(&id) {
            book.title = Field::new(title, at);
        }
        self.bump();
        true
    }

    /// Sets a book's own design choice; none follows the reader's.
    pub fn set_design(&mut self, id: BookId, design: Option<Design>, wall_ms: u64) -> bool {
        if self.book(id).is_none_or(|b| b.design.value == design) {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        if let Some(book) = self.books.get_mut(&id) {
            book.design = Field::new(design, at);
        }
        self.bump();
        true
    }
}

/// `Title (3)` is `Title`; other titles stay.
fn strip_number(title: &str) -> &str {
    let Some(open) = title.strip_suffix(')').and_then(|t| t.rfind(" (")) else {
        return title;
    };
    let digits = title.get(open + 2..title.len() - 1).unwrap_or_default();
    let numbered = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    match numbered {
        true => title.get(..open).unwrap_or(title),
        false => title,
    }
}
