//! Loose books an AI files on shelves: what it's asked, and its plan.

use std::collections::BTreeSet;

use mareader_library::{Book, BookId, Library, Origin, ShelfId};

use super::counted::counted;
use super::runs::{Beat, Phase, RunId};
use super::{LibraryState, Pick, ShelfAction};
use crate::command::{Command, Commands};
use crate::toast::Toast;

/// A book the model may file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loose {
    pub id: BookId,
    pub title: String,
    pub author: Option<String>,
}

/// A shelf there already, which the model may file onto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
    pub id: ShelfId,
    pub name: String,
    pub parent: Option<ShelfId>,
}

/// What the model is asked to organize.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub books: Vec<Loose>,
    pub shelves: Vec<Standing>,
    /// Every loose book, so books loose later join a resumed run.
    pub whole: bool,
}

/// A run a failure, a stop or quitting left, kept to carry on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paused {
    pub done: usize,
    pub total: usize,
    pub books: BTreeSet<BookId>,
    pub whole: bool,
    /// Every step is done: only its plan waits to be reviewed.
    pub ready: bool,
}

impl Paused {
    /// How far it got, for menus and its card.
    pub fn said(&self) -> String {
        let books = counted(self.books.len(), "book", "books");
        match self.ready {
            true => format!("{books} · plan ready"),
            false => format!("{} of {} steps · {books}", self.done, self.total),
        }
    }

    /// How far it got, in a few words for a menu row.
    pub fn short(&self) -> String {
        match self.ready {
            true => "plan ready".to_owned(),
            false => format!("{} of {}", self.done, self.total),
        }
    }
}

/// A shelf in a plan: one there already, or the plan's `n`th new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Spot {
    Shelf(ShelfId),
    New(usize),
}

/// The model's plan; a new shelf's parent always comes before it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Proposal {
    pub shelves: Vec<(String, Option<Spot>)>,
    pub placed: Vec<(BookId, Spot)>,
    pub left: Vec<BookId>,
}

/// What applying a plan did, for its Undo.
/// Where a plan puts one book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    /// Filed on the shelf of this name, new or standing.
    Shelf(String),
    /// Left where it is.
    Stays,
}

impl Proposal {
    /// Where this plan puts `book`, if the plan names it.
    pub fn bound(&self, model: &Library, book: BookId) -> Option<Bound> {
        if self.left.contains(&book) {
            return Some(Bound::Stays);
        }
        let (_, spot) = self.placed.iter().find(|(b, _)| *b == book)?;
        let name = match *spot {
            Spot::Shelf(id) => model.shelf(id)?.name.value.clone(),
            Spot::New(i) => self.shelves.get(i)?.0.clone(),
        };
        Some(Bound::Shelf(name))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub filed: Vec<(BookId, ShelfId)>,
    /// New shelves, parents first.
    pub made: Vec<ShelfId>,
    /// Books that changed meanwhile and stayed put.
    pub skipped: usize,
}

/// News from the model, as the library takes it.
#[derive(Clone, Debug, PartialEq)]
pub enum Heard {
    /// Requests done of `total`; `filing` once shelves are planned.
    Progress {
        filing: bool,
        done: usize,
        total: usize,
    },
    Plan(Proposal),
    Failed(Toast),
}

/// A run, then its plan under review; its books stay put meanwhile.
#[derive(Debug, Default)]
pub struct Organizer {
    books: BTreeSet<BookId>,
    run: Option<RunId>,
    review: Option<Box<Proposal>>,
    /// A stopped run, its books free until it resumes.
    paused: Option<Paused>,
    paused_card: Option<RunId>,
}

impl Organizer {
    /// A run is going or its plan waits: no second one starts.
    pub fn busy(&self) -> bool {
        !self.books.is_empty()
    }

    /// Books a run holds: they open, but don't move.
    pub fn held(&self) -> &BTreeSet<BookId> {
        &self.books
    }

    pub fn review(&self) -> Option<&Proposal> {
        self.review.as_deref()
    }

    pub fn paused(&self) -> Option<&Paused> {
        self.paused.as_ref()
    }
}

/// Whether the AI may file it: an imported copy, present, on no shelf.
pub fn organizable(model: &Library, book: &Book) -> bool {
    let stored = book.home().is_some_and(|l| l.origin == Origin::Store);
    book.alive() && stored && !book.missing() && model.shelves_of(book.id).is_empty()
}

impl LibraryState {
    pub fn organizer(&self) -> &Organizer {
        &self.organizer
    }

