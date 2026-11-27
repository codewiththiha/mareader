//! What the library's page, its menus and its sheets ask for.

use std::path::PathBuf;

use mareader_library::{
    BookId, ContentId, Departure, Design, FolderId, Format, Item, Library, LinkId, Measure, Move,
    Options, Placement, ShelfId, Target,
};

mod acting;
mod books;
mod duplicating;
mod filing;
mod find;
mod folders;
mod held;
mod io;
mod landing;
mod moving;
mod news;
mod opening;
mod organizing;
mod question;
mod runs;
mod searching;
mod seed;
mod sheeting;
mod shelving;
mod state;
pub use find::{file_name, folder_name};
pub use folders::{Folders, Going, Pending, RESCAN_GAP, SETTLE_WAIT, Screening, offline};
pub use io::{Copies, CopyIn, Files, Io, Job, Measured, News, Probe, Tag, Walk, Walked};
pub use moving::listed;
pub use organizing::{
    Ask, Bound, Heard, Loose, Organizer, Paused, Proposal, Spot, Standing, apply, natural,
    organizable, undo,
};
pub use question::{Kind, Question};
pub use runs::{Beat, DELAY, Phase, Run, RunEnd, RunId, Runs};
pub use searching::{Field, Listing, Lit, RING_END, RING_HOLD, Reveal, Search, Shape};
pub use shelving::Crumb;
pub use state::{SaveStatus, State, Status, wall_ms};

/// "3 books", "1 shelf": a count and its noun.
pub fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// A book pointed at a file again: the reader picked it, or a search found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relink {
    pub book: BookId,
    pub path: PathBuf,
    pub force: bool,
}

/// What the library asks the shell for, once its own state is settled.
///
/// The engine never calls upward, and its workers are its own: a note on
/// screen and a command for the session are what only a window can do.
pub enum Out {
    /// A note for the reader; the front end decides how it looks.
    Toast(crate::toast::Toast),
    /// A command for the session to run.
    Command(crate::command::Command),
}

/// Books, shelves and links picked on the page: the library's own item.
pub use mareader_library::{Pick, PickAction, Picks};

/// Which books the library page lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    All,
    Missing,
}

/// Anything the library shows under a name of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Named {
    Book(BookId),
    Shelf(ShelfId),
    Link(LinkId),
}

/// Finding a lost book: what it's doing, or what it found.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub book: BookId,
    pub step: Step,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Ask,
    Working(String),
    /// Why that didn't work; the choices stay.
    Note(String),
    /// A file under its name with other content, offered.
    Differs {
        path: PathBuf,
        format: Format,
        measure: Measure,
    },
}

/// A copy asked about before books leave their folder's ground.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CopyWork {
    /// Books and shelves carried off their folder's ground.
    Move { mv: Move, owed: Departure },
    /// A rung taken apart: its own books come up as copies.
    Apart(ShelfId),
    /// A removal whose rungs carry books read in place.
    Removal {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        copies: Vec<BookId>,
        forget: bool,
    },
}

impl CopyWork {
    /// The books that become copies, each once.
    pub fn books(&self, model: &Library) -> Vec<BookId> {
        match self {
            Self::Move { owed, .. } => owed.copies(),
            Self::Apart(id) => model.rung_books(*id, false),
            Self::Removal { copies, .. } => copies.clone(),
        }
    }
}

/// The copy ask's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyAnswer {
    Copy,
    /// Put shelves back, or let the folder make them again.
    Without,
}

/// What every shelf made by hand is called until renamed.
pub const NEW_SHELF: &str = "New shelf";

/// What a drag carries, in the order shown; the UI holds it until dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Carry {
    pub items: Vec<Item>,
    /// The level it was shown on.
    pub from: Option<ShelfId>,
}

impl Carry {
    /// What dropping it on `target` (none: the top level) does.
    pub fn onto(&self, target: Option<ShelfId>, keep: bool) -> Option<Action> {
        let mv = Move {
            from: self.from,
            to: target,
            keep,
            ..Move::of(&self.items)
        };
        // Onto where it came from, or onto one of its own shelves: nothing.
        let home = !self.only_shelves() && self.from == target;
        if home || target.is_some_and(|t| mv.shelves.contains(&t)) {
            return None;
        }
        Some(Action::Shelf(ShelfAction::Carry(mv)))
    }

    /// The cards it moves within `level`, if it came from there.
    pub fn items_at(&self, level: Option<ShelfId>, model: &Library) -> Option<Vec<Item>> {
        let here = |s: &ShelfId| model.shelf(*s).is_some_and(|s| model.parent_of(s) == level);
        let shelves = Move::of(&self.items).shelves;
        let from_here = self.only_shelves() || self.from == level;
        (from_here && shelves.iter().all(here)).then(|| self.items.clone())
    }

