//! The library domain: the books, the shelves they are filed on, the folders
//! watched for new ones, the cover-art cache, and the view the shelf renders
//! in.
//!
//! The RULES are not here — they are `library_core`, which is pure and
//! host-tested. This module is the reactive half: the signals those rules are
//! applied to, the import dock's task list, and the cover cache's own budget.
//!
//! Kept OUT of `Settings` on purpose, for the reason the previous schema gave
//! and this one inherits: reading position changes on every page turn, while
//! `Settings` is the appearance blob that repaints and re-serialises on every
//! write. The library's own view knobs (columns, sort, cover fit) change at the
//! same pace as a page turn, so they ride with the library rather than joining
//! the settings blob — one localStorage key, one write schedule.
//!
//! That schedule is unchanged: reading progress saves on a debounce through
//! `crate::effects::reader::reading_progress`, and the last moments before a
//! teardown (a document closing, a book leaving the shelf) write immediately
//! through `crate::storage::persist_library`, because a debounced save there is
//! a save that may never land.

use std::collections::HashSet;
use std::sync::Arc;

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use library_core::blob::LibraryBlob;
use library_core::book::Row;
use library_core::folder::{self as folder_ops, FolderMode, WatchedFolder};
use library_core::governance::Governance;
use library_core::id;
use library_core::shelf::{self, ALL_SHELF, Shelf};
use library_core::text::plural;
use library_core::view::LibraryView;
use library_core::wire::{ImportPhase, ImportProgress};

use crate::services::library::arrange::ShelfDepartureAsk;
use crate::services::library::conflict::{ConflictAsk, ShelfConflictAsk};
use crate::time::now_ms;

/// How many covers the cache holds. A cover is a base64 JPEG of a few tens of
/// kilobytes, so this — not [`BOOKS_CAP`](library_core::book::BOOKS_CAP)
/// — is the library's real memory and quota budget. Past it the least recently
/// read covers go; they are derived, and reopening a book renders its page 1
/// again.
/// Persisted cover art for one book: the first page rendered to a small JPEG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

/// The cover-art cache: page-1 JPEG data URLs keyed by the address a book is
/// read from. Behind an `Arc` because a cover is tens of kilobytes and the map
/// is read out of a signal on every shelf render and cloned whole before every
/// save; sharing the images makes those reads pointer copies.
pub type CoverMap = std::collections::HashMap<String, Arc<CoverImage>>;

/// Which half of an import a dock card is showing. The shell reports
/// [`ImportPhase`] for the two it can see; `Done` and `Failed` are the
/// frontend's, because only the side that owns the task list knows when the
/// whole run — scan, then copies, then the state write — has finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPhase {
    Scanning,
    Copying,
    Done,
    Failed,
}

impl TaskPhase {
    /// From a shell beat. The shell never says "done", so a beat always means
    /// still working.
    fn of(phase: ImportPhase) -> Self {
        match phase {
            ImportPhase::Scan => TaskPhase::Scanning,
            ImportPhase::Copy => TaskPhase::Copying,
        }
    }

    /// True once the card has nothing left to report and can leave the dock.
    pub fn is_finished(self) -> bool {
        matches!(self, TaskPhase::Done | TaskPhase::Failed)
    }
}

/// One card in the import dock.
///
/// Deliberately a plain value and not a projection of the shell's beat: the run
/// outlives the beat (it ends with a state write the shell knows nothing
/// about), and a card that only mirrored the last event would sit at "100%"
/// while the books were still being filed.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportTask {
    /// The id the shell echoes on every beat, so two runs in flight never mix.
    pub id: String,
    /// What the card calls the run: the folder's name, or "3 files".
    pub label: String,
    pub phase: TaskPhase,
    pub done: u32,
    pub total: u32,
    /// How many of the run's files went to the conflict sheet instead of
    /// landing — questions the reader still owes an answer to. The finished
    /// card says so rather than claiming an import that is waiting on
    /// somebody; see `crate::services::library::conflict`.
    pub waiting: u32,
    /// The file being worked on — the card's second line.
    pub name: String,
    pub error: Option<String>,
}