    /// What to ask about `picked`, else every loose book; or why not.
    pub fn ask_organize(&self, picked: Option<&[BookId]>) -> Result<Ask, &'static str> {
        if self.organizer.busy() {
            return Err("Already organizing. Wait until it's done.");
        }
        let model = &self.model;
        let mut books: Vec<&Book> = match picked {
            Some(ids) => ids.iter().filter_map(|id| model.book(*id)).collect(),
            None => model.books().collect(),
        };
        books.retain(|b| organizable(model, b));
        if books.is_empty() {
            return Err(match picked {
                Some(_) => "None of these can be organized: only imported books on no shelf can.",
                None => "Nothing to organize: every imported book is on a shelf.",
            });
        }
        books.sort_by(|a, b| natural(&a.title.value, &b.title.value));
        let books = (books.into_iter())
            .map(|b| Loose {
                id: b.id,
                title: b.title.value.clone(),
                author: b.author.value.clone(),
            })
            .collect();
        let shelves = (model.shelves())
            .map(|s| Standing {
                id: s.id,
                name: s.name.value.clone(),
                parent: model.parent_of(s),
            })
            .collect();
        Ok(Ask {
            books,
            shelves,
            whole: picked.is_none(),
        })
    }

    /// What resuming asks: its books still loose, and newcomers if whole.
    pub fn ask_resume(&self) -> Result<Ask, &'static str> {
        let Some(paused) = &self.organizer.paused else {
            return self.ask_organize(None);
        };
        let gone = "Nothing left to organize: the paused run's books are all \
                    on shelves or removed.";
        let mut ask = match self.ask_organize(None) {
            Err(why) if self.organizer.busy() => return Err(why),
            Err(_) => return Err(gone),
            Ok(ask) => ask,
        };
        if !paused.whole {
            ask.books.retain(|b| paused.books.contains(&b.id));
            ask.whole = false;
        }
        match ask.books.is_empty() {
            true => Err(gone),
            false => Ok(ask),
        }
    }

    /// The paused run as the service keeps it; a card shows it when idle.
    pub fn set_paused(&mut self, paused: Option<Paused>) {
        self.organizer.paused = paused;
        self.paused_card();
    }

    /// A paused run's card shows while nothing else runs or waits.
    fn paused_card(&mut self) {
        self.runs.finish(self.organizer.paused_card.take());
        let Some(paused) = self.organizer.paused.as_ref() else {
            return;
        };
        if self.organizer.busy() {
            return;
        }
        let phase = match paused.ready {
            true => Phase::Ready,
            false => Phase::Paused,
        };
        let label = match paused.ready {
            true => format!("{} to review", counted(paused.books.len(), "book", "books")),
            false => paused.said(),
        };
        let run = self.runs.start(label, phase);
        let (done, total) = (paused.done, paused.total);
        let name = String::new();
        self.runs.beat(Beat {
            run,
            phase,
            done,
            total,
            name,
        });
        self.runs.show_now();
        self.organizer.paused_card = Some(run);
    }

    /// Asks whether a new run over `picked` replaces the paused one.
    pub fn ask_replace(&mut self, picked: Option<Vec<BookId>>) {
        self.folders.sheet = Some(super::Sheet::Replace(picked));
    }

    /// Drops the sheet asking to replace the paused run, if it's up.
    pub fn close_replace(&mut self) {
        if matches!(self.folders.sheet, Some(super::Sheet::Replace(_))) {
            self.close_sheet();
        }
    }

    /// A run began for `books`; its card says what with `label`.
    pub fn organizing(&mut self, books: impl IntoIterator<Item = BookId>, label: &str) {
        self.organizer_end();
        self.organizer.books = books.into_iter().collect();
        self.paused_card();
        let label = format!("{} books · {label}", self.organizer.books.len());
        self.organizer.run = Some(self.runs.start(label, Phase::Shelving));
        // Held books can't move, so none stay picked.
        let held = &self.organizer.books;
        let keep: Vec<Pick> = (self.picks.items().into_iter())
            .filter(|p| !matches!(p, Pick::Book(b) if held.contains(b)))
            .collect();
        let action = match keep.is_empty() {
            true => super::PickAction::Clear,
            false => super::PickAction::Only(keep),
        };
        self.picks.apply(action);
    }

    pub fn heard(&mut self, heard: Heard, out: &mut Commands) {
        match heard {
            Heard::Progress {
                filing,
                done,
                total,
            } => {
                let Some(run) = self.organizer.run else {
                    return;
                };
                let phase = if filing {
                    Phase::Filing
                } else {
                    Phase::Shelving
                };
                let name = String::new();
                self.runs.beat(Beat {
                    run,
                    phase,
                    done,
                    total,
                    name,
                });
            }
            Heard::Plan(plan) => {
                self.runs.finish(self.organizer.run.take());
                if plan.placed.is_empty() {
                    self.organizer_end();
                    let said = "The AI found no shelf that suits these books.";
                    out.push(Command::Notify(Toast::info(said)));
                } else {
                    self.organizer.review = Some(Box::new(plan));
                    // The plan shows on the library page; elsewhere, a nudge.
                    if !self.showing {
                        let said = "The AI's shelf plan is ready. Open the library to review it.";
                        out.push(Command::Notify(Toast::info(said)));
                    }
                }
            }
            Heard::Failed(toast) => {
                self.organizer_end();
                // What was done is kept; the toast offers to carry on.
                let resume = Command::Library(super::Action::ResumeOrganizing);
                let toast = match &self.organizer.paused {
                    Some(p) if !p.ready => toast.action("Resume", resume),
                    _ => toast,
                };
                out.push(Command::Notify(toast));
            }
        }
    }

    /// The run stopped, or its plan was applied or let go: books move again.
    pub fn organizer_end(&mut self) {
        self.runs.finish(self.organizer.run.take());
        let paused = self.organizer.paused.take();
        self.runs.finish(self.organizer.paused_card.take());
        self.organizer = Organizer::default();
        self.set_paused(paused);
    }

    pub(super) fn apply_organized(&mut self, out: &mut Commands) {
        let Some(plan) = self.organizer.review.take() else {
            return;
        };
        self.organizer_end();
        let done = apply(&mut self.model, &plan, super::wall_ms());
        let books = done.filed.len();
        let shelves: BTreeSet<ShelfId> = done.filed.iter().map(|(_, s)| *s).collect();
        let mut said = format!(
            "Filed {} on {}",
            counted(books, "book", "books"),
            counted(shelves.len(), "shelf", "shelves")
        );
        if !done.made.is_empty() {
            said.push_str(&format!(", {} new", done.made.len()));
        }
        said.push('.');
        if done.skipped > 0 {
            let changed = counted(done.skipped, "book", "books");
            said.push_str(&format!(" {changed} changed meanwhile and stayed put."));
        }
        let toast = match books {
            0 => Toast::info(said),
            _ => Toast::info(said).undo(Command::Library(super::Action::UndoOrganized(Box::new(
                done,
            )))),
        };
        out.push(Command::Notify(toast));
    }

    /// A shelf move without the books a run holds; a toast says why.
    pub(super) fn unheld(&self, action: ShelfAction, out: &mut Commands) -> Option<ShelfAction> {
        let held = &self.organizer.books;
        if held.is_empty() {
            return Some(action);
        }
        let free = |p: &Pick| !matches!(p, Pick::Book(b) if held.contains(b));
        let (action, dropped) = match action {
            ShelfAction::Carry(mut mv) => {
                let n = mv.books.len();
                mv.books.retain(|b| !held.contains(b));
                let dropped = mv.books.len() < n;
                (Some(ShelfAction::Carry(mv)), dropped)
            }
            ShelfAction::Stack {
                onto,
                mut carry,
                level,
                keep,
            } => {
                let n = carry.items.len();
                carry.items.retain(free);
                let dropped = held.contains(&onto) || carry.items.len() < n;
                let stack = ShelfAction::Stack {
                    onto,
                    carry,
                    level,
                    keep,
                };
                ((!dropped).then_some(stack), dropped)
            }
            other => (Some(other), false),
        };
        if dropped {
            out.push(Command::Notify(Toast::info(HELD)));
        }
        action
    }

    /// Picks without the books a run holds; a toast says why.
    pub(super) fn unheld_picks(&self, mut picks: Vec<Pick>, out: &mut Commands) -> Vec<Pick> {
        let held = &self.organizer.books;
        let n = picks.len();
        picks.retain(|p| !matches!(p, Pick::Book(b) if held.contains(b)));
        if picks.len() < n {
            out.push(Command::Notify(Toast::info(HELD)));
        }
        picks
    }

    pub(super) fn undo_organized(&mut self, done: &Applied) {
        undo(&mut self.model, done, super::wall_ms());
    }
}