    /// Every card it carries, shelves first.
    pub fn picks(&self) -> Vec<Item> {
        let mut items = self.items.clone();
        items.sort_by_key(|i| !matches!(i, Item::Shelf(_)));
        items
    }

    /// Whether a book is among what it carries, so it can stack.
    pub fn has_books(&self) -> bool {
        self.items.iter().any(|i| matches!(i, Item::Book(_)))
    }

    /// A shelf moves off its parent, never off the level it showed on.
    fn only_shelves(&self) -> bool {
        self.items.iter().all(|i| matches!(i, Item::Shelf(_)))
    }

    /// Whether it was carried off a shelf, so Alt can add instead.
    pub fn off_shelf(&self) -> bool {
        self.from.is_some() && !self.only_shelves()
    }

    /// The words under the pointer while it's carried.
    pub fn label(&self, model: &Library) -> String {
        if let [one] = self.items.as_slice() {
            return model.item_name(*one).unwrap_or_default().to_owned();
        }
        let Move {
            books,
            shelves,
            links,
            ..
        } = Move::of(&self.items);
        let parts = [
            (books.len(), "book", "books"),
            (shelves.len(), "shelf", "shelves"),
            (links.len(), "link", "links"),
        ];
        let parts: Vec<String> = (parts.iter())
            .filter(|(n, ..)| *n > 0)
            .map(|&(n, one, many)| counted(n, one, many))
            .collect();
        parts.join(", ")
    }
}

/// Shelf work asked for by the page, its crumbs and its sheets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShelfAction {
    /// Opens a shelf, or the library's top level.
    GoTo(Option<ShelfId>),
    /// Makes "New shelf" where the reader is and opens it.
    New,
    /// Makes "New shelf" inside `parent`, staying put; the rail names it.
    NewIn(Option<ShelfId>),
    /// Names the open shelf in its crumb; Escape cancels.
    StartRename(ShelfId),
    CancelRename,
    /// Takes a shelf apart, asking first if its books leave a folder.
    Apart(ShelfId),
    /// Books and shelves carried to a level, by a drop or a menu.
    Carry(Move),
    /// The copy ask's answer.
    Copy(CopyAnswer),
    /// What's carried rested on a book: both go on a new shelf there.
    Stack {
        onto: BookId,
        carry: Carry,
        level: Option<ShelfId>,
        keep: bool,
    },
    /// Watches a folder's shelf for new books, or stops.
    Watch(ShelfId, bool),
    /// Unfolds a shelf's row in the list to show what's on it, or folds it.
    Unfold(ShelfId),
    /// Shows a folder's shelf in the file manager; the app does it.
    Reveal(ShelfId),
    /// Cards moved in a level's own order, before another or to the end.
    Arrange {
        items: Vec<Item>,
        level: Option<ShelfId>,
        before: Option<Item>,
    },
}

/// Work asked of one book or many, from the page or a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookAction {
    /// Asks first, telling what goes and what stays.
    AskRemove {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        links: Vec<LinkId>,
    },
    /// Asks before removing the page's missing books.
    AskRemoveMissing(Vec<BookId>),
    /// Removes only those still missing, keeping data unless requested.
    RemoveMissing { books: Vec<BookId>, forget: bool },
    /// Asks first for whatever the page has picked; nothing picked, no sheet.
    AskRemovePicked,
    /// `cascade` takes what's inside the shelves; `forget`, marks and places.
    Remove {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        links: Vec<LinkId>,
        cascade: bool,
        forget: bool,
    },
    /// A separate book of the same file, with its own place.
    Twin(BookId),
    /// Shows its file in the system's file manager.
    Reveal(BookId),
    /// Copies in files onto a shelf if `to`; a folder asks how.
    Import {
        paths: Vec<PathBuf>,
        to: Option<ShelfId>,
    },
    /// A new shelf where the reader is, holding these.
    ShelveTogether(Vec<Pick>),
    /// The library's own copies of these, filed beside them.
    Duplicate(Vec<Pick>),
}

/// The search field's asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchAction {
    /// Focuses the field with its text selected; Ctrl+F.
    Focus,
    Text(String),
    /// Down opens the suggestions or lights the next; Up goes back.
    Move(i32),
    /// Shows the book or shelf where it sits on the page.
    Choose(Target),
    /// Enter: chooses the lit suggestion, the first unless moved.
    ChooseLit,
    Close,
    Clear,
    /// Leaves a shelf or filter that hides every match.
    Widen,
}

