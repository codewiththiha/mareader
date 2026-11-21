//! Reactive study mode's recall in the session: a shaded word opened, its
//! answers judged and its outcome saved.
//!
//! A recall opens only in a pane studying in Reactive mode. Its word's lemma
//! and band come from the levels the page was shaded with. A finished recall
//! saves the word with how many tries it took, met by Reactive; a document
//! outside the library keeps nothing.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use mareader_library::{BookId, LearnedBy, Library, NewWord, WordStatus};
use mareader_study::mode::{Done, Judge, Key, Mode, Outcome, lemma};
use mareader_workspace::PaneId;

use super::Session;
use crate::event::Event;
use crate::library::wall_ms;
use crate::study::words::asked_at;
use crate::study_mode::sightings::Read;
use crate::study_mode::{self as mode, Ask, Recalling, Word};
use crate::toast::Toast;

impl Session {
    /// Lets Reactive mode's AI judge recalls with `judge`.
    pub fn study_set_judge(&mut self, judge: Arc<dyn Judge>) {
        self.study_mode.set_judge(judge);
    }

    /// Does what the recall popover asks; whether anything changed.
    pub(crate) fn study_recall(&mut self, pane: PaneId, ask: Ask) -> Result<bool, String> {
        match ask {
            Ask::Open { page, chars } => return self.study_recall_open(pane, page, chars),
            Ask::Finish(outcome) => return self.study_recall_finish(pane, outcome),
            Ask::Close => return Ok(self.study_recall_close(pane)),
            Ask::Answer(_) | Ask::Again | Ask::Reveal => {}
        }
        let judge = self.study_mode.judge();
        let Some(recalling) = self.study_mode.recalling_mut(pane) else {
            return Ok(false);
        };
        let moved = match ask {
            Ask::Answer(text) => recalling.send(&text, judge),
            Ask::Again => recalling.recall.again(),
            Ask::Reveal => {
                let was = recalling.recall.phase;
                recalling.recall.reveal();
                recalling.recall.phase != was
            }
            Ask::Open { .. } | Ask::Finish(_) | Ask::Close => false,
        };
        Ok(moved)
    }

    /// Opens a recall on the word at `chars` of `page`.
    fn study_recall_open(
        &mut self,
        pane: PaneId,
        page: usize,
        chars: Range<usize>,
    ) -> Result<bool, String> {
        let reactive = self.study_mode.run(pane).is_some_and(|r| r.mode == Mode::Reactive);
        if !reactive {
            return Err(mode::REACTIVE_ONLY.to_owned());
        }
        let layer = self.documents.layer(pane, page).ok_or("That page has no text.")?;
        let asked = asked_at(&layer, page, chars).ok_or("No word is there.")?;
        let word = self.study_word(asked.word, asked.context, page, asked.chars);
        let by = self.study_mode.judge().map(|j| j.by()).unwrap_or_default();
        let writer = self.study_mode.writer();
        self.study_mode.open_recall(Recalling::new(pane, word, writer, by));
        Ok(true)
    }

    /// The word printed as `form`, keyed and banded by the levels.
    fn study_word(
        &mut self,
        form: String,
        sentence: String,
        page: usize,
        chars: Range<usize>,
    ) -> Word {
        let read = Read {
            norm: mareader_cefr::normalize(&form),
            form,
            sentence,
        };
        let keys = read.keys();
        let found: HashMap<String, u8> = keys
            .iter()
            .cloned()
            .zip(self.vocab.bands(&keys))
            .filter_map(|(key, band)| Some((key, band?)))
            .collect();
        let lemma = lemma(&read.norm, |k: &str| found.get(k).is_some_and(|b| *b > 0));
        let own = found.get(&lemma).copied().filter(|b| *b > 0);
        let band = own.or_else(|| found.values().copied().max()).unwrap_or(0);
        Word {
            key: Key::new(lemma, None),
            form: read.form,
            sentence: read.sentence,
            band,
            page,
            chars,
        }
    }

    /// Ends the recall with `outcome`, saving the word, when it may end so.
    fn study_recall_finish(&mut self, pane: PaneId, outcome: Outcome) -> Result<bool, String> {
        let Some(recalling) = self.study_mode.recalling(pane) else {
            return Ok(false);
        };
        let done = recalling.ending(outcome).ok_or("The recall can't end that way now.")?;
        self.study_recall_save(pane, done);
        Ok(true)
    }

    /// Closes the recall; a revealed word is saved as missed first.
    fn study_recall_close(&mut self, pane: PaneId) -> bool {
        let Some(recalling) = self.study_mode.recalling(pane) else {
            return false;
        };
        match recalling.ending(Outcome::Missed) {
            Some(done) => self.study_recall_save(pane, done),
            None => drop(self.study_mode.take_recall()),
        }
        true
    }

    /// Saves `done` for the open recall's word and closes it.
    fn study_recall_save(&mut self, pane: PaneId, done: Done) {
        let Some(recalling) = self.study_mode.take_recall() else {
            return;
        };
        let word = &recalling.word;
        let run = self.study_mode.run_mut(pane);
        let book = run.and_then(|r| {
            r.recalled.push((word.page, word.chars.clone(), done.outcome));
            r.book
        });
        if let Some(book) = book {
            keep(&mut self.library.model, book, &recalling, done);
        }
        if done.outcome == Outcome::Memorized {
            let said = format!("“{}” added to memorized.", recalling.word.key.lemma);
            self.emit(Event::Toast(Toast::info(said)));
        }
    }

    /// Takes in the open recall's back and verdict; whether anything moved.
    pub(super) fn tick_study_recall(&mut self) -> bool {
        let ask = self.settings.study.mode.ask_memorize;
        let writer = self.study_mode.writer();
        let Some(pane) = self.study_mode.recalling_pane() else {
            return false;
        };
        let Some(recalling) = self.study_mode.recalling_mut(pane) else {
            return false;
        };
        let (moved, done) = recalling.step(ask, writer.as_ref());
        if let Some(done) = done {
            self.study_recall_save(pane, done);
        }
        moved
    }
}

/// Keeps a recalled word and where `book` showed it, with its tries.
fn keep(model: &mut Library, book: BookId, recalling: &Recalling, done: Done) {
    let word = &recalling.word;
    let status = match done.outcome {
        Outcome::Learning => WordStatus::Learning,
        Outcome::Memorized => WordStatus::Memorized,
        Outcome::Missed => WordStatus::Missed,
    };
    let meaning = recalling.back.as_ref().map(|b| b.meaning.clone());
    let new = NewWord {
        lemma: word.key.lemma.clone(),
        pos: word.key.pos.map(|p| p.name().to_owned()),
        band: word.band,
        form: word.form.clone(),
        context: word.sentence.clone(),
        meaning: meaning.unwrap_or_default(),
        by: LearnedBy::Reactive,
        status,
        tries: done.attempts,
        first_try: done.first_try,
    };
    model.record_word(book, new, wall_ms());
}
