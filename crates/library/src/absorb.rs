//! Taking in another machine's records, then mending what merging leaves.

use std::collections::{BTreeMap, BTreeSet};

use serde::de::DeserializeOwned;

use crate::ai_pages::AiPage;
use crate::ai_selections::AiSelection;
use crate::book::{Book, State};
use crate::clock::{Clock, Field, Stamp};
use crate::extra::Extra;
use crate::id::{BookId, ContentId, ShelfId};
use crate::library::Library;
use crate::link::Link;
use crate::marks::Mark;
use crate::merge::Merge;
use crate::records::{Kind, Row};
use crate::shelves::{Membership, Shelf};
use crate::tracked::Tracked;

/// What taking in a batch of records did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Absorbed {
    /// Records that changed something here.
    pub changed: usize,
    /// Rows not taken: unreadable, or this machine's own kinds.
    pub skipped: usize,
    /// Books folded into another original of the same content.
    pub folded: usize,
    /// Shelves moved back out of a loop of parents.
    pub unlooped: usize,
}

impl Library {
    /// Merges another machine's synced rows in, then mends the result.
    pub fn absorb(&mut self, rows: impl IntoIterator<Item = Row>, wall_ms: u64) -> Absorbed {
        let mut done = Absorbed::default();
        for row in rows {
            match self.take(&row) {
                Some(changed) => done.changed += usize::from(changed),
                None => done.skipped += 1,
            }
        }
        let at = self.clock.tick(wall_ms);
        done.folded = self.fold_originals(at);
        done.unlooped = self.unloop(at);
        self.mirror_changed();
        self.index();
        self.bump();
        done
    }

    /// Merges one row; whether it changed anything, none if not taken.
    fn take(&mut self, row: &Row) -> Option<bool> {
        match row.kind {
            Kind::Book => {
                let book: Book = parse(row)?;
                Some(self.take_book(book))
            }
            Kind::Shelf => {
                let shelf: Shelf = parse(row)?;
                self.clock.observe(shelf.latest());
                let id = shelf.id;
                Some(merge_into(self.shelves.get(&id), shelf).is_some_and(|s| {
                    self.shelves.insert(id, s);
                    true
                }))
            }
            Kind::Member => {
                let member: Membership = parse(row)?;
                self.clock.observe(member.latest());
                let key = (member.shelf, member.book);
                Some(merge_into(self.members.get(&key), member).is_some_and(|m| {
                    self.members.insert(key, m);
                    true
                }))
            }
            Kind::State => {
                let id = BookId::try_from(row.id.clone()).ok()?;
                let state: State = parse(row)?;
                self.clock.observe(state.latest());
                Some(merge_into(self.states.get(&id), state).is_some_and(|s| {
                    self.states.insert(id, s);
                    self.states_dirty.insert(id);
                    true
                }))
            }
            Kind::Mark => {
                let mark: Mark = parse(row)?;
                self.clock.observe(mark.latest());
                let id = mark.id;
                Some(merge_into(self.marks.get(&id), mark).is_some_and(|m| {
                    self.marks.insert(id, m);
                    true
                }))
            }
            Kind::AiPage => {
                let page: AiPage = parse(row)?;
                self.clock.observe(page.latest());
                let id = page.id;
                Some(merge_into(self.ai_pages.get(&id), page).is_some_and(|p| {
                    self.ai_pages.insert(id, p);
                    true
                }))
            }
            Kind::AiSelection => {
                let selection: AiSelection = parse(row)?;
                self.clock.observe(selection.latest());
                let id = selection.id;
                Some(merge_into(self.ai_selections.get(&id), selection).is_some_and(|s| {
                    self.ai_selections.insert(id, s);
                    true
                }))
            }
            Kind::Link => {
                let link: Link = parse(row)?;
                self.clock.observe(link.latest());
                let id = link.id;
                Some(merge_into(self.links.get(&id), link).is_some_and(|l| {
                    self.links.insert(id, l);
                    true
                }))
            }
            Kind::Word => take_into(&mut self.clock, &mut self.words, row, |w| w.id),
            Kind::Sighting => take_into(&mut self.clock, &mut self.sightings, row, |s| s.id),
            Kind::Locations | Kind::OldFolder | Kind::Folder => None,
        }
    }

    fn take_book(&mut self, book: Book) -> bool {
        self.clock.observe(book.latest());
        let id = book.id;
        let Some(mut merged) = merge_into(self.books.get(&id), book) else {
            return false;
        };
        // Its paths here are this machine's; the record never carries any.
        merged.locations = self
            .books
            .get(&id)
            .map(|b| b.locations.clone())
            .unwrap_or_default();
        self.books.insert(id, merged);
        true
    }

    /// Two living originals of one content: the lower id keeps it all.
    fn fold_originals(&mut self, at: Stamp) -> usize {
        let mut by_content: BTreeMap<ContentId, Vec<BookId>> = BTreeMap::new();
        for book in self.books().filter(|b| b.shared()) {
            by_content.entry(book.content).or_default().push(book.id);
        }
        let mut folded = 0;
        for ids in by_content.into_values() {
            let mut ids = ids.into_iter();
            let Some(keeper) = ids.next() else {
                continue;
            };
            for gone in ids {
                self.fold(gone, keeper, at);
                folded += 1;
            }
        }
        folded
    }

