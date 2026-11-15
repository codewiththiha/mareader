//! What a folder walk keeps between frames: the walk on its way, the work
//! waiting on the worker, and the folders no walk enters.
//!
//! The walking itself runs on the library's worker. This is the state that
//! answers it, so a walk's news lands on a model that remembers the walk.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use mareader_library::disk::Known;
use mareader_library::gate::Gate;
use mareader_library::picks::Pick;
use mareader_library::{
    Asked, Book, BookId, ContentId, FolderId, Found, Options, Origin, Placement, Plan, Planned,
    ShelfId,
};

use super::{
    CopyWork, Files, Job, Out, Phase, Question, RunId, Sheet, State, Status, Tag, Walk, Walked,
    file_name, folder_name, wall_ms,
};
use crate::toast::Toast;

/// Seconds between rescans when the window keeps regaining focus.
pub const RESCAN_GAP: f64 = 2.0;
/// Seconds before looking again at files still being written.
pub const SETTLE_WAIT: f64 = 3.5;

/// A walk on its way: what it walks, as what, and its card.
#[derive(Clone, Debug)]
pub struct Going {
    pub root: PathBuf,
    pub opts: Options,
    pub plan: Plan,
    pub asked: Asked,
    pub run: Option<RunId>,
    /// Files the walk found, for the run's last word.
    pub found: usize,
}

/// Work waiting on the worker, under the tag its news carries.
#[derive(Debug)]
pub enum Pending {
    /// A folder walking; a rescan names the folder it walks.
    Walk(Going, Option<FolderId>),
    /// A planned walk's copies, landed once made.
    Land(Box<Planned>, Going),
    /// Loose files measured, then screened onto the level.
    Files(Option<ShelfId>, Option<RunId>),
    /// Screened files copied, then landed onto the level.
    Loose(Box<Screening>),
    /// An answer's copy, then the answer written.
    Keep(Box<Question>, Placement),
    /// Books read in place copied, then the gesture that owed them.
    Depart(Box<CopyWork>),
    /// A removed book's file measured, then copied if its folder copies.
    Restore(FolderId, ContentId),
    Restored(FolderId, ContentId, Box<Found>),
    /// The picks' books copied, then the duplicates made.
    Duplicate(Vec<Pick>),
}

/// Loose files measured, on their way onto a level.
#[derive(Debug)]
pub struct Screening {
    pub to: Option<ShelfId>,
    pub run: Option<RunId>,
    pub files: Vec<Found>,
    /// Questions the screen queued.
    pub asked: usize,
}

/// When folders are walked, what they said, and what waits on the worker.
#[derive(Debug, Default)]
pub struct Folders {
    pub next: Tag,
    pub pending: HashMap<Tag, Pending>,
    /// Watched folders walking quietly now.
    pub walking: BTreeSet<FolderId>,
    /// A rescan asked for while one ran.
    pub again: bool,
    /// When the last rescan left, in seconds since the app started.
    pub last: Option<f64>,
    pub settle_at: Option<f64>,
    pub focused: bool,
    pub offline: BTreeSet<FolderId>,
    /// Folders dropped while a sheet was up, asked about in turn.
    pub later: VecDeque<PathBuf>,
    /// The import sheet's last answers, the next one's start.
    pub opts: Option<Options>,
}

impl State {
    /// The library's own folders, which no walk enters.
    pub fn avoid(&self) -> Vec<PathBuf> {
        let own = self.folder.as_ref().map(|f| f.root().to_path_buf());
        own.into_iter().chain(self.cache.clone()).collect()
    }

    /// Whether a rescan is walking.
    pub fn scanning(&self) -> bool {
        !self.folders.walking.is_empty()
    }

    /// Folders whose drive wasn't there at the last look.
    pub fn offline(&self) -> &BTreeSet<FolderId> {
        &self.folders.offline
    }

    /// Questions waiting, the next first; shown when no sheet is up.
    pub fn questions(&self) -> impl Iterator<Item = &Question> {
        self.asks.iter()
    }