impl LibraryState {
    /// A made-up run over the loose books, stopped at 3 of 8 or `ready`.
    pub fn organize_paused_demo(&mut self, ready: bool) {
        let Ok(ask) = self.ask_organize(None) else {
            return;
        };
        let books = ask.books.iter().map(|b| b.id).collect();
        let done = if ready { 8 } else { 3 };
        self.set_paused(Some(Paused {
            done,
            total: 8,
            books,
            whole: true,
            ready,
        }));
    }

    /// A made-up run over the loose books, its plan up if `review`.
    pub fn organize_demo(&mut self, review: bool) {
        let Ok(ask) = self.ask_organize(None) else {
            return;
        };
        let books: Vec<BookId> = ask.books.iter().map(|b| b.id).collect();
        self.organizing(books.iter().copied(), "Fast model");
        let mut out = Commands::new();
        let progress = Heard::Progress {
            filing: true,
            done: 3,
            total: 4,
        };
        self.heard(progress, &mut out);
        self.runs.show_now();
        if !review {
            return;
        }
        let shelves = vec![
            ("Fiction".to_owned(), None),
            ("Classics".to_owned(), Some(Spot::New(0))),
            ("Essays".to_owned(), None),
        ];
        let (left, filed) = books
            .split_last()
            .map_or((None, &[][..]), |(l, f)| (Some(*l), f));
        let spots = [Spot::New(1), Spot::New(0), Spot::New(2)];
        let placed = (filed.iter().zip(spots.iter().cycle()))
            .map(|(b, s)| (*b, *s))
            .collect();
        let plan = Proposal {
            shelves,
            placed,
            left: left.into_iter().collect(),
        };
        self.heard(Heard::Plan(plan), &mut out);
    }
}

