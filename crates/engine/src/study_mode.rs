//! Study mode in the reader: batches gated by decks of their hard words, or
//! in Reactive mode a recall of each shaded word clicked.
//!
//! Each pane runs its own mode; a book's gate is saved with the book, so two
//! panes on one book share it. Documents outside the library keep theirs for
//! the session. See `docs/study-mode.md`.

pub mod backs;
pub mod counts;
pub mod deck_job;
pub mod known;
pub(crate) mod location;
pub mod open;
pub mod recalling;
pub mod run;
pub mod sightings;
pub mod stand;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use mareader_library::Spot;
use mareader_session::DocId;
use mareader_settings::{Settings, TaskId};
use mareader_study::mode::{CardWriter, Judge, Key, Mode};
use mareader_workspace::PaneId;

pub use backs::{Backs, Shown, Writer};
pub use deck_job::{Built, DeckJob, Doing, Tags};
pub use known::Known;
pub use open::Rest;
pub use recalling::{Ask, Recalling, Word};
pub use run::{Ahead, DeckState, Open, Prior, Run, Setup};
pub use stand::{Stand, Stands};

/// Who could write study's cards: a model chosen for study (the desktop
/// app's AI), then a dictionary, a pack's or the AI dictionary's.
pub fn writers(settings: &Settings, dict: &crate::dict::Service) -> (bool, bool) {
    let ai = cfg!(not(target_arch = "wasm32")) && settings.ai.model(TaskId::StudyCards).is_some();
    (ai, !dict.languages().is_empty())
}

/// What the study menu and the deck ask of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Turns `mode` on in `pane`, or off when it is already on there.
    Toggle {
        pane: PaneId,
        mode: Mode,
    },
    Off(PaneId),
    /// Ends the open deck at once: its words are kept and its pages unlock.
    Start(PaneId),
    /// The deck came into sight or left it: its time runs only in sight.
    Shown {
        pane: PaneId,
        shown: bool,
    },
    /// Opens again the cards left unfinished, or those kept for these pages.
    Reopen(PaneId),
    /// Saves a card's word as memorized; its card leaves the deck.
    Known {
        pane: PaneId,
        key: Key,
    },
    /// Brings back the last card marked known.
    Undo(PaneId),
    /// Takes the pane to the gate, where the deck waits.
    NextBatch(PaneId),
    /// Asks again for a deck that failed.
    Retry(PaneId),
    /// Writes the gate's deck from the dictionary, as the AI is slow or failed.
    UseDictionary(PaneId),
    /// Takes the pane to the page just before the gate.
    ReadPrevious(PaneId),
    /// Goes on with the cards an open deck has, its rest failed or slow.
    SkipRest(PaneId),
    /// What Reactive mode's recall popover asks.
    Recall {
        pane: PaneId,
        ask: Ask,
    },
}

pub const ON: &str = "Proactive study mode on. Words come first, then the pages.";
pub const ON_REACTIVE: &str = "Reactive study mode on. Click a shaded word to recall it.";
/// Why a recall can't open where it was asked.
pub const REACTIVE_ONLY: &str = "Recall works in Reactive study mode.";
pub const OFF: &str = "Study mode off.";
pub const AUTO_OFF: &str = "Study mode turned off: what it needs is no longer available.";
pub const READ_BACK: &str = "Read back as long as you like. The cards wait for your return.";
/// The button on the toast that brings the cards back.
pub const OPEN_CARDS: &str = "Open cards";
pub const REFUSED: &str = "Locked in study mode: no cards or dictionary until you turn it off.";
pub const READ_ALOUD_STOP: &str = "Read aloud stops at the study gate.";

/// The words for a locked batch, as its toast says.
pub fn locked(range: &str) -> String {
    format!("Locked. Study the words for {range} first.")
}

/// Why a page before a locked start won't show.
pub fn before(start: usize) -> String {
    format!("Pages before {} are locked while you study.", start + 1)
}

/// The cards offered again on coming back to `range`, with the time left.
pub fn ask_back(range: &str, left: Option<f64>) -> String {
    match left {
        Some(secs) => {
            let whole = secs.max(0.0).ceil().min(f64::from(u32::MAX)) as u64;
            let (m, s) = (whole / 60, whole % 60);
            format!("Open the cards for {range}? {m}:{s:02} left.")
        }
        None => format!("Open the cards for {range} again?"),
    }
}

/// A deck whose time ran out, as its toast says.
pub fn time_up(range: &str) -> String {
    format!("Time's up for the cards on {range}. Read on.")
}

