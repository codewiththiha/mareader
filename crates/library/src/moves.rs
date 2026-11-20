//! Hand moves: books and shelves carried to another level.

use std::path::Path;

use crate::arrange::Item;
use crate::book::{Location, Origin};
use crate::clock::Field;
use crate::conflict::{Arrival, Entry, Level, Placement, collide};
use crate::folder::{Folder, dir_of_rung, key_in_zone, rel_under};
use crate::ground;
use crate::id::{BookId, FolderId, LinkId, ShelfId};
use crate::library::Library;
use crate::link::LinkTo;
use crate::removal::Data;
use crate::run::Copied;
use crate::screen::Answer;
use crate::shelves::ShelfKind;
use crate::words::free_name;

/// Books, shelves and links carried to one level, as a drag or a menu does.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Move {
    pub books: Vec<BookId>,
    pub shelves: Vec<ShelfId>,
    /// Links only ever move: a pointer owes no copy and never collides.
    pub links: Vec<LinkId>,
    /// The level the books were carried off; they leave it.
    pub from: Level,
    pub to: Level,
    /// The books stay where they were and show here too.
    pub keep: bool,
    /// Lands before this card on `to`; `None` appends.
    pub before: Option<Item>,
}

impl Move {
    /// The cards `items` names, sorted by kind, bound nowhere yet.
    pub fn of(items: &[Item]) -> Self {
        let mut mv = Self::default();
        for item in items {
            match *item {
                Item::Book(id) => mv.books.push(id),
                Item::Shelf(id) => mv.shelves.push(id),
                Item::Link(id) => mv.links.push(id),
            }
        }
        mv
    }
}

/// What a move owes before it lands: books read in place become copies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Departure {
    /// Books read in place leaving their ground, in gesture order.
    pub books: Vec<BookId>,
    /// Rungs taken off their seats; one inside another rides with it.
    pub shelves: Vec<ShelfId>,
    /// The books read in place those shelves carry out.
    pub carried: Vec<BookId>,
    /// Departing shelves that can go back to their seats instead.
    pub homeward: Vec<ShelfId>,
}

impl Departure {
    pub fn is_empty(&self) -> bool {
        self.books.is_empty() && self.shelves.is_empty()
    }

    /// Every book that becomes a copy, each once.
    pub fn copies(&self) -> Vec<BookId> {
        let mut out = self.books.clone();
        out.extend(self.carried.iter().filter(|b| !self.books.contains(b)));
        out
    }
}

/// A moved book meets a book of its name on the level it lands on.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveAsk {
    pub arrival: Arrival,
    pub existing: BookId,
    pub existing_name: String,
    pub keep: bool,
    pub before: Option<Item>,
    /// It just became a copy, and the book there is the library's own.
    pub converted: bool,
}

impl MoveAsk {
    pub fn offers(&self) -> &'static [Placement] {
        match self.converted {
            true => Placement::MOVE_KEEPING_BOTH,
            false => Placement::MOVE,
        }
    }

    fn book(&self) -> Option<BookId> {
        match self.arrival.moving {
            Some(Entry::Book(id)) => Some(id),
            _ => None,
        }
    }
}

/// What a landing did, and what asks before the rest lands.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Moved {
    pub books: usize,
    pub shelves: usize,
    pub links: usize,
    /// Departing shelves left where they were: a copy never came.
    pub stayed: Vec<ShelfId>,
    pub asks: Vec<MoveAsk>,
}

/// A way back for a shelf off its seat.
enum Home {
    /// A root shelf whose folder stands inside another tree folds into it.
    Fold {
        tree: FolderId,
        gone: FolderId,
        rel: String,
    },
    /// Any other rung hangs again from the rung its disk names.
    Seat(Option<ShelfId>),
}