/// Why a book stayed put when moved.
const HELD: &str = "Books the AI is organizing stay put until it's done.";

/// Makes the plan's shelves and files its books that are still loose.
pub fn apply(model: &mut Library, plan: &Proposal, wall_ms: u64) -> Applied {
    let mut made: Vec<Option<ShelfId>> = Vec::with_capacity(plan.shelves.len());
    for (name, parent) in &plan.shelves {
        let parent = match parent {
            None => Some(None),
            Some(spot) => locate(model, &made, *spot).map(Some),
        };
        let id = parent.and_then(|p| model.create_shelf(name, p, wall_ms).ok());
        made.push(id);
    }
    let mut done = Applied::default();
    for (book, spot) in &plan.placed {
        let loose = model.book(*book).is_some_and(|b| organizable(model, b));
        let shelf = locate(model, &made, *spot).filter(|_| loose);
        match shelf {
            Some(shelf) if model.file(*book, shelf, wall_ms) => done.filed.push((*book, shelf)),
            _ => done.skipped += 1,
        }
    }
    done.made = made.into_iter().flatten().collect();
    // Shelves whose books all changed meanwhile aren't left empty.
    let emptied = prune(model, &done.made, wall_ms);
    done.made.retain(|s| !emptied.contains(s));
    done
}

/// Takes the books off again, and the new shelves left empty.
pub fn undo(model: &mut Library, done: &Applied, wall_ms: u64) {
    for (book, shelf) in &done.filed {
        model.unfile(*book, *shelf, wall_ms);
    }
    prune(model, &done.made, wall_ms);
}

/// Removes those of `shelves` holding nothing, children first.
fn prune(model: &mut Library, shelves: &[ShelfId], wall_ms: u64) -> BTreeSet<ShelfId> {
    let mut gone = BTreeSet::new();
    for id in shelves.iter().rev() {
        let empty = model.shelf(*id).is_some() && model.counts(*id) == (0, 0);
        if empty && model.remove_shelf(*id, wall_ms).is_some() {
            gone.insert(*id);
        }
    }
    gone
}

