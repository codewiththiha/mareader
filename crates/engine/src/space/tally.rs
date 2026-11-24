//! Each book's notes and words as Space counts them, for library cards.

use std::collections::BTreeMap;

use mareader_library::{BookId, Library};

use super::notes::kept;
use super::query::met;

/// One book's highlights, notes and bookmarks, and its words met.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub notes: usize,
    pub words: usize,
}

impl Tally {
    pub fn is_empty(self) -> bool {
        self.notes == 0 && self.words == 0
    }
}

/// Every book with a note or a word, counted as Space's rails count them.
pub fn tally(lib: &Library) -> BTreeMap<BookId, Tally> {
    let mut out: BTreeMap<BookId, Tally> = BTreeMap::new();
    for mark in kept(lib) {
        out.entry(mark.book.value).or_default().notes += 1;
    }
    for m in met(lib) {
        out.entry(m.seen.book.value).or_default().words += 1;
    }
    out
}
