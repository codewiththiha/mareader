//! The library domain: the books, the shelves they are filed on, the folders
//! watched for new ones, the cover-art cache, and the view the shelf renders
//! in.
//!
//! The rules are not here — they are `library_core`, pure and host-tested.
//! This module is the reactive half: the signals those rules are applied to.

use std::collections::HashSet;
use std::sync::Arc;

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use library_core::blob::LibraryBlob;
use library_core::book::Row;
use library_core::folder::{self as folder_ops, FolderMode, WatchedFolder};
use library_core::governance::Governance;
use library_core::shelf::{self, ALL_SHELF, Shelf};
use library_core::text::plural;
use library_core::view::LibraryView;
use library_core::wire::{ImportPhase, ImportProgress};

use crate::services::library::arrange::CopyAsk;
use crate::services::library::conflict::{ConflictAsk, ShelfConflictAsk};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

/// Behind an `Arc`: a cover is tens of kilobytes, and the map is read out of
/// a signal on every shelf render and cloned whole before every save.
pub type CoverMap = std::collections::HashMap<String, Arc<CoverImage>>;

/// The shell reports [`ImportPhase`] for the two phases it can see; `Done`
/// and `Failed` are the frontend's, because only the side that owns the task
/// list knows when the whole run has finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPhase {
    Scanning,
    Copying,
    Done,
    Failed,
}

impl TaskPhase {
    fn of(phase: ImportPhase) -> Self {
        match phase {
            ImportPhase::Scan => TaskPhase::Scanning,
            ImportPhase::Copy => TaskPhase::Copying,
        }
    }

    pub fn is_finished(self) -> bool {
        matches!(self, TaskPhase::Done | TaskPhase::Failed)
    }
}

/// Deliberately a plain value, not a projection of the shell's beat: the run
/// ends with a state write the shell knows nothing about, and a card that
/// only mirrored the last event would sit at "100%" while books were still
/// being filed.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportTask {
    pub id: String,
    pub label: String,
    pub phase: TaskPhase,
    pub done: u32,
    pub total: u32,
    /// Questions the reader still owes an answer to; see
    /// `crate::services::library::conflict`.
    pub waiting: u32,
    pub name: String,
    pub error: Option<String>,
}

impl ImportTask {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            phase: TaskPhase::Scanning,
            done: 0,
            total: 0,
            waiting: 0,
            name: String::new(),
            error: None,
        }
    }

    pub fn beat(&mut self, beat: &ImportProgress) {
        self.phase = TaskPhase::of(beat.phase);
        self.done = beat.done;
        self.total = beat.total;
        self.name = beat.name.clone();
    }

    pub fn finish(&mut self) {
        self.phase = TaskPhase::Done;
        self.error = None;
    }

    pub fn fail(&mut self, message: impl Into<String>) {
        self.phase = TaskPhase::Failed;
        self.error = Some(message.into());
    }

    pub fn fraction(&self) -> Option<f64> {
        if self.total == 0 {
            return None;
        }
        Some((f64::from(self.done) / f64::from(self.total)).clamp(0.0, 1.0))
    }

    pub fn percent(&self) -> Option<u32> {
        self.fraction().map(|f| (f * 100.0).round() as u32)
    }

    pub fn headline(&self) -> String {
        match self.phase {
            TaskPhase::Scanning => "Scanning…".to_string(),
            TaskPhase::Failed => "Import failed".to_string(),
            TaskPhase::Done => {
                if self.waiting > 0 {
                    format!(
                        "{} waiting for your choice",
                        plural(self.waiting as usize, "book", "books")
                    )
                } else {
                    format!("Imported {}", plural(self.total as usize, "book", "books"))
                }
            }
            TaskPhase::Copying => match self.total {
                0 => "Importing…".to_string(),
                n => format!("Importing {} of {n}", self.done.min(n)),
            },
        }
    }
}

/// The row the shelf is lighting up. Asked through the one shell every shelf
/// item draws ([`crate::features::library::entry`]), so the pair is read in
/// one place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reveal {
    /// A shelf id is a letter apart from a book's
    /// ([`library_core::id::is_shelf`]), so one signal serves both kinds.
    pub id: String,
    /// Monotonic, so revealing one thing twice in a row works twice: a plain
    /// `Option<String>` would be unchanged by the second and notify nobody.
    pub nonce: u64,
}

