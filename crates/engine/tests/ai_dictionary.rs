//! Explain answers go to the AI dictionary through `Session::tick`; a
//! differing kept meaning waits for the card's Replace.
#![cfg(not(target_arch = "wasm32"))]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mareader_dict::{Query, Reply, Saved};
use mareader_engine::event::Event;
use mareader_engine::host::{Fake, Shared};
use mareader_engine::session::Session;
use mareader_engine::study::state::Key;
use mareader_study::{Explainer, Failure, WordInfo};

const WAIT: Duration = Duration::from_secs(5);

/// Answers every word as a noun with the meaning set last.
struct Writes(Mutex<String>);

impl Explainer for Writes {
    fn explain(
        &self,
        _: &str,
        _: &str,
        _: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        let meaning = self.0.lock().map(|m| m.clone()).unwrap_or_default();
        Ok(WordInfo {
            pos: "noun".into(),
            meaning,
            ..WordInfo::default()
        })
    }

    fn by(&self) -> String {
        "Test model".into()
    }

    fn language(&self) -> Option<&'static str> {
        Some("English")
    }
}

fn wait_until(mut check: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < WAIT {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    check()
}

/// The meaning the AI dictionary keeps for "basin" in English, if any.
fn kept(session: &mut Session) -> Option<String> {
    let ticket = session.dict_ask(Query::Remembered {
        words: vec!["basin".into()],
        lang: "en".into(),
    });
    let mut got: Option<Reply> = None;
    let answered = wait_until(|| {
        session.tick(1.0);
        for event in session.drain() {
            if let Event::DictReplies { replies, .. } = event
                && let Some(reply) = replies.into_iter().find(|r| r.ticket == ticket)
            {
                got = Some(reply);
            }
        }
        got.is_some()
    });
    assert!(answered, "the AI dictionary should answer");
    let saved: Vec<Saved> = got.map(|r| r.saved).unwrap_or_default();
    saved.first().map(|s| s.meaning.clone())
}

/// Asks about "basin" and waits for the answer to be filed.
fn explain(session: &mut Session, key: Option<Key>) -> Key {
    let (pane, words) = (session.active(), ("basin", "The river basin."));
    let key = match key {
        Some(key) => {
            session.study_ask_again(pane, key, words);
            key
        }
        None => session.study_explain_loose(pane, words.0, words.1),
    };
    let done = wait_until(|| {
        session.tick(1.0);
        session.drain();
        session.study().answer(key).is_some()
    });
    assert!(done, "the explainer should answer");
    key
}

#[test]
fn an_explain_answer_is_kept_and_a_different_one_waits_for_replace() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("dictionaries")).expect("dictionaries dir");
    let host: Shared = Arc::new(Fake::under(dir.path()));
    let mut session = Session::new(host, Arc::new(|| {}));
    let writes = Arc::new(Writes(Mutex::new("a bowl for water".into())));
    session.study_set_explainer(writes.clone());
    let key = explain(&mut session, None);
    let first = wait_until(|| kept(&mut session).is_some());
    assert!(first, "the first answer is kept");
    *writes.0.lock().expect("meaning") = "land a river drains".into();
    explain(&mut session, Some(key));
    let offered = wait_until(|| {
        session.tick(1.0);
        session.study().offer(key).is_some()
    });
    assert!(offered, "a different answer offers to replace");
    let offer = session.study().offer(key).expect("offer").clone();
    assert_eq!(offer.kept.meaning, "a bowl for water");
    let before = kept(&mut session);
    assert_eq!(before.as_deref(), Some("a bowl for water"), "unchanged");
    session.study_replace(key);
    let now = |s: &mut Session| kept(s).is_some_and(|m| m == "land a river drains");
    let replaced = wait_until(|| now(&mut session));
    assert!(replaced, "Replace keeps the new answer");
    assert!(session.study().offer(key).is_none());
}
