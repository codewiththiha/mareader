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

use mareader_library::{BookId, LearnedBy, Library, MeaningBy, NewWord, WordId, WordStatus};
use mareader_session::TextLayer;
use mareader_study::mode::{Done, Judge, Key, Mode, Outcome, Pos};
use mareader_workspace::PaneId;

use super::Session;
use crate::event::Event;
use crate::library::wall_ms;
use crate::study::words::{asked_at, read};
use crate::study_mode::backs::said;
use crate::study_mode::deck_job::Tags;
use crate::study_mode::sightings::{Read, sentence_of};
use crate::study_mode::{self as mode, Ask, Recalling, Word};
use crate::toast::Toast;
use crate::vocab::Vocab;

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
        let reactive = self
            .study_mode
            .run(pane)
            .is_some_and(|r| r.mode == Mode::Reactive);
        if !reactive {
            return Err(mode::REACTIVE_ONLY.to_owned());
        }
        let layer = self
            .documents
            .layer(pane, page)
            .ok_or("That page has no text.")?;
        let asked = asked_at(&layer, page, chars).ok_or("No word is there.")?;
        let around = sentence_around(&layer, &asked.chars);
        let sentence = around.unwrap_or(asked.context);
        let word = self.study_word(asked.word, sentence, page, asked.chars);
        let by = self.study_mode.judge().map(|j| j.by()).unwrap_or_default();
        let writer = self.study_mode.writer();
        let tag = self.vocab.ask_tag(&word.form, &word.sentence);
        let mut recalling = Recalling::new(pane, word, writer, by);
        recalling.asks.0 = tag;
        look_up(&mut recalling, &mut self.dict);
        self.study_mode.open_recall(recalling);
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
        let band = read.band(&found);
        Word {
            key: Key::new(read.lemma(&found, band), None),
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
        let done = recalling
            .ending(outcome)
            .ok_or("The recall can't end that way now.")?;
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
            None => {
                let book = self.study_mode.run(pane).and_then(|run| run.book);
                if let (Some(book), Some(recalling)) = (book, self.study_mode.take_recall())
                    && !recalling.answers.is_empty()
                {
                    let first = recalling.recall.attempts == 1
                        && recalling.recall.verdict == Some(mareader_study::mode::Verdict::Correct);
                    let done = Done {
                        outcome: Outcome::Learning,
                        attempts: recalling.recall.attempts,
                        first_try: first,
                    };
                    keep(&mut self.library.model, book, &recalling, done, true);
                }
            }
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
            r.recalled
                .push((word.page, word.chars.clone(), done.outcome));
            r.book
        });
        if let Some(book) = book {
            keep(&mut self.library.model, book, &recalling, done, false);
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
        let heard = hear(recalling, &mut self.vocab, &mut self.dict);
        let (moved, done) = recalling.step(ask, writer.as_ref());
        look_up(recalling, &mut self.dict);
        if let Some(done) = done {
            self.study_recall_save(pane, done);
        }
        moved || heard
    }
}

/// Asks the dictionary the word's class once no other source named one.
fn look_up(recalling: &mut Recalling, dict: &mut crate::dict::Service) {
    if recalling.wants_dictionary() {
        recalling.asks.1 = dict.ask_class(&recalling.word.key.lemma);
        recalling.looked_up = true;
    }
}

/// Takes the tagger's and the dictionary's classes in, as they come.
fn hear(recalling: &mut Recalling, vocab: &mut Vocab, dict: &mut crate::dict::Service) -> bool {
    let mut heard = false;
    if let Some(tag) = recalling.asks.0.and_then(|t| vocab.take_tag(t)) {
        recalling.heard.tagger = tag.as_deref().and_then(Pos::of_tag);
        recalling.asks.0 = None;
        heard = true;
    }
    if let Some(class) = recalling.asks.1.and_then(|t| dict.take_class(t)) {
        recalling.heard.dictionary = class;
        recalling.asks.1 = None;
        heard = true;
    }
    heard
}

/// Keeps a recalled word and where `book` showed it, with its tries.
fn keep(model: &mut Library, book: BookId, recalling: &Recalling, done: Done, preserve: bool) {
    let word = &recalling.word;
    let status = match done.outcome {
        Outcome::Learning => WordStatus::Learning,
        Outcome::Memorized => WordStatus::Memorized,
        Outcome::Missed => WordStatus::Missed,
    };
    let class = recalling.class().map(|p| p.name().to_owned());
    let id = WordId::of(&word.key.lemma, class.as_deref());
    let status = if preserve {
        model
            .word(id)
            .map_or(status, |existing| existing.status.value)
    } else {
        status
    };
    let meaning = recalling.back.as_ref().map(|b| b.meaning.clone());
    let new = NewWord {
        lemma: word.key.lemma.clone(),
        pos: class,
        band: word.band,
        form: word.form.clone(),
        context: word.sentence.clone(),
        meaning_by: meaning.as_deref().and_then(|m| said(m, MeaningBy::Ai)),
        meaning: meaning.unwrap_or_default(),
        by: LearnedBy::Reactive,
        status,
        tries: done.attempts,
        first_try: done.first_try,
        answers: recalling.answers.clone(),
        synonyms: recalling
            .back
            .as_ref()
            .map_or_else(Vec::new, |b| b.synonyms.clone()),
        usages: recalling.back.as_ref().map_or_else(Vec::new, |b| {
            if b.usage.is_empty() {
                Vec::new()
            } else {
                vec![b.usage.clone()]
            }
        }),
    };
    model.record_word(book, new, wall_ms());
}

/// The whole sentence around page characters `chars`, as a card shows it.
fn sentence_around(layer: &TextLayer, chars: &Range<usize>) -> Option<String> {
    let (text, index) = read(layer);
    let at = index.iter().position(|i| *i == Some(chars.start))?;
    let full: String = text.iter().collect();
    Some(sentence_of(&full, at))
}
