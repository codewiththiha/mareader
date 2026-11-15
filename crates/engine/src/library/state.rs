//! What the library remembers between frames: its model, where the page
//! stands in it, and the sheet over it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{BuildHasher, RandomState};
use std::path::PathBuf;
use std::sync::Arc;

use mareader_library::{BookId, DeviceId, Library, ShelfId, disk::Folder};
use mareader_workspace::PaneId;

use crate::documents::OpenRequest;
use crate::session::Waker;

use super::folders::Folders;
use super::searching::Search;
use super::{Io, Job, Organizer, PickAction, Picks, Question, Runs, Scope, Sheet};

/// Seconds between writes while reading.
pub const SAVE_DELAY: f64 = 0.5;

/// Whether the library's files can be written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Its files are being read; opens wait for their positions.
    Loading,
    #[default]
    Ready,
    /// A newer app's files, or none readable: kept in memory only.
    ReadOnly,
}

/// The library's own state: the part that outlives a frame.
pub struct State {
    pub model: Library,
    /// None: in memory only, as in tests and shots.
    pub folder: Option<Folder>,
    /// Questions waiting for the reader, oldest first.
    pub asks: VecDeque<Question>,
    /// Books a filing run holds: they open, but they don't move.
    pub held: BTreeSet<BookId>,
    pub status: Status,
    /// Disk work runs on the frame rather than on a worker.
    pub inline: bool,
    /// The file each pane showed when last looked at, and as which book.
    pub seen: BTreeMap<PaneId, (PathBuf, Option<BookId>)>,
    /// Opens held until the library has loaded.
    pub waiting: Vec<OpenRequest>,
    /// When the last write left, in seconds since the app started.
    pub wrote: f64,
    /// The library page is up in place of the panes.
    pub showing: bool,
    pub scope: Scope,
    /// The shelf open on the page; none is the top level.
    pub at: Option<ShelfId>,
    /// The shelf whose name the breadcrumb is editing in place.
    pub renaming: Option<ShelfId>,
    /// A shelf the rail's tree just made, for it to name.
    pub made: Option<ShelfId>,
    pub picks: Picks,
    /// Shelf rows the list shows unfolded; going elsewhere folds them.
    pub unfolded: BTreeSet<ShelfId>,
    /// The shelf under files dragged in from outside, if any.
    pub file_target: Option<ShelfId>,
    /// The sheet over the page; none while the page is clear of one.
    pub sheet: Option<Sheet>,
    /// The runs in flight, oldest first; a card shows one.
    pub runs: Runs,
    /// Disk work: the jobs sent and the news come back.
    pub io: Io,
    /// Asks for a frame when disk work lands; kept, so the mode can change.
    pub wake: Waker,
    /// Folder walks, and the work waiting on them.
    pub folders: Folders,
    /// The search's field, its listing, and the card a choice lit.
    pub search: Search,
    /// Where covers are kept between launches; no walk enters it.
    pub cache: Option<PathBuf>,
    /// The AI's run filing loose books, and its plan.
    pub organizer: Organizer,
}

impl Default for State {
    fn default() -> Self {
        Self {
            model: Library::new(new_device()),
            folder: None,
            status: Status::Ready,
            inline: false,
            seen: BTreeMap::new(),
            waiting: Vec::new(),
            wrote: f64::NEG_INFINITY,
            showing: false,
            scope: Scope::All,
            at: None,
            renaming: None,
            made: None,
            picks: Picks::default(),
            unfolded: BTreeSet::new(),
            file_target: None,
            sheet: None,
            asks: VecDeque::new(),
            held: BTreeSet::new(),
            runs: Runs::default(),
            io: Io::default(),
            wake: Arc::new(|| {}),
            folders: Folders::default(),
            search: Search::default(),
            cache: None,
            organizer: Organizer::default(),
        }
    }
}

impl State {
    pub fn model(&self) -> &Library {
        &self.model
    }

    /// For edits outside the library's own: marks, from the study tools.
    pub fn model_mut(&mut self) -> &mut Library {
        &mut self.model
    }

    pub fn loading(&self) -> bool {
        self.status == Status::Loading
    }

    /// The books and shelves picked on the page.
    pub fn picks(&self) -> &Picks {
        &self.picks
    }

    pub fn unfolded(&self) -> &BTreeSet<ShelfId> {
        &self.unfolded
    }

    /// Escape lets go of a selection, then the search; true if it did.
    pub fn escape(&mut self) -> bool {
        if self.picks.active() {
            self.picks.apply(PickAction::Clear);
            return true;
        }
        self.search.clear()
    }

    /// Keeps covers in `<dir>/covers` between launches.
    pub fn attach_cache(&mut self, dir: PathBuf) {
        self.cache = Some(dir.join("covers"));
    }

    /// Reads the library kept in `root`, and saves to it from now on.
    pub fn attach(&mut self, root: PathBuf, wake: Waker) {
        self.wake = wake;
        self.load(Folder::new(root));
    }

    /// Disk work on the frame instead of a worker, asked for at any time.
    ///
    /// A library already reading its folder builds its worker again and reads
    /// it once more, so the mode a client asked for is the mode it gets
    /// whichever way round the two calls come.
    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
        if let Some(folder) = self.folder.clone() {
            self.load(folder);
        }
    }

    /// The library's own read of `folder`, on the worker its mode asks for.
    fn load(&mut self, folder: Folder) {
        let device = self.model.device();
        self.io = match self.inline {
            true => Io::default(),
            false => Io::thread(self.wake.clone()),
        };
        self.io.send(Job::Load(folder.clone(), device));
        self.folder = Some(folder);
        self.status = Status::Loading;
    }

    /// Writes changes at most every [`SAVE_DELAY`]; else the wait left.
    pub fn save(&mut self, now: f64) -> Option<f64> {
        let (Some(_), Status::Ready) = (&self.folder, self.status) else {
            return None;
        };
        if !self.model.is_dirty() {
            return None;
        }
        let wait = self.wrote + SAVE_DELAY - now;
        if wait > 0.0 {
            return Some(wait);
        }
        let dirty = self.model.take_dirty();
        let changes = self.model.changes(&dirty);
        self.io.send(Job::Write(Box::new(changes)));
        self.wrote = now;
        None
    }

    /// Sends every pending change now: the app is saving or exiting.
    pub fn flush(&mut self) {
        self.wrote = f64::NEG_INFINITY;
        self.save(0.0);
    }
}

/// This run's own device, so two apps never share a library in step.
fn new_device() -> DeviceId {
    DeviceId(RandomState::new().hash_one(wall_ms()))
}

/// Milliseconds since 1970; the browser preview has no clock here.
pub fn wall_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        since.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}