impl Library {
    /// What `mv` owes before it lands; empty lands it as it is.
    pub fn departure(&self, mv: &Move) -> Departure {
        let folders = self.folder_list();
        let mut out = Departure::default();
        if !mv.keep && mv.from != mv.to {
            let converting = |id: &&BookId| self.converts(**id, mv.to, &folders);
            out.books = mv.books.iter().filter(converting).copied().collect();
        }
        for id in self.departing(mv, &folders) {
            for book in self.rung_books(id, true) {
                if !out.carried.contains(&book) {
                    out.carried.push(book);
                }
            }
            if self.way_home(id, mv.to, &folders).is_some() {
                out.homeward.push(id);
            }
            out.shelves.push(id);
        }
        out
    }

    /// Books read in place on a rung: its own, or its whole subtree's.
    pub fn rung_books(&self, id: ShelfId, whole: bool) -> Vec<BookId> {
        let Some((folder, _)) = self.shelf(id).and_then(|s| s.kind.value.rung()) else {
            return Vec::new();
        };
        let Some(tree) = self.folder(folder).filter(|f| f.mode().in_place()) else {
            return Vec::new();
        };
        let mut subtree = vec![id];
        subtree.extend(self.subtree(&[id]));
        let rung_of = |s: &ShelfId| self.shelf(*s).and_then(|s| s.kind.value.rung());
        let rungs: Vec<ShelfId> = match whole {
            true => (subtree.iter().copied())
                .filter(|s| rung_of(s).is_some_and(|(f, _)| f == folder))
                .collect(),
            false => vec![id],
        };
        let stands = |path: &Path| tree.rungs_for(path).0.is_some_and(|r| rungs.contains(&r));
        self.books()
            .filter(|b| reads_in_place(b) && tree.placed.contains(&b.content))
            .filter(|b| b.path().is_some_and(stands))
            .filter(|b| self.shelves_of(b.id).iter().any(|s| subtree.contains(s)))
            .map(|b| b.id)
            .collect()
    }

    /// A book read in place becomes the library's own copy, logged moved.
    pub fn depart(&mut self, id: BookId, copy: &Copied, wall_ms: u64) -> bool {
        let Some(home) = self.book(id).and_then(|b| b.home()).cloned() else {
            return false;
        };
        if !matches!(home.origin, Origin::Folder(_)) {
            return false;
        }
        self.tombstone_book(id, true, wall_ms);
        if let Some(book) = self.books.get_mut(&id) {
            book.locations.retain(|l| l.path != home.path);
            book.locations.insert(
                0,
                Location {
                    path: copy.path.clone(),
                    origin: Origin::Store,
                    stamp: copy.measure.stamp,
                    from: Some(home.path),
                    present: true,
                },
            );
        }
        self.mirror(id);
        self.index();
        self.bump();
        true
    }

    /// No copies: departing shelves go back where their folders name.
    pub fn send_home(&mut self, mv: &Move, wall_ms: u64) -> Vec<ShelfId> {
        let folders = self.folder_list();
        let mut seated = Vec::new();
        for id in self.departing(mv, &folders) {
            let done = match self.way_home(id, mv.to, &folders) {
                Some(Home::Fold { tree, gone, rel }) => {
                    self.fold_into(tree, gone, &rel, wall_ms).is_some()
                }
                Some(Home::Seat(seat)) => self.move_shelf(id, seat, wall_ms).is_ok(),
                None => false,
            };
            if done {
                seated.push(id);
            }
        }
        seated
    }

