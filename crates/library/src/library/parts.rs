//! What a saved library is made of, before it becomes one.

use std::collections::BTreeMap;

use crate::ai_pages::AiPage;
use crate::book::{Book, State};
use crate::clock::Clock;
use crate::folder::Folder;
use crate::id::{AiPageId, BookId, FolderId, LinkId, MarkId, ShelfId, SightingId, WordId};
use crate::link::Link;
use crate::marks::Mark;
use crate::shelves::{Membership, Shelf};
use crate::study_words::{WordSighting, WordState};

/// What a saved library is made of, read back from disk.
#[derive(Debug)]
pub(crate) struct Parts {
    pub clock: Clock,
    pub books: BTreeMap<BookId, Book>,
    pub states: BTreeMap<BookId, State>,
    /// Folder rows from before ledgers, adopted on load.
    pub legacy: Vec<crate::legacy::OldFolder>,
    pub folders: BTreeMap<FolderId, Folder>,
    pub links: BTreeMap<LinkId, Link>,
    pub shelves: BTreeMap<ShelfId, Shelf>,
    pub members: BTreeMap<(ShelfId, BookId), Membership>,
    pub marks: BTreeMap<MarkId, Mark>,
    pub ai_pages: BTreeMap<AiPageId, AiPage>,
    pub words: BTreeMap<WordId, WordState>,
    pub sightings: BTreeMap<SightingId, WordSighting>,
}

impl Parts {
    pub fn new(clock: Clock) -> Self {
        Self {
            clock,
            books: BTreeMap::new(),
            states: BTreeMap::new(),
            legacy: Vec::new(),
            folders: BTreeMap::new(),
            links: BTreeMap::new(),
            shelves: BTreeMap::new(),
            members: BTreeMap::new(),
            marks: BTreeMap::new(),
            ai_pages: BTreeMap::new(),
            words: BTreeMap::new(),
            sightings: BTreeMap::new(),
        }
    }
}