/// The shelf a spot names, if it's still there.
fn locate(model: &Library, made: &[Option<ShelfId>], spot: Spot) -> Option<ShelfId> {
    let id = match spot {
        Spot::Shelf(id) => id,
        Spot::New(i) => made.get(i).copied().flatten()?,
    };
    model.shelf(id).map(|s| s.id)
}

fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `copies` imported books, then one read in place.
    fn library(copies: usize) -> LibraryState {
        let mut state = LibraryState::default();
        state.seed_copies(copies);
        state.seed(1);
        state
    }

    /// The imported books, in a steady order.
    fn ids(state: &LibraryState) -> Vec<BookId> {
        let model = &state.model;
        let stored = |b: &&Book| b.home().is_some_and(|l| l.origin == Origin::Store);
        let mut ids: Vec<BookId> = model.books().filter(stored).map(|b| b.id).collect();
        ids.sort();
        ids
    }

    #[test]
    fn only_present_imported_books_on_no_shelf_are_asked_about() {
        let mut state = LibraryState::default();
        state.seed(3);
        let none = state.ask_organize(None);
        assert!(none.is_err(), "books read in place are never organized");
        let mut state = library(6);
        let all = ids(&state);
        assert_eq!(all.len(), 6);
        let shelf = state.model.create_shelf("Kept", None, 1).ok();
        let first = all.first().copied();
        if let (Some(book), Some(shelf)) = (first, shelf) {
            state.model.file(book, shelf, 1);
        }
        // One file is gone from the store.
        state
            .model
            .check_present(|p| !p.to_string_lossy().starts_with("/store/2-"));
        let ask = state
            .ask_organize(None)
            .ok()
            .unwrap_or_else(|| panic!("asked"));
        // Six, less the shelved one and the missing one.
        assert_eq!(ask.books.len(), 4);
        assert_eq!(ask.shelves.len(), 1);
        assert!(ask.books.iter().all(|b| Some(b.id) != first));
        let picked = state.ask_organize(Some(first.as_slice()));
        assert!(picked.is_err(), "a shelved book alone has nothing to do");
    }

    #[test]
    fn an_imported_book_once_opened_can_still_be_organized() {
        let mut state = library(1);
        let first = ids(&state).first().copied();
        let book = first.and_then(|id| state.model.book(id));
        let opened = book.and_then(|b| {
            Some(mareader_library::Opened {
                path: b.path()?.to_path_buf(),
                measure: mareader_library::Measure {
                    content: b.content,
                    stamp: Default::default(),
                },
                format: b.format,
                title: b.title.value.clone(),
                author: None,
            })
        });
        let opened = opened.unwrap_or_else(|| panic!("a copy with its file"));
        state.model.record_open(opened, 9);
        assert_eq!(ids(&state), first.into_iter().collect::<Vec<_>>());
        let ask = state.ask_organize(None);
        assert_eq!(ask.map(|a| a.books.len()).ok(), Some(1));
    }

    #[test]
    fn a_plan_files_books_on_new_and_standing_shelves_then_undoes() {
        let mut state = library(3);
        let all = ids(&state);
        let (a, b) = (all.first().copied(), all.get(1).copied());
        let (Some(a), Some(b)) = (a, b) else {
            panic!("two books")
        };
        let Ok(old) = state.model.create_shelf("Fiction", None, 1) else {
            panic!("shelf")
        };
        let plan = Proposal {
            shelves: vec![
                ("Classics".into(), Some(Spot::Shelf(old))),
                ("Victorian".into(), Some(Spot::New(0))),
                ("Never used".into(), None),
            ],
            placed: vec![(a, Spot::New(1)), (b, Spot::Shelf(old))],
            left: Vec::new(),
        };
        let done = apply(&mut state.model, &plan, 2);
        assert_eq!(done.filed.len(), 2);
        assert_eq!(done.skipped, 0);
        // Classics holds only Victorian; the unused shelf went again.
        assert_eq!(done.made.len(), 2);
        let names: Vec<String> = (state.model.shelves())
            .map(|s| s.name.value.clone())
            .collect();
        assert!(!names.contains(&"Never used".to_owned()), "{names:?}");
        let on_a = state.model.shelves_of(a);
        let victorian = done.made.last().copied();
        assert_eq!(on_a.into_iter().next(), victorian);
        undo(&mut state.model, &done, 3);
        assert!(state.model.shelves_of(a).is_empty());
        assert!(state.model.shelves_of(b).is_empty());
        // Only the standing shelf is left.
        assert_eq!(state.model.shelves().count(), 1);
    }

    #[test]
    fn a_plan_names_each_book_s_shelf_for_its_card() {
        let mut state = library(3);
        let all = ids(&state);
        let [a, b, c] = all.as_slice() else {
            panic!("three books")
        };
        let Ok(old) = state.model.create_shelf("Fiction", None, 1) else {
            panic!("shelf")
        };
        let plan = Proposal {
            shelves: vec![("Poems".into(), None)],
            placed: vec![(*a, Spot::New(0)), (*b, Spot::Shelf(old))],
            left: vec![*c],
        };
        let shelf = |name: &str| Some(Bound::Shelf(name.into()));
        assert_eq!(plan.bound(&state.model, *a), shelf("Poems"));
        assert_eq!(plan.bound(&state.model, *b), shelf("Fiction"));
        assert_eq!(plan.bound(&state.model, *c), Some(Bound::Stays));
        let other = state.model.books().map(|x| x.id).find(|x| !all.contains(x));
        let other = other.unwrap_or_else(|| panic!("the book read in place"));
        assert_eq!(plan.bound(&state.model, other), None);
    }

    #[test]
    fn books_that_changed_meanwhile_stay_put() {
        let mut state = library(3);
        let all = ids(&state);
        let (Some(a), Some(b)) = (all.first().copied(), all.get(1).copied()) else {
            panic!("two books")
        };
        let Ok(mine) = state.model.create_shelf("Mine", None, 1) else {
            panic!("shelf")
        };
        // The reader shelved one while the model thought.
        state.model.file(a, mine, 1);
        let plan = Proposal {
            shelves: vec![("Poems".into(), None), ("Lonely".into(), None)],
            placed: vec![(a, Spot::New(1)), (b, Spot::New(0))],
            left: Vec::new(),
        };
        let done = apply(&mut state.model, &plan, 2);
        assert_eq!(done.filed.len(), 1);
        assert_eq!(done.skipped, 1);
        assert_eq!(done.made.len(), 1, "the shelf it'd have had isn't made");
        assert_eq!(state.model.shelves_of(a).len(), 1);
    }

    #[test]
    fn a_run_holds_its_books_until_its_plan_is_let_go() {
        let mut state = library(4);
        let all = ids(&state);
        state.organizing(all.clone(), "Fast");
        assert!(state.organizer().busy());
        assert!(!state.picks.active(), "a run doesn't start selecting");
        assert!(state.ask_organize(None).is_err(), "one run at a time");
        let mut out = Commands::new();
        let progress = Heard::Progress {
            filing: true,
            done: 1,
            total: 2,
        };
        state.heard(progress, &mut out);
        assert_eq!(state.runs.list().len(), 1);
        let first = all.first().copied().unwrap_or(BookId(0));
        let plan = Proposal {
            shelves: vec![("Poems".into(), None)],
            placed: vec![(first, Spot::New(0))],
            left: Vec::new(),
        };
        state.heard(Heard::Plan(plan), &mut out);
        assert!(state.runs.list().is_empty(), "the card goes with the run");
        assert!(state.organizer().review().is_some());
        assert!(state.organizer().held().contains(&first));
        state.act(super::super::Action::ApplyOrganized, 0.0, &mut out);
        assert!(!state.organizer().busy());
        assert_eq!(state.model.shelves_of(first).len(), 1);
        let undo = out.iter().find_map(|c| match c {
            Command::Notify(t) => t.undo.clone(),
            _ => None,
        });
        let Some(undo) = undo else {
            panic!("an Undo toast")
        };
        let Command::Library(action) = *undo else {
            panic!("a library undo")
        };
        state.act(action, 0.0, &mut out);
        assert!(state.model.shelves_of(first).is_empty());
        assert_eq!(state.model.shelves().count(), 0);
    }
}
