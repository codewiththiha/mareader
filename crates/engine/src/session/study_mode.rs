//! Study mode in the session: each pane's plan, its deck and its gate.
//!
//! A frame plans every run on its pane's layout, makes the deck at the gate
//! and saves the gate where the book keeps it. A run whose document went, or
//! whose needs are unmet, turns itself off with a toast.

use std::sync::Arc;

use mareader_library::{BookId, Library, NewWord, WordId, WordStatus};
use mareader_session::Document;
use mareader_study::mode::{Batch, Have, Key, Mode, Pos, Size, check};
use mareader_workspace::PaneId;

use super::Session;
use crate::event::{Event, PaneAction};
use crate::library::wall_ms;
use crate::notes_panel::{bookmark_spot, page_of};
use crate::study_mode::deck_job::Ask;
use crate::study_mode::run::Sheet;
use crate::study_mode::{self as mode, DeckJob, DeckState, Known, Open, Prior, Run, Shown, State};
use crate::toast::Toast;

impl Session {
    /// Every pane's study mode, read-only.
    pub fn study_mode(&self) -> &State {
        &self.study_mode
    }

    /// The first page `pane` may not pass while study mode holds it.
    pub fn study_gate(&self, pane: PaneId) -> Option<usize> {
        self.study_mode.gate(pane)
    }

    /// What the machine and `pane`'s book have, as study mode needs it.
    pub fn study_have(&self, pane: PaneId) -> Have {
        let english = match self.documents.document_of(pane).as_deref() {
            Some(Document::Epub { book, .. }) => book
                .language()
                .is_none_or(|tag| tag.trim().to_ascii_lowercase().starts_with("en")),
            _ => true,
        };
        Have {
            cefr: self.vocab.ready() || self.vocab.coming(),
            ai: self.settings.ai.study_preset().is_some(),
            dict: !self.dict.languages().is_empty(),
            english,
        }
    }

    /// Turns `mode` on in `pane`; why not, when it can't run.
    pub(crate) fn study_mode_on(&mut self, pane: PaneId, wanted: Mode) -> Result<(), String> {
        if wanted == Mode::Reactive {
            return Err(mode::REACTIVE_LATER.to_owned());
        }
        let Some(place) = self.documents.place(pane) else {
            return Err("Open a book to study it.".to_owned());
        };
        let (doc, current) = (place.doc, place.current);
        check(wanted, self.study_have(pane)).map_err(|m| m.reason().to_owned())?;
        let book = self.study_book(pane);
        self.page_ai_stop_follow(pane);
        let run = Run::new(wanted, book, current);
        self.study_mode.start(pane, doc, run);
        Ok(())
    }

    pub(crate) fn study_mode_off(&mut self, pane: PaneId) -> bool {
        self.study_mode.stop(pane)
    }

    /// Saves `key` as memorized; its card leaves the deck.
    pub(crate) fn study_mode_known(&mut self, pane: PaneId, key: &Key) -> bool {
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        let book = run.book;
        let DeckState::Open(open) = &mut run.deck else {
            return false;
        };
        let Some((at, shown)) = open.take_known(key) else {
            return false;
        };
        let model = &mut self.library.model;
        let prior = match book {
            Some(book) => {
                let prior = prior_of(model, key);
                record(model, book, &shown, WordStatus::Memorized);
                prior
            }
            None => Prior::Unsaved,
        };
        open.keep_known(at, shown, prior);
        true
    }

    /// Brings back the last card marked known, and its word's old state.
    pub(crate) fn study_mode_undo(&mut self, pane: PaneId) -> bool {
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        let DeckState::Open(open) = &mut run.deck else {
            return false;
        };
        let Some((key, prior)) = open.undo_known() else {
            return false;
        };
        let id = [word_id(&key)];
        let model = &mut self.library.model;
        match prior {
            Prior::New => model.set_words_alive(&id, false, wall_ms()),
            Prior::Was(status) => model.set_word_status(&id, status, wall_ms()),
            Prior::Unsaved => 0,
        };
        true
    }