impl ImportTask {
    /// A run that has just been handed to the shell: scanning, total unknown.
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

    /// Fold one shell beat into the card. A beat for another run is the
    /// caller's problem (it looks the card up by id first).
    pub fn beat(&mut self, beat: &ImportProgress) {
        self.phase = TaskPhase::of(beat.phase);
        self.done = beat.done;
        self.total = beat.total;
        self.name = beat.name.clone();
    }

    /// The run finished: everything the shell reported is behind it.
    pub fn finish(&mut self) {
        self.phase = TaskPhase::Done;
        self.error = None;
    }

    /// The run could not finish.
    pub fn fail(&mut self, message: impl Into<String>) {
        self.phase = TaskPhase::Failed;
        self.error = Some(message.into());
    }

    /// Fraction complete, or `None` while the total is unknown — which is the
    /// whole of a scan, and is what makes the card's ring indeterminate.
    pub fn fraction(&self) -> Option<f64> {
        if self.total == 0 {
            return None;
        }
        Some((f64::from(self.done) / f64::from(self.total)).clamp(0.0, 1.0))
    }

    /// The number the ring prints, when there is one to print.
    pub fn percent(&self) -> Option<u32> {
        self.fraction().map(|f| (f * 100.0).round() as u32)
    }

    /// The card's first line: what is happening, and to how much.
    pub fn headline(&self) -> String {
        match self.phase {
            TaskPhase::Scanning => "Scanning…".to_string(),
            TaskPhase::Failed => "Import failed".to_string(),
            TaskPhase::Done => {
                // A run that raised questions is not an import that finished:
                // the books it asked about are waiting on the reader, and a
                // card that said "Imported" over them would be a promise the
                // shelf has to take back.
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

/// The library domain's signals.
///
/// Four of these persist together as one [`LibraryBlob`] and are separate
/// signals anyway: the shelf re-renders when a book's resume point moves, and
/// nothing else should. The rest are the page's own and a restart forgets them — a
/// search that survived would be one the reader did not type, a dock card that
/// survived would report an import that finished last week, and a selection that
/// survived would be a set of books the reader cannot see selected.
/// One "light this up" gesture: the row or shelf to scroll to, and the nonce
/// that makes a second reveal of the SAME thing a second reveal.
///
/// A named value rather than the `(id, nonce)` pair it replaced, because six
/// surfaces ask "am I the one being revealed" and each of them destructured the
/// pair to compare its first half. A counter that reads as `.1` at every one of
/// those sites is a fact nobody can see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reveal {
    /// The row's or the shelf's id. A shelf id is a letter apart from a book's
    /// ([`library_core::id::is_shelf`]), so one signal serves both kinds and the
    /// surfaces tell whose reveal is whose by the letter.
    pub id: String,
    /// Monotonic, so revealing one thing twice in a row works twice: a plain
    /// `Option<String>` would be unchanged by the second and notify nobody.
    pub nonce: u64,
}

/// Which sentence the "already a shelf here" note says. Both are a report and
/// a highlight — the import that could have asked a question put the shelf
/// back instead, or walked ground it already read and found every book
/// standing, and the note is how the reader is told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoteKind {
    /// The report's: a re-import of ground the library already reads in place
    /// — the tree's own root or any rung of it — DID walk and reconcile, and
    /// found nothing new: every book already stood and no log came back.
    NothingNew,
    /// The fold's: the import put a shelf BACK — the picked folder into the
    /// family its ground names, or a member an outer tree's walk found
    /// standing outside it — on the rung its directory names, with the folder
    /// that was reading it folded into the tree's ledger. The note names the
    /// shelf that went home, and the light rides the close onto the level
    /// that holds it now.
    Returned,
}

impl NoteKind {
    /// The line under the note's heading.
    pub fn sublabel(&self) -> &'static str {
        match self {
            NoteKind::NothingNew => "Nothing new to import",
            NoteKind::Returned => "Back where its folder names",
        }
    }
}

/// The "that folder is already a shelf here" note. None of its sentences is a
/// question: the modal's one job is to say the sentence and then light the
/// shelf up, and the highlight rides its CLOSE so a light cannot burn its
/// seconds behind a modal nobody has dismissed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlreadyNote {
    /// The shelf to reveal when the note closes.
    pub shelf_id: String,
    /// The name the modal speaks.
    pub name: String,
    pub kind: NoteKind,
}