/// Both kinds are a report and a highlight: the note tells the reader what
/// the import did instead of asking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteKind {
    /// A re-import of ground the library already reads in place did walk and
    /// reconcile, and found nothing new.
    NothingNew,
    Returned,
}

impl NoteKind {
    pub fn sublabel(&self) -> &'static str {
        match self {
            NoteKind::NothingNew => "Nothing new to import",
            NoteKind::Returned => "Back where its folder names",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlreadyNote {
    pub shelf_id: String,
    pub name: String,
    pub kind: NoteKind,
}

/// One sheet's state: the question it is showing, and whether it is up. One
/// spelling of the pair, so a raise cannot be two writes in the wrong order
/// and "open with nothing asked" is not a state the type allows.
pub struct Sheet<T: Send + Sync + 'static> {
    pub ask: RwSignal<Option<T>>,
    pub open: RwSignal<bool>,
}

impl<T: Send + Sync + 'static> Sheet<T> {
    pub fn new() -> Self {
        Self {
            ask: RwSignal::new(None),
            open: RwSignal::new(false),
        }
    }

    pub fn raise(&self, ask: T) {
        self.ask.set(Some(ask));
        self.open.set(true);
    }

    pub fn dismiss(&self) {
        self.ask.set(None);
        self.open.set(false);
    }

    /// Close without spending the ask, for the one sheet whose close is an
    /// answer in itself (the already-imported note lights its shelf on the way
    /// down): the consuming effect reads the ask after this.
    pub fn lower(&self) {
        self.open.set(false);
    }
}

impl<T: Send + Sync + 'static> Default for Sheet<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Hand-written rather than derived: `RwSignal` is `Copy` whatever it holds.
impl<T: Send + Sync + 'static> Clone for Sheet<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Send + Sync + 'static> Copy for Sheet<T> {}

/// The name is captured at the ask rather than read at the click, so a
/// rename landing while the sheet is up cannot change the question being
/// answered.
#[derive(Clone, PartialEq)]
pub struct RelinkAsk {
    pub book_id: String,
    pub name: String,
}

/// Four of these persist together as one [`LibraryBlob`] and are separate signals anyway: the
/// shelf re-renders when a book's resume point moves, and nothing else should.
#[derive(Clone, Copy)]
pub struct LibraryState {
    pub books: RwSignal<Vec<Row>>,
    pub shelves: RwSignal<Vec<Shelf>>,
    pub folders: RwSignal<Vec<WatchedFolder>>,
    pub view: RwSignal<LibraryView>,
    pub covers: RwSignal<CoverMap>,
    pub query: RwSignal<String>,
    pub shelf: RwSignal<String>,
    pub tasks: RwSignal<Vec<ImportTask>>,
    /// Written by a "show it in its shelf" action, cleared by the shelf that
    /// scrolled to it. Ask [`Self::is_revealed`] rather than reading the
    /// signal.
    pub reveal: RwSignal<Option<Reveal>>,
    pub selecting: RwSignal<bool>,
    /// A set rather than a list: toggling is the high-frequency operation and
    /// membership is asked by every card on every repaint.
    pub selected: RwSignal<HashSet<String>>,
    /// Raised by the services — a drop, a filing, an import — not by a
    /// component: an import asks from inside a spawned future that outlived
    /// every component.
    pub conflict: Sheet<ConflictAsk>,
    /// One question at a time is the sheet's whole shape, and a batch can
    /// raise several: without somewhere to put the rest, the second raise
    /// would overwrite the first and a placement would vanish.
    pub conflict_waiting: RwSignal<Vec<ConflictAsk>>,
    /// Its own sheet rather than a variant of [`Self::conflict`]: a folder
    /// has no [`Arrival`](library_core::conflict::Arrival) yet — nothing has
    /// been measured when its name is the question.
    pub shelf_conflict: Sheet<ShelfConflictAsk>,
    /// Closes without a dismiss — the reveal on close reads the ask.
    pub already_imported: Sheet<AlreadyNote>,
    /// One sheet for every copy the library is about to make of a book it
    /// reads in place: a book's move, a shelf's move, a level coming apart
    /// and a shelf leaving the list are one cost, so they are one question.
    pub copy_ask: Sheet<CopyAsk>,
    pub relink: Sheet<RelinkAsk>,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            books: RwSignal::new(Vec::new()),
            shelves: RwSignal::new(Vec::new()),
            folders: RwSignal::new(Vec::new()),
            view: RwSignal::new(LibraryView::default()),
            covers: RwSignal::new(CoverMap::default()),
            query: RwSignal::new(String::new()),
            shelf: RwSignal::new(ALL_SHELF.to_string()),
            tasks: RwSignal::new(Vec::new()),
            reveal: RwSignal::new(None),
            selecting: RwSignal::new(false),
            selected: RwSignal::new(HashSet::new()),
            conflict: Sheet::new(),
            conflict_waiting: RwSignal::new(Vec::new()),
            shelf_conflict: Sheet::new(),
            already_imported: Sheet::new(),
            copy_ask: Sheet::new(),
            relink: Sheet::new(),
        }
    }
}