    /// Lands a move; same-name books ask. `departed` bind no log.
    pub fn land_move(&mut self, mv: &Move, departed: &[BookId], wall_ms: u64) -> Moved {
        let folders = self.folder_list();
        let mut done = Moved::default();
        let mut landed: Vec<Item> = Vec::new();
        let departing = self.departing(mv, &folders);
        let (clean, _) = ground::departing_moves(&self.shelves, &folders, &mv.shelves, mv.to);
        let mut names: Vec<String> = (self.children(mv.to).iter())
            .map(|s| s.name.value.clone())
            .collect();
        for id in departing {
            if !self.rung_books(id, true).is_empty() {
                done.stayed.push(id);
                continue;
            }
            self.take_out(id, &mut names, wall_ms);
            if self.move_shelf(id, mv.to, wall_ms).is_ok() {
                landed.push(Item::Shelf(id));
            }
        }
        for id in clean {
            if ground::can_nest(&self.shelves, id, mv.to)
                && self.move_shelf(id, mv.to, wall_ms).is_ok()
            {
                landed.push(Item::Shelf(id));
            }
        }
        done.shelves = landed.len();
        if mv.keep || mv.from != mv.to {
            let rows = self.level_rows(mv.to);
            for &id in &mv.books {
                let Some(book) = self.book(id) else {
                    continue;
                };
                // A copy that never came costs the book its move.
                if !mv.keep && self.converts(id, mv.to, &folders) {
                    continue;
                }
                let arrival =
                    Arrival::moved(Entry::Book(id), book.title.value.clone(), mv.to, None)
                        .leaving(mv.from);
                match collide(rows.iter().map(|(e, n)| (*e, n.as_str())), &arrival) {
                    Some(Entry::Book(existing)) => done.asks.push(MoveAsk {
                        existing_name: self
                            .book(existing)
                            .map(|b| b.title.value.clone())
                            .unwrap_or_default(),
                        arrival,
                        existing,
                        keep: mv.keep,
                        before: mv.before,
                        converted: departed.contains(&id)
                            && self.book(existing).is_some_and(crate::holding::stored),
                    }),
                    _ => {
                        self.seat_book(
                            id,
                            mv.from,
                            mv.to,
                            mv.keep,
                            !departed.contains(&id),
                            wall_ms,
                        );
                        landed.push(Item::Book(id));
                        done.books += 1;
                    }
                }
            }
        }
        for &id in &mv.links {
            if self.relevel_link(id, mv.to, wall_ms) {
                landed.push(Item::Link(id));
                done.links += 1;
            }
        }
        if mv.before.is_some() && !landed.is_empty() {
            self.arrange(mv.to, &landed, mv.before, wall_ms);
        }
        self.index();
        self.bump();
        done
    }

    /// Stands a link on `to`, last; true if it's there now.
    fn relevel_link(&mut self, id: LinkId, to: Level, wall_ms: u64) -> bool {
        let Some(from) = self.link(id).map(|l| l.level.value) else {
            return false;
        };
        if to.is_some_and(|s| self.shelf(s).is_none()) {
            return false;
        }
        if from == to {
            return true;
        }
        let at = self.clock.tick(wall_ms);
        let key = self.next_key(to);
        if let Some(link) = self.links.get_mut(&id) {
            link.level = Field::new(to, at);
            link.key = Field::new(key, at);
        }
        true
    }

    /// Writes a move's answer: one book, this one instead, or both.
    pub fn answer_move(&mut self, ask: &MoveAsk, answer: Placement, wall_ms: u64) -> Answer {
        let Some(id) = ask.book().filter(|id| self.book(*id).is_some()) else {
            return Answer::Nothing;
        };
        if !ask.offers().contains(&answer) || self.book(ask.existing).is_none() {
            return Answer::Nothing;
        }
        let (from, to) = (ask.arrival.from.flatten(), ask.arrival.level);
        let answered = match answer {
            Placement::Merge => {
                self.fold_moved(ask.existing, id, from, wall_ms);
                ask.existing
            }
            // The fresh copy dissolves into a pointer; the move-out log stays.
            Placement::LinkOnly => {
                self.purge(&[id], Data::Keep, false, wall_ms);
                let at = LinkTo::Book(ask.existing);
                if let Some(link) = self.link_to(at, to, wall_ms)
                    && ask.before.is_some()
                {
                    self.arrange(to, &[Item::Link(link)], ask.before, wall_ms);
                }
                return Answer::Linked;
            }
            Placement::Replace => {
                let slot = self.slot_of(ask.existing, to);
                let shelves = self.shelves_of(ask.existing);
                self.drop_books(&[ask.existing], wall_ms);
                self.seat_book(id, from, to, ask.keep, true, wall_ms);
                for shelf in shelves {
                    self.file(id, shelf, wall_ms);
                }
                if let Some(slot) = slot {
                    self.set_slot(id, to, slot, wall_ms);
                }
                id
            }
            _ => {
                let name = self.free_on(to, &ask.arrival.name);
                self.rename_book(id, &name, wall_ms);
                self.seat_book(id, from, to, ask.keep, true, wall_ms);
                if ask.before.is_some() {
                    self.arrange(to, &[Item::Book(id)], ask.before, wall_ms);
                }
                id
            }
        };
        self.index();
        self.bump();
        Answer::Landed(answered)
    }