    /// Ends the open deck: its cards are kept as learning, its pages unlock.
    ///
    /// Refused while a strict time floor still runs.
    pub(crate) fn study_mode_start(&mut self, pane: PaneId, now: f64) -> Result<(), String> {
        let strict = self.settings.study.mode.strict;
        let Some(run) = self.study_mode.run_mut(pane) else {
            return Err("Study mode is off here.".to_owned());
        };
        let book = run.book;
        let DeckState::Open(open) = &run.deck else {
            return Err("No deck is open.".to_owned());
        };
        if open.floor.locked(strict, now) {
            return Err("Study the cards a little longer.".to_owned());
        }
        let batch = open.batch.clone();
        if let Some(book) = book {
            for shown in &open.cards {
                record(&mut self.library.model, book, shown, WordStatus::Learning);
            }
        }
        run.unlock_through(batch.to);
        self.study_save_gate(pane);
        self.study_show(pane, batch.from, now);
        Ok(())
    }

    /// Asks again for the deck that failed at `pane`'s gate.
    pub(crate) fn study_mode_retry(&mut self, pane: PaneId) -> bool {
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        let failed = matches!(run.deck, DeckState::Failed { .. });
        if failed {
            run.deck = DeckState::None;
        }
        failed
    }

    /// Moves `pane` to `page` and tells the front end, as a jump does.
    pub(crate) fn study_show(&mut self, pane: PaneId, page: usize, now: f64) {
        if let Some(landed) = self.documents.go_to(pane, page) {
            let action = PaneAction::GoTo(landed);
            self.emit(Event::PaneView { pane, action, now });
        }
    }

    /// Plans, makes decks and holds the gate; true when a frame should follow.
    pub(super) fn tick_study_mode(&mut self, now: f64) -> bool {
        if self.study_mode.is_empty() {
            return false;
        }
        let mut changed = false;
        for pane in self.study_mode.panes() {
            changed |= self.tick_study_pane(pane, now);
        }
        changed || self.study_mode.busy()
    }

    fn tick_study_pane(&mut self, pane: PaneId, now: f64) -> bool {
        if !self.tree.panes().contains(&pane) {
            return self.study_mode.stop(pane);
        }
        let Some(why) = self.study_lost(pane) else {
            return self.study_plan(pane) | self.study_deck(pane, now) | self.study_hold(pane);
        };
        self.study_mode.stop(pane);
        if !why.is_empty() {
            self.emit(Event::Toast(Toast::warning(why)));
        }
        true
    }

    /// Why `pane`'s run must end: empty for a quiet end, none to go on.
    fn study_lost(&self, pane: PaneId) -> Option<String> {
        let doc = self.study_mode.doc(pane)?;
        let running = self.study_mode.run(pane)?.mode;
        match self.documents.place(pane) {
            Some(place) if place.doc == doc => {}
            Some(_) => return Some(String::new()),
            None => return Some(mode::AUTO_OFF.to_owned()),
        }
        check(running, self.study_have(pane))
            .err()
            .map(|_| mode::AUTO_OFF.to_owned())
    }

    /// Plans the batches on the current layout; saves the gate it lands on.
    fn study_plan(&mut self, pane: PaneId) -> bool {
        let Some(doc) = self.study_mode.doc(pane) else {
            return false;
        };
        let Some(document) = self.documents.document_of(pane) else {
            return false;
        };
        let current = self.documents.place(pane).map_or(0, |p| p.current);
        let book = self.study_run_book(pane);
        let spot = match book {
            Some(book) => self.library.model.unlocked(book).cloned(),
            None => self.study_mode.loose(doc).cloned(),
        };
        let sheet = Sheet {
            document: &document,
            wake: Arc::clone(&self.waker),
            inline: self.inline,
            size: self.study_size(),
            saved: spot.and_then(|s| page_of(&document, s)),
            current,
        };
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        match run.plan(sheet) {
            Ok(false) => false,
            Ok(true) => {
                self.study_save_gate(pane);
                true
            }
            Err(why) => {
                self.study_mode.stop(pane);
                self.emit(Event::Toast(Toast::error(why)));
                true
            }
        }
    }