    /// `gone` becomes a removed twin of `keeper`, which takes its places.
    fn fold(&mut self, gone: BookId, keeper: BookId, at: Stamp) {
        let Some(old) = self.books.get_mut(&gone) else {
            return;
        };
        old.removed = Some(at);
        old.twin_of = Field::new(Some(keeper), at);
        let old = old.clone();
        if let Some(book) = self.books.get_mut(&keeper) {
            book.title.merge(old.title);
            book.author.merge(old.author);
            book.opened.merge(old.opened);
            book.added = book.added.min(old.added);
            for location in old.locations {
                if !book.locations.iter().any(|l| l.path == location.path) {
                    book.locations.push(location);
                }
            }
        }
        if let Some(book) = self.books.quiet_get_mut(&gone) {
            book.locations.clear();
        }
        let twins = self
            .books
            .values_mut_where(|b| b.twin_of.value == Some(gone));
        for twin in twins {
            twin.twin_of = Field::new(Some(keeper), at);
        }
        let shelves = self
            .members
            .values()
            .filter(|m| m.book == gone && m.alive.value);
        let shelves: Vec<(ShelfId, String)> =
            shelves.map(|m| (m.shelf, m.key.value.clone())).collect();
        for (shelf, key) in shelves {
            let on = self
                .members
                .get(&(shelf, keeper))
                .is_some_and(|m| m.alive.value);
            if !on {
                let member = self
                    .members
                    .entry((shelf, keeper))
                    .or_insert_with(|| Membership {
                        shelf,
                        book: keeper,
                        key: Field::default(),
                        alive: Field::default(),
                        extra: Extra::default(),
                    });
                member.key = Field::new(key, at);
                member.alive = Field::new(true, at);
            }
        }
        if let Some(state) = self.states.get(&gone).cloned() {
            let into = self.states.entry(keeper).or_insert_with(|| state.clone());
            into.merge(state);
            self.states_dirty.insert(keeper);
        }
        self.move_data(gone, keeper, at);
    }

    /// `gone`'s marks, results and study go to `keeper`.
    pub(crate) fn move_data(&mut self, gone: BookId, keeper: BookId, at: Stamp) {
        self.move_marks(gone, keeper, at);
        self.move_ai_pages(gone, keeper, at);
        self.move_ai_selections(gone, keeper, at);
        self.move_study(gone, keeper, at);
    }

    /// Breaks each loop of parents at its newest move, which goes back.
    fn unloop(&mut self, at: Stamp) -> usize {
        let mut undone: BTreeSet<ShelfId> = BTreeSet::new();
        let mut moved = 0;
        while let Some(ring) = self.a_loop() {
            let newest = ring.iter().filter_map(|id| self.shelves.get(id));
            let Some(last) = newest.max_by_key(|s| (s.parent.at, s.id)).map(|s| s.id) else {
                break;
            };
            // Back where it was; a second time round, the top level.
            let back = self.shelves.get(&last).and_then(|s| s.from);
            let back = back.filter(|p| self.shelf(*p).is_some() && undone.insert(last));
            if let Some(shelf) = self.shelves.get_mut(&last) {
                shelf.parent = Field::new(back, at);
                shelf.from = None;
            }
            moved += 1;
        }
        moved
    }

    /// One loop of living shelves, each the parent of the next.
    fn a_loop(&self) -> Option<Vec<ShelfId>> {
        let living = self.shelves.len();
        for start in self.shelves() {
            let mut ring = vec![start.id];
            let mut at = self.parent_of(start);
            while let Some(id) = at {
                if id == start.id {
                    return Some(ring);
                }
                if ring.len() > living || ring.contains(&id) {
                    break;
                }
                ring.push(id);
                at = self.shelf(id).and_then(|s| self.parent_of(s));
            }
        }
        None
    }
}

/// The merged record if it differs from what's here, else none.
/// Merges one row into `map` by `key`; whether it changed anything.
fn take_into<K, T>(
    clock: &mut Clock,
    map: &mut Tracked<K, T>,
    row: &Row,
    key: fn(&T) -> K,
) -> Option<bool>
where
    K: Ord + Copy,
    T: Merge + Clone + PartialEq + DeserializeOwned,
{
    let theirs: T = parse(row)?;
    clock.observe(theirs.latest());
    let id = key(&theirs);
    Some(merge_into(map.get(&id), theirs).is_some_and(|t| {
        map.insert(id, t);
        true
    }))
}

fn merge_into<T: Merge + Clone + PartialEq>(here: Option<&T>, theirs: T) -> Option<T> {
    let Some(here) = here else {
        return Some(theirs);
    };
    let mut merged = here.clone();
    merged.merge(theirs);
    (merged != *here).then_some(merged)
}

fn parse<T: DeserializeOwned>(row: &Row) -> Option<T> {
    serde_json::from_str(&row.body).ok()
}