    /// The import sheet's start: the last answers, else the defaults.
    pub fn last_options(&self) -> Options {
        self.folders.opts.clone().unwrap_or_default()
    }

    pub fn tag(&mut self, pending: Pending) -> Tag {
        self.folders.next += 1;
        let tag = self.folders.next;
        self.folders.pending.insert(tag, pending);
        tag
    }

    pub fn store(&self) -> Option<PathBuf> {
        self.folder.as_ref().map(|f| f.store())
    }

    /// The import sheet for `root`, or after the sheet that's up.
    pub fn ask_folder(&mut self, root: PathBuf) {
        // "Change…" on the import sheet swaps its folder.
        match self.sheet {
            Some(Sheet::Import(_)) | None => self.sheet = Some(Sheet::Import(root)),
            Some(_) => self.folders.later.push_back(root),
        }
    }

    /// Closes the sheet; a folder dropped meanwhile asks next.
    pub fn close_sheet(&mut self) {
        self.sheet = self.folders.later.pop_front().map(Sheet::Import);
    }

    /// The import sheet's answer: through the gate, then a walk.
    pub fn import_folder(&mut self, root: PathBuf, opts: Options, now: f64, out: &mut Vec<Out>) {
        self.close_sheet();
        self.folders.opts = Some(opts.clone());
        let root = std::fs::canonicalize(&root).unwrap_or(root);
        if self.avoid().iter().any(|own| own.starts_with(&root)) {
            let note = "That folder holds Mareader's own files; choose one inside it.";
            out.push(Out::Toast(Toast::error(note)));
            return;
        }
        if opts.mode().copies() && self.store().is_none() {
            let note = "The library has no folder to keep books in.";
            out.push(Out::Toast(Toast::error(note)));
            return;
        }
        match self.model.gate(&root, &opts) {
            Gate::Walk { root, plan } => self.walk(root, opts, plan, now),
            Gate::Clash(clash) => self.asks.push_back(Question::Clash { clash, opts }),
        }
    }

    /// An import's walk, with a card.
    pub fn walk(&mut self, root: PathBuf, opts: Options, plan: Plan, now: f64) {
        let run = Some(self.runs.start(folder_name(&root), Phase::Scanning));
        let going = Going {
            root,
            opts,
            plan,
            asked: Asked::Explicitly,
            run,
            found: 0,
        };
        self.send_walk(going, None);
        self.folders.last = Some(now);
    }

    fn send_walk(&mut self, going: Going, folder: Option<FolderId>) {
        let (root, opts, run) = (going.root.clone(), going.opts.clone(), going.run);
        let (known, contents) = self.known();
        let tag = self.tag(Pending::Walk(going, folder));
        let avoid = self.avoid();
        self.io.send(Job::Walk(Box::new(Walk {
            tag,
            run,
            root,
            opts,
            known,
            contents,
            avoid,
            now_ms: wall_ms(),
        })));
    }

    /// Content ids by path, and every content the library has met.
    fn known(&self) -> (Known, HashSet<ContentId>) {
        let mut known = Known::new();
        let mut contents = HashSet::new();
        for book in self.model.all_books() {
            contents.insert(book.content);
            for at in &book.locations {
                known.insert(at.path.clone(), (at.stamp, book.content));
            }
        }
        (known, contents)
    }

    /// Rescans when the window comes back, and after files settle.
    ///
    /// Answers the wait the shell owes a repaint for a settling file.
    pub fn tick(&mut self, now: f64, focused: bool) -> Option<f64> {
        // Removals and replacements free store copies; they go now.
        let swept = self.model.take_swept();
        if let Some(store) = self.folder.as_ref().map(|f| f.store())
            && !swept.is_empty()
        {
            self.io.send(Job::Delete {
                store,
                paths: swept,
            });
        }
        let back = focused && !self.folders.focused;
        self.folders.focused = focused;
        let rested = self.folders.last.is_none_or(|t| now - t >= RESCAN_GAP);
        let watched = self.model.folders().any(|f| f.owes_walk());
        if back && rested && watched && self.status != Status::Loading {
            self.rescan(now);
        }
        let at = self.folders.settle_at?;
        match now >= at {
            true => {
                self.rescan(now);
                None
            }
            false => Some(at - now),
        }
    }