/// One sheet's state: the question it is showing, and whether it is up.
///
/// The app's four sheets each spelled this pair on their own — a raise was
/// two writes in an order nothing enforced, a cancel was two more, and an
/// "open with nothing asked" was a state the type allowed and no reader could
/// explain. One pair, one [`raise`](Sheet::raise), one
/// [`dismiss`](Sheet::dismiss).
///
/// The signals stay public because two sheets close in ways that are more
/// than a dismiss: the conflict queue pops the next question into `ask`
/// without touching `open`, and the already-imported note closes with its ask
/// still standing, because the reveal rides the close and reads it.
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

    /// Put the question on screen.
    pub fn raise(&self, ask: T) {
        self.ask.set(Some(ask));
        self.open.set(true);
    }

    /// Take the sheet down, question and all.
    pub fn dismiss(&self) {
        self.ask.set(None);
        self.open.set(false);
    }
}

impl<T: Send + Sync + 'static> Default for Sheet<T> {
    fn default() -> Self {
        Self::new()
    }
}

// Hand-written rather than derived: `RwSignal` is `Copy` whatever it holds,
// and a derived `Copy` would demand `T: Copy` of questions that are values.
impl<T: Send + Sync + 'static> Clone for Sheet<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Send + Sync + 'static> Copy for Sheet<T> {}

/// The Find-again sheet's payload: the row whose address stopped resolving,
/// and the name its folder search looks for. The name is captured at the ask
/// rather than read at the click, so a rename that lands while the sheet is
/// up cannot change the question being answered.
#[derive(Clone, PartialEq)]
pub struct RelinkAsk {
    pub book_id: String,
    pub name: String,
}

#[derive(Clone, Copy)]
pub struct LibraryState {
    /// Every ROW, in the order the "All" shelf shows them: the books, and the
    /// links that point at them. This list IS the All order; there is no shelf
    /// row for it.
    pub books: RwSignal<Vec<Row>>,
    pub shelves: RwSignal<Vec<Shelf>>,
    pub folders: RwSignal<Vec<WatchedFolder>>,
    /// The view knobs, persisted with the books.
    pub view: RwSignal<LibraryView>,
    /// Cover art (page-1 JPEG data URLs) keyed by address.
    pub covers: RwSignal<CoverMap>,
    /// The titlebar search.
    pub query: RwSignal<String>,
    /// Which shelf the page is drilled into; [`ALL_SHELF`] at the root.
    pub shelf: RwSignal<String>,
    /// The import dock's cards, oldest first.
    pub tasks: RwSignal<Vec<ImportTask>>,
    /// The row or shelf to scroll to and light up, with a nonce so revealing the
    /// same one twice in a row re-triggers. Written by a "show it in its shelf"
    /// action, cleared by the shelf that scrolled to it. Ask
    /// [`Self::is_revealed`] rather than reading the signal: the nonce is the
    /// shelf's business and no surface wants it.
    pub reveal: RwSignal<Option<Reveal>>,
    /// Whether a long-press has put the shelf into multi-select. While it is on a
    /// click toggles instead of opening, and the action bar owns the bottom-right
    /// corner.
    pub selecting: RwSignal<bool>,
    /// The selected book ids. A set rather than a list because toggling is the
    /// high-frequency operation and "is this one selected" is asked by every card
    /// on every repaint.
    pub selected: RwSignal<HashSet<String>>,
    /// The name collision the sheet is asking about, waiting for the reader's
    /// answer. Raised by the services — a drop, a filing, an import — rather
    /// than by a component, which is why it lives here and not in a sheet's own
    /// handle: an import asks from inside a spawned future that outlived every
    /// component. See `crate::services::library::conflict`.
    pub conflict: Sheet<ConflictAsk>,
    /// The collisions behind the one on screen.
    ///
    /// One question at a time is the sheet's whole shape, and a batch — a drag
    /// of four books, an import of ten files — can raise several. Without
    /// somewhere to put the rest, the second raise would overwrite the first
    /// and a placement would vanish, which is the one thing the sheet exists
    /// to stop. Answering pops the next one onto the screen; Cancel drops them,
    /// which is what Cancel has always meant — the placements already answered
    /// keep their answers and the ones not asked simply do not land.
    pub conflict_waiting: RwSignal<Vec<ConflictAsk>>,
    /// The folder question a name collision at import raises: the shelf the
    /// level already holds, and the folder arriving under the same name. Its
    /// own sheet rather than a variant of [`Self::conflict`] because the two
    /// answer different arrivals — a book sheet's payload is an
    /// [`Arrival`](library_core::conflict::Arrival), and a folder has none
    /// yet: nothing has been measured when its name is the question.
    pub shelf_conflict: Sheet<ShelfConflictAsk>,
    /// The "that folder is already a shelf here" note: which shelf to light when
    /// it closes, the name the modal speaks, and which sentence it says. Two of
    /// the three are a report; the third is a question with a second answer.
    /// Closes WITHOUT a dismiss — the reveal on close reads the ask.
    pub already_imported: Sheet<AlreadyNote>,
    /// The shelf departure's question: a read-at-place shelf a hand is taking
    /// off the seat its folder's tree names for it, which is a move the library
    /// owes copies for. Its own sheet rather than a variant of the name sheet
    /// because nothing collides — no membership arrives on any level — and the
    /// question is the move's COST, with two answers: pay it, or leave the
    /// shelf where the tree put it. See `crate::services::library::arrange`.
    pub shelf_departure: Sheet<ShelfDepartureAsk>,
    /// The Find-again sheet: what a click on a book whose address died gets —
    /// the two doors that point the row at the file it is now (a pick, or a
    /// folder the app walks looking for the book's own name). Raised by the
    /// open's dead-address gate and the card's own button alike.
    pub relink: Sheet<RelinkAsk>,
}

