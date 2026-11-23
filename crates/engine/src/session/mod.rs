//! The app's state, and the one door that changes it.
//!
//! `Session` owns what outlives a frame and is not a drawing: the settings,
//! the pane tree and which pane is awake, the panes by last focus, the route,
//! and the last-opened document read from disk. A front end draws that state
//! and asks for changes by handing a [`Command`] to `Session::execute`
//! (`execute.rs`); what the session cannot show itself leaves as an [`Event`],
//! drained once a frame.
//!
//! [`Command`]: crate::command::Command

mod dials;
mod documents;
mod library;
mod looks;
mod page_ai;
mod panes;
mod read_aloud;
mod services;
mod skills;
mod space;
mod study;
mod study_deck;
mod study_mode;
mod study_recall;
mod tick;
mod updates;

use std::sync::Arc;

use mareader_appearance::{PaneLooks, Routing};
use mareader_session::{Anchor, SpinePlace};
use mareader_settings::Settings;
use mareader_workspace::{Bounds, PaneId, PaneTree};

use crate::documents::{OpenRequest, State as DocumentsState};
use crate::event::Event;
use crate::guard;
use crate::host::Shared;
use crate::journal::{Journal, Step};
use crate::library::State as LibraryState;
use crate::page_copy::PageCopy;
use crate::pane_texts::PaneTexts;
use crate::route::Route;
use crate::save::{self, LastDocument, Loaded, Save};
use crate::search::Search;
use crate::sounds::Sounds;
use crate::toast::Toast;
use crate::triggers::Clips;
use crate::vocab::Vocab;
use crate::word_list::WordLists;

pub use looks::AppearanceSnapshot;
pub use read_aloud::PaneDriver;
pub use tick::Tick;

/// The Dictionary's tab parked on the window's edge, as F6 finds it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EdgeTab {
    /// The tab is on screen to be visited.
    pub shown: bool,
    /// The keys are on it now.
    pub focused: bool,
}

/// Default pane area used before a front end reports its viewport.
const DEFAULT_AREA: Bounds = Bounds {
    x: 0.0,
    y: 0.0,
    w: 1200.0,
    h: 800.0,
};

/// Asks for a frame from any thread; the same alias `speech` exports.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

pub struct Session {
    settings: Settings,
    tree: PaneTree,
    active: PaneId,
    /// Panes by last focus, latest first: blend's paper follows them.
    recent: Vec<PaneId>,
    route: Route,
    /// Where the Dictionary page was opened from.
    dict_from: Option<Route>,
    /// Space's hub, or the tool it was left on.
    pub(crate) room: crate::space::Room,
    /// Split panes' own looks, and the routing they were last fitted to.
    looks: PaneLooks<PaneId>,
    routed: Routing,
    /// Split panes' own text, while text is independent.
    texts: PaneTexts,
    /// Picks new panes' hues and textures.
    random: u64,
    /// Run background service work inline when true (for tests/shots).
    inline: bool,
    /// Word levels dataset and CEFR shading cache.
    vocab: Vocab,
    /// Software update check, staging, and mock state.
    updates: crate::updates::State,
    /// Pronunciation pack and lookup cache.
    sounds: Sounds,
    /// Per-document full-text search indexes.
    search: Search,
    /// Multi-page text copy job headed for the host clipboard.
    page_copy: PageCopy,
    /// Embedded trigger audio/video clips.
    clips: Clips,
    /// Instant word-list builder job.
    word_lists: WordLists,
    /// Library model, shelves, searches, runs, and disk worker.
    library: LibraryState,
    /// Per-pane page-AI runs, results, continuous follow, and batch state.
    page_ai: crate::page_ai::ledger::Ledger,
    /// Loose study marks, AI gloss answers, and active explainer lookups.
    study: crate::study::state::State,
    /// Each pane's study mode, and gates of documents outside the library.
    study_mode: crate::study_mode::State,
    /// Skills asked of a selection or a page, and their answers.
    skills: crate::skills::State,
    /// Dictionary packs, queries, and word-list meaning lookups.
    dict: crate::dict::Service,
    /// Read-aloud playback, voice booths, and speech model shelf.
    read_aloud: crate::read_aloud::state::State,
    /// Open documents, per-pane reading places, text layers, and file watch.
    documents: DocumentsState,
    /// The room the panes share, in points: a split measures against it.
    area: Bounds,
    /// The Dictionary's parked edge tab state, as F6 inspects it.
    edge_tab: EdgeTab,
    /// Last document loaded from `session.json`, waiting to be resumed.
    last: Option<LastDocument>,
    /// Results not yet drained, oldest first.
    events: Vec<Event>,
    /// Bounded command journal for inspection and replay.
    pub(crate) journal: Journal,
    /// Current nesting depth of `command.batch`.
    pub(crate) batch_depth: u32,
    /// True while `execute_json` is recording the active command.
    pub(crate) recording_json: bool,
    waker: Waker,
    /// The machine it runs on, held from the moment it is built.
    host: Shared,
    /// Its own two files; a session with no directory writes nothing.
    save: Save,
}

