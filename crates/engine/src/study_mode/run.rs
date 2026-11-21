//! One pane's study mode: its batches, its gate and the deck before it.
//!
//! Pages before `unlocked` read freely; the batch starting there waits on
//! its deck. Until the pages are measured on the current layout the pane
//! is held at the page it showed when planning began.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use mareader_library::{BookId, WordStatus};
use mareader_session::Document;
use mareader_study::mode::{Batch, Floor, Key, Mode, Size, Via, entering, gate, plan, retypeset};

use super::backs::Shown;
use super::deck_job::{Built, DeckJob, Tags};
use crate::session::Waker;
use crate::word_list::Meanings;
use crate::word_marks::off_thread;

/// The deck at the gate, as far as it got.
pub enum DeckState {
    /// No batch is locked, or none is planned yet.
    None,
    Making { batch: Batch, job: DeckJob },
    Open(Open),
    Failed { batch: Batch, why: String },
}

impl DeckState {
    fn batch(&self) -> Option<&Batch> {
        match self {
            Self::None => None,
            Self::Making { batch, .. } | Self::Failed { batch, .. } => Some(batch),
            Self::Open(open) => Some(&open.batch),
        }
    }
}

/// A deck made ahead, for a batch past the gate.
pub enum Ahead {
    Making { batch: Batch, job: DeckJob },
    Ready { batch: Batch, built: Built },
    Failed { batch: Batch, why: String },
}

impl Ahead {
    pub fn batch(&self) -> &Batch {
        match self {
            Self::Making { batch, .. } | Self::Ready { batch, .. } | Self::Failed { batch, .. } => {
                batch
            }
        }
    }

    /// Steps a deck being made; true once it is built or has failed.
    pub fn step(&mut self, words: &mut dyn Tags, meanings: &mut dyn Meanings, now: f64) -> bool {
        let Self::Making { batch, job } = self else {
            return false;
        };
        let next = match job.step(words, meanings, now) {
            Ok(None) => return false,
            Ok(Some(built)) => Self::Ready {
                batch: batch.clone(),
                built,
            },
            Err(why) => Self::Failed {
                batch: batch.clone(),
                why,
            },
        };
        *self = next;
        true
    }
}

/// What a word was before Known saved it, so undo can put it back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prior {
    /// Known made it.
    New,
    Was(WordStatus),
    /// Nothing was saved: the document is outside the library.
    Unsaved,
}

/// A deck being studied: its cards, the time floor and the undo trail.
pub struct Open {
    pub batch: Batch,
    pub cards: Vec<Shown>,
    pub found: usize,
    pub skipped_known: usize,
    pub via: Via,
    pub floor: Floor,
    /// Cards marked known, latest last, each with the place it left.
    known: Vec<(usize, Shown, Prior)>,
}

impl Open {
    pub fn new(batch: Batch, built: Built, per_card: u32, now: f64) -> Self {
        Self {
            floor: Floor::new(built.cards.len(), per_card, now),
            batch,
            cards: built.cards,
            found: built.found,
            skipped_known: built.skipped_known,
            via: built.via,
            known: Vec::new(),
        }
    }

    /// Takes `key`'s card out of the deck, giving its seconds back.
    pub fn take_known(&mut self, key: &Key) -> Option<(usize, Shown)> {
        let at = self.cards.iter().position(|s| &s.card.key == key)?;
        self.floor.known();
        Some((at, self.cards.remove(at)))
    }

    /// Files the card [`Self::take_known`] took from `at`, for undo.
    pub fn keep_known(&mut self, at: usize, shown: Shown, prior: Prior) {
        self.known.push((at, shown, prior));
    }

    /// Puts the last known card back where it was; its word's prior state.
    pub fn undo_known(&mut self) -> Option<(Key, Prior)> {
        let (at, shown, prior) = self.known.pop()?;
        let key = shown.card.key.clone();
        self.cards.insert(at.min(self.cards.len()), shown);
        self.floor.unknown();
        Some((key, prior))
    }

    pub fn can_undo(&self) -> bool {
        !self.known.is_empty()
    }
}

/// What planning reads from the session in one frame.
pub struct Sheet<'a> {
    pub document: &'a Arc<Document>,
    pub wake: Waker,
    pub inline: bool,
    pub size: Size,
    /// The saved gate on this layout, if the book has one.
    pub saved: Option<usize>,
    pub current: usize,
}

/// Study mode in one pane.
pub struct Run {
    pub mode: Mode,
    pub book: Option<BookId>,
    pub batches: Vec<Batch>,
    /// The first locked page; past the last page, nothing is locked.
    pub unlocked: usize,
    pub deck: DeckState,
    /// Decks for the batches past the gate's, nearest first.
    pub ahead: Vec<Ahead>,
    /// The batch, by its first page, whose deck the dictionary writes instead.
    pub fallback: Option<usize>,
    /// What the decks were made from: the source, the level, the writer.
    recipe: String,
    /// The layout measured: the document's layout key and page count.
    layout: Option<(Option<u64>, usize)>,
    size: Option<Size>,
    sizes: Vec<usize>,
    measuring: Option<Receiver<Vec<usize>>>,
    /// Where the pane is held until batches are planned.
    at: usize,
    entered: bool,
    /// The saved gate moved: plan again from it.
    stale: bool,
}