impl Default for LibraryState {
    /// Hand-written for one field: the page opens drilled out of nothing, which
    /// is the root shelf, and [`ALL_SHELF`] is its id. A derived `String::new()`
    /// would be a shelf that does not exist, and the breadcrumb would render an
    /// empty crumb.
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
            shelf_departure: Sheet::new(),
            relink: Sheet::new(),
        }
    }
}

impl LibraryState {
    /// The library as one persisted value: an untracked read of every signal
    /// that belongs in the blob. Storage takes a value and writes it, so this
    /// is the only place the reactive graph and localStorage meet.
    pub fn snapshot(&self) -> LibraryBlob {
        LibraryBlob {
            books: self.books.get_untracked(),
            shelves: self.shelves.get_untracked(),
            folders: self.folders.get_untracked(),
            view: self.view.get_untracked(),
        }
    }

    /// The two lists a collision is asked of, read once and untracked: the
    /// rows and the shelves. One read of each rather than three nested reads
    /// is one chance to see a library, and a rule asked of a shelf list and a
    /// row list that were read at different moments can be asked of two.
    pub fn snapshot_rows(&self) -> (Vec<Row>, Vec<Shelf>) {
        (self.books.get_untracked(), self.shelves.get_untracked())
    }

    /// The row an id names, whichever kind it is — what an open, a drag and a
    /// removal all start from.
    pub fn row(&self, row_id: &str) -> Option<Row> {
        self.books
            .with_untracked(|rows| library_core::book::find_row(rows, row_id).cloned())
    }

    /// The name a row shows, and the name a collision compares. Empty for a
    /// row that is not there, which is what makes a drag of a row another
    /// surface just removed a no-op rather than a placement of nothing.
    pub fn row_name(&self, row_id: &str) -> String {
        self.row(row_id).map_or_else(String::new, |r| r.display_name())
    }

