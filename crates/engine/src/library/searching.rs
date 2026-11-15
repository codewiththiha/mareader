//! The library's search: its field, what the page lists, and the card a choice
//! lights.
//!
//! The bar draws the field and the suggestions; this is what they read and what
//! a choice leaves behind, so a listing survives the frames that show it.

use std::rc::Rc;

use mareader_library::picks::{Pick, PickAction};
use mareader_library::{
    Book, BookId, Home, Item, Query, SUGGEST_LIMIT, ShelfId, SortKey, Suggestion, Target, View,
    plural,
};

use super::{Scope, SearchAction, State};

/// The ring holds, then fades out by [`RING_END`].
pub const RING_HOLD: f64 = 4.0;
pub const RING_END: f64 = 4.6;

/// The field's text, its query, and what the suggestions list shows.
#[derive(Default)]
pub struct Search {
    pub text: String,
    pub query: Query,
    /// Suggestions list under the field while it's focused.
    pub open: bool,
    pub lit: Option<usize>,
    pub reveal: Option<Reveal>,
    pub listing: Option<(Shape, Rc<Listing>)>,
    pub suggestions: Option<(u64, Query, Rc<[Suggestion]>)>,
}

impl Search {
    pub fn set_text(&mut self, text: String) {
        self.query = Query::new(&text);
        self.text = text;
        self.open = !self.query.is_empty();
        // The first row is lit, so Enter goes to the best match.
        self.lit = self.open.then_some(0);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.lit = None;
    }

    /// Empties the field; true if it held anything.
    pub fn clear(&mut self) -> bool {
        let had = !self.text.is_empty();
        self.set_text(String::new());
        had
    }
}

/// A card shown by choosing it: ringed for a while, scrolled to once.
#[derive(Clone, Copy, Debug)]
pub struct Reveal {
    pub pick: Pick,
    pub since: f64,
    pub scroll: bool,
}

/// What the page lists, kept until the library or the page changes.
#[derive(Debug, Default)]
pub struct Listing {
    /// Shelves and books in the order shown.
    pub items: Vec<Item>,
    /// Books before the search: the 12 in "3 of 12 books".
    pub total: usize,
    /// Matches hidden by the shelf or filter, when none show.
    pub elsewhere: usize,
}

/// Everything a listing depends on.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub revision: u64,
    pub at: Option<ShelfId>,
    pub scope: Scope,
    pub sort: SortKey,
    pub ascending: bool,
    pub home: Home,
    pub query: Query,
}

/// What the bar's field shows; the bar edits a copy.
#[derive(Clone, Debug, Default)]
pub struct Field {
    pub text: String,
    pub placeholder: String,
    pub open: bool,
    pub lit: Option<usize>,
}

/// The card being revealed: what it is, and how long ago it was chosen.
#[derive(Clone, Copy, Debug)]
pub struct Lit {
    pub pick: Pick,
    pub age: f64,
}

impl State {
    /// The shelves and books the page shows, in order.
    pub fn listing(&mut self, view: &View) -> Rc<Listing> {
        let key = Shape {
            revision: self.model.revision(),
            at: self.at,
            scope: self.scope,
            sort: view.sort,
            ascending: view.ascending,
            home: view.home,
            query: self.search.query.clone(),
        };
        if let Some((held, listing)) = &self.search.listing
            && *held == key
        {
            return Rc::clone(listing);
        }
        let listing = Rc::new(self.list(view));
        self.search.listing = Some((key, Rc::clone(&listing)));
        listing
    }

