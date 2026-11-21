//! Reading aids as pure rules (L1); see docs/study.md and docs/study-mode.md.
//!
//! The gloss side (Explain, marks, CEFR shading) lives here, and now the study
//! mode side too: the batch planner, the deck builder and dedupe, rank and cap,
//! the StudyJudge verdict, and the `CardMaker` / `StudyJudge` contracts whose
//! real answerers are provided by the front end (AI or dictionary pack), with
//! `Unavailable` and `Canned` stand-ins for tests and shots.
//!
//! Nothing here names a toolkit.

#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod answer;
mod batches;
mod deck;
mod explain;
mod gloss;
mod judge;
pub mod mode;
pub mod page;

pub use answer::WordInfo;
pub use batches::{Batch, BatchCfg, BatchUnit, LIGHT_CHARS, MAX_PAGES, build_batches};
pub use deck::{
    DEFAULT_MAX_CARDS, DeckCard, HardSpan, HardWord, MAX_CARDS_MAX, MAX_CARDS_MIN, WordClass,
    build_deck,
};
pub use explain::{Canned, Explainer, Failure, Unavailable, Why};
pub use gloss::{CONTEXT_CHARS, MAX_GLOSS_CHARS, context, word_in};
pub use judge::{Judged, JudgedEntry, Verdict, judge};
pub use mode::{CardSource, StudyMode, StudyModeSettings};

/// A word the deck builder should turn into a card: its identity and the
/// sentence it first appears in on these pages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CardItem {
    pub key: String,
    pub lemma: String,
    pub class: WordClass,
    pub sentence: String,
}

/// A written card: what the AI or the dictionary pack answers for one word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub key: String,
    pub meaning: String,
    pub synonyms: Vec<String>,
    pub usage: String,
}

/// Who writes a batch's cards: the AI, an offline dictionary pack, or nobody.
///
/// Like [`Explainer`], the front end provides the real answerer (backed by
/// the AI or a downloaded pack); `Unavailable` and `Canned` stand in for
/// tests and shots. Runs on a worker, so it may block.
pub trait CardMaker: Send + Sync {
    /// Who answers, as the card's small print names them; blank for none.
    fn by(&self) -> String;
    /// One request for a batch's cards not already cached.
    fn make_cards(&self, items: &[CardItem], language: Option<&str>) -> Result<Vec<Card>, Failure>;
}

/// Explains nothing: this build or machine has no card source.
#[derive(Clone, Copy, Debug, Default)]
pub struct CardMakerUnavailable;

impl CardMaker for CardMakerUnavailable {
    fn by(&self) -> String {
        String::new()
    }
    fn make_cards(&self, _: &[CardItem], _: Option<&str>) -> Result<Vec<Card>, Failure> {
        Err(Failure::from(Why::NotHere))
    }
}

/// Answers from a script, for tests and shots.
#[derive(Clone, Copy, Debug, Default)]
pub struct CardMakerCanned;

impl CardMaker for CardMakerCanned {
    fn by(&self) -> String {
        "Sample cards".to_owned()
    }
    fn make_cards(&self, items: &[CardItem], _: Option<&str>) -> Result<Vec<Card>, Failure> {
        let mut out = Vec::new();
        for item in items {
            let meaning = format!("What “{}” means here, in plain words.", item.lemma);
            out.push(Card {
                key: item.key.clone(),
                meaning,
                synonyms: Vec::new(),
                usage: format!("As in: {}", item.sentence),
            });
        }
        Ok(out)
    }
}

/// Who judges a reader's attempt at recalling a word: the AI, or nobody.
pub trait StudyJudge: Send + Sync {
    /// Who answers, as the popover's small print names them; blank for none.
    fn by(&self) -> String;
    /// One attempt: the word, its sentence and the reader's answer in, a
    /// verdict and feedback out.
    fn judge(&self, attempt: &str, entry: &JudgedEntry) -> Result<Judged, Failure>;
}

/// Judges nothing: this build or machine has no model.
#[derive(Clone, Copy, Debug, Default)]
pub struct StudyJudgeUnavailable;

impl StudyJudge for StudyJudgeUnavailable {
    fn by(&self) -> String {
        String::new()
    }
    fn judge(&self, _: &str, _: &JudgedEntry) -> Result<Judged, Failure> {
        Err(Why::NotHere.into())
    }
}

/// Judges from a script, for tests and shots: the canned heuristic.
#[derive(Clone, Copy, Debug, Default)]
pub struct StudyJudgeCanned;

impl StudyJudge for StudyJudgeCanned {
    fn by(&self) -> String {
        "Sample judge".to_owned()
    }
    fn judge(&self, attempt: &str, entry: &JudgedEntry) -> Result<Judged, Failure> {
        Ok(judge(attempt, entry))
    }
}
