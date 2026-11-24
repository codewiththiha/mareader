//! One pane's study mode: its batches, its gate and the deck before it.
//!
//! Batches start at the page shown when the mode turned on; the pages
//! before it are not studied. Pages before `unlocked` read freely; the batch
//! starting there waits on its deck. Until the pages are measured on the
//! current layout the pane is held at the page it showed when planning began.

use std::ops::Range;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use mareader_library::{BookId, Spot, WordStatus};
use mareader_session::Document;
use mareader_study::mode::{
    Allowance, Batch, Key, Mode, Outcome, Size, Via, gate, plan_from, retypeset,
};

use super::backs::Shown;
use super::deck_job::{Built, DeckJob, Tags};
use super::known::Known;
use crate::session::Waker;
use crate::word_list::Meanings;
use crate::word_marks::off_thread;

/// The deck at the gate, as far as it got.
pub enum DeckState {
    /// No batch is locked, or none is planned yet.
    None,
    Making {
        batch: Batch,
        job: Box<DeckJob>,
    },
    Open(Open),
    Failed {
        batch: Batch,
        why: String,
        setup: Option<Setup>,
    },
}

/// What a failed deck needs set up; another try alone won't mend it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setup {
    /// Nothing can write the cards: no model and no dictionary.
    Writer,
    /// The model turned the deck away over its keys or settings.
    Model,
    /// The word levels aren't here.
    WordData,
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
    Making {
        batch: Batch,
        job: Box<DeckJob>,
    },
    Ready {
        batch: Batch,
        built: Built,
    },
    Failed {
        batch: Batch,
        why: String,
        setup: Option<Setup>,
    },
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
                setup: job.setup(),
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

/// A deck being studied: its cards, its time and the undo trail.
pub struct Open {
    pub batch: Batch,
    pub cards: Vec<Shown>,
    pub found: usize,
    pub skipped_known: usize,
    pub via: Via,
    pub allowance: Allowance,
    /// Left unfinished for the pages before; it waits to be asked back.
    pub away: bool,
    /// Cards marked known, latest last, each with the place it left.
    known: Vec<(usize, Shown, Prior)>,
}

impl Open {
    /// A deck that may stay open `minutes`, or with no limit.
    pub fn new(batch: Batch, built: Built, minutes: Option<u32>) -> Self {
        Self {
            allowance: Allowance::new(minutes),
            away: false,
            batch,
            cards: built.cards,
            found: built.found,
            skipped_known: built.skipped_known,
            via: built.via,
            known: Vec::new(),
        }
    }

    /// Takes `key`'s card out of the deck.
    pub fn take_known(&mut self, key: &Key) -> Option<(usize, Shown)> {
        let at = self.cards.iter().position(|s| &s.card.key == key)?;
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
    /// The run's first page on this layout, once it has one.
    pub start: Option<usize>,
    pub current: usize,
}

/// Study mode in one pane.
pub struct Run {
    pub mode: Mode,
    pub book: Option<BookId>,
    pub batches: Vec<Batch>,
    /// The page the mode turned on at: the first batch starts there.
    pub start: usize,
    /// Where `start` stands in the book, so a re-typeset keeps it.
    pub anchor: Option<Spot>,
    /// The first locked page; past the last page, nothing is locked.
    pub unlocked: usize,
    pub deck: DeckState,
    /// Decks read with no time limit, offered again on coming back.
    pub kept: Vec<Open>,
    /// A kept deck opened again over its pages.
    pub review: Option<Open>,
    /// Decks for the batches past the gate's, nearest first.
    pub ahead: Vec<Ahead>,
    /// The batch, by its first page, whose deck the dictionary writes instead.
    pub fallback: Option<usize>,
    /// Words recalled this run: their page, characters and outcome.
    pub recalled: Vec<(usize, Range<usize>, Outcome)>,
    /// Words marked known this run; later decks skip them, saved or not.
    pub known: Known,
    /// What the decks were made from: the source, the level, the writer.
    recipe: String,
    /// The layout measured: the document's layout key and page count.
    layout: Option<(Option<u64>, usize)>,
    size: Option<Size>,
    sizes: Vec<usize>,
    measuring: Option<Receiver<Vec<usize>>>,
    /// The page read last frame, to see the reader come back to a batch.
    seen: Option<usize>,
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
            start: at,
            anchor: None,
            unlocked: 0,
            deck: DeckState::None,
            kept: Vec::new(),
            review: None,
            ahead: Vec::new(),
            fallback: None,
            recalled: Vec::new(),
            known: Known::default(),
            recipe: String::new(),
            layout: None,
            size: None,
            sizes: Vec::new(),
            measuring: None,
            seen: None,
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

    /// The first page the pane may show: the start with `lock`, else 0.
    pub fn floor(&self, lock: bool) -> usize {
        match lock && self.mode == Mode::Proactive {
            true => self.start,
            false => 0,
        }
    }

    /// The page just before the gate, where Read back goes; with `lock`
    /// never one before the start.
    pub fn back(&self, lock: bool) -> Option<usize> {
        let page = self.gate()?.checked_sub(1)?;
        (!lock || page >= self.start).then_some(page)
    }

    /// The deck on screen when the gate is reached: one opened again first.
    pub fn shown_deck(&self) -> Option<&Open> {
        if self.review.is_some() {
            return self.review.as_ref();
        }
        match &self.deck {
            DeckState::Open(open) => Some(open),
            _ => None,
        }
    }

    pub fn shown_deck_mut(&mut self) -> Option<&mut Open> {
        if self.review.is_some() {
            return self.review.as_mut();
        }
        match &mut self.deck {
            DeckState::Open(open) => Some(open),
            _ => None,
        }
    }

    /// The batch whose cards to offer, as the reader comes into it from the
    /// pages before: the gate's left unfinished, or one kept.
    pub fn arrived(&mut self, current: usize) -> Option<Batch> {
        let before = self.seen.replace(current)?;
        let into = |b: &Batch| before < b.from && current >= b.from;
        if self.review.is_some() {
            return None;
        }
        if let DeckState::Open(open) = &self.deck {
            let back = open.away && into(&open.batch);
            if back {
                return Some(open.batch.clone());
            }
        }
        let mut kept = self.kept.iter().map(|k| &k.batch);
        kept.find(|&b| into(b) && b.holds(current)).cloned()
    }

    /// Shows the cards again: the gate's left unfinished, else those kept
    /// for the batch read now.
    pub fn reopen(&mut self) -> bool {
        if let DeckState::Open(open) = &mut self.deck {
            let away = std::mem::replace(&mut open.away, false);
            if away {
                return true;
            }
        }
        let Some(at) = self.seen else {
            return false;
        };
        let Some(i) = self.kept.iter().position(|k| k.batch.holds(at)) else {
            return false;
        };
        self.review = Some(self.kept.remove(i));
        true
    }

    /// Puts a deck opened again back with those kept.
    pub fn close_review(&mut self) -> bool {
        let Some(open) = self.review.take() else {
            return false;
        };
        self.kept.push(open);
        true
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
        self.start = match (self.entered, sheet.start) {
            (true, Some(start)) => start,
            (true, None) => self.start,
            (false, _) => self.at,
        };
        self.start = self.start.min(self.sizes.len());
        self.batches = plan_from(&self.sizes, sheet.size, self.start);
        self.unlocked = match (self.entered, sheet.saved) {
            (true, Some(saved)) if saved > self.start => {
                retypeset(saved, &self.batches, self.sizes.len())
            }
            _ => self.start,
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
