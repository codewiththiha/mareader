//! Study mode's decks in the session: the gate's, and those made ahead.
//!
//! The gate's deck is taken from the decks made ahead, or made when its
//! batch is planned; with `ahead` at 0 it waits until the reader comes near
//! the gate. Up to `ahead - 1` decks past it are made while the reader
//! reads, and dropped, which cancels their asks, once their batch is no
//! longer coming. Words memorized since a deck was made leave it as it
//! opens. Decks not yet open are made again once the source, the level or
//! the model changes.

use std::sync::Arc;

use mareader_library::{Library, WordStatus};
use mareader_study::mode::{Batch, CardWriter, Key, Pos, Via, via};
use mareader_workspace::PaneId;

use super::Session;
use crate::event::Event;
use crate::study_mode::deck_job::Ask;
use crate::study_mode::{self as mode, Ahead, Built, DeckJob, DeckState, Known, Open, Stands};
use crate::toast::Toast;

impl Session {
    /// Lets study mode's AI write cards with `cards`.
    pub fn study_set_cards(&mut self, cards: Arc<dyn CardWriter>) {
        self.study_mode.set_cards(cards);
    }

    /// Forgets `pane`'s decks once the source, the level or the writer
    /// changed, so the next ones follow the settings.
    pub(super) fn study_follow(&mut self, pane: PaneId) -> bool {
        let mode = &self.settings.study.mode;
        let writer = self.study_mode.writer().map(|w| w.cards.id());
        let recipe = format!(
            "{:?}|{:?}|{}|{}|{}",
            mode.source,
            self.settings.study.cefr.level,
            mode.max_cards,
            writer.unwrap_or_default(),
            self.study_dict_language().unwrap_or_default(),
        );
        self.study_mode
            .run_mut(pane)
            .is_some_and(|r| r.follow(recipe))
    }

    /// Starts or steps the deck for the batch at `pane`'s gate.
    pub(super) fn study_deck(&mut self, pane: PaneId, now: f64) -> bool {
        let Some(run) = self.study_mode.run(pane) else {
            return false;
        };
        let Some(batch) = run.gate_batch().cloned() else {
            return false;
        };
        if matches!(run.deck, DeckState::None) {
            return self.study_start_deck(pane, batch);
        }
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        let DeckState::Making { job, .. } = &mut run.deck else {
            return false;
        };
        match job.step(&mut self.vocab, &mut self.dict, now) {
            Ok(None) => false,
            Ok(Some(built)) => self.study_open(pane, batch, built),
            Err(why) => {
                let setup = job.setup();
                run.deck = DeckState::Failed { batch, why, setup };
                true
            }
        }
    }

    /// Takes the deck made ahead for `batch`, or starts making one.
    fn study_start_deck(&mut self, pane: PaneId, batch: Batch) -> bool {
        let made = self
            .study_mode
            .run_mut(pane)
            .and_then(|r| r.take_ahead(&batch));
        let deck = match made {
            Some(Ahead::Ready { built, .. }) => return self.study_open(pane, batch, built),
            Some(Ahead::Making { job, .. }) => DeckState::Making { batch, job },
            Some(Ahead::Failed { why, setup, .. }) => DeckState::Failed { batch, why, setup },
            None if self.settings.study.mode.ahead == 0 && !self.study_near_gate(pane) => {
                return false;
            }
            None => {
                let Some(ask) = self.study_ask(pane, &batch) else {
                    return false;
                };
                let job = Box::new(DeckJob::new(Arc::clone(&self.waker), ask));
                DeckState::Making { batch, job }
            }
        };
        if let Some(run) = self.study_mode.run_mut(pane) {
            run.deck = deck;
        }
        true
    }

    /// Opens `built` at the gate; a deck with nothing new unlocks its pages.
    fn study_open(&mut self, pane: PaneId, batch: Batch, built: Built) -> bool {
        let built = fresh(built, &self.study_known(pane));
        let m = &self.settings.study.mode;
        let per_card = m.limit.then_some(u32::from(m.card_seconds));
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        if !built.cards.is_empty() {
            run.deck = DeckState::Open(Open::new(batch, built, per_card));
            return true;
        }
        run.unlock_through(batch.to);
        self.study_save_gate(pane);
        let toast = Toast::info(mode::nothing_new(&batch.label()));
        self.emit(Event::Toast(toast));
        true
    }

    /// Makes and steps the decks past the gate's; drops those not coming.
    pub(super) fn study_ahead(&mut self, pane: PaneId, now: f64) -> bool {
        let count = usize::from(self.settings.study.mode.ahead).saturating_sub(1);
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        run.trim_ahead(count);
        let coming = run.upcoming(count).into_iter();
        let missing: Vec<Batch> = coming.filter(|b| !run.has_ahead(b)).collect();
        let mut changed = false;
        for batch in missing {
            let Some(ask) = self.study_ask(pane, &batch) else {
                continue;
            };
            let job = Box::new(DeckJob::new(Arc::clone(&self.waker), ask));
            if let Some(run) = self.study_mode.run_mut(pane) {
                run.ahead.push(Ahead::Making { batch, job });
                changed = true;
            }
        }
        let Some(run) = self.study_mode.run_mut(pane) else {
            return changed;
        };
        for ahead in &mut run.ahead {
            changed |= ahead.step(&mut self.vocab, &mut self.dict, now);
        }
        changed
    }