    fn list(&self, view: &View) -> Listing {
        let (model, query) = (&self.model, &self.search.query);
        // Unsearched, the reader's own order mixes shelves among books.
        if self.arranging(view) {
            let items = model.arranged(self.at, view.home);
            let total = items.iter().filter(|i| matches!(i, Item::Book(_))).count();
            return Listing {
                items,
                total,
                elsewhere: 0,
            };
        }
        let pool = self.pool(view);
        let books: Vec<BookId> = (pool.iter())
            .filter(|b| query.keeps(model, b))
            .map(|b| b.id)
            .collect();
        // A folder or Missing is a flat list of what matches.
        let shelves = match (self.scope, self.at) {
            // As with books, a search at the top reaches every shelf.
            (Scope::All, None) if !query.is_empty() => model.every_shelf(),
            (Scope::All, at) => model.children(at),
            _ => Vec::new(),
        };
        let shelves: Vec<Item> = (shelves.into_iter())
            .filter(|s| query.keeps_name(&s.name.value))
            .map(|s| Item::Shelf(s.id))
            .collect();
        // Links show on their own level, found by the name they wear.
        let all = self.scope == Scope::All;
        let links: Vec<Item> = (model.links())
            .filter(|l| all && l.level.value == self.at && query.keeps_name(&l.name.value))
            .map(|l| Item::Link(l.id))
            .collect();
        let narrowed = self.at.is_some() || self.scope != Scope::All;
        let none = books.is_empty() && shelves.is_empty() && links.is_empty();
        let elsewhere = match narrowed && none && !query.is_empty() {
            true => model.books().filter(|b| query.keeps(model, b)).count(),
            false => 0,
        };
        let mut rows: Vec<Item> = books.into_iter().map(Item::Book).collect();
        if !links.is_empty() {
            rows.extend(links);
            if view.sort != SortKey::Arranged {
                view.sort_items(model, &mut rows);
            }
        }
        Listing {
            items: shelves.into_iter().chain(rows).collect(),
            total: pool.len(),
            elsewhere,
        }
    }

    /// Whether the page shows a level in the reader's own order.
    pub fn arranging(&self, view: &View) -> bool {
        let plain = self.scope == Scope::All && self.search.query.is_empty();
        plain && view.sort == SortKey::Arranged
    }

    /// The books the page would list unsearched, sorted.
    fn pool(&self, view: &View) -> Vec<&Book> {
        let model = &self.model;
        if let Some(shelf) = self.at {
            let own = model.shelf_books(shelf);
            return match view.sort {
                SortKey::Arranged => own,
                _ => view.sort(own.into_iter()),
            };
        }
        let scope = self.scope;
        // In a folder, its subfolder's own books; searching, all below it.
        if scope == Scope::Missing {
            return view.sort(model.books().filter(|b| b.missing()));
        }
        // A search at the top looks through every book, shelved or not.
        if view.sort == SortKey::Arranged {
            let top = model.arranged(None, Home::Everything).into_iter();
            let book = |i| match i {
                Item::Book(id) => model.book(id),
                _ => None,
            };
            return top.filter_map(book).collect();
        }
        match (view.home, self.search.query.is_empty()) {
            (Home::Unshelved, true) => {
                let shelved = model.shelved();
                view.sort(model.books().filter(|b| !shelved.contains(&b.id)))
            }
            _ => view.sort(model.books()),
        }
    }

    /// The best few books and shelves for what's typed.
    pub fn suggestions(&mut self) -> Rc<[Suggestion]> {
        let revision = self.model.revision();
        if let Some((held, query, found)) = &self.search.suggestions
            && *held == revision
            && *query == self.search.query
        {
            return Rc::clone(found);
        }
        let found: Rc<[Suggestion]> = self.model.suggest(&self.search.query, SUGGEST_LIMIT).into();
        let query = self.search.query.clone();
        self.search.suggestions = Some((revision, query, Rc::clone(&found)));
        found
    }

    /// What's typed in the bar.
    pub fn search_text(&self) -> &str {
        &self.search.text
    }

    pub fn searching(&self) -> bool {
        !self.search.query.is_empty()
    }

    /// What the bar's field shows this frame.
    pub fn field(&self) -> Field {
        let s = &self.search;
        Field {
            text: s.text.clone(),
            placeholder: self.placeholder(),
            open: s.open,
            lit: s.lit,
        }
    }

    /// How many books there are to search, or what it's limited to.
    fn placeholder(&self) -> String {
        let shelf = self.at.and_then(|id| self.model.shelf(id));
        if let Some(shelf) = shelf {
            return format!("Search “{}”", shelf.name.value);
        }
        match self.scope {
            Scope::Missing => "Search missing books".into(),
            Scope::All => match self.model.books().count() {
                0 => "Search the library".into(),
                n => format!("Search {}", plural(n, "book", "books")),
            },
        }
    }

    /// The card last chosen from the suggestions, while it's ringed.
    pub fn revealed(&self) -> Option<Pick> {
        self.search.reveal.map(|r| r.pick)
    }