    /// The name a shelf has right now, read untracked. Empty for a shelf the
    /// list no longer holds and for the root, which is not a shelf — the answer
    /// [`shelf::find`] gives, and the reason the callers that used to walk the
    /// list for it now ask here: a hand-rolled `find(|s| s.id == id)` answers the
    /// root by luck, while `sanitize` happens to drop a row wearing that id.
    pub fn shelf_name(&self, shelf_id: &str) -> String {
        self.shelves.with_untracked(|shelves| {
            shelf::find(shelves, shelf_id).map_or_else(String::new, |s| s.name.clone())
        })
    }

    /// The same, reactively: what a folder card, a tree row and a crumb all
    /// paint their own name from, so a rename reaches every one of them on the
    /// frame it happens. One spelling rather than one derive per surface, and
    /// one place that knows a shelf's name is `shelf::find`'s answer.
    pub fn shelf_name_signal(&self, shelf_id: &str) -> Signal<String> {
        let shelves = self.shelves;
        let id = shelf_id.to_string();
        Signal::derive(move || {
            shelves.with(|list| shelf::find(list, &id).map_or_else(String::new, |s| s.name.clone()))
        })
    }

    /// Whether `id` is the row or shelf being revealed right now — the light a
    /// book card, a folder card, a link and a tree row all paint from.
    pub fn is_revealed(&self, id: &str) -> Signal<bool> {
        let reveal = self.reveal;
        let id = id.to_string();
        Signal::derive(move || reveal.with(|at| at.as_ref().is_some_and(|each| each.id == id)))
    }

    /// Whether `id` is in the page's selection — the check mark on a cover, on a
    /// folder's plate and on a row's thumbnail. The same set the shelf item's
    /// own selected class reads, so a card cannot check a book it is not dimmed
    /// for.
    pub fn is_selected(&self, id: &str) -> Signal<bool> {
        let selected = self.selected;
        let id = id.to_string();
        Signal::derive(move || selected.with(|set| set.contains(&id)))
    }

    /// The watched folder a shelf was cut from, when it was cut from one.
    ///
    /// The question four call sites asked by walking the shelf list for a row
    /// and then reading its kind: whether a move is a departure (a book leaving
    /// ground its folder owns), whether a landing is a return (a copy back on a
    /// shelf of the folder it left), which folder's import menu a card belongs
    /// to, and what a folder link's row says. One answer here, so the four
    /// cannot drift about what "this shelf's folder" means.
    pub fn shelf_folder_id(&self, shelf_id: &str) -> Option<String> {
        self.shelves.with_untracked(|shelves| {
            shelf::find(shelves, shelf_id).and_then(|s| s.kind.folder_id().map(str::to_string))
        })
    }

    /// Whether a shelf is tracked — the watch dot's one rule, for the four
    /// surfaces that draw or read one (a folder card, a breadcrumb crumb, a
    /// removal receipt's shelf line, and the shelf's own menu).
    ///
    /// A shelf of a read-at-place folder answers with that folder's decision for
    /// the rung the shelf stands on, so a subfolder turned off under a tracked
    /// root stops showing a dot while the tree above it keeps watching. A shelf
    /// the reader made inside such a tree answers with the tree it stands in.
    /// `false` for anything no read-at-place folder answers for — the reader's own
    /// shelf on the reader's own ground, a shelf of a copying folder, and the
    /// pseudo-shelf — because none of them has a dot to draw.
    ///
    /// Reactive: it reads both lists, so a card repaints on the frame a hand
    /// turns the folder's tracking off rather than at the next mount.
    pub fn shelf_tracked(&self, shelf_id: &str) -> bool {
        self.folders.with(|folders| {
            self.shelves
                .with(|shelves| Governance::new(folders, shelves).shelf_tracked(shelf_id))
        }) == Some(true)
    }

