//! A deck being studied: its cards, its time, its parts and the undo trail.
//!
//! A deck over 1.5 times Words per deck comes in parts of about that many
//! cards ([`chunks`]), back to back before its pages, each part timed on
//! its own. Only a page too crowded for one deck makes one in words mode.

use mareader_library::WordStatus;
use mareader_study::mode::{Allowance, Batch, Key, Via, chunks};

use super::backs::Shown;
use super::deck_job::Built;

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
    /// This part, from 1, and how many there are.
    pub part: (usize, usize),
    /// The parts still to come.
    later: Vec<Vec<Shown>>,
    /// The cards of the parts before, kept to save and to open again.
    studied: Vec<Shown>,
}

impl Open {
    /// A deck that may stay open `per_card` seconds for each card it
    /// opens with, or with no limit.
    pub fn new(batch: Batch, built: Built, per_card: Option<u32>) -> Self {
        Self {
            allowance: Allowance::new(per_card, built.cards.len()),
            away: false,
            batch,
            cards: built.cards,
            found: built.found,
            skipped_known: built.skipped_known,
            via: built.via,
            known: Vec::new(),
            part: (1, 1),
            later: Vec::new(),
            studied: Vec::new(),
        }
    }

    /// The deck in parts of about `target` cards once it holds 1.5 times
    /// that, the first part timed at `per_card`.
    pub fn in_parts(mut self, target: usize, per_card: Option<u32>) -> Self {
        let cuts = chunks(self.cards.len(), target);
        if cuts.len() < 2 {
            return self;
        }
        let mut all = std::mem::take(&mut self.cards);
        let backwards = cuts.iter().rev().map(|c| all.split_off(c.start));
        let mut parts: Vec<Vec<Shown>> = backwards.collect();
        parts.reverse();
        self.cards = parts.remove(0);
        self.later = parts;
        self.part = (1, cuts.len());
        self.allowance = Allowance::new(per_card, self.cards.len());
        self
    }

    /// Parts after this one.
    pub fn parts_left(&self) -> usize {
        self.later.len()
    }

    /// Turns to the next part, this one's cards studied; false on the last.
    pub fn next_part(&mut self, per_card: Option<u32>, now: f64) -> bool {
        if self.later.is_empty() {
            return false;
        }
        let next = self.later.remove(0);
        self.studied.append(&mut self.cards);
        self.cards = next;
        self.known.clear();
        self.part.0 += 1;
        let running = self.allowance.running();
        self.allowance = Allowance::new(per_card, self.cards.len());
        if running {
            self.allowance.resume(now);
        }
        true
    }

    /// Every part studied as one deck again, to be saved or kept.
    pub fn gather(&mut self) {
        let mut all = std::mem::take(&mut self.studied);
        all.append(&mut self.cards);
        self.cards = all;
        self.part = (1, 1);
    }

    /// Takes `key`'s card out of the deck; a bare word takes its card
    /// whatever the class, as a script names it.
    pub fn take_known(&mut self, key: &Key) -> Option<(usize, Shown)> {
        let exact = self.cards.iter().position(|s| &s.card.key == key);
        let bare = || {
            let lemma = |s: &Shown| key.pos.is_none() && s.card.key.lemma == key.lemma;
            self.cards.iter().position(lemma)
        };
        let at = exact.or_else(bare)?;
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