impl LibraryState {
    pub fn snapshot(&self) -> LibraryBlob {
        LibraryBlob {
            books: self.books.get_untracked(),
            shelves: self.shelves.get_untracked(),
            folders: self.folders.get_untracked(),
            view: self.view.get_untracked(),
        }
    }

    /// One read of each list rather than nested reads: a rule asked of lists
    /// read at different moments can be asked of two different libraries.
    pub fn snapshot_rows(&self) -> (Vec<Row>, Vec<Shelf>) {
        (self.books.get_untracked(), self.shelves.get_untracked())
    }

    pub fn row(&self, row_id: &str) -> Option<Row> {
        self.books
            .with_untracked(|rows| library_core::book::find_row(rows, row_id).cloned())
    }

    /// Empty for a row that is not there, which makes a drag of a row another
    /// surface just removed a no-op.
    pub fn row_name(&self, row_id: &str) -> String {
        self.row(row_id).map_or_else(String::new, |r| r.display_name())
    }

    /// Empty for a shelf the list no longer holds and for the root, which is not a shelf.
    pub fn shelf_name(&self, shelf_id: &str) -> String {
        self.shelves.with_untracked(|shelves| {
            shelf::find(shelves, shelf_id).map_or_else(String::new, |s| s.name.clone())
        })
    }

    pub fn shelf_name_signal(&self, shelf_id: &str) -> Signal<String> {
        let shelves = self.shelves;
        let id = shelf_id.to_string();
        Signal::derive(move || {
            shelves.with(|list| shelf::find(list, &id).map_or_else(String::new, |s| s.name.clone()))
        })
    }

    /// The row's own name, read back by id: a keyed card is not re-created
    /// when its content changes, so a name captured at the mount goes stale.
    pub fn row_name_signal(&self, row_id: &str) -> Signal<String> {
        let books = self.books;
        let id = row_id.to_string();
        Signal::derive(move || {
            books.with(|rows| {
                library_core::book::find_row(rows, &id)
                    .map_or_else(String::new, |r| r.display_name())
            })
        })
    }

    pub fn is_revealed(&self, id: &str) -> Signal<bool> {
        let reveal = self.reveal;
        let id = id.to_string();
        Signal::derive(move || reveal.with(|at| at.as_ref().is_some_and(|each| each.id == id)))
    }

    pub fn is_selected(&self, id: &str) -> Signal<bool> {
        let selected = self.selected;
        let id = id.to_string();
        Signal::derive(move || selected.with(|set| set.contains(&id)))
    }

    /// The folder a shelf is a rung of, if any: what decides whether a move
    /// is a departure and whether a landing is a return.
    pub fn shelf_folder_id(&self, shelf_id: &str) -> Option<String> {
        self.shelves.with_untracked(|shelves| {
            shelf::find(shelves, shelf_id).and_then(|s| s.kind.folder_id().map(str::to_string))
        })
    }

    /// That folder's tracking decision for the rung the shelf stands on, so
    /// a subfolder turned off under a tracked root stops showing a dot while
    /// the tree above it keeps watching.
    pub fn shelf_tracked(&self, shelf_id: &str) -> bool {
        self.folders.with(|folders| {
            self.shelves
                .with(|shelves| Governance::new(folders, shelves).shelf_tracked(shelf_id))
        })
    }