    /// Re-checks known files and walks watched folders, quietly.
    pub fn rescan(&mut self, now: f64) {
        if self.status == Status::Loading {
            return;
        }
        if !self.folders.walking.is_empty() {
            self.folders.again = true;
            return;
        }
        let living = (self.model.books()).flat_map(|b| b.locations.iter().map(|l| l.path.clone()));
        // The lazy iterator holds the model; gather before sending.
        let paths: Vec<PathBuf> = living.collect();
        self.io.send(Job::Check(paths));
        let watched: Vec<(FolderId, PathBuf, Options)> = (self.model.folders())
            .filter(|f| f.owes_walk())
            .map(|f| (f.id, f.root.clone(), f.opts.clone()))
            .collect();
        for (id, root, opts) in watched {
            let going = Going {
                root,
                opts,
                plan: Plan::default(),
                asked: Asked::OnFocus,
                run: None,
                found: 0,
            };
            self.folders.walking.insert(id);
            self.send_walk(going, Some(id));
        }
        self.folders.last = Some(now);
        self.folders.settle_at = None;
    }

    pub fn checked(&mut self, gone: &HashSet<PathBuf>) {
        self.model.check_present(|p| !gone.contains(p));
    }

    /// A quiet walk of a watched folder, landed as it found.
    pub fn rescanned(&mut self, id: FolderId, done: Walked, now: f64) {
        self.folders.walking.remove(&id);
        match done.offline {
            true => self.folders.offline.insert(id),
            false => self.folders.offline.remove(&id),
        };
        if !done.offline {
            let plan = Plan::default();
            let planned = self.model.plan_walk(id, done.found, Asked::OnFocus, plan);
            if let Some(planned) = planned.filter(|p| !p.is_empty()) {
                let asks = planned.asks.clone();
                self.model.land_walk(planned, &HashMap::new(), wall_ms());
                self.asks.extend(asks.into_iter().map(Question::Merge));
            }
        }
        if self.folders.walking.is_empty() && std::mem::take(&mut self.folders.again) {
            self.rescan(now);
        }
    }

    /// Files picked or dropped: folders ask, files are measured.
    pub fn import_files(&mut self, paths: Vec<PathBuf>, to: Option<ShelfId>, out: &mut Vec<Out>) {
        let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) =
            paths.into_iter().partition(|p| p.is_dir());
        for dir in dirs {
            self.ask_folder(dir);
        }
        if files.is_empty() {
            return;
        }
        if self.store().is_none() {
            let note = "The library has no folder to keep books in.";
            out.push(Out::Toast(Toast::error(note)));
            return;
        }
        let label = match files.as_slice() {
            [one] => file_name(one),
            many => format!("{} files", many.len()),
        };
        let run = Some(self.runs.start(label, Phase::Reading));
        let tag = self.tag(Pending::Files(to, run));
        let paths = files;
        self.io
            .send(Job::Files(Box::new(Files { tag, run, paths })));
    }

    /// A moved book shown on the open folder shelf too.
    pub fn show_moved(&mut self, folder: FolderId, book: BookId, now: f64) {
        self.sheet = None;
        let rung = |s: ShelfId| self.model.shelf(s).and_then(|s| s.kind.value.rung());
        let here = self
            .at
            .filter(|s| rung(*s).is_some_and(|(f, _)| f == folder));
        if let Some(shelf) = here {
            self.model.file(book, shelf, wall_ms());
            self.reveal(Pick::Book(book), now);
        }
    }
}

/// The folder an absent book's last address belongs to, if offline.
pub fn offline(book: &Book, offline: &BTreeSet<FolderId>) -> bool {
    book.missing()
        && book.locations.iter().any(|l| match l.origin {
            Origin::Folder(id) => offline.contains(&id),
            Origin::Opened | Origin::Store => false,
        })
}