    /// The MODE of the folder whose tree this shelf stands in, when it stands
    /// in one: `Copy` for a shelf cut from a folder the library copies into
    /// its own store, a read-at-place mode for a shelf that is a door onto a
    /// directory on disk, and `None` for a shelf the reader made themselves.
    ///
    /// The folder's seat is what answers (`Governance::seat_of`), so a shelf
    /// nested inside a tree reports the tree's mode — the same seat
    /// [`Self::shelf_tracked`] asks about — and the card that draws it can say
    /// where its books are without walking the shelf's kind by hand. What it
    /// is FOR is the one difference a reader has to know before removing a
    /// shelf: the books of a copying shelf are the library's own, and the
    /// books of a read-at-place shelf are not.
    ///
    /// Reactive, the way [`Self::shelf_tracked`] is: a rescan that turns a
    /// folder around, or a hand that re-picks one, repaints the badge.
    pub fn shelf_mode(&self, shelf_id: &str) -> Option<FolderMode> {
        self.folders.with(|folders| {
            let seat = self
                .shelves
                .with(|shelves| Governance::new(folders, shelves).seat_of(shelf_id))?;
            folder_ops::find(folders, &seat.folder_id).map(|f| f.mode())
        })
    }

    /// [`shelf_tracked`] without the subscription, for a caller that is building
    /// a snapshot rather than painting — a removal receipt reads the library once
    /// and writes a value the sheet prints, and subscribing there would tie a
    /// one-shot read to the reactive graph for nothing.
    pub fn shelf_tracked_untracked(&self, shelf_id: &str) -> bool {
        self.folders.with_untracked(|folders| {
            self.shelves.with_untracked(|shelves| {
                Governance::new(folders, shelves).shelf_tracked(shelf_id)
            })
        }) == Some(true)
    }

    /// The watched folder an id names, cloned and read untracked: what the
    /// folder's import menu and the covered sheet both start from.
    pub fn folder(&self, folder_id: &str) -> Option<WatchedFolder> {
        self.folders
            .with_untracked(|folders| folder_ops::find(folders, folder_id).cloned())
    }

    /// Give a row a new name: a book's title, a link's own. What the sheet's
    /// *add as new* answer does to a row it is moving, and what the shelf's
    /// rename sheet does to a row the reader pointed at.
    ///
    /// A book's new title is LOCKED, and the lock is the difference between a
    /// name the reader chose and a name a document supplied: the load-time
    /// sweep drops a title shaped like a filename because that shape is
    /// download debris wearing a title's clothes, and a name typed at the
    /// shelf is not debris whatever it looks like
    /// (`library_core::book::Book::title_locked`). The persist is the caller's,
    /// which is why the two callers each write the blob right after.
    pub fn rename_row(&self, row_id: &str, name: &str) {
        self.books.update(|rows| {
            let Some(row) = library_core::book::find_row_mut(rows, row_id) else {
                return;
            };
            match row {
                Row::Book(b) => {
                    b.title = Some(name.to_string());
                    b.title_locked = true;
                }
                Row::Link { name: own, .. } => *own = name.to_string(),
            }
        });
    }

    /// Put a link to `target` on `shelf_id`, and return its id.
    ///
    /// The name is the target's own at this moment, which is what makes the
    /// row recognisable on the shelf beside the book it points at; a rename of
    /// the book later does not rewrite it, because a link is a row the reader
    /// placed and not a view of another row.
    pub fn add_link(&self, name: &str, target: &str, shelf_id: &str) -> String {
        let now = now_ms();
        let link_id = id::next_id(now);
        let made = link_id.clone();
        self.books.update(|rows| {
            rows.push(Row::link(link_id, name.to_string(), target.to_string(), now));
        });
        // The root is not a shelf and has no member list, so a link made "on
        // All" is filed nowhere — and the write is skipped rather than made and
        // answered with nothing.
        if shelf_id != ALL_SHELF {
            self.shelves.update(|shelves| {
                if let Some(shelf) = shelf::find_mut(shelves, shelf_id) {
                    shelf::shelf_add(shelf, &made);
                }
            });
        }
        crate::storage::persist_library(*self);
        made
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
        // Ten files measured, eight landed, two went to the conflict sheet:
        // the card counts what landed and ends on what is still owed.
        let mut task = ImportTask::new("t1", "10 files");
        task.total = 8;
        task.done = 8;
        task.waiting = 2;
        task.finish();
        assert_eq!(task.headline(), "2 books waiting for your choice");
        // One question reads as one book...
        task.waiting = 1;
        assert_eq!(task.headline(), "1 book waiting for your choice");
        // ...and an import that asked nothing claims its books as before.
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
