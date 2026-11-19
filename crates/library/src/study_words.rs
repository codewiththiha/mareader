//! Words learned in study mode, kept with the books they came from.

use std::hash::{BuildHasher, RandomState};

use serde::{Deserialize, Serialize};

use crate::clock::{Field, Stamp};
use crate::extra::Extra;
use crate::id::{BookId, StudyWordId};
use crate::library::Library;
use crate::marks::Spot;

/// Where a word stands with the reader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordStatus {
    #[default]
    Learning,
    Memorized,
    Missed,
}

impl WordStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Learning => "Learning",
            Self::Memorized => "Memorized",
            Self::Missed => "Missed",
        }
    }
}

/// How a word was learned: a flash card or a recall.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedBy {
    #[default]
    Card,
    Recall,
}

/// Where one book's study lock stands, as a text position.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudyProgress {
    /// The spot up to which reading is unlocked; none locks page one.
    pub unlocked_to: Option<Spot>,
    /// `off`, `proactive` or `reactive`.
    pub mode: String,
}

/// One word met in one book. Only `book`, `status` and `alive` change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StudyWord {
    pub id: StudyWordId,
    pub book: Field<BookId>,
    /// A normalised lemma.
    pub lemma: String,
    /// A coarse class: noun, verb, adjective, adverb or other.
    pub class: String,
    /// CEFR band 1 (A1) to 6 (C2) when first seen.
    pub band: u8,
    pub meaning: String,
    /// The book's own sentence.
    pub context: String,
    pub spot: Spot,
    pub by: LearnedBy,
    pub status: Field<WordStatus>,
    pub attempts: u8,
    pub first_try: bool,
    /// When first met, in wall milliseconds.
    pub added: u64,
    /// When last touched, in wall milliseconds.
    pub touched: u64,
    pub alive: Field<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl StudyWord {
    /// The `lemma|class` key decks and shading exclude by.
    pub fn key(&self) -> String {
        format!("{}|{}", self.lemma, self.class)
    }
}

/// A word to record, as study mode met it.
#[derive(Clone, Debug, PartialEq)]
pub struct NewStudyWord {
    pub lemma: String,
    pub class: String,
    pub band: u8,
    pub meaning: String,
    pub context: String,
    pub spot: Spot,
    pub by: LearnedBy,
    pub status: WordStatus,
    pub attempts: u8,
    pub first_try: bool,
}

impl Library {
    /// New whenever any study word changes.
    pub fn study_words_stamp(&self) -> u64 {
        self.study_words.stamp()
    }

    /// Living study words, oldest first; all books when `book` is none.
    pub fn study_words(&self, book: Option<BookId>) -> Vec<&StudyWord> {
        let mut out: Vec<&StudyWord> = self
            .study_words
            .values()
            .filter(|w| w.alive.value && book.is_none_or(|b| w.book.value == b))
            .collect();
        out.sort_by_key(|w| (w.added, w.id));
        out
    }

    pub fn study_word(&self, id: StudyWordId) -> Option<&StudyWord> {
        self.study_words.get(&id).filter(|w| w.alive.value)
    }

    /// The `lemma|class` keys memorized in any book.
    pub fn memorized_keys(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .study_words(None)
            .into_iter()
            .filter(|w| w.status.value == WordStatus::Memorized)
            .map(StudyWord::key)
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Records a word in `book`, updating its one record there.
    pub fn add_study_word(
        &mut self,
        book: BookId,
        new: NewStudyWord,
        wall_ms: u64,
    ) -> Option<StudyWordId> {
        self.book(book)?;
        let found = self
            .study_words(Some(book))
            .into_iter()
            .find(|w| w.lemma == new.lemma && w.class == new.class)
            .map(|w| w.id);
        let at = self.clock.tick(wall_ms);
        if let Some(id) = found {
            if let Some(word) = self.study_words.get_mut(&id) {
                word.meaning = new.meaning;
                word.context = new.context;
                word.spot = new.spot;
                word.by = new.by;
                word.attempts = new.attempts;
                word.first_try = new.first_try;
                word.touched = wall_ms;
                word.status = Field::new(new.status, at);
            }
            return Some(id);
        }
        let id = self.new_study_word_id(at);
        let word = StudyWord {
            id,
            book: Field::new(book, at),
            lemma: new.lemma,
            class: new.class,
            band: new.band.min(6),
            meaning: new.meaning,
            context: new.context,
            spot: new.spot,
            by: new.by,
            status: Field::new(new.status, at),
            attempts: new.attempts,
            first_try: new.first_try,
            added: wall_ms,
            touched: wall_ms,
            alive: Field::new(true, at),
            extra: Extra::default(),
        };
        self.study_words.insert(id, word);
        Some(id)
    }

    /// Sets one word's status; false if there's no such word.
    pub fn set_study_status(&mut self, id: StudyWordId, status: WordStatus, wall_ms: u64) -> bool {
        if self.study_word(id).is_none() {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        if let Some(word) = self.study_words.get_mut(&id) {
            word.status = Field::new(status, at);
            word.touched = wall_ms;
        }
        true
    }

    /// Removes or brings back study words; how many changed.
    pub fn set_study_words_alive(
        &mut self,
        ids: &[StudyWordId],
        alive: bool,
        wall_ms: u64,
    ) -> usize {
        let ids: Vec<StudyWordId> = ids
            .iter()
            .filter(|id| {
                self.study_words
                    .get(id)
                    .is_some_and(|w| w.alive.value != alive)
            })
            .copied()
            .collect();
        if ids.is_empty() {
            return 0;
        }
        let at = self.clock.tick(wall_ms);
        for id in &ids {
            if let Some(word) = self.study_words.get_mut(id) {
                word.alive = Field::new(alive, at);
            }
        }
        ids.len()
    }

    /// Where `book`'s study lock stands, if it was ever set.
    pub fn study_progress(&self, book: BookId) -> Option<&StudyProgress> {
        self.states.get(&book).and_then(|s| s.study.value.as_ref())
    }

    /// Keeps where `book`'s study lock stands.
    pub fn record_study_progress(
        &mut self,
        book: BookId,
        progress: StudyProgress,
        wall_ms: u64,
    ) -> bool {
        if self.book(book).is_none() {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        let position = self.states.get(&book).map(|s| s.position.clone());
        let state = match position {
            Some(position) => self
                .states
                .entry(book)
                .or_insert_with(|| crate::book::State::new(position)),
            None => self.states.entry(book).or_insert_with(|| {
                crate::book::State::new(Field::new(
                    crate::book::Position {
                        page: 0,
                        pages: 0,
                        place: None,
                        spine: None,
                    },
                    at,
                ))
            }),
        };
        state.study = Field::new(Some(progress), at);
        self.states_dirty.insert(book);
        true
    }

    /// `gone`'s study words go to `keeper`, as a merge folding decides.
    pub(crate) fn move_study_words(&mut self, gone: BookId, keeper: BookId, at: Stamp) {
        for word in self.study_words.values_mut_where(|w| w.book.value == gone) {
            word.book = Field::new(keeper, at);
        }
    }

    fn new_study_word_id(&self, at: Stamp) -> StudyWordId {
        let state = RandomState::new();
        let seed = (at.ms, at.counter, self.device().0);
        let half = |salt: u8| u128::from(state.hash_one((seed, salt)));
        let mut id = StudyWordId(half(5) << 64 | half(6));
        while self.study_words.contains_key(&id) {
            id = StudyWordId(id.0.wrapping_add(1));
        }
        id
    }
}

#[cfg(test)]
#[path = "study_words_tests.rs"]
mod tests;
