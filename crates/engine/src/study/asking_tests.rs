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

struct GateExplainer {
    by: &'static str,
    meaning: &'static str,
    release: Arc<std::sync::Condvar>,
    open: Arc<Mutex<bool>>,
}

impl Explainer for GateExplainer {
    fn by(&self) -> String {
        self.by.to_owned()
    }

    fn explain(
        &self,
        _word: &str,
        _context: &str,
        _partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        if let Ok(mut open) = self.open.lock() {
            while !*open {
                let Ok(next) = self.release.wait(open) else {
                    break;
                };
                open = next;
            }
        }
        Ok(info(self.meaning))
    }
}

#[test]
fn ask_again_replaces_saved_gloss_and_stays_clear_while_pending() {
    use super::super::state::{Key, State};
    use super::super::words::Asked;
    use mareader_library::{DeviceId, Library, Spot};

    let (wake, _) = asked();
    let mut main = Library::new(DeviceId(1));
    let mut state = State::default();
    state.set_inline(true);
    let first = Arc::new(GateExplainer {
        by: "Model A",
        meaning: "old meaning",
        release: Arc::new(std::sync::Condvar::new()),
        open: Arc::new(Mutex::new(true)),
    });
    state.set_explainer(first);
    let ask = Asked {
        word: "slumber".into(),
        context: "deep slumber".into(),
        page: 0,
        chars: 5..12,
        spot: Some(Spot::Page {
            page: 0,
            rect: [40.0, 0.0, 50.0, 20.0],
        }),
    };
    let key = state.explain_mark(&mut main, None, "book", &ask, 1);
    state.prepare_card(&mut main, Arc::clone(&wake), key, &ask.word, &ask.context);
    assert_eq!(
        state.answer(key).map(|s| s.info.meaning.as_str()),
        Some("old meaning")
    );
    let Key::Mark(id) = key else {
        panic!("expected a mark key");
    };

    // Switch to a gated explainer and ask again off-thread.
    let release = Arc::new(std::sync::Condvar::new());
    let open = Arc::new(Mutex::new(false));
    state.set_inline(false);
    state.set_explainer(Arc::new(GateExplainer {
        by: "Model B",
        meaning: "fresh meaning",
        release: Arc::clone(&release),
        open: Arc::clone(&open),
    }));
    state.ask_again(&mut main, Arc::clone(&wake), key, &ask.word, &ask.context);
    assert!(state.answer(key).is_none());
    // Reopening mid-flight must not resurrect the old saved gloss.
    state.prepare_card(&mut main, Arc::clone(&wake), key, &ask.word, &ask.context);
    assert!(state.answer(key).is_none());

    // Release the new answer: session state and saved mark replace old data.
    if let Ok(mut flag) = open.lock() {
        *flag = true;
        release.notify_all();
    }
    assert!(waited(|| {
        state.receive(&mut main);
        state.answer(key).is_some()
    }));
    let got = state.answer(key).expect("new answer landed");
    assert_eq!(got.info.meaning, "fresh meaning");
    assert_eq!(got.by, "Model B");
    let saved = state
        .mark(&main, id)
        .and_then(|m| m.gloss.value.as_ref())
        .expect("saved gloss updated");
    assert_eq!(saved.meaning, "fresh meaning");
    assert_eq!(saved.by, "Model B");
}
