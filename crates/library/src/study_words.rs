//! Words studied: where each stands for the reader, and where it was met.

mod history;

pub use history::{RecallAnswer, RecallHistory, RecallSession};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::book::{Position, State};
use crate::clock::{Field, NEVER, Stamp};
use crate::extra::Extra;
use crate::id::{BookId, SightingId, WordId};
use crate::library::Library;
use crate::marks::Spot;
use crate::merge::{Merge, newest};

/// Where a word stands; memorized holds across every book.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordStatus {
    #[default]
    Learning,
    Memorized,
    Missed,
}

/// Which study mode met the word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedBy {
    /// Its card came before the reading.
    #[default]
    Proactive,
    /// The reader was asked it after reading.
    Reactive,
}

/// Who wrote a sighting's meaning; rows from before it knew are unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeaningBy {
    /// A downloaded dictionary pack.
    Dictionary,
    /// An AI model, now or kept earlier in the AI dictionary.
    Ai,
}

/// One word for the reader, by lemma and class, in every book.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WordState {
    pub id: WordId,
    pub lemma: String,
    /// Its class, such as `noun`; none when the tagger gave none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<String>,
    /// Its CEFR band, 1 (A1) to 6 (C2).
    pub band: u8,
    pub status: Field<WordStatus>,
    /// When it was first studied, in wall milliseconds.
    pub added: u64,
    pub alive: Field<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A word met in one book: its sentence, meaning and tries there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WordSighting {
    pub id: SightingId,
    pub book: Field<BookId>,
    pub word: WordId,
    pub by: Field<LearnedBy>,
    /// The word as the book spells it.
    pub form: Field<String>,
    /// The book's sentence around it.
    pub context: Field<String>,
    pub meaning: Field<String>,
    #[serde(default, skip_serializing_if = "Field::is_unset")]
    pub meaning_by: Field<Option<MeaningBy>>,
    /// Answers given the last time it was asked; 0 for a card.
    pub tries: Field<u8>,
    pub first_try: Field<bool>,
    #[serde(default, skip_serializing_if = "RecallHistory::is_empty")]
    pub recall_history: RecallHistory,
    #[serde(default)]
    pub synonyms: Field<Vec<String>>,
    #[serde(default)]
    pub usages: Field<Vec<String>>,
    pub added: u64,
    /// When it was last met, in wall milliseconds.
    pub seen: Field<u64>,
    pub alive: Field<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A word studied in a book, to keep.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewWord {
    pub lemma: String,
    pub pos: Option<String>,
    pub band: u8,
    pub form: String,
    pub context: String,
    pub meaning: String,
    pub meaning_by: Option<MeaningBy>,
    pub by: LearnedBy,
    pub status: WordStatus,
    pub tries: u8,
    pub first_try: bool,
    pub answers: Vec<RecallAnswer>,
    pub synonyms: Vec<String>,
    pub usages: Vec<String>,
}

impl WordId {
    /// The same lemma and class give the same id on every machine.
    pub fn of(lemma: &str, pos: Option<&str>) -> Self {
        let lemma = lemma.to_lowercase();
        let pos = pos.unwrap_or("");
        let parts: [&[u8]; 3] = [lemma.as_bytes(), b"|", pos.as_bytes()];
        Self(digest(&parts))
    }
}

impl SightingId {
    pub fn of(book: BookId, word: WordId) -> Self {
        let (book, word) = (book.0.to_le_bytes(), word.0.to_le_bytes());
        let parts: [&[u8]; 2] = [&book, &word];
        Self(digest(&parts))
    }
}

/// The first 16 bytes of the SHA-256 of `parts`.
fn digest(parts: &[&[u8]]) -> u128 {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    let sum = hasher.finalize();
    let mut head = [0u8; 16];
    for (to, from) in head.iter_mut().zip(sum.iter()) {
        *to = *from;
    }
    u128::from_le_bytes(head)
}

/// Writes `value` only when it differs, so a save stays small.
fn put<T: PartialEq>(field: &mut Field<T>, value: T, at: Stamp) {
    if field.value != value {
        *field = Field::new(value, at);
    }
}

impl Library {
    /// New whenever a word or sighting changes; caches key on it.
    pub fn words_stamp(&self) -> u64 {
        self.words.stamp().max(self.sightings.stamp())
    }

    /// The living words, in id order.
    pub fn words(&self) -> impl Iterator<Item = &WordState> {
        self.words.values().filter(|w| w.alive.value)
    }

    pub fn word(&self, id: WordId) -> Option<&WordState> {
        self.words.get(&id).filter(|w| w.alive.value)
    }

    /// Whether the reader memorized it, in any book.
    pub fn is_memorized(&self, lemma: &str, pos: Option<&str>) -> bool {
        let word = self.word(WordId::of(lemma, pos));
        word.is_some_and(|w| w.status.value == WordStatus::Memorized)
    }

    /// `book`'s living sightings, oldest first.
    pub fn sightings(&self, book: BookId) -> Vec<&WordSighting> {
        let of = |s: &&WordSighting| s.alive.value && s.book.value == book;
        let mut out: Vec<&WordSighting> = self.sightings.values().filter(of).collect();
        out.sort_by_key(|s| (s.added, s.id));
        out
    }

    /// Where `word` was met, in books still here, oldest first.
    pub fn sightings_of(&self, word: WordId) -> Vec<&WordSighting> {
        let of = |s: &&WordSighting| s.alive.value && s.word == word;
        let here = |s: &&WordSighting| self.book(s.book.value).is_some();
        let found = self.sightings.values().filter(of).filter(here);
        let mut out: Vec<&WordSighting> = found.collect();
        out.sort_by_key(|s| (s.added, s.id));
        out
    }

    /// Every living sighting of a living word, in books still here.
    pub fn every_sighting(&self) -> impl Iterator<Item = &WordSighting> {
        let here = |s: &&WordSighting| s.alive.value && self.book(s.book.value).is_some();
        let known = |s: &&WordSighting| self.word(s.word).is_some();
        self.sightings.values().filter(here).filter(known)
    }

    /// Keeps a word studied in `book`; a card never lowers Memorized.
    pub fn record_word(&mut self, book: BookId, new: NewWord, wall_ms: u64) -> Option<WordId> {
        self.book(book)?;
        let id = WordId::of(&new.lemma, new.pos.as_deref());
        let at = self.clock.tick(wall_ms);
        self.keep_word(id, &new, at, wall_ms);
        self.keep_seen(book, id, new, at, wall_ms);
        Some(id)
    }

    fn keep_word(&mut self, id: WordId, new: &NewWord, at: Stamp, wall_ms: u64) {
        let Some(word) = self.words.get_mut(&id) else {
            let word = WordState {
                id,
                lemma: new.lemma.to_lowercase(),
                pos: new.pos.clone(),
                band: new.band,
                status: Field::new(new.status, at),
                added: wall_ms,
                alive: Field::new(true, at),
                extra: Extra::default(),
            };
            self.words.insert(id, word);
            return;
        };
        let keep = word.alive.value
            && word.status.value == WordStatus::Memorized
            && new.status == WordStatus::Learning;
        if !keep {
            put(&mut word.status, new.status, at);
        }
        put(&mut word.alive, true, at);
    }

    fn keep_seen(&mut self, book: BookId, word: WordId, new: NewWord, at: Stamp, wall_ms: u64) {
        let id = SightingId::of(book, word);
        let Some(seen) = self.sightings.get_mut(&id) else {
            let sighting = WordSighting {
                id,
                book: Field::new(book, at),
                word,
                by: Field::new(new.by, at),
                form: Field::new(new.form, at),
                context: Field::new(new.context, at),
                meaning: Field::new(new.meaning, at),
                meaning_by: Field::new(new.meaning_by, at),
                tries: Field::new(new.tries, at),
                first_try: Field::new(new.first_try, at),
                recall_history: {
                    let mut history = RecallHistory::default();
                    history.add(at, wall_ms, new.answers);
                    history
                },
                synonyms: Field::new(new.synonyms, at),
                usages: Field::new(new.usages, at),
                added: wall_ms,
                seen: Field::new(wall_ms, at),
                alive: Field::new(true, at),
                extra: Extra::default(),
            };
            self.sightings.insert(id, sighting);
            return;
        };
        if !new.context.is_empty() {
            put(&mut seen.form, new.form, at);
            put(&mut seen.context, new.context, at);
        }
        if !new.meaning.is_empty() {
            put(&mut seen.meaning, new.meaning, at);
            put(&mut seen.meaning_by, new.meaning_by, at);
        }
        if !new.synonyms.is_empty() {
            put(&mut seen.synonyms, new.synonyms, at);
        }
        if !new.usages.is_empty() {
            put(&mut seen.usages, new.usages, at);
        }
        seen.recall_history.add(at, wall_ms, new.answers);
        put(&mut seen.by, new.by, at);
        put(&mut seen.tries, new.tries, at);
        put(&mut seen.first_try, new.first_try, at);
        put(&mut seen.seen, wall_ms, at);
        put(&mut seen.alive, true, at);
    }

    /// Sets living words' status; how many changed.
    pub fn set_word_status(&mut self, ids: &[WordId], status: WordStatus, wall_ms: u64) -> usize {
        let differs = |id: &&WordId| self.word(**id).is_some_and(|w| w.status.value != status);
        let ids: Vec<WordId> = ids.iter().filter(differs).copied().collect();
        if ids.is_empty() {
            return 0;
        }
        let at = self.clock.tick(wall_ms);
        for id in &ids {
            if let Some(word) = self.words.get_mut(id) {
                word.status = Field::new(status, at);
            }
        }
        ids.len()
    }

    /// Forgets words or brings them back; how many changed.
    pub fn set_words_alive(&mut self, ids: &[WordId], alive: bool, wall_ms: u64) -> usize {
        let flips = |id: &&WordId| self.words.get(id).is_some_and(|w| w.alive.value != alive);
        let ids: Vec<WordId> = ids.iter().filter(flips).copied().collect();
        if ids.is_empty() {
            return 0;
        }
        let at = self.clock.tick(wall_ms);
        for id in &ids {
            if let Some(word) = self.words.get_mut(id) {
                word.alive = Field::new(alive, at);
            }
        }
        ids.len()
    }

    /// Removes one book's studied occurrence without changing other books.
    pub fn remove_word_from_book(&mut self, book: BookId, word: WordId, wall_ms: u64) -> bool {
        let exists = self
            .sightings
            .values()
            .any(|s| s.alive.value && s.book.value == book && s.word == word);
        if !exists {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        for seen in self
            .sightings
            .values_mut_where(|s| s.alive.value && s.book.value == book && s.word == word)
        {
            clear_sighting(seen, at);
        }
        if self.sightings_of(word).is_empty()
            && let Some(state) = self.words.get_mut(&word)
        {
            state.alive = Field::new(false, at);
        }
        true
    }

    /// Study mode's gate in `book`: the text unlocked so far.
    pub fn unlocked(&self, book: BookId) -> Option<&Spot> {
        self.states.get(&book)?.unlocked.value.as_ref()
    }

    /// Moves `book`'s gate; false if the book is gone or nothing changed.
    pub fn set_unlocked(&mut self, book: BookId, spot: Option<Spot>, wall_ms: u64) -> bool {
        if self.book(book).is_none() || self.unlocked(book) == spot.as_ref() {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        self.state_mut(book).unlocked = Field::new(spot, at);
        true
    }

    /// `gone`'s sightings and gate go to `keeper`, as a fold decides.
    pub(crate) fn move_study(&mut self, gone: BookId, keeper: BookId, at: Stamp) {
        for seen in self.sightings.values_mut_where(|s| s.book.value == gone) {
            seen.book = Field::new(keeper, at);
        }
        let theirs = self.states.get(&gone).map(|s| s.unlocked.clone());
        let theirs = theirs.filter(|f| f.value.is_some());
        if let Some(theirs) = theirs.filter(|_| self.unlocked(keeper).is_none()) {
            self.state_mut(keeper).unlocked = theirs;
        }
    }

    /// `book`'s state to change, made unread if it has none; saved next.
    fn state_mut(&mut self, book: BookId) -> &mut State {
        self.states_dirty.insert(book);
        let start = Position {
            page: 0,
            pages: 0,
            place: None,
            spine: None,
        };
        (self.states)
            .entry(book)
            .or_insert_with(|| State::new(Field::new(start, NEVER)))
    }

    /// `book`'s sightings die with their words, kept only for Undo.
    pub(crate) fn forget_sightings(&mut self, book: BookId, at: Stamp) {
        for seen in self.sightings.values_mut_where(|s| s.book.value == book) {
            clear_sighting(seen, at);
        }
    }
}

fn clear_sighting(seen: &mut WordSighting, at: Stamp) {
    seen.alive = Field::new(false, at);
    seen.form = Field::new(String::new(), at);
    seen.context = Field::new(String::new(), at);
    seen.meaning = Field::new(String::new(), at);
    seen.meaning_by = Field::new(None, at);
    seen.synonyms = Field::new(Vec::new(), at);
    seen.usages = Field::new(Vec::new(), at);
    seen.tries = Field::new(0, at);
    seen.first_try = Field::new(false, at);
    seen.recall_history.clear(at);
}

impl Merge for WordState {
    fn merge(&mut self, other: Self) {
        self.band = self.band.max(other.band);
        self.added = self.added.min(other.added);
        self.status.merge(other.status);
        self.alive.merge(other.alive);
        self.extra.merge(other.extra);
    }

    fn latest(&self) -> Stamp {
        let fields = [Some(self.status.at), Some(self.alive.at)];
        newest(fields.into_iter().chain([self.extra.latest()]))
    }
}

impl Merge for WordSighting {
    fn merge(&mut self, other: Self) {
        self.added = self.added.min(other.added);
        self.book.merge(other.book);
        self.by.merge(other.by);
        self.form.merge(other.form);
        self.context.merge(other.context);
        self.meaning.merge(other.meaning);
        self.meaning_by.merge(other.meaning_by);
        self.tries.merge(other.tries);
        self.first_try.merge(other.first_try);
        self.recall_history.merge(other.recall_history);
        self.synonyms.merge(other.synonyms);
        self.usages.merge(other.usages);
        self.seen.merge(other.seen);
        self.alive.merge(other.alive);
        self.extra.merge(other.extra);
    }

    fn latest(&self) -> Stamp {
        let fields = [
            self.book.at,
            self.by.at,
            self.form.at,
            self.context.at,
            self.meaning.at,
            self.meaning_by.at,
            self.tries.at,
            self.first_try.at,
            self.synonyms.at,
            self.usages.at,
            self.seen.at,
            self.alive.at,
        ];
        let fields = fields.map(Some);
        newest(
            fields
                .into_iter()
                .chain([self.extra.latest(), self.recall_history.latest()]),
        )
    }
}

#[cfg(test)]
#[path = "study_words_tests.rs"]
mod tests;
