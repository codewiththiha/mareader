//! A word asked of an explainer off the frame, and what comes back.

use std::sync::{Arc, Mutex};

use mareader_library::{DeviceId, Library};
use mareader_study::Why;

use super::super::state::Key;
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
        book_only: false,
    }
}

/// An explainer that keeps what it was asked, streams one part, then answers.
struct Canned {
    asks: Arc<Mutex<Vec<Asked>>>,
    part: WordInfo,
    answer: Result<WordInfo, Failure>,
}

impl Explainer for Canned {
    fn by(&self) -> String {
        "the model".to_owned()
    }

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
fn asked_of(one: Arc<Canned>, wake: Waker) -> Ask {
    let (word, context) = ("fox".to_owned(), "The quick brown fox.".to_owned());
    ask(wake, one, (word, context, None))
}

#[test]
fn a_word_asked_off_the_frame_streams_and_then_answers() {
    let (wake, frames) = asked();
    let (one, asks) = canned(Ok(info("what the word means here")));
    let got = asked_of(one, wake);
    let Got::Partial(part) = got.rx.recv().expect("the worker sends as it goes") else {
        panic!("the first thing it sends is the answer so far");
    };
    assert_eq!(part.meaning, "the first words of it", "as it streamed");
    let Got::Done(done) = got.rx.recv().expect("the worker sends the end") else {
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
    let first = got.rx.recv().expect("the worker sends as it goes");
    assert!(matches!(first, Got::Partial(_)), "it streams first");
    let last = got.rx.recv().expect("the worker sends the end");
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
        book_only: false,
    };
    let (one, _) = canned(Ok(long.clone()));
    let got = asked_of(one, wake);
    let first = got.rx.recv().expect("the worker sends as it goes");
    let Got::Partial(part) = first else {
        panic!("the first thing it sends is the answer so far");
    };
    assert_eq!(part.meaning, "the first words of it", "streamed as given");
    let Got::Done(done) = got.rx.recv().expect("the worker sends the end") else {
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
        word: "brisk".to_owned(),
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

/// An explainer that waits until it is told to stop, and says it gave up.
struct Stoppable {
    gave_up: Arc<Mutex<bool>>,
}

impl Explainer for Stoppable {
    fn explain(
        &self,
        _: &str,
        _: &str,
        _: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        Err(Why::NotHere.into())
    }

    fn explain_until(
        &self,
        _: (&str, &str, Option<Pos>),
        _: &(dyn Fn(WordInfo) + Sync),
        stop: &(dyn Fn() -> bool + Sync),
    ) -> Result<(WordInfo, String), Failure> {
        while !stop() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if let Ok(mut gave_up) = self.gave_up.lock() {
            *gave_up = true;
        }
        Err(Why::NotHere.into())
    }
}

/// A word on the page to explain, with its spot.
fn slumber() -> super::super::words::Asked {
    super::super::words::Asked {
        word: "slumber".into(),
        context: "deep slumber".into(),
        page: 0,
        chars: 5..12,
        spot: Some(mareader_library::Spot::Page {
            page: 0,
            rect: [40.0, 0.0, 50.0, 20.0],
        }),
    }
}

/// Whether `key`'s mark still shows.
fn shows(state: &super::super::state::State, main: &Library, key: Key) -> bool {
    let Key::Mark(id) = key else {
        return false;
    };
    state.mark(main, id).is_some_and(|m| m.alive.value)
}

#[test]
fn cancelling_an_ask_stops_its_explainer_and_unmarks_the_word() {
    use super::super::state::State;
    let (wake, _) = asked();
    let mut main = Library::new(DeviceId(1));
    let mut state = State::default();
    let gave_up = Arc::new(Mutex::new(false));
    let told = Arc::clone(&gave_up);
    state.set_explainer(Arc::new(Stoppable { gave_up: told }));
    let ask = slumber();
    let key = state.explain_mark(&mut main, None, "book", &ask, 1);
    state.prepare_card(&mut main, wake, key, &ask.word, &ask.context);
    assert!(shows(&state, &main, key), "marked while it asks");
    assert!(state.cancel_ask(&mut main, key), "an ask was coming");
    let stopped = || gave_up.lock().map(|g| *g).unwrap_or(false);
    assert!(waited(stopped), "the explainer itself gave up");
    assert!(!shows(&state, &main, key), "the word as it was");
}

#[test]
fn removing_a_mark_stops_its_ask() {
    use super::super::state::State;
    let (wake, _) = asked();
    let mut main = Library::new(DeviceId(1));
    let mut state = State::default();
    let gave_up = Arc::new(Mutex::new(false));
    let told = Arc::clone(&gave_up);
    state.set_explainer(Arc::new(Stoppable { gave_up: told }));
    let ask = slumber();
    let key = state.explain_mark(&mut main, None, "book", &ask, 1);
    state.prepare_card(&mut main, wake, key, &ask.word, &ask.context);
    let Key::Mark(id) = key else {
        panic!("expected a mark key");
    };
    let _ = state.remove_marks(&mut main, vec![id], 2);
    let stopped = || gave_up.lock().map(|g| *g).unwrap_or(false);
    assert!(waited(stopped), "nobody asks for a mark that went");
    assert!(state.asking_marks().is_empty());
}

#[test]
fn a_failed_ask_unmarks_the_word_and_try_again_marks_it_back() {
    use super::super::state::State;
    let (wake, _) = asked();
    let mut main = Library::new(DeviceId(1));
    let mut state = State::default();
    state.set_inline(true);
    let failed = Failure::detailed(Why::Busy, "busy");
    state.set_explainer(canned(Err(failed)).0);
    let ask = slumber();
    let key = state.explain_mark(&mut main, None, "book", &ask, 1);
    state.prepare_card(&mut main, Arc::clone(&wake), key, &ask.word, &ask.context);
    assert!(state.failure(key).is_some(), "it failed");
    assert!(!shows(&state, &main, key), "its mark went with it");
    state.set_explainer(canned(Ok(info("asleep"))).0);
    state.retry(&mut main, wake, key, &ask.word, &ask.context);
    assert!(shows(&state, &main, key), "Try again marks it back");
    let meaning = state.answer(key).map(|s| s.info.meaning.clone());
    assert_eq!(meaning.as_deref(), Some("asleep"), "and answers");
}
