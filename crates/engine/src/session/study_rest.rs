//! Another try for a deck: the gate's that failed, or the rest of one
//! that opened half written.
//!
//! The job that wrote an open deck's first cards writes on while the
//! reader studies them; each step lands what it finished. When it fails,
//! the reader may try again, have the dictionary write the rest, or skip
//! it.

use std::sync::Arc;

use mareader_workspace::PaneId;

use super::Session;
use super::study_deck::fresh;
use crate::study_mode::{DeckJob, DeckState, Open, Rest};

impl Session {
    /// Asks again for the deck that failed at `pane`'s gate.
    pub(crate) fn study_mode_retry(&mut self, pane: PaneId) -> bool {
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        if matches!(run.deck, DeckState::Open(_)) {
            return self.study_rest_retry(pane);
        }
        let failed = matches!(run.deck, DeckState::Failed { .. });
        if failed {
            run.deck = DeckState::None;
            run.fallback = None;
        }
        failed
    }

    /// Writes the gate's deck from the dictionary, while it waits or failed.
    pub(crate) fn study_mode_dictionary(&mut self, pane: PaneId) -> bool {
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        let Some(from) = run.gate_batch().map(|b| b.from) else {
            return false;
        };
        if matches!(run.deck, DeckState::Open(_)) {
            return self.study_rest_dictionary(pane);
        }
        let none = matches!(run.deck, DeckState::None);
        if none || run.fallback == Some(from) {
            return false;
        }
        run.fallback = Some(from);
        run.deck = DeckState::None;
        true
    }

    /// Seconds per card while the time limit is on.
    pub(super) fn study_per_card(&self) -> Option<u32> {
        let mode = &self.settings.study.mode;
        mode.limit.then_some(u32::from(mode.card_seconds))
    }

    /// Steps the open deck's rest: lands its cards, or keeps why it failed.
    pub(super) fn study_rest(&mut self, pane: PaneId, now: f64) -> bool {
        let per_card = self.study_per_card();
        let known = self.study_known(pane);
        let deck = self.study_mode.run_mut(pane).map(|r| &mut r.deck);
        let Some(DeckState::Open(open)) = deck else {
            return false;
        };
        let Rest::Writing(job) = &mut open.rest else {
            return false;
        };
        match job.step(&mut self.vocab, &mut self.dict, now) {
            Ok(None) => false,
            Ok(Some(built)) => {
                open.rest = Rest::Done;
                open.land(fresh(built, &known).cards, per_card);
                true
            }
            Err(why) => {
                let setup = job.setup();
                open.rest = Rest::Failed { why, setup };
                true
            }
        }
    }

    /// Writes the failed rest again, by whoever writes the batch's decks.
    fn study_rest_retry(&mut self, pane: PaneId) -> bool {
        let open = open_of(self, pane);
        let failed = open.is_some_and(|o| matches!(o.rest, Rest::Failed { .. }));
        failed && self.study_rest_again(pane)
    }

    /// Has the dictionary write the rest, failed or slow.
    fn study_rest_dictionary(&mut self, pane: PaneId) -> bool {
        let Some(open) = open_of(self, pane) else {
            return false;
        };
        if matches!(open.rest, Rest::Done) {
            return false;
        }
        let from = open.batch.from;
        if let Some(run) = self.study_mode.run_mut(pane) {
            run.fallback = Some(from);
        }
        self.study_rest_again(pane)
    }

    /// Goes without the rest: the deck is the cards it has.
    pub(crate) fn study_mode_skip_rest(&mut self, pane: PaneId) -> bool {
        let Some(open) = open_of(self, pane) else {
            return false;
        };
        let had = !matches!(open.rest, Rest::Done);
        open.rest = Rest::Done;
        had
    }

    fn study_rest_again(&mut self, pane: PaneId) -> bool {
        let Some(batch) = open_of(self, pane).map(|o| o.batch.clone()) else {
            return false;
        };
        let Some(ask) = self.study_ask(pane, &batch) else {
            return false;
        };
        let job = Box::new(DeckJob::new(Arc::clone(&self.waker), ask));
        let Some(open) = open_of(self, pane) else {
            return false;
        };
        open.rest = Rest::Writing(job);
        true
    }
}

/// `pane`'s open deck at the gate.
fn open_of(session: &mut Session, pane: PaneId) -> Option<&mut Open> {
    match session.study_mode.run_mut(pane).map(|r| &mut r.deck) {
        Some(DeckState::Open(open)) => Some(open),
        _ => None,
    }
}
