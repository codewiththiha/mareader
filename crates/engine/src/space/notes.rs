//! Space's notes: every highlight, note and bookmark, across books.
//!
//! [`notes`] reads the library's living marks into one book's group each,
//! newest book first and each in reading order; Explain's own marks stay
//! out, as in the reader's notes panel.

use std::collections::BTreeMap;

use mareader_library::{AiSelection, BookId, Library, Mark, MarkId, MarkKind, Spot, Style};

use super::Shelf;
use crate::notes_panel::{query, reading_order};

/// The chips over the notes: everything, those written in, or bookmarks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kept {
    #[default]
    All,
    Notes,
    Bookmarks,
}

impl Kept {
    pub const ALL: [Kept; 3] = [Kept::All, Kept::Notes, Kept::Bookmarks];

    pub fn label(self) -> &'static str {
        match self {
            Kept::All => "All",
            Kept::Notes => "Notes",
            Kept::Bookmarks => "Bookmarks",
        }
    }

    fn holds(self, mark: &Mark) -> bool {
        match self {
            Kept::All => true,
            Kept::Notes => mark.note.value.is_some(),
            Kept::Bookmarks => mark.kind == MarkKind::Bookmark,
        }
    }
}

/// Which notes show; the window keeps it while the page is away.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotesFilter {
    pub text: String,
    pub book: Option<BookId>,
    pub kept: Kept,
}

/// One highlight, note or bookmark, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteRow {
    pub id: MarkId,
    pub book: BookId,
    /// The words marked, or a bookmark's name.
    pub quote: String,
    pub note: Option<String>,
    pub style: Style,
    pub bookmark: bool,
    pub spot: Spot,
    /// The page, when the spot names one without the book open.
    pub page: Option<usize>,
    pub added: u64,
}

/// One book's notes under its title.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteGroup {
    pub book: BookId,
    pub title: String,
    pub rows: Vec<NoteRow>,
}

/// Saved selection answers in one book.
#[derive(Clone, Debug, PartialEq)]
pub struct AiNoteGroup {
    pub book: BookId,
    pub title: String,
    pub rows: Vec<AiSelection>,
}

/// Everything the notes page shows for one filter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotesView {
    pub groups: Vec<NoteGroup>,
    pub ai_groups: Vec<AiNoteGroup>,
    /// The books in the rail, each with every note it holds.
    pub books: Vec<Shelf>,
    /// Every note in every book, for the rail's first row.
    pub total: usize,
    /// The chips' counts in the picked book, ignoring the search.
    pub counts: [usize; 3],
    /// The rows shown, after the chips and the search.
    pub shown: usize,
}

/// Reads `lib`'s marks into the rows `filter` keeps.
pub fn notes(lib: &Library, filter: &NotesFilter) -> NotesView {
    let marks: Vec<&Mark> = kept(lib).collect();
    let mut books = shelves(lib, &marks);
    let saved: Vec<_> = lib.books().flat_map(|book| lib.ai_selections(book.id)).collect();
    for item in &saved {
        if let Some(shelf) = books.iter_mut().find(|b| b.id == item.book.value) {
            shelf.count += 1;
        } else if let Some(book) = lib.book(item.book.value) {
            books.push(Shelf { id: book.id, title: book.title.value.clone(), format: book.format, count: 1 });
        }
    }
    books.sort_by(|a, b| a.title.cmp(&b.title).then(a.id.cmp(&b.id)));
    let book = filter.book.filter(|b| books.iter().any(|s| s.id == *b));
    let mine: Vec<&Mark> = marks
        .iter()
        .copied()
        .filter(|m| book.is_none_or(|b| m.book.value == b))
        .collect();
    let mut counts = Kept::ALL.map(|k| mine.iter().filter(|m| k.holds(m)).count());
    counts[0] += saved.iter().filter(|s| book.is_none_or(|id| id == s.book.value)).count();
    let terms = query::terms(&filter.text);
    let kept = |m: &&Mark| {
        let note = m.note.value.as_deref().unwrap_or("");
        filter.kept.holds(m) && query::keeps(&terms, &[&m.word, note])
    };
    let rows: Vec<&Mark> = mine.into_iter().filter(kept).collect();
    let mut ai_groups = Vec::new();
    if filter.kept == Kept::All {
        for shelf in &books {
            if book.is_some_and(|id| id != shelf.id) { continue; }
            let items: Vec<_> = saved.iter().filter(|s| s.book.value == shelf.id
                && query::keeps(&terms, &[&s.source, &s.result, &s.name]))
                .map(|s| (*s).clone()).collect();
            if !items.is_empty() { ai_groups.push(AiNoteGroup { book: shelf.id, title: shelf.title.clone(), rows: items }); }
        }
    }
    let ai_shown: usize = ai_groups.iter().map(|g| g.rows.len()).sum();
    NotesView {
        shown: rows.len() + ai_shown,
        groups: groups(lib, rows),
        ai_groups,
        total: marks.len() + saved.len(),
        books,
        counts,
    }
}

/// A group per book, the latest written in first, rows in reading order.
fn groups(lib: &Library, rows: Vec<&Mark>) -> Vec<NoteGroup> {
    let mut by: BTreeMap<BookId, Vec<&Mark>> = BTreeMap::new();
    for m in rows {
        by.entry(m.book.value).or_default().push(m);
    }
    let mut out: Vec<(u64, NoteGroup)> = by
        .into_iter()
        .map(|(book, mut marks)| {
            marks.sort_by_key(|m| (reading_order(&m.spot), m.added));
            let latest = marks.iter().map(|m| m.added).max().unwrap_or(0);
            let title = lib.book(book).map(|b| b.title.value.clone());
            let group = NoteGroup {
                book,
                title: title.unwrap_or_default(),
                rows: marks.into_iter().map(row).collect(),
            };
            (latest, group)
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.title.cmp(&b.1.title)));
    out.into_iter().map(|(_, g)| g).collect()
}

fn row(m: &Mark) -> NoteRow {
    let page = match m.spot {
        Spot::Page { page, .. } => Some(page),
        Spot::Chars { from, .. } => Some(from[0]),
        Spot::Text { .. } | Spot::Blocks { .. } => None,
    };
    NoteRow {
        id: m.id,
        book: m.book.value,
        quote: m.word.clone(),
        note: m.note.value.clone(),
        style: m.look(),
        bookmark: m.kind == MarkKind::Bookmark,
        spot: m.spot,
        page,
        added: m.added,
    }
}

/// The marks Space shows: living, in a held book, Explain's own left out.
pub(super) fn kept(lib: &Library) -> impl Iterator<Item = &Mark> {
    lib.every_mark()
        .filter(move |m| m.kind != MarkKind::Explain && lib.book(m.book.value).is_some())
}

/// Each book with a note, by title.
fn shelves(lib: &Library, marks: &[&Mark]) -> Vec<Shelf> {
    let mut counts: BTreeMap<BookId, usize> = BTreeMap::new();
    for m in marks {
        *counts.entry(m.book.value).or_default() += 1;
    }
    let mut out: Vec<Shelf> = counts
        .into_iter()
        .filter_map(|(id, count)| {
            let book = lib.book(id)?;
            Some(Shelf {
                id,
                title: book.title.value.clone(),
                format: book.format,
                count,
            })
        })
        .collect();
    out.sort_by(|a, b| a.title.cmp(&b.title).then(a.id.cmp(&b.id)));
    out
}