    /// The card a choice lights, if still lit; true once to scroll to it.
    pub fn revealing(&mut self, now: f64) -> Option<(Lit, bool)> {
        let reveal = self.search.reveal.as_mut()?;
        let age = now - reveal.since;
        if !(0.0..RING_END).contains(&age) {
            self.search.reveal = None;
            return None;
        }
        let scroll = std::mem::take(&mut reveal.scroll);
        let pick = reveal.pick;
        Some((Lit { pick, age }, scroll))
    }

    pub fn search_act(&mut self, action: SearchAction, now: f64) {
        let s = &mut self.search;
        match action {
            // The app focuses the field; it holds the context.
            SearchAction::Focus => {}
            SearchAction::Text(text) => s.set_text(text),
            SearchAction::Move(by) => self.move_lit(by),
            SearchAction::Choose(target) => self.choose(target, now),
            SearchAction::ChooseLit => {
                let lit = s.lit.filter(|_| s.open);
                let found = self.suggestions();
                match lit.and_then(|i| found.get(i)) {
                    Some(chosen) => self.choose(chosen.target, now),
                    None => self.search.close(),
                }
            }
            SearchAction::Close => s.close(),
            SearchAction::Clear => {
                s.clear();
            }
            SearchAction::Widen => {
                self.at = None;
                self.scope = Scope::All;
                self.renaming = None;
                self.picks.apply(PickAction::Clear);
            }
        }
    }

    /// Down opens the list on its first row; Up off the top closes it.
    fn move_lit(&mut self, by: i32) {
        let rows = self.suggestions().len();
        let s = &mut self.search;
        if s.query.is_empty() || rows == 0 {
            return;
        }
        if !s.open {
            s.open = by > 0;
            s.lit = s.open.then_some(0);
            return;
        }
        let next = match s.lit {
            None if by > 0 => Some(0),
            None => None,
            Some(at) => at.checked_add_signed(by as isize),
        };
        match next {
            Some(at) => s.lit = Some(at.min(rows - 1)),
            None => s.close(),
        }
    }

    /// Lights the target where it shows, going there if it's elsewhere.
    fn choose(&mut self, target: Target, now: f64) {
        let model = &self.model;
        let (pick, there) = match target {
            Target::Book(id) => {
                let Some(book) = model.book(id) else {
                    return;
                };
                // A search at the top lists every match.
                let here = match (self.at, self.scope) {
                    (Some(shelf), _) => model.shelves_of(id).contains(&shelf),
                    (None, Scope::All) => true,
                    (None, Scope::Missing) => book.missing(),
                };
                (Pick::Book(id), (!here).then_some(None))
            }
            Target::Shelf(id) => {
                let Some(shelf) = model.shelf(id) else {
                    return;
                };
                let parent = model.parent_of(shelf);
                // A search at the top lists every matching shelf.
                let top = self.at.is_none() && !self.search.query.is_empty();
                let here = self.scope == Scope::All && (top || self.at == parent);
                (Pick::Shelf(id), (!here).then_some(parent))
            }
        };
        if let Some(at) = there {
            self.at = at;
            self.scope = Scope::All;
            self.renaming = None;
        }
        self.picks.apply(PickAction::Clear);
        self.search.close();
        let scroll = true;
        self.search.reveal = Some(Reveal {
            pick,
            since: now,
            scroll,
        });
    }

    /// Goes where a row stands, then lights it; a book's first shelf.
    pub fn reveal(&mut self, pick: Pick, now: f64) {
        let model = &self.model;
        let at = match pick {
            Pick::Book(id) => {
                let shelves = model.shelves_of(id);
                match self.at.filter(|s| shelves.contains(s)) {
                    Some(here) => Some(here),
                    None => shelves.first().copied(),
                }
            }
            Pick::Shelf(id) => model.shelf(id).and_then(|s| model.parent_of(s)),
            Pick::Link(id) => model.link(id).and_then(|l| l.level.value),
        };
        if at != self.at {
            self.renaming = None;
        }
        self.at = at;
        self.scope = Scope::All;
        self.search.clear();
        self.picks.apply(PickAction::Clear);
        let scroll = true;
        self.search.reveal = Some(Reveal {
            pick,
            since: now,
            scroll,
        });
    }
}