    /// Whether `pane` reads the page before its gate, or shows the gate.
    fn study_near_gate(&self, pane: PaneId) -> bool {
        let Some(gate) = self.study_mode.gate(pane) else {
            return false;
        };
        let place = self.documents.place(pane);
        place.is_some_and(|p| p.current.saturating_add(1) >= gate || p.visible.contains(&gate))
    }

    /// How `pane`'s pages stand in its study; none while it studies none.
    pub fn study_stands(&self, pane: PaneId) -> Stands {
        let Some(run) = self.study_mode.run(pane) else {
            return Stands::new();
        };
        run.stands(self.settings.study.mode.lock, self.study_via(pane))
    }

    /// Who can write each batch's deck in `pane`: the dictionary for its
    /// fallback batch, else as the settings and what is set up allow.
    fn study_via(&self, pane: PaneId) -> impl Fn(&Batch) -> Option<Via> {
        let fallback = self.study_mode.run(pane).and_then(|run| run.fallback);
        let source = self.settings.study.mode.source;
        let mut have = self.study_have(pane);
        // The AI writes once its writer is wired; a headless session has none.
        have.ai &= self.study_mode.writer().is_some();
        move |batch: &Batch| match fallback == Some(batch.from) && have.dict {
            true => Some(Via::Dictionary),
            false => via(source, have),
        }
    }

    /// What `batch`'s deck is made of, and who writes its backs.
    fn study_ask(&self, pane: PaneId, batch: &Batch) -> Option<Ask> {
        let study = &self.settings.study;
        let writer = self.study_mode.writer();
        // With no one to write, the dictionary's deck says what is missing.
        let via = self.study_via(pane)(batch).unwrap_or(Via::Dictionary);
        Some(Ask {
            document: self.documents.document_of(pane)?,
            pages: (batch.from..=batch.to).collect(),
            level: study.cefr.level.band(),
            max: usize::from(study.mode.max_cards),
            known: self.study_known(pane),
            to: self.study_dict_language(),
            via,
            writer: writer.filter(|_| via == Via::Ai),
        })
    }

    /// Words memorized in the library or marked known in `pane`'s run.
    pub(super) fn study_known(&self, pane: PaneId) -> Known {
        let mut known = memorized(&self.library.model);
        if let Some(run) = self.study_mode.run(pane) {
            known.extend(&run.known);
        }
        known
    }

    /// Meanings' language: study's, else Explain's, if a pack has it.
    fn study_dict_language(&self) -> Option<String> {
        let ready = self.dict.languages();
        let tasks = &self.settings.ai.tasks;
        let wanted = [&tasks.study.language, &tasks.explain.language];
        let has = |tag: &&str| ready.iter().any(|r| r == tag);
        let picked = wanted.into_iter().filter_map(Option::as_deref).find(has);
        picked.or_else(|| ready.first().copied()).map(str::to_owned)
    }
}

/// `built` without the words memorized since it was made.
fn fresh(mut built: Built, known: &Known) -> Built {
    let before = built.cards.len();
    built.cards.retain(|shown| !known.has(&shown.card.key));
    built.skipped_known += before - built.cards.len();
    built
}

/// Every living memorized word, as a deck skips it.
fn memorized(model: &Library) -> Known {
    let held = model.words().filter(|w| w.alive.value);
    let memorized = held.filter(|w| w.status.value == WordStatus::Memorized);
    Known::of(memorized.map(|w| {
        let pos = w.pos.as_deref().and_then(Pos::parse);
        Key::new(w.lemma.as_str(), pos)
    }))
}

#[cfg(test)]
mod tests {
    use mareader_study::mode::Card;

    use super::*;
    use crate::study_mode::Shown;

    #[test]
    fn words_memorized_since_a_deck_was_made_leave_it() {
        let card = |lemma: &str| {
            let card = Card {
                key: Key::new(lemma, None),
                form: lemma.to_owned(),
                sentence: String::new(),
                band: 5,
                count: 1,
                also: Vec::new(),
            };
            Shown::new(card, String::new())
        };
        let built = Built {
            cards: vec![card("arcane"), card("brisk")],
            found: 2,
            skipped_known: 1,
            via: Via::Ai,
        };
        let left = fresh(built, &Known::of([Key::new("brisk", None)]));
        assert_eq!(left.cards.len(), 1);
        assert_eq!(left.skipped_known, 2);
    }
}
