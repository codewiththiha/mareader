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
use crate::command::Command;
use crate::event::{Event, PaneAction};
use crate::library::wall_ms;
use crate::notes_panel::{bookmark_spot, page_of};
use crate::study_mode::run::Sheet;
use crate::study_mode::{self as mode, DeckState, Prior, Rest, Run, Shown, State};
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

    /// The first page `pane` may show while study mode locks the ones before.
    pub fn study_floor(&self, pane: PaneId) -> usize {
        let lock = self.settings.study.mode.lock;
        self.study_mode.run(pane).map_or(0, |run| run.floor(lock))
    }

    /// What the machine and `pane`'s book have, as study mode needs it.
    pub fn study_have(&self, pane: PaneId) -> Have {
        let english = match self.documents.document_of(pane).as_deref() {
            Some(Document::Epub { book, .. }) => book
                .language()
                .is_none_or(|tag| tag.trim().to_ascii_lowercase().starts_with("en")),
            _ => true,
        };
        let (ai, dict) = crate::study_mode::writers(&self.settings, &self.dict);
        Have {
            cefr: self.vocab.ready() || self.vocab.coming(),
            ai,
            dict,
            english,
        }
    }

    /// Turns `mode` on in `pane`; why not, when it can't run.
    pub(crate) fn study_mode_on(&mut self, pane: PaneId, wanted: Mode) -> Result<(), String> {
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
        let Some(open) = run.shown_deck_mut() else {
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
        let card = shown.card.key.clone();
        open.keep_known(at, shown, prior);
        run.known.add(card);
        true
    }

    /// Brings back the last card marked known, and its word's old state.
    pub(crate) fn study_mode_undo(&mut self, pane: PaneId) -> bool {
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        let Some(open) = run.shown_deck_mut() else {
            return false;
        };
        let Some((key, prior)) = open.undo_known() else {
            return false;
        };
        run.known.remove(&key);
        let id = [word_id(&key)];
        let model = &mut self.library.model;
        match prior {
            Prior::New => model.set_words_alive(&id, false, wall_ms()),
            Prior::Was(status) => model.set_word_status(&id, status, wall_ms()),
            Prior::Unsaved => 0,
        };
        true
    }

    /// Reads now: ends the open deck, or closes one opened again.
    pub(crate) fn study_mode_start(&mut self, pane: PaneId, now: f64) -> Result<(), String> {
        let Some(run) = self.study_mode.run_mut(pane) else {
            return Err("Study mode is off here.".to_owned());
        };
        if run.close_review() || self.study_next_part(pane, now) {
            return Ok(());
        }
        match self.study_finish(pane, now) {
            Some(_) => Ok(()),
            None => Err("No deck is open.".to_owned()),
        }
    }

    /// Ends the gate's deck: its cards are kept as learning and its pages
    /// unlock. With no time limit the deck is kept, to open again.
    fn study_finish(&mut self, pane: PaneId, now: f64) -> Option<Batch> {
        let run = self.study_mode.run_mut(pane)?;
        if !matches!(run.deck, DeckState::Open(_)) {
            return None;
        }
        let DeckState::Open(mut open) = std::mem::replace(&mut run.deck, DeckState::None) else {
            return None;
        };
        open.allowance.pause(now);
        open.away = false;
        open.gather();
        open.rest = Rest::Done;
        let batch = open.batch.clone();
        if let Some(book) = run.book {
            for shown in &open.cards {
                record(&mut self.library.model, book, shown, WordStatus::Learning);
            }
        }
        if !open.allowance.limited() {
            run.kept.push(open);
        }
        run.unlock_through(batch.to);
        self.study_save_gate(pane);
        self.study_show(pane, batch.from, now);
        Some(batch)
    }

    /// The deck came into sight or left it; one seen and left waits to be
    /// asked back, and one opened again goes back with those kept.
    pub(crate) fn study_mode_shown(&mut self, pane: PaneId, shown: bool, now: f64) -> bool {
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        if !shown && run.close_review() {
            return true;
        }
        let Some(open) = run.shown_deck_mut() else {
            return false;
        };
        match shown {
            true if open.away => return false,
            true => open.allowance.resume(now),
            false => {
                open.away |= open.allowance.running();
                open.allowance.pause(now);
            }
        }
        true
    }

    /// Shows the cards left unfinished, or those kept for the pages read.
    pub(crate) fn study_mode_reopen(&mut self, pane: PaneId) -> bool {
        self.study_mode.run_mut(pane).is_some_and(Run::reopen)
    }

    /// Leaves the deck for the page before it, keeping its time; that page.
    pub(crate) fn study_mode_leave(&mut self, pane: PaneId, now: f64) -> Option<usize> {
        let lock = self.settings.study.mode.lock;
        let run = self.study_mode.run_mut(pane)?;
        if let Some(open) = &run.review {
            let page = open.batch.from.checked_sub(1)?;
            run.close_review();
            return Some(page);
        }
        let page = run.back(lock)?;
        if let DeckState::Open(open) = &mut run.deck {
            open.allowance.pause(now);
            open.away = true;
        }
        Some(page)
    }

    /// Closes the gate's deck for good once its time is spent.
    fn study_expire(&mut self, pane: PaneId, now: f64) -> bool {
        let deck = self.study_mode.run(pane).map(|r| &r.deck);
        let spent = matches!(deck, Some(DeckState::Open(o)) if o.allowance.spent(now));
        if !spent || self.study_next_part(pane, now) {
            return spent;
        }
        if let Some(batch) = self.study_finish(pane, now) {
            self.emit(Event::Toast(Toast::info(mode::time_up(&batch.label()))));
        }
        true
    }

    /// Offers the cards again as the reader comes back into their pages.
    fn study_arrive(&mut self, pane: PaneId, now: f64) -> bool {
        let Some(current) = self.documents.place(pane).map(|p| p.current) else {
            return false;
        };
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        let Some(batch) = run.arrived(current) else {
            return false;
        };
        let left = match &run.deck {
            DeckState::Open(open) if open.batch == batch => open.allowance.left(now),
            _ => None,
        };
        let open = Command::StudyMode(mode::Action::Reopen(pane));
        let said = mode::ask_back(&batch.label(), left);
        let toast = Toast::info(said).action(mode::OPEN_CARDS, open);
        self.emit(Event::Toast(toast));
        true
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
        let mut changed = self.tick_study_recall();
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
            if self
                .study_mode
                .run(pane)
                .is_some_and(|r| r.mode == Mode::Reactive)
            {
                return false;
            }
            let planned = self.study_plan(pane) | self.study_count(pane);
            let followed = self.study_follow(pane);
            let decks = self.study_deck(pane, now) | self.study_ahead(pane, now);
            let timed = self.study_expire(pane, now) | self.study_arrive(pane, now);
            return planned | followed | decks | timed | self.study_hold(pane);
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
        let (wake, inline, size) = (Arc::clone(&self.waker), self.inline, self.study_size());
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        let sheet = Sheet {
            document: &document,
            wake,
            inline,
            size,
            saved: spot.and_then(|s| page_of(&document, s)),
            start: run.anchor.and_then(|s| page_of(&document, s)),
            current,
        };
        match run.plan(sheet) {
            Ok(false) => false,
            Ok(true) => {
                if run.anchor.is_none() {
                    run.anchor = Some(bookmark_spot(&document, run.start));
                }
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

    /// Counts a round of `pane`'s pages' hard words in words mode.
    fn study_count(&mut self, pane: PaneId) -> bool {
        let size = self.study_size();
        let run = self.study_mode.run(pane);
        let wants = run.is_some_and(|r| r.wants_count(size));
        if !wants {
            return false;
        }
        let known = self.study_known(pane);
        let level = self.settings.study.cefr.level.band();
        let Some(run) = self.study_mode.run_mut(pane) else {
            return false;
        };
        run.count(size, &mut self.vocab, &known, level)
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
    pub(super) fn study_save_gate(&mut self, pane: PaneId) {
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
            words: usize::from(mode.max_cards),
        }
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
        meaning_by: shown.by,
        synonyms: shown.synonyms.clone(),
        usages: if shown.usage.is_empty() {
            Vec::new()
        } else {
            vec![shown.usage.clone()]
        },
        status,
        ..NewWord::default()
    };
    model.record_word(book, word, wall_ms());
}
