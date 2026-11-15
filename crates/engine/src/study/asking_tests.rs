//! A word asked of an explainer off the frame, and what comes back.

use std::sync::{Arc, Mutex};

use mareader_study::Why;

use super::*;
use crate::testing::{asked, waited, wakes};

/// What an explainer was asked: the word, and the sentence it stands in.
type Asked = (String, String);

/// An answer in four parts, small enough to survive the clamp untouched.
fn info(meaning: &str) -> WordInfo {
    WordInfo {
        pos: "noun".to_owned(),
        meaning: meaning.to_owned(),
        synonyms: vec!["one".to_owned()],
        usages: vec!["A sentence that uses the word.".to_owned()],
    }
}

/// An explainer that keeps what it was asked, streams one part, then answers.
struct Canned {
    asks: Arc<Mutex<Vec<Asked>>>,
    part: WordInfo,
    answer: Result<WordInfo, Failure>,
}

impl Explainer for Canned {
    fn explain(
        &self,
        word: &str,
        context: &str,
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        if let Ok(mut asks) = self.asks.lock() {
            asks.push((word.to_owned(), context.to_owned()));
        }
        partial(self.part.clone());
        self.answer.clone()
    }
}

/// A canned explainer, and the asks it keeps.
fn canned(answer: Result<WordInfo, Failure>) -> (Arc<Canned>, Arc<Mutex<Vec<Asked>>>) {
    let asks = Arc::new(Mutex::new(Vec::new()));
    let one = Canned {
        asks: Arc::clone(&asks),
        part: info("the first words of it"),
        answer,
    };
    (Arc::new(one), asks)
}

/// Asks `one` about the fox, as the card and the mark menu do.
fn asked_of(one: Arc<Canned>, wake: Waker) -> Receiver<Got> {
    ask(
        wake,
        one,
        "the model".to_owned(),
        "fox".to_owned(),
        "The quick brown fox.".to_owned(),
    )
}

#[test]
fn a_word_asked_off_the_frame_streams_and_then_answers() {
    let (wake, frames) = asked();
    let (one, asks) = canned(Ok(info("what the word means here")));
    let got = asked_of(one, wake);
    let Got::Partial(part) = got.recv().expect("the worker sends as it goes") else {
        panic!("the first thing it sends is the answer so far");
    };
    assert_eq!(part.meaning, "the first words of it", "as it streamed");
    let Got::Done(done) = got.recv().expect("the worker sends the end") else {
        panic!("the last thing it sends is the answer, done");
    };
    let said = done.expect("the canned explainer answered");
    assert_eq!(said.by, "the model", "the answer names who gave it");
    assert_eq!(said.info.meaning, "what the word means here");
    assert!(waited(|| wakes(&frames) >= 2), "a frame was asked for each");
    let kept = asks.lock().map(|a| a.clone()).unwrap_or_default();
    assert_eq!(kept.len(), 1, "one explainer was asked once");
    let (word, context) = kept.first().expect("the ask was kept");
    assert_eq!(word, "fox", "the word asked for is the one sent");
    assert_eq!(context, "The quick brown fox.", "and its sentence");
}

#[test]
fn an_explainer_that_fails_says_why_it_could_not_answer() {
    let (wake, frames) = asked();
    let failed = Failure::detailed(Why::Busy, "the model is busy with another");
    let (one, _) = canned(Err(failed.clone()));
    let got = asked_of(one, wake);
    let first = got.recv().expect("the worker sends as it goes");
    assert!(matches!(first, Got::Partial(_)), "it streams first");
    let last = got.recv().expect("the worker sends the end");
    let Got::Done(done) = last else {
        panic!("the last thing it sends is the answer, done");
    };
    let why = done.expect_err("the canned explainer failed");
    assert_eq!(why, failed, "the card is told what the provider said");
    assert!(waited(|| wakes(&frames) >= 2), "a frame was asked for each");
}

#[test]
fn the_answer_that_comes_back_is_within_the_card_s_bounds() {
    let (wake, _) = asked();
    let long = WordInfo {
        pos: "n".repeat(200),
        meaning: "m".repeat(2_000),
        synonyms: (0..50).map(|at| format!("synonym {at}")).collect(),
        usages: (0..50).map(|at| format!("usage {at}")).collect(),
    };
    let (one, _) = canned(Ok(long.clone()));
    let got = asked_of(one, wake);
    let first = got.recv().expect("the worker sends as it goes");
    let Got::Partial(part) = first else {
        panic!("the first thing it sends is the answer so far");
    };
    assert_eq!(part.meaning, "the first words of it", "streamed as given");
    let Got::Done(done) = got.recv().expect("the worker sends the end") else {
        panic!("the last thing it sends is the answer, done");
    };
    let said = done.expect("the canned explainer answered");
    let meaning = said.info.meaning.clone();
    assert!(meaning.ends_with('…'), "a meaning over the bound is cut");
    let synonyms = said.info.synonyms.len();
    assert!(synonyms < long.synonyms.len(), "and so are the synonyms");
    let usages = said.info.usages.len();
    assert!(usages < long.usages.len(), "and so are the usages");
}

#[test]
fn an_answer_saves_as_the_gloss_of_the_mark_it_answers() {
    let said = Said {
        info: info("what the word means here"),
        by: "the model".to_owned(),
    };
    let gloss = said.gloss();
    assert_eq!(gloss.by, said.by, "who gave it is kept with it");
    assert_eq!(gloss.meaning, said.info.meaning, "and the meaning");
    assert_eq!(gloss.pos, said.info.pos, "and the part of speech");
    assert_eq!(gloss.synonyms, said.info.synonyms, "and the synonyms");
    assert_eq!(gloss.usages, said.info.usages, "and the usages");
}