    /// The top departing shelves of a move, each able to land there.
    fn departing(&self, mv: &Move, folders: &[Folder]) -> Vec<ShelfId> {
        let (_, departing) = ground::departing_moves(&self.shelves, folders, &mv.shelves, mv.to);
        let nests = |id: &ShelfId| ground::can_nest(&self.shelves, *id, mv.to);
        departing.into_iter().filter(nests).collect()
    }

    /// Whether a book read in place, landing on `to`, must become a copy.
    fn converts(&self, id: BookId, to: Level, folders: &[Folder]) -> bool {
        let Some(book) = self.book(id).filter(|b| reads_in_place(b)) else {
            return false;
        };
        let Some(path) = book.path() else {
            return false;
        };
        ground::converts_on_move(folders, book.content, path, to)
    }

    /// Where a departing shelf can go back to, if the drop lands in its tree.
    fn way_home(&self, id: ShelfId, to: Level, folders: &[Folder]) -> Option<Home> {
        let shelf = self.shelf(id)?;
        let (folder, key) = shelf.kind.value.rung()?;
        let tree = folders
            .iter()
            .find(|f| f.id == folder && f.mode().in_place())?;
        let ground = dir_of_rung(&tree.root, key);
        let family = to
            .and_then(|t| self.shelf(t))
            .and_then(|s| s.kind.value.rung());
        let family = family.is_some_and(|(f, _)| {
            (folders.iter())
                .any(|x| x.id == f && x.mode().in_place() && rel_under(&ground, &x.root).is_some())
        });
        if !family {
            return None;
        }
        if key.is_empty()
            && let Some((into, rel)) = ground::family(&self.shelves, folders, &tree.root)
        {
            return Some(Home::Fold {
                tree: into,
                gone: folder,
                rel,
            });
        }
        let seat = self.seat(id)?;
        (self.parent_of(shelf) != seat).then_some(Home::Seat(seat))
    }

    /// A departing shelf's rungs become the library's own, under a copy's name.
    fn take_out(&mut self, id: ShelfId, names: &mut Vec<String>, wall_ms: u64) {
        let Some(shelf) = self.shelf(id) else {
            return;
        };
        let Some((folder, zone)) = shelf.kind.value.rung().map(|(f, k)| (f, k.to_owned())) else {
            return;
        };
        let own = shelf.name.value.clone();
        // A copy takes the next free name, leaving the folder's own free.
        let taken = names.iter().map(String::as_str).chain([own.as_str()]);
        let name = free_name(&own, taken);
        names.push(name.clone());
        let mut subtree = vec![id];
        subtree.extend(self.subtree(&[id]));
        let at = self.clock.tick(wall_ms);
        let mut rungs = Vec::new();
        for s in self.shelves.values_mut_where(|s| subtree.contains(&s.id)) {
            match s.kind.value.rung() {
                Some((f, _)) if f == folder => {
                    rungs.push(s.id);
                    s.kind = Field::new(ShelfKind::Departed, at);
                    s.moved = Field::new(false, at);
                }
                // Another folder's rung rides along, off its own seat.
                Some(_) => s.moved = Field::new(true, at),
                None => {}
            }
            if s.id == id {
                s.name = Field::new(name.clone(), at);
            }
        }
        for f in self.folders.values_mut_where(|f| f.id == folder) {
            f.shelf_map
                .retain(|key, s| !key_in_zone(key, &zone) && !rungs.contains(s));
        }
    }