/// One applied AI plan, kept for Undo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    pub filed: Vec<(BookId, ShelfId)>,
    /// New shelves, parents first.
    pub made: Vec<ShelfId>,
    /// Books that changed meanwhile and stayed put.
    pub skipped: usize,
}

/// The sheet over the page, if any; questions wait behind it.
#[derive(Clone, Debug, PartialEq)]
pub enum Sheet {
    Find(Finding),
    /// Removing books and shelves, with what that does.
    Removal {
        books: Vec<BookId>,
        shelves: Vec<ShelfId>,
        links: Vec<LinkId>,
    },
    /// The page's missing books, checked again before removal.
    MissingRemoval {
        books: Vec<BookId>,
    },
    Rename(Named),
    /// Books read in place about to leave their folder: copy them first?
    Copy(Box<CopyWork>),
    /// How a folder's books are taken in.
    Import(PathBuf),
    /// A book that left its folder's shelves: shown here, or gone to.
    Moved {
        folder: FolderId,
        book: BookId,
    },
    /// An import that only met what was there: its shelf lights on close.
    Note {
        shelf: ShelfId,
        returned: bool,
    },
    /// A new run would replace the paused one; these books were picked.
    Replace(Option<Vec<BookId>>),
}

/// Work asked for by the page, its menus and its sheets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Opens the plus card's menu.
    ShowAdd,
    /// Asks how a folder's books are taken in.
    AskFolder(PathBuf),
    ImportFolder {
        root: PathBuf,
        opts: Options,
    },
    /// Walks every watched folder again.
    Rescan,
    /// Answers the waiting question; `all` answers its kind waiting too.
    Answer(Placement, bool),
    /// Leaves every waiting question unanswered.
    CancelAsks,
    /// Brings back a book the reader removed from a folder.
    Restore {
        folder: FolderId,
        content: ContentId,
    },
    /// Asks where a book that left the folder's shelves shows.
    AskMoved {
        folder: FolderId,
        book: BookId,
    },
    /// Files that book on its folder shelf again, as well.
    ShowMoved {
        folder: FolderId,
        book: BookId,
    },
    /// Goes where a row stands and lights it.
    Reveal(Pick),
    Scope(Scope),
    FindAgain(BookId),
    /// The find-again sheet for the first book whose file has gone.
    FindAgainLost,
    Relink {
        book: BookId,
        path: PathBuf,
    },
    /// Takes the file offered after "isn't the same file".
    UseAnyway(BookId),
    Seek {
        book: BookId,
        folder: PathBuf,
    },
    CloseSheet,
    /// Opens the one rename sheet for a book, a shelf or a link.
    AskRename(Named),
    /// That sheet for the book read last.
    AskRenameLast,
    Rename(Named, String),
    /// A book's own design choice; none follows the reader's setting.
    Design(BookId, Option<Design>),
    Shelf(ShelfAction),
    Pick(PickAction),
    /// Picks the `count` books read last, as the page's bar picks them.
    PickRecent(usize),
    Book(BookAction),
    /// The removal sheet for the `count` books read last.
    AskRemoveRecent(usize),
    /// That sheet for a shelf by name.
    AskRemoveShelf(String),
    Search(SearchAction),
    /// Asks the AI to shelve the picked books, else every loose one.
    Organize(Option<Vec<BookId>>),
    /// Starts afresh, replacing any paused run.
    OrganizeAnew(Option<Vec<BookId>>),
    /// Pauses the AI run; its books move again, its steps are kept.
    StopOrganizing,
    /// Carries on the paused run, or shows its finished plan.
    ResumeOrganizing,
    /// Sets the paused run aside; a toast can bring it back.
    DiscardPaused,
    RestorePaused,
    /// Makes the plan's shelves and files its books.
    ApplyOrganized,
    /// Lets the plan go; nothing moved.
    DiscardOrganized,
    UndoOrganized(Box<Applied>),
    /// A fixture's shelf: `count` books, as a shot asks for.
    Seed(usize),
    /// A fixture's shelves, with the books filed on them.
    SeedShelves,
    /// A fixture's twins: `count` copies of one book.
    SeedCopies(usize),
    /// A fixture's studied words, in the first books, for Space.
    SeedWords,
    /// A fixture's highlights, notes and bookmarks, for Space's notes.
    SeedMarks,
    /// A fixture's saved selection AI answers, for Space.
    SeedAiNotes,
    /// A fixture's made-up AI run; `review` shows the plan it came to.
    OrganizeDemo(bool),
    /// A fixture's paused AI run; `ready` has it finished.
    OrganizePaused(bool),
    /// Two fixture imports under way, their cards shown at once.
    RunsDemo,
    /// Goes to a shelf by its names from the top, as "Fiction/Classics".
    OpenShelf(String),
}
