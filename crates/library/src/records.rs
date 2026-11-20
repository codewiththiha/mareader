//! The library as rows: what changed becomes rows, rows become a library.

use std::collections::HashMap;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::ai_pages::AiPage;
use crate::book::{Book, Location, State};
use crate::clock::Clock;
use crate::folder::Folder;
use crate::library::{Dirty, Library, Parts};
use crate::link::Link;
use crate::marks::Mark;
use crate::shelves::{Membership, Shelf};
use crate::study_words::{WordSighting, WordState};

/// What a row holds. Synced kinds are shared; the rest stay on this machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Book,
    Shelf,
    Member,
    /// A book's paths here, newest first.
    Locations,
    /// A folder as 0.0.1 saved it; read once into a [`Kind::Folder`].
    OldFolder,
    /// Where the reader is in one book.
    State,
    /// A word explained in a book (M6).
    Mark,
    /// A folder the library takes books from, and its ledger.
    Folder,
    /// A pointer row at a book or a shelf.
    Link,
    /// A page the AI summarized, translated or simplified.
    AiPage,
    /// A word studied, and where it stands, in every book.
    Word,
    /// A word studied in one book.
    Sighting,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Book => "book",
            Self::Shelf => "shelf",
            Self::Member => "member",
            Self::Locations => "locations",
            Self::OldFolder => "source",
            Self::State => "state",
            Self::Mark => "mark",
            Self::Folder => "folder",
            Self::Link => "link",
            Self::AiPage => "ai_page",
            Self::Word => "word",
            Self::Sighting => "sighting",
        }
    }

    /// None for a kind from a newer Mareader.
    pub fn parse(name: &str) -> Option<Self> {
        let all = [
            Self::Book,
            Self::Shelf,
            Self::Member,
            Self::Locations,
            Self::OldFolder,
            Self::State,
            Self::Mark,
            Self::Folder,
            Self::Link,
            Self::AiPage,
            Self::Word,
            Self::Sighting,
        ];
        all.into_iter().find(|k| k.name() == name)
    }

    /// Shared by a reader's devices, once they sync.
    pub fn synced(self) -> bool {
        let shared = [
            Self::Book,
            Self::Shelf,
            Self::Member,
            Self::State,
            Self::Mark,
            Self::Link,
            Self::AiPage,
            Self::Word,
            Self::Sighting,
        ];
        shared.contains(&self)
    }
}

/// One stored row: a kind, an id within it, and JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub id: String,
    pub body: String,
}

/// One save's rows: written, deleted, and the clock that stamped them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    pub put: Vec<Row>,
    /// Only local rows go; records end in tombstones instead.
    pub gone: Vec<(Kind, String)>,
    pub clock: Option<String>,
    /// Erased data: the store wipes old copies once this is written.
    pub scrub: bool,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.put.is_empty() && self.gone.is_empty()
    }

    fn put(&mut self, kind: Kind, id: String, value: &impl Serialize) {
        // Library types always serialize; a failure would lose one row.
        if let Ok(body) = serde_json::to_string(value) {
            self.put.push(Row { kind, id, body });
        }
    }
}

/// A membership's id: its shelf and book.
fn member_id(m: &Membership) -> String {
    format!("{}/{}", m.shelf, m.book)
}

impl Library {
    /// The rows `dirty` names, as the library now has them.
    pub fn changes(&self, dirty: &Dirty) -> Changes {
        changes(self, dirty)
    }
}