    /// Starts or steps the deck for the batch at `pane`'s gate.
    fn study_deck(&mut self, pane: PaneId, now: f64) -> bool {
        let Some(run) = self.study_mode.run(pane) else {
            return false;
        };
        let Some(batch) = run.gate_batch().cloned() else {
            return false;
        };
        if matches!(run.deck, DeckState::None) {
            let Some(ask) = self.study_ask(pane, &batch) else {
                return false;
            };
            let job = DeckJob::new(Arc::clone(&self.waker), ask);
            if let Some(run) = self.study_mode.run_mut(pane) {
                run.deck = DeckState::Making { batch, job };
            }
            return true;
        }
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        let DeckState::Making { job, .. } = &mut run.deck else {
            return false;
        };
        match job.step(&mut self.vocab, &mut self.dict, now) {
            Ok(None) => false,
            Ok(Some(built)) if built.cards.is_empty() => {
                run.unlock_through(batch.to);
                self.study_save_gate(pane);
                let toast = Toast::info(mode::nothing_new(&batch.label()));
                self.emit(Event::Toast(toast));
                true
            }
            Ok(Some(built)) => {
                let per_card = u32::from(self.settings.study.mode.sec_per_card);
                run.deck = DeckState::Open(Open::new(batch, built, per_card, now));
                true
            }
            Err(why) => {
                run.deck = DeckState::Failed { batch, why };
                true
            }
        }
    }

    /// What `batch`'s deck is made of.
    fn study_ask(&self, pane: PaneId, batch: &Batch) -> Option<Ask> {
        let study = &self.settings.study;
        Some(Ask {
            document: self.documents.document_of(pane)?,
            pages: (batch.from..=batch.to).collect(),
            level: study.cefr.level.band(),
            max: usize::from(study.mode.max_cards),
            known: memorized(&self.library.model),
            to: self.study_dict_language(),
        })
    }

    /// Stops read-aloud that reaches `pane`'s gate.
    fn study_hold(&mut self, pane: PaneId) -> bool {
        let Some(gate) = self.study_mode.gate(pane) else {
            return false;
        };
        let Some((reading, page, _)) = self.read_aloud.spot() else {
            return false;
        };
        if reading != pane || page < gate {
            return false;
        }
        self.read_aloud_stop();
        self.emit(Event::Toast(Toast::info(mode::READ_ALOUD_STOP)));
        true
    }

    /// Saves `pane`'s gate with its book; other panes on it plan again.
    fn study_save_gate(&mut self, pane: PaneId) {
        let Some(doc) = self.study_mode.doc(pane) else {
            return;
        };
        let Some(document) = self.documents.document_of(pane) else {
            return;
        };
        let Some((unlocked, book)) = self.study_mode.run(pane).map(|r| (r.unlocked, r.book)) else {
            return;
        };
        let spot = bookmark_spot(&document, unlocked);
        let moved = match book {
            Some(book) => self.library.model.set_unlocked(book, Some(spot), wall_ms()),
            None if self.study_mode.loose(doc) == Some(&spot) => false,
            None => {
                self.study_mode.set_loose(doc, spot);
                true
            }
        };
        if moved {
            self.study_mode.restale_others(pane);
        }
    }

    /// The batch size the settings ask for.
    fn study_size(&self) -> Size {
        let mode = &self.settings.study.mode;
        Size {
            unit: mode.unit,
            pages: usize::from(mode.pages),
            chars: usize::try_from(mode.chars).unwrap_or(usize::MAX),
        }
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

    /// `pane`'s run's book, filled in once the library has measured the file.
    fn study_run_book(&mut self, pane: PaneId) -> Option<BookId> {
        if let Some(book) = self.study_mode.run(pane)?.book {
            return Some(book);
        }
        let book = self.study_book(pane)?;
        if let Some(run) = self.study_mode.run_mut(pane) {
            run.book = Some(book);
        }
        Some(book)
    }

    /// The library book `pane` reads, by its id or its file.
    fn study_book(&self, pane: PaneId) -> Option<BookId> {
        if let Some(book) = self.documents.book(pane) {
            return Some(book);
        }
        let path = self.documents.path(pane)?;
        self.library.model.book_at(path)
    }
}

fn word_id(key: &Key) -> WordId {
    WordId::of(&key.lemma, key.pos.map(Pos::name))
}

/// What `key`'s word was before a card saves it.
fn prior_of(model: &Library, key: &Key) -> Prior {
    match model.word(word_id(key)) {
        Some(word) if word.alive.value => Prior::Was(word.status.value),
        _ => Prior::New,
    }
}

/// Keeps a card's word and where `book` showed it.
fn record(model: &mut Library, book: BookId, shown: &Shown, status: WordStatus) {
    let card = &shown.card;
    let word = NewWord {
        lemma: card.key.lemma.clone(),
        pos: card.key.pos.map(|p| p.name().to_owned()),
        band: card.band,
        form: card.form.clone(),
        context: card.sentence.clone(),
        meaning: shown.meaning.clone(),
        status,
        ..NewWord::default()
    };
    model.record_word(book, word, wall_ms());
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