    /// A clean book onto `to`: off `from` unless kept, off every shelf at top.
    fn seat_book(
        &mut self,
        id: BookId,
        from: Level,
        to: Level,
        keep: bool,
        bind: bool,
        wall_ms: u64,
    ) {
        match to {
            // Off the shelf it left; its other shelves keep it.
            None => {
                if let Some(from) = from {
                    self.unfile(id, from, wall_ms);
                }
            }
            Some(to) => {
                if let Some(from) = from.filter(|f| !keep && *f != to) {
                    self.unfile(id, from, wall_ms);
                }
                self.file(id, to, wall_ms);
                if bind {
                    self.bind_returned(id, to);
                }
            }
        }
    }

    /// The library's copy back on its folder's shelf spends the move-out log.
    fn bind_returned(&mut self, id: BookId, shelf: ShelfId) {
        let Some(src) = self
            .book(id)
            .and_then(|b| b.home())
            .filter(|l| l.origin == Origin::Store)
        else {
            return;
        };
        let Some(src) = src.from.clone() else {
            return;
        };
        let Some((folder, _)) = self.shelf(shelf).and_then(|s| s.kind.value.rung()) else {
            return;
        };
        let owed = |f: &Folder| {
            f.id == folder
                && (f.ignored.iter())
                    .any(|s| s.moved && s.last_path == src && s.returned != Some(id))
        };
        for f in self.folders.values_mut_where(owed) {
            for stone in f
                .ignored
                .iter_mut()
                .filter(|s| s.moved && s.last_path == src)
            {
                stone.returned = Some(id);
            }
        }
    }

    /// `gone` folds into `keeper`: the further place, its marks and shelves.
    fn fold_moved(&mut self, keeper: BookId, gone: BookId, left: Level, wall_ms: u64) {
        let at = self.clock.tick(wall_ms);
        let Some(old) = self.book(gone).cloned() else {
            return;
        };
        let theirs = self.position(gone);
        if let Some(book) = self.books.get_mut(&keeper) {
            if book.author.value.is_none() && old.author.value.is_some() {
                book.author = Field::new(old.author.value.clone(), at);
            }
            if old.opened.value > book.opened.value {
                book.opened = Field::new(old.opened.value, at);
            }
            book.added = book.added.min(old.added);
        }
        if let Some(theirs) =
            theirs.filter(|t| self.position(keeper).is_none_or(|m| t.page > m.page))
        {
            self.record_position(keeper, theirs, wall_ms);
        }
        self.move_data(gone, keeper, at);
        // The level the move left is the one shelf the keeper doesn't take.
        for shelf in self.shelves_of(gone) {
            if Some(shelf) != left {
                self.file(keeper, shelf, wall_ms);
            }
        }
        // Folding into the library's copy of its file leaves the log behind.
        let copy_of = |path: &Path| {
            let keeper = self.book(keeper).and_then(|b| b.home());
            keeper.is_some_and(|l| l.from.as_deref() == Some(path))
        };
        if old.path().is_some_and(copy_of) && reads_in_place(&old) {
            self.tombstone_book(gone, true, wall_ms);
            let src = old.path().map(Path::to_path_buf);
            for f in self.folders.values_mut_where(|f| {
                f.ignored
                    .iter()
                    .any(|s| s.moved && Some(&s.last_path) == src.as_ref())
            }) {
                for stone in f
                    .ignored
                    .iter_mut()
                    .filter(|s| s.moved && Some(&s.last_path) == src.as_ref())
                {
                    stone.returned = Some(keeper);
                }
            }
        }
        self.drop_books(&[gone], wall_ms);
    }
}

fn reads_in_place(book: &crate::book::Book) -> bool {
    book.home()
        .is_some_and(|l| matches!(l.origin, Origin::Folder(_)))
}