impl Run {
    pub fn new(mode: Mode, book: Option<BookId>, at: usize) -> Self {
        Self {
            mode,
            book,
            batches: Vec::new(),
            unlocked: 0,
            deck: DeckState::None,
            ahead: Vec::new(),
            fallback: None,
            recipe: String::new(),
            layout: None,
            size: None,
            sizes: Vec::new(),
            measuring: None,
            at,
            entered: false,
            stale: false,
        }
    }

    /// Whether batches stand on the current layout.
    pub fn planned(&self) -> bool {
        self.layout.is_some() && self.measuring.is_none() && self.size.is_some()
    }

    /// The page the pane may not pass, if any is locked.
    pub fn gate(&self) -> Option<usize> {
        if self.mode == Mode::Reactive {
            return None;
        }
        if !self.planned() {
            return Some(self.at);
        }
        (self.unlocked < self.sizes.len()).then_some(self.unlocked)
    }

    /// The batch waiting on its deck.
    pub fn gate_batch(&self) -> Option<&Batch> {
        if self.mode == Mode::Reactive || !self.planned() {
            return None;
        }
        gate(&self.batches, self.unlocked)
    }

    /// The batch before the gate, to read again.
    pub fn previous(&self) -> Option<&Batch> {
        let end = self.gate()?;
        self.batches.iter().rev().find(|b| b.to < end)
    }

    pub fn busy(&self) -> bool {
        let making = |a: &Ahead| matches!(a, Ahead::Making { .. });
        self.measuring.is_some()
            || matches!(self.deck, DeckState::Making { .. })
            || self.ahead.iter().any(making)
    }

    /// Up to `count` batches past the gate's, nearest first.
    pub fn upcoming(&self, count: usize) -> Vec<Batch> {
        let Some(at) = self.gate_batch() else {
            return Vec::new();
        };
        let past = self.batches.iter().filter(|b| b.from > at.to);
        past.take(count).cloned().collect()
    }

    /// Drops the decks made ahead for batches past the window, which
    /// cancels their asks.
    pub fn trim_ahead(&mut self, count: usize) {
        let coming = self.upcoming(count);
        self.ahead.retain(|a| coming.contains(a.batch()));
    }

    pub fn has_ahead(&self, batch: &Batch) -> bool {
        self.ahead.iter().any(|a| a.batch() == batch)
    }

    /// Takes the deck made ahead for `batch`, as the gate reaches it.
    pub fn take_ahead(&mut self, batch: &Batch) -> Option<Ahead> {
        let at = self.ahead.iter().position(|a| a.batch() == batch)?;
        Some(self.ahead.remove(at))
    }

    /// Plans again from the gate saved elsewhere, as another pane moved it.
    pub fn restale(&mut self) {
        self.stale = true;
    }

    /// Measures when the layout moved and plans once sizes land.
    ///
    /// True when the gate was planned this frame and should be saved.
    pub fn plan(&mut self, sheet: Sheet<'_>) -> Result<bool, String> {
        let document = sheet.document;
        let layout = (document.layout(), document.page_count());
        if self.layout != Some(layout) && document.is_complete() {
            self.layout = Some(layout);
            self.at = sheet.current;
            let measured = Arc::clone(document);
            let work = move || sizes(&measured);
            self.measuring = Some(off_thread(sheet.wake, sheet.inline, "study-sizes", work));
        }
        if let Some(rx) = &self.measuring {
            match rx.try_recv() {
                Ok(sizes) => {
                    self.sizes = sizes;
                    self.measuring = None;
                    self.stale = true;
                }
                Err(TryRecvError::Empty) => return Ok(false),
                Err(TryRecvError::Disconnected) => return Err("Couldn't read the pages".into()),
            }
        }
        if self.layout.is_none() || (!self.stale && self.size == Some(sheet.size)) {
            return Ok(false);
        }
        self.stale = false;
        self.size = Some(sheet.size);
        self.batches = plan(&self.sizes, sheet.size);
        let saved = retypeset(sheet.saved.unwrap_or(0), &self.batches, self.sizes.len());
        self.unlocked = match self.entered {
            true => saved,
            false => entering(self.at, saved, &self.batches),
        };
        self.entered = true;
        self.drop_stale_deck();
        Ok(true)
    }

    /// Forgets the decks made from another recipe, but for one open now.
    pub fn follow(&mut self, recipe: String) -> bool {
        if self.recipe == recipe {
            return false;
        }
        let first = self.recipe.is_empty();
        self.recipe = recipe;
        if first {
            return false;
        }
        self.ahead.clear();
        self.fallback = None;
        if !matches!(self.deck, DeckState::Open(_)) {
            self.deck = DeckState::None;
        }
        true
    }

    /// Unlocks every page through `to`; the deck before them is done.
    pub fn unlock_through(&mut self, to: usize) {
        self.unlocked = self.unlocked.max(to.saturating_add(1));
        self.deck = DeckState::None;
    }

    /// Forgets a deck made for a batch other than the one at the gate.
    pub fn drop_stale_deck(&mut self) {
        if self.deck.batch() != self.gate_batch() {
            self.deck = DeckState::None;
        }
    }
}

/// Each page's character count, as the reader's text holds it.
fn sizes(document: &Document) -> Vec<usize> {
    let count = document.page_count();
    let size = |page| {
        let layer = document.text_layer(page);
        layer.map_or(0, |layer| crate::study::words::read(&layer).0.len())
    };
    (0..count).map(size).collect()
}

#[cfg(test)]
#[path = "run_tests.rs"]
mod tests;
