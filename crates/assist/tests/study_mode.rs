//! Study mode's card writer and judge end to end, against a loopback server.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use mareader_assist::{Assist, Kind, Slots};
use mareader_settings::{AiSettings, Provider, TaskId};
use mareader_study::Why;
use mareader_study::mode::{Answer as Backs, Attempt, CardJob, Key, Verdict, Wanted};
use support::{Answer, Server};

fn server(answers: Vec<Answer>) -> (tokio::runtime::Runtime, Server) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(Server::start(answers));
    (rt, server)
}

fn assist(dir: &tempfile::TempDir) -> Assist {
    let assist = Assist::new(dir.path().to_owned(), Arc::new(|| {})).unwrap();
    assist.without_keychain()
}

/// One saved model, `m` on Ollama at `url`; study follows Explain's.
fn settings(url: &str) -> AiSettings {
    let mut ai = AiSettings {
        presets: Vec::new(),
        ..AiSettings::default()
    };
    let id = ai.add_preset(Provider::Ollama, "m");
    ai.tasks.choose(TaskId::Explain, Some(id));
    if let Some(e) = ai.endpoint_mut(Provider::Ollama) {
        e.url = url.to_owned();
    }
    ai
}

fn wanted(lemma: &str) -> Wanted {
    Wanted {
        key: Key::new(lemma, None),
        form: lemma.to_owned(),
        sentence: format!("An {lemma} thing."),
    }
}

/// Polls `job` until it answers, or fails after ten seconds.
fn finish(job: &mut dyn CardJob) -> Backs {
    let start = Instant::now();
    loop {
        if let Some(answer) = job.take() {
            return answer;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "no answer");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Polls `take` past the service's slot wait, or fails after thirty seconds.
fn slowly<T>(mut take: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(got) = take() {
            return got;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "no answer");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn attempt() -> Attempt {
    Attempt {
        form: "wandered".to_owned(),
        lemma: "wander".to_owned(),
        sentence: "He wandered off.".to_owned(),
        answer: "walked".to_owned(),
    }
}

fn until(done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(10), "never happened");
        std::thread::sleep(Duration::from_millis(10));
    }
}

const BACKS: &str = r#"{"cards":[
{"n":2,"meaning":"Lively.","synonyms":["quick"],"usage":"A brisk walk."},
{"n":1,"meaning":"Hidden.","synonyms":[],"usage":"An arcane rule."}]}"#;

#[test]
fn a_deck_s_backs_come_in_one_request_by_number() {
    let (_rt, server) = server(vec![Answer::stream(&[&BACKS[..40], &BACKS[40..]])]);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let _explainer = a.configure(&settings(&server.url));
    let cards = a.cards();
    assert_eq!(cards.by(), "Ollama · m");
    assert!(cards.id().ends_with("|English"), "{}", cards.id());
    let mut job = cards.write(vec![wanted("arcane"), wanted("brisk")]);
    let backs = finish(job.as_mut()).unwrap();
    assert_eq!(backs[0].as_ref().unwrap().meaning, "Hidden.");
    assert_eq!(backs[1].as_ref().unwrap().usage, "A brisk walk.");
    assert!(job.take().is_none(), "a deck answers once");
    let sent = server.seen()[0].body.to_string();
    assert!(
        sent.contains(r#"1. \"arcane\" in: \"An arcane thing.\""#),
        "{sent}"
    );
    assert_eq!(server.seen().len(), 1, "one request for the deck");
}

#[test]
fn a_dropped_deck_frees_its_model_and_no_model_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    // A server that takes the request and never answers.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ai = settings(&format!("http://{}/v1/", silent.local_addr().unwrap()));
    let id = ai.model(TaskId::Explain).unwrap().id;
    let _explainer = a.configure(&ai);
    let job = a.cards().write(vec![wanted("arcane")]);
    until(|| a.in_use(id) == 1);
    drop(job);
    until(|| a.in_use(id) == 0);
    let mut none = ai.clone();
    none.tasks.choose(TaskId::Explain, None);
    let _ = a.configure(&none);
    let answer = finish(a.cards().write(vec![wanted("arcane")]).as_mut());
    assert_eq!(answer.err().map(|f| f.why), Some(Why::NotHere));
}

#[test]
fn a_recall_is_judged_by_study_s_model_with_the_answer_quoted() {
    let verdict = r#"{"verdict":"close","feedback":"Nearly: here it means roamed."}"#;
    let (_rt, server) = server(vec![Answer::stream(&[verdict])]);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let _explainer = a.configure(&settings(&server.url));
    let judge = a.judge();
    assert_eq!(judge.by(), "Ollama · m");
    let mut job = judge.judge(attempt());
    let start = Instant::now();
    let judged = loop {
        if let Some(judged) = job.take() {
            break judged.unwrap();
        }
        assert!(start.elapsed() < Duration::from_secs(10), "no verdict");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(judged.verdict, Verdict::Close);
    assert!(judged.feedback.starts_with("Nearly"));
    let sent = server.seen()[0].body.to_string();
    assert!(sent.contains(r#"Answer: \"walked\""#), "{sent}");
}

#[test]
fn a_deck_and_a_judge_behind_a_held_model_wait_then_are_turned_away() {
    let (_rt, server) = server(vec![Answer::stream(&[BACKS])]);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let ai = settings(&server.url);
    let id = ai.model(TaskId::Explain).unwrap().id;
    let _explainer = a.configure(&ai);
    let held = a.slots().try_take(id, 1, Kind::Short);
    assert!(held.is_some());
    let start = Instant::now();
    let mut deck = a.cards().write(vec![wanted("arcane")]);
    let mut verdict = a.judge().judge(attempt());
    std::thread::sleep(Duration::from_secs(1));
    assert!(deck.take().is_none(), "the deck waits");
    assert!(verdict.take().is_none(), "the judge waits");
    // F3 as it stands; Phase 4 of the AI plan queues them instead.
    let backs = slowly(|| deck.take());
    assert_eq!(backs.err().map(|f| f.why), Some(Why::InUse));
    let waited = start.elapsed();
    assert!(waited >= Slots::SHORT_WAIT, "{waited:?}");
    let judged = slowly(|| verdict.take());
    assert_eq!(judged.err().map(|f| f.why), Some(Why::InUse));
    assert!(server.seen().is_empty(), "nothing is sent while busy");
    assert_eq!(a.in_use(id), 1, "only the held slot is taken");
}

#[test]
fn a_dropped_judge_frees_its_model() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    // A server that takes the request and never answers.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let ai = settings(&format!("http://{}/v1/", silent.local_addr().unwrap()));
    let id = ai.model(TaskId::Explain).unwrap().id;
    let _explainer = a.configure(&ai);
    let job = a.judge().judge(attempt());
    until(|| a.in_use(id) == 1);
    drop(job);
    until(|| a.in_use(id) == 0);
}