/// What a command the session carries out alone came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Done {
    /// True when the state changed and the window should redraw.
    pub dirty: bool,
    /// Why a command was refused, if it was: a bad path, a bad value.
    pub refused: Option<String>,
}

impl Done {
    pub(super) fn quiet() -> Self {
        Self::default()
    }

    pub(super) fn changed() -> Self {
        Self {
            dirty: true,
            refused: None,
        }
    }

    pub(crate) fn refused(reason: String) -> Self {
        Self {
            dirty: false,
            refused: Some(reason),
        }
    }
}

impl Session {
    /// A session on `host`, woken by `waker`, holding what its own file says.
    ///
    /// The settings come from `settings.json` under [`Host::state_dir`]; a
    /// file that is not there, or cannot be read, leaves the defaults.
    ///
    /// [`Host::state_dir`]: crate::host::Host::state_dir
    pub fn new(host: Shared, waker: Waker) -> Self {
        let save = Save::new(host.state_dir());
        let loaded = save::load(save.dir());
        Self::build(host, waker, save, loaded, false)
    }

    /// A session initialized from pre-loaded state (such as a one-time import).
    pub fn from_loaded(host: Shared, waker: Waker, loaded: Loaded) -> Self {
        let save = Save::new(host.state_dir());
        Self::build(host, waker, save, loaded, false)
    }

    /// A headless session with inline background work and zero workers.
    pub fn headless(host: Shared) -> Self {
        let save = Save::new(host.state_dir());
        let loaded = save::load(save.dir());
        let mut session = Self::build(host, Arc::new(|| {}), save, loaded, true);
        if session.library.loading() {
            let _ = session.library_receive(0.0);
        }
        session
    }

    fn build(host: Shared, waker: Waker, save: Save, mut loaded: Loaded, inline: bool) -> Self {
        loaded.settings.sanitize();
        let tree = PaneTree::new();
        let mut page_copy = PageCopy::default();
        page_copy.attach(Arc::clone(&waker));
        let mut session = Self {
            host,
            waker,
            save,
            settings: loaded.settings,
            active: tree.panes().first().copied().unwrap_or(PaneId(1)),
            tree,
            recent: Vec::new(),
            route: Route::Reader,
            dict_from: None,
            room: crate::space::Room::Hub,
            looks: PaneLooks::default(),
            routed: Routing::default(),
            texts: PaneTexts::default(),
            random: 0x9e37_79b9_7f4a_7c15,
            inline: false,
            vocab: Vocab::default(),
            updates: crate::updates::State::default(),
            sounds: Sounds::default(),
            search: Search::default(),
            page_copy,
            clips: Clips::default(),
            word_lists: WordLists::default(),
            library: LibraryState::default(),
            page_ai: crate::page_ai::ledger::Ledger::default(),
            study: crate::study::state::State::default(),
            study_mode: crate::study_mode::State::default(),
            skills: crate::skills::State::default(),
            dict: crate::dict::Service::default(),
            read_aloud: crate::read_aloud::state::State::default(),
            documents: DocumentsState::default(),
            area: DEFAULT_AREA,
            edge_tab: EdgeTab::default(),
            last: loaded.last,
            events: Vec::new(),
            journal: Journal::default(),
            batch_depth: 0,
            recording_json: false,
        };
        let wake = Arc::clone(&session.waker);
        session.save.set_waker(Arc::clone(&wake));
        session.save.set_inline(inline);
        if inline {
            session.set_document_workers(0);
            session.set_inline(true);
        }
        session.page_ai.waker = Arc::clone(&wake);
        session.read_aloud.attach_waker(wake);
        session.wire_host();
        session
    }

