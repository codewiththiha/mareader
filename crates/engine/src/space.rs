//! Space: every studied word across books, filtered, grouped and sorted.
//!
//! [`view`] reads the library into rows for one [`Filter`]: the window keeps
//! the filter, the session keeps the words. In [`Scope::Books`] a row is one
//! book's sighting; [`Scope::Merged`] folds a word's sightings into one row.
//! A word's status holds in every book, so each action acts on the word.

mod csv;
mod notes;
mod query;
mod room;
mod seed;
mod seed_marks;
mod tally;

use mareader_library::{BookId, Format, LearnedBy, MeaningBy, RecallAnswer, WordId, WordStatus};

pub use csv::{CSV_NAME, csv};
pub use notes::{Kept, NoteGroup, NoteRow, NotesFilter, NotesView, notes};
pub use query::{date, day, detail_row, history, view};
pub use room::Room;
pub(crate) use seed::seed;
pub(crate) use seed_marks::seed_marks;
pub use tally::{Tally, tally};

/// What Space asks of words; each acts in every book.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Never shaded or carded again.
    Memorize(Vec<WordId>),
    /// Memorized words go back to learning; the rest stay.
    Unmemorize(Vec<WordId>),
    /// Gone from Space, and carded again when met.
    Forget(Vec<WordId>),
    /// Remove the selected book-word occurrences and their study history.
    RemoveOccurrences(Vec<(BookId, WordId)>),
}

/// A row per book's sighting, or a row per word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    Books,
    Merged,
}

impl Scope {
    pub const ALL: [Scope; 2] = [Scope::Books, Scope::Merged];

    pub fn label(self) -> &'static str {
        match self {
            Scope::Books => "With books",
            Scope::Merged => "Merged",
        }
    }
}

/// The status tabs above the rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    All,
    Memorized,
    Learning,
    Missed,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::All, Tab::Memorized, Tab::Learning, Tab::Missed];

    pub fn label(self) -> &'static str {
        match self {
            Tab::All => "All",
            Tab::Memorized => status_label(WordStatus::Memorized),
            Tab::Learning => status_label(WordStatus::Learning),
            Tab::Missed => status_label(WordStatus::Missed),
        }
    }

    fn holds(self, status: WordStatus) -> bool {
        match self {
            Tab::All => true,
            Tab::Memorized => status == WordStatus::Memorized,
            Tab::Learning => status == WordStatus::Learning,
            Tab::Missed => status == WordStatus::Missed,
        }
    }
}

/// What the rows are grouped under.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Group {
    None,
    /// Not offered when merged: a merged row has many books.
    #[default]
    Book,
    Level,
    Day,
    Month,
    Status,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Group::None,
        Group::Book,
        Group::Level,
        Group::Day,
        Group::Month,
        Group::Status,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Group::None => "None",
            Group::Book => "Book",
            Group::Level => "Level",
            Group::Day => "Day",
            Group::Month => "Month",
            Group::Status => "Status",
        }
    }
}

/// The rows' order inside each group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Recent,
    Az,
    Hardest,
    Tries,
    Book,
}

impl Sort {
    pub const ALL: [Sort; 5] = [
        Sort::Recent,
        Sort::Az,
        Sort::Hardest,
        Sort::Tries,
        Sort::Book,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Sort::Recent => "Recent",
            Sort::Az => "A–Z",
            Sort::Hardest => "Hardest first",
            Sort::Tries => "Most tries",
            Sort::Book => "Book",
        }
    }
}

/// Which words show, and how; empty fields filter nothing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    /// Matched in the lemma, the meaning and the sentence, any case.
    pub text: String,
    pub book: Option<BookId>,
    /// CEFR bands, 1 (A1) to 6 (C2).
    pub levels: Vec<u8>,
    pub by: Option<LearnedBy>,
    /// Who wrote the meaning; older rows, which don't say, show only for none.
    pub meaning_by: Option<MeaningBy>,
    pub pos: Option<String>,
    pub tab: Tab,
    pub scope: Scope,
    pub group: Group,
    pub sort: Sort,
}

/// One word, as one book met it or as every book did.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Unique in a view: the sighting's id, or the word's when merged.
    pub key: u128,
    pub word: WordId,
    pub lemma: String,
    pub pos: Option<String>,
    pub band: u8,
    pub status: WordStatus,
    pub meaning: String,
    /// The book's sentence around it.
    pub context: String,
    /// The form appearing in the chosen sentence.
    pub form: String,
    /// The titles of the books it was met in, most recent first.
    pub books: Vec<String>,
    /// The ids of `books`, in the same order.
    pub book_ids: Vec<BookId>,
    /// Where `context` was met, to open its book there.
    pub met_at: Option<MetAt>,
    /// When it was last met, in wall milliseconds.
    pub seen: u64,
    /// Answers given, summed over books; 0 for cards.
    pub tries: u32,
    pub by: LearnedBy,
    pub first_try: bool,
    pub synonyms: Vec<String>,
    pub usages: Vec<String>,
}

/// One recall session in a book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    pub book: String,
    pub when: u64,
    pub answers: Vec<RecallAnswer>,
}

/// A sentence's book, and the words around the word to find it by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetAt {
    pub book: BookId,
    pub quote: String,
}

/// Rows under one heading; the heading is empty when ungrouped.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    pub label: String,
    pub rows: Vec<Row>,
}

/// Rows per status tab, after every filter but the tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub all: usize,
    pub memorized: usize,
    pub learning: usize,
    pub missed: usize,
}

impl Counts {
    pub fn of(self, tab: Tab) -> usize {
        match tab {
            Tab::All => self.all,
            Tab::Memorized => self.memorized,
            Tab::Learning => self.learning,
            Tab::Missed => self.missed,
        }
    }
}

/// Recall answers and memorized words per level, after the filters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// Reactive sightings answered on the first try.
    pub first_try: usize,
    /// Reactive sightings answered at all.
    pub answered: usize,
    /// Memorized rows at B1, B2, C1 and C2.
    pub by_level: [(u8, usize); 4],
}

impl Stats {
    /// First-try answers as a whole percentage; 0 with none answered.
    pub fn first_try_percent(&self) -> usize {
        (self.first_try * 100)
            .checked_div(self.answered)
            .unwrap_or(0)
    }
}

/// A book in the rail, with its sightings counted.
#[derive(Clone, Debug, PartialEq)]
pub struct Shelf {
    pub id: BookId,
    pub title: String,
    pub format: Format,
    pub count: usize,
}

/// Everything Space shows for one filter.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub sections: Vec<Section>,
    pub counts: Counts,
    pub stats: Stats,
    /// Each book with a sighting, by title.
    pub books: Vec<Shelf>,
    /// Every sighting, unfiltered: the rail's All books.
    pub total: usize,
    /// Each class a word has, sorted.
    pub poses: Vec<String>,
}

impl View {
    /// The rows shown, in order, across sections.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.sections.iter().flat_map(|s| &s.rows)
    }
}

/// The bands the level chips offer: B1 to C2.
pub const BANDS: [u8; 4] = [3, 4, 5, 6];

pub fn status_label(status: WordStatus) -> &'static str {
    match status {
        WordStatus::Memorized => "Memorized",
        WordStatus::Learning => "Learning",
        WordStatus::Missed => "Missed",
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod notes_tests;

#[cfg(test)]
mod tally_tests;