    /// `Copy` for a shelf the library keeps copies of, a read-at-place mode
    /// for a shelf that is a door onto a directory on disk, and `None` for a
    /// shelf no folder answers for.
    pub fn shelf_mode(&self, shelf_id: &str) -> Option<FolderMode> {
        self.folders.with(|folders| {
            self.shelves
                .with(|shelves| Governance::new(folders, shelves).mode_of(shelf_id))
        })
    }

    /// What a shelf actually holds: on-disk, stored copies, mixed, or nothing.
    /// The badge rule the folder surfaces paint from — the shelf's own members
    /// decide it, and a shelf holding no books falls back to its subtree, which
    /// is what keeps a duplicated folder from reading "On disk" when every
    /// book it holds is a stored copy.
    pub fn shelf_content_kind(&self, shelf_id: &str) -> library_core::shelf::ContentKind {
        self.shelves.with(|shelves| {
            self.books
                .with(|rows| shelf::badge_kind(rows, shelves, shelf_id))
        })
    }

    pub fn shelf_tracked_untracked(&self, shelf_id: &str) -> bool {
        self.folders.with_untracked(|folders| {
            self.shelves.with_untracked(|shelves| {
                Governance::new(folders, shelves).shelf_tracked(shelf_id)
            })
        })
    }

    pub fn folder(&self, folder_id: &str) -> Option<WatchedFolder> {
        self.folders
            .with_untracked(|folders| folder_ops::find(folders, folder_id).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_beat_moves_the_card_and_never_finishes_it() {
        let mut task = ImportTask::new("t1", "Books");
        assert_eq!(task.phase, TaskPhase::Scanning);
        assert_eq!(task.fraction(), None, "a scan has no total yet");
        assert_eq!(task.headline(), "Scanning…");

        task.beat(&ImportProgress {
            task: "t1".into(),
            phase: ImportPhase::Copy,
            done: 12,
            total: 48,
            name: "dune.pdf".into(),
        });
        assert_eq!(task.phase, TaskPhase::Copying);
        assert_eq!(task.percent(), Some(25));
        assert_eq!(task.headline(), "Importing 12 of 48");
        assert_eq!(task.name, "dune.pdf");
        assert!(!task.phase.is_finished());

        task.finish();
        assert_eq!(task.phase, TaskPhase::Done);
        assert_eq!(task.headline(), "Imported 48 books");
        assert!(task.phase.is_finished());
    }

    #[test]
    fn a_failed_run_says_so_and_stops_counting() {
        let mut task = ImportTask::new("t1", "Books");
        task.fail("no such folder");
        assert_eq!(task.phase, TaskPhase::Failed);
        assert_eq!(task.error.as_deref(), Some("no such folder"));
        assert_eq!(task.headline(), "Import failed");
        assert!(task.phase.is_finished());
    }

    #[test]
    fn one_book_reads_as_one_book() {
        let mut task = ImportTask::new("t1", "Books");
        task.beat(&ImportProgress {
            task: "t1".into(),
            phase: ImportPhase::Copy,
            done: 1,
            total: 1,
            name: "a.pdf".into(),
        });
        task.finish();
        assert_eq!(task.headline(), "Imported 1 book");
    }

    #[test]
    fn a_run_that_asked_ends_on_the_question() {
        let mut task = ImportTask::new("t1", "10 files");
        task.total = 8;
        task.done = 8;
        task.waiting = 2;
        task.finish();
        assert_eq!(task.headline(), "2 books waiting for your choice");
        task.waiting = 1;
        assert_eq!(task.headline(), "1 book waiting for your choice");
        task.waiting = 0;
        assert_eq!(task.headline(), "Imported 8 books");
    }

    #[test]
    fn a_count_past_the_total_clamps_rather_than_boasts() {
        let mut task = ImportTask::new("t1", "Books");
        task.beat(&ImportProgress {
            task: "t1".into(),
            phase: ImportPhase::Copy,
            done: 40,
            total: 10,
            name: String::new(),
        });
        assert_eq!(task.percent(), Some(100));
        assert_eq!(task.headline(), "Importing 10 of 10");
    }
}
