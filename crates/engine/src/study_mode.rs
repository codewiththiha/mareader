//! Study mode in the reader: batches gated by decks of their hard words.
//!
//! Each pane runs its own mode; a book's gate is saved with the book, so two
//! panes on one book share it. Documents outside the library keep theirs for
//! the session. See `docs/study-mode.md`.

pub mod deck_job;
pub mod run;
pub mod sightings;

use std::collections::{BTreeMap, HashMap};

use mareader_library::Spot;
use mareader_session::DocId;
use mareader_study::mode::{Key, Mode};
use mareader_workspace::PaneId;

pub use deck_job::{Built, DeckJob, Known, Shown, Tags};
pub use run::{DeckState, Open, Prior, Run};

/// What the study menu and the deck ask of the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Turns `mode` on in `pane`, or off when it is already on there.
    Toggle { pane: PaneId, mode: Mode },
    Off(PaneId),
    /// Ends the open deck: its words are kept and its pages unlock.
    Start(PaneId),
    /// Saves a card's word as memorized; its card leaves the deck.
    Known { pane: PaneId, key: Key },
    /// Brings back the last card marked known.
    Undo(PaneId),
    /// Takes the pane to the gate, where the deck waits.
    NextBatch(PaneId),
    /// Asks again for a deck that failed.
    Retry(PaneId),
    /// Takes the pane back to the batch before the gate.
    ReadPrevious(PaneId),
}

pub const ON: &str = "Proactive study mode on. Words come first, then the pages.";
pub const OFF: &str = "Study mode off.";
pub const AUTO_OFF: &str = "Study mode turned off: what it needs is no longer available.";
pub const READ_BACK: &str =
    "Scroll back to re-read. The words open again when you return to the end.";
pub const REFUSED: &str = "Locked in study mode: no cards or dictionary until you turn it off.";
pub const REACTIVE_LATER: &str = "Reactive study mode comes in a later update.";
pub const READ_ALOUD_STOP: &str = "Read aloud stops at the study gate.";

/// The words for a locked batch, as its toast says.
pub fn locked(range: &str) -> String {
    format!("Locked. Study the words for {range} first.")
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
}

impl State {
    pub fn run(&self, pane: PaneId) -> Option<&Run> {
        self.runs.get(&pane).map(|(_, run)| run)
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
        self.runs.values().any(|(_, run)| run.busy())
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