/// A deck with nothing new, as its toast says.
pub fn nothing_new(range: &str) -> String {
    format!("No new words in {range}. Read on.")
}

/// Every pane's study mode, and the gates of documents outside the library.
#[derive(Default)]
pub struct State {
    runs: BTreeMap<PaneId, (DocId, Run)>,
    loose: HashMap<DocId, Spot>,
    /// The AI's card writer, once the app has one.
    cards: Option<Arc<dyn CardWriter>>,
    backs: Backs,
    /// The AI's judge of recalls, once the app has one.
    judge: Option<Arc<dyn Judge>>,
    /// The recall open in a Reactive pane, if any.
    recalling: Option<Recalling>,
}

impl State {
    pub(crate) fn set_cards(&mut self, cards: Arc<dyn CardWriter>) {
        self.cards = Some(cards);
    }

    pub(crate) fn set_judge(&mut self, judge: Arc<dyn Judge>) {
        self.judge = Some(judge);
    }

    pub(crate) fn judge(&self) -> Option<Arc<dyn Judge>> {
        self.judge.clone()
    }

    /// The recall open in `pane`, if any.
    pub fn recalling(&self, pane: PaneId) -> Option<&Recalling> {
        self.recalling.as_ref().filter(|r| r.pane == pane)
    }

    pub(crate) fn recalling_pane(&self) -> Option<PaneId> {
        self.recalling.as_ref().map(|r| r.pane)
    }

    pub(crate) fn recalling_mut(&mut self, pane: PaneId) -> Option<&mut Recalling> {
        self.recalling.as_mut().filter(|r| r.pane == pane)
    }

    /// Opens `recalling`, closing any other.
    pub(crate) fn open_recall(&mut self, recalling: Recalling) {
        self.recalling = Some(recalling);
    }

    pub(crate) fn take_recall(&mut self) -> Option<Recalling> {
        self.recalling.take()
    }

    /// The AI's writer with the backs it wrote before; none without one.
    pub(crate) fn writer(&self) -> Option<Writer> {
        let backs = self.backs.clone();
        self.cards.clone().map(|cards| {
            let cards = cards.snapshot().unwrap_or(cards);
            Writer { cards, backs }
        })
    }

    pub fn run(&self, pane: PaneId) -> Option<&Run> {
        self.runs.get(&pane).map(|(_, run)| run)
    }

    /// Every studying pane's run, by pane.
    pub fn runs(&self) -> impl Iterator<Item = (PaneId, &Run)> {
        self.runs.iter().map(|(pane, (_, run))| (*pane, run))
    }

    /// Whether study mode is on in `pane`.
    pub fn on(&self, pane: PaneId) -> bool {
        self.runs.contains_key(&pane)
    }

    /// The first page `pane` may not pass, if one is locked.
    pub fn gate(&self, pane: PaneId) -> Option<usize> {
        self.run(pane)?.gate()
    }

    pub fn busy(&self) -> bool {
        let recalling = self.recalling.as_ref().is_some_and(Recalling::busy);
        recalling || self.runs.values().any(|(_, run)| run.busy())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }

    pub(crate) fn panes(&self) -> Vec<PaneId> {
        self.runs.keys().copied().collect()
    }

    pub(crate) fn doc(&self, pane: PaneId) -> Option<DocId> {
        self.runs.get(&pane).map(|(doc, _)| *doc)
    }

    pub(crate) fn run_mut(&mut self, pane: PaneId) -> Option<&mut Run> {
        self.runs.get_mut(&pane).map(|(_, run)| run)
    }

    pub(crate) fn start(&mut self, pane: PaneId, doc: DocId, run: Run) {
        self.runs.insert(pane, (doc, run));
    }

    pub(crate) fn stop(&mut self, pane: PaneId) -> bool {
        if self.recalling(pane).is_some() {
            self.recalling = None;
        }
        self.runs.remove(&pane).is_some()
    }

    pub(crate) fn loose(&self, doc: DocId) -> Option<&Spot> {
        self.loose.get(&doc)
    }

    pub(crate) fn set_loose(&mut self, doc: DocId, spot: Spot) {
        self.loose.insert(doc, spot);
    }

    /// Other panes' runs on the same book or document plan again.
    pub(crate) fn restale_others(&mut self, pane: PaneId) {
        let Some((doc, book)) = self.runs.get(&pane).map(|(d, r)| (*d, r.book)) else {
            return;
        };
        for (other, (their_doc, run)) in &mut self.runs {
            let same = *their_doc == doc || (book.is_some() && run.book == book);
            if *other != pane && same {
                run.restale();
            }
        }
    }
}