fn changes(library: &Library, dirty: &Dirty) -> Changes {
    let mut out = Changes {
        scrub: dirty.erased,
        ..Changes::default()
    };
    for book in dirty.books.iter().filter_map(|id| library.books.get(id)) {
        let id = book.id.to_string();
        let record = Book {
            locations: Vec::new(),
            ..book.clone()
        };
        out.put(Kind::Book, id.clone(), &record);
        match book.locations.is_empty() {
            true => out.gone.push((Kind::Locations, id)),
            false => out.put(Kind::Locations, id, &book.locations),
        }
    }
    for shelf in dirty
        .shelves
        .iter()
        .filter_map(|id| library.shelves.get(id))
    {
        out.put(Kind::Shelf, shelf.id.to_string(), shelf);
    }
    for member in dirty.members.iter().filter_map(|k| library.members.get(k)) {
        out.put(Kind::Member, member_id(member), member);
    }
    for id in &dirty.retired {
        out.gone.push((Kind::OldFolder, id.to_string()));
    }
    for id in &dirty.folders {
        match library.folders.get(id) {
            Some(folder) => out.put(Kind::Folder, id.to_string(), folder),
            None => out.gone.push((Kind::Folder, id.to_string())),
        }
    }
    for link in dirty.links.iter().filter_map(|id| library.links.get(id)) {
        out.put(Kind::Link, link.id.to_string(), link);
    }
    for (id, state) in dirty
        .states
        .iter()
        .filter_map(|id| Some((id, library.states.get(id)?)))
    {
        out.put(Kind::State, id.to_string(), state);
    }
    for mark in dirty.marks.iter().filter_map(|id| library.marks.get(id)) {
        out.put(Kind::Mark, mark.id.to_string(), mark);
    }
    for page in dirty
        .ai_pages
        .iter()
        .filter_map(|id| library.ai_pages.get(id))
    {
        out.put(Kind::AiPage, page.id.to_string(), page);
    }
    for word in dirty.words.iter().filter_map(|id| library.words.get(id)) {
        out.put(Kind::Word, word.id.to_string(), word);
    }
    for seen in dirty
        .sightings
        .iter()
        .filter_map(|id| library.sightings.get(id))
    {
        out.put(Kind::Sighting, seen.id.to_string(), seen);
    }
    if !out.is_empty() {
        out.clock = serde_json::to_string(&library.clock).ok();
    }
    out
}

/// A library from its rows, and how many rows couldn't be read.
pub(crate) fn assemble(rows: Vec<Row>, clock: Clock) -> (Parts, usize) {
    let mut parts = Parts::new(clock);
    let mut skipped = 0;
    let mut locations: HashMap<String, Vec<Location>> = HashMap::new();
    let mut states: HashMap<String, State> = HashMap::new();
    for row in rows {
        let read = match row.kind {
            Kind::Book => parse::<Book>(&row).map(|b| {
                parts.books.insert(b.id, b);
            }),
            Kind::Shelf => parse::<Shelf>(&row).map(|s| {
                parts.shelves.insert(s.id, s);
            }),
            Kind::Member => parse::<Membership>(&row).map(|m| {
                parts.members.insert((m.shelf, m.book), m);
            }),
            Kind::OldFolder => parse::<crate::legacy::OldFolder>(&row).map(|s| {
                parts.legacy.push(s);
            }),
            Kind::Folder => parse::<Folder>(&row).map(|f| {
                parts.folders.insert(f.id, f);
            }),
            Kind::Link => parse::<Link>(&row).map(|l| {
                parts.links.insert(l.id, l);
            }),
            Kind::Locations => parse::<Vec<Location>>(&row).map(|l| {
                locations.insert(row.id.clone(), l);
            }),
            Kind::State => parse::<State>(&row).map(|s| {
                states.insert(row.id.clone(), s);
            }),
            Kind::Mark => parse::<Mark>(&row).map(|m| {
                parts.marks.insert(m.id, m);
            }),
            Kind::AiPage => parse::<AiPage>(&row).map(|p| {
                parts.ai_pages.insert(p.id, p);
            }),
            Kind::Word => parse::<WordState>(&row).map(|w| {
                parts.words.insert(w.id, w);
            }),
            Kind::Sighting => parse::<WordSighting>(&row).map(|s| {
                parts.sightings.insert(s.id, s);
            }),
        };
        skipped += usize::from(read.is_none());
    }
    for book in parts.books.values_mut() {
        let id = book.id.to_string();
        if let Some(found) = locations.remove(&id) {
            book.locations = found;
        }
        if let Some(state) = states.remove(&id) {
            parts.states.insert(book.id, state);
        }
    }
    (parts, skipped)
}

fn parse<T: DeserializeOwned>(row: &Row) -> Option<T> {
    serde_json::from_str(&row.body).ok()
}