    /// Wires engine services to the folders named by the session's host.
    fn wire_host(&mut self) {
        let machine = crate::host::Machine::under(self.host.as_ref());
        if let Some(root) = machine.library.clone() {
            self.wire_library(root);
        }
        if let Some(dir) = machine.cache.clone() {
            self.wire_library_cache(dir);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.wire_host_services(&machine);
    }

    /// The session's wake callback for background work that needs a new frame.
    pub fn waker(&self) -> Waker {
        Arc::clone(&self.waker)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn wire_host_services(&mut self, machine: &crate::host::Machine) {
        if let Some(dir) = machine.dictionaries.clone()
            && let Err(why) = self.wire_dict(dir)
        {
            self.emit(Event::Toast(Toast::warning(format!(
                "Dictionaries are off: {why}"
            ))));
        }
        if let Some(dir) = machine.vocabulary.clone() {
            self.wire_vocab(dir);
        }
        if let Some(dir) = machine.voices.clone()
            && let Err(why) = self.wire_voices(dir, mareader_speech::MODELS)
        {
            self.emit(Event::Toast(Toast::warning(format!(
                "Voice models are off: {why}"
            ))));
        }
    }

    /// Whether it has somewhere to write, so a caller can build what to put.
    pub fn persists(&self) -> bool {
        self.save.dir().is_some()
    }

    /// Remembers what was read, then writes when a write is due.
    ///
    /// The answer is the wait left when it skipped, for a frame to book.
    /// Any write failure leaves as an [`Event::Toast`].
    pub fn keep(&mut self, now: f64, last: Option<LastDocument>) -> Option<f64> {
        match guard::catch_panic(|| {
            self.save.poll();
            self.save.note(last);
            let wait = self.save.write(now, &self.settings);
            self.emit_save_failure();
            wait
        }) {
            Ok(wait) => wait,
            Err(msg) => {
                self.emit_panic("save", &msg);
                None
            }
        }
    }

    /// Writes what is owed at once: the window is going.
    pub fn settle(&mut self, last: Option<LastDocument>) {
        self.save.note(last);
        self.flush();
        self.updates.apply_on_exit(self.host.as_ref());
    }

    /// Both files at once, with the document last noted.
    ///
    /// What a front end's persist hook asks for, and what a test can: the
    /// cadence is skipped, and a session with nowhere to write writes nothing.
    pub fn flush(&mut self) {
        if let Err(msg) = guard::catch_panic(|| {
            self.library.flush();
            self.save.flush(&self.settings);
            self.emit_save_failure();
        }) {
            self.emit_panic("flush", &msg);
        }
    }

    /// Emits an error toast when the panic firewall catches an engine unwind.
    pub(crate) fn emit_panic(&mut self, where_: &str, msg: &str) {
        let text = format!("Engine {where_} error: {msg}");
        self.emit(Event::Toast(Toast::error(text)));
    }

    /// The session's bounded command journal, oldest entry first.
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// Clears all retained journal entries.
    pub fn clear_journal(&mut self) {
        self.journal.clear();
    }

    /// Replays a list of journal steps without re-recording them.
    pub fn replay(&mut self, steps: &[Step], now: f64) -> Result<usize, String> {
        self.journal.set_replaying(true);
        let mut outcome = Ok(steps.len());
        for (idx, step) in steps.iter().enumerate() {
            if let Err(why) = self.execute_json(&step.id, step.params.clone(), now) {
                outcome = Err(format!("step {}: {why}", idx + 1));
                break;
            }
        }
        self.journal.set_replaying(false);
        outcome
    }

    fn emit_save_failure(&mut self) {
        if let Some(why) = self.save.take_failure() {
            self.emit(Event::Toast(Toast::error(why)));
        }
    }

    /// The two files' text, for a platform that has no files to write them to.
    pub fn blobs(&self) -> Result<save::Blobs, String> {
        save::blobs(&self.settings, self.save.last())
    }

    /// Builds an open request for the saved document if its file still exists.
    pub fn resume_request(&self) -> Option<OpenRequest> {
        let last = self.last.as_ref().filter(|l| l.path.is_file())?;
        let place = last.place.map(|(block, offset)| Anchor { block, offset });
        let spine = last.spine.as_ref().map(|place| SpinePlace {
            href: place.href.clone(),
            item: place.item,
            block: place.block,
            offset: place.offset,
        });
        Some(OpenRequest {
            page: last.page,
            place,
            spine,
            pages: last.pages,
            book: last.book,
            ..OpenRequest::path(last.path.clone())
        })
    }

    pub(crate) fn clear_last(&mut self) {
        self.last = None;
    }

    /// The machine it runs on, for a command that reaches the clipboard.
    pub fn host(&self) -> &Shared {
        &self.host
    }

    /// Says something a front end shows when it drains.
    pub fn report(&mut self, toast: Toast) {
        self.emit(Event::Toast(toast));
    }

    /// Asks the front end for a frame.
    pub fn wake(&self) {
        (self.waker)();
    }

    /// Everything reported since the last drain, oldest first.
    pub fn drain(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn emit(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn tree(&self) -> &PaneTree {
        &self.tree
    }

    pub fn active(&self) -> PaneId {
        self.active
    }

    pub fn route(&self) -> Route {
        self.route
    }

    /// The page Space shows when its route is up.
    pub fn room(&self) -> crate::space::Room {
        self.room
    }

    pub fn dict_from(&self) -> Option<Route> {
        self.dict_from
    }

    /// Panes by last focus, latest first.
    pub fn recent(&self) -> &[PaneId] {
        &self.recent
    }

    /// The room the panes share, in points.
    pub fn area(&self) -> Bounds {
        self.area
    }

    /// Updates the room the panes share before running layout commands.
    pub fn set_area(&mut self, area: Bounds) {
        self.area = area;
    }

    /// The Dictionary's parked edge tab state, as F6 inspects it.
    pub fn edge_tab(&self) -> EdgeTab {
        self.edge_tab
    }

    /// Updates the parked edge tab state reported by the front end.
    pub fn set_edge_tab(&mut self, tab: EdgeTab) {
        self.edge_tab = tab;
    }
}
