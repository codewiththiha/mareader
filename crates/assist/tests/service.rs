//! The app's service end to end: settings in, answers and key jobs out.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod support;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mareader_assist::{
    Assist, Board, Job, KeyState, Load, Loose, Organized, Organizing, Plan, Progress, Request,
    Spot, Stage,
};
use mareader_settings::{AiSettings, ModelPreset, Provider, TaskId};
use mareader_study::mode::{Key, Wanted};
use mareader_study::{Explainer, Why, WordInfo};
use support::{Answer, QUICK, Server};

/// A runtime for the loopback server, apart from the service's own.
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

/// Polls until `done` holds for the board, or fails after ten seconds.
fn wait(a: &mut Assist, done: impl Fn(&Board) -> bool) {
    let start = Instant::now();
    while !done(a.board()) {
        assert!(start.elapsed() < Duration::from_secs(10), "{:?}", a.board());
        std::thread::sleep(Duration::from_millis(10));
        a.poll();
    }
}

fn explain(e: &dyn Explainer) -> (Result<WordInfo, Why>, Vec<WordInfo>) {
    let seen = Mutex::new(Vec::new());
    let push = |i| seen.lock().unwrap().push(i);
    let got = e
        .explain("quick", "The quick fox", &push)
        .map_err(|f| f.why);
    (got, seen.into_inner().unwrap())
}

/// One saved model, `m` on `p` at `url`, explaining words.
fn settings(p: Provider, url: &str) -> AiSettings {
    let mut ai = AiSettings {
        presets: Vec::new(),
        ..AiSettings::default()
    };
    let id = ai.add_preset(p, "m");
    ai.tasks.choose(TaskId::Explain, Some(id));
    if let Some(e) = ai.endpoint_mut(p) {
        e.url = url.to_owned();
    }
    ai
}

fn preset(ai: &AiSettings) -> ModelPreset {
    ai.model(TaskId::Explain).cloned().unwrap()
}

#[test]
fn a_local_server_answers_without_keys() {
    let models = r#"{"data":[{"id":"llama3.2"}]}"#;
    let pieces = [&QUICK[..30], &QUICK[30..]];
    let (_rt, server) = server(vec![Answer::stream(&pieces), Answer::json(200, models)]);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let explainer = a.configure(&settings(Provider::Ollama, &server.url));
    let (got, seen) = explain(explainer.as_ref());
    assert_eq!(got.map(|i| i.meaning).as_deref(), Ok("Fast."));
    assert!(!seen.is_empty(), "partials reach the card");
    assert_eq!(server.seen()[0].auth, None);
    a.send(Job::Models(Provider::Ollama));
    wait(&mut a, |b| b.models(Provider::Ollama).ready().is_some());
    let listed = a.board().models(Provider::Ollama).ready().cloned();
    assert_eq!(listed, Some(vec!["llama3.2".to_owned()]));
}

#[test]
fn keys_are_added_used_banned_recovered_and_removed() {
    let (_rt, server) = server(vec![Answer::stream(&[QUICK])]);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let p = Provider::Custom;
    let explainer = a.configure(&settings(p, &server.url));
    let keys = |b: &Board| b.keys(p).ready().cloned().unwrap_or_default();

    a.send(Job::AddKey(p, "  sk-test-0123456789  ".into()));
    wait(&mut a, |b| keys(b).len() == 1);
    let row = keys(a.board())[0].clone();
    assert!(!row.shown.contains("0123456789"), "{}", row.shown);
    assert!(dir.path().join("master.key").exists(), "no keychain asked");
    let (got, _) = explain(explainer.as_ref());
    assert!(got.is_ok(), "{got:?}");
    let auth = server.seen().last().and_then(|s| s.auth.clone());
    assert_eq!(auth.as_deref(), Some("Bearer sk-test-0123456789"));

    a.send(Job::BanKey(p, row.id.clone()));
    wait(&mut a, |b| {
        matches!(
            keys(b).first().map(|r| &r.state),
            Some(KeyState::Banned { .. })
        )
    });
    assert_eq!(explain(explainer.as_ref()).0.err(), Some(Why::KeysRejected));
    a.send(Job::RecoverKey(p, row.id.clone()));
    wait(&mut a, |b| {
        keys(b).first().map(|r| &r.state) == Some(&KeyState::Ready)
    });

    a.send(Job::AddKey(p, "two words".into()));
    wait(&mut a, |b| b.problems.contains_key(&p));
    a.send(Job::RemoveKey(p, row.id));
    wait(&mut a, |b| b.keys(p).ready().is_some_and(Vec::is_empty));
    // With no keys left, a custom server is asked without one.
    let (got, _) = explain(explainer.as_ref());
    assert!(got.is_ok(), "{got:?}");
    assert_eq!(server.seen().last().and_then(|s| s.auth.clone()), None);
}

#[test]
fn providers_that_cannot_answer_say_why() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let (_rt, server) = server(vec![Answer::stream(&[QUICK])]);
    let openai = a.configure(&settings(Provider::OpenAi, &server.url));
    assert_eq!(explain(openai.as_ref()).0.err(), Some(Why::NoKey));
    assert!(server.seen().is_empty(), "nothing is sent without a key");
    let mut blank = settings(Provider::Custom, "");
    blank.custom.url = "localhost:1234".into();
    let custom = a.configure(&blank);
    assert_eq!(explain(custom.as_ref()).0.err(), Some(Why::Setup));
    // A deleted preset leaves Explain with no model.
    let mut unset = settings(Provider::Ollama, "");
    unset.tasks.choose(TaskId::Explain, None);
    let unset = a.configure(&unset);
    assert_eq!(explain(unset.as_ref()).0.err(), Some(Why::NotHere));
    if !cfg!(target_os = "macos") {
        let apple = a.configure(&settings(Provider::Apple, ""));
        assert_eq!(explain(apple.as_ref()).0.err(), Some(Why::NotHere));
        a.send(Job::Probe);
        wait(&mut a, |b| matches!(b.apple, Load::Ready(_)));
    }
    let openai = preset(&settings(Provider::OpenAi, &server.url));
    a.send(Job::Try(openai.clone()));
    wait(&mut a, |b| matches!(b.tried(openai.id), Load::Failed(_)));
}

/// Explains on a thread of its own, as the word card does.
fn explain_apart(e: &Arc<dyn Explainer>) -> std::thread::JoinHandle<Result<WordInfo, Why>> {
    let e = Arc::clone(e);
    std::thread::spawn(move || explain(e.as_ref()).0)
}

fn until(done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(10), "never happened");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn explain_waits_for_a_held_model_and_more_room_lets_it_in() {
    let (_rt, server) = server(vec![Answer::stream(&[QUICK])]);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let mut ai = settings(Provider::Ollama, &server.url);
    let model = preset(&ai);
    let organizing = a.slots().try_take(&model, TaskId::Organize);
    assert!(organizing.is_some());
    let explainer = a.configure(&ai);
    let asked = explain_apart(&explainer);
    until(|| a.slots().queued(model.id) == 1);
    assert!(server.seen().is_empty(), "nothing is sent while busy");
    let told = a.queued();
    assert_eq!(told.len(), 1, "the reader is told once");
    assert!(told[0].said().contains("is organizing your library"));
    drop(organizing);
    assert!(asked.join().unwrap().is_ok(), "it runs once freed");
    let organizing = a.slots().try_take(&model, TaskId::Organize);
    if let Some(p) = ai.preset_mut(model.id) {
        p.at_once = 2;
    }
    let explainer = a.configure(&ai);
    let (got, _) = explain(explainer.as_ref());
    assert!(got.is_ok(), "{got:?}");
    assert_eq!(a.in_use(model.id), 1, "explain gave its place back");
    drop(organizing);
    assert_eq!(a.in_use(model.id), 0);
}

#[test]
fn an_explain_click_during_deck_writing_is_answered_next() {
    let (_rt, server) = server(vec![Answer::stream(&[QUICK])]);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let ai = settings(Provider::Ollama, &server.url);
    let model = preset(&ai);
    let explainer = a.configure(&ai);
    // A deck being written holds the model; another deck waits.
    let writing = a.slots().try_take(&model, TaskId::StudyCards);
    assert!(writing.is_some());
    let word = Wanted {
        key: Key::new("arcane", None),
        form: "arcane".to_owned(),
        sentence: "An arcane rule.".to_owned(),
    };
    let next_deck = a.cards().write(vec![word]);
    until(|| a.slots().queued(model.id) == 1);
    let asked = explain_apart(&explainer);
    until(|| a.slots().queued(model.id) == 2);
    drop(writing);
    assert!(asked.join().unwrap().is_ok(), "answered, not refused");
    let first = server.seen()[0].body.to_string();
    assert!(
        first.contains("The quick fox"),
        "explain went first: {first}"
    );
    drop(next_deck);
}

#[test]
fn a_chosen_language_reaches_the_prompt_and_the_card_names_the_model() {
    let (_rt, server) = server(vec![Answer::stream(&[QUICK])]);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let mut ai = settings(Provider::Ollama, &server.url);
    ai.tasks.explain.language = Some("my".into());
    let explainer = a.configure(&ai);
    assert_eq!(explainer.by(), "Ollama · m");
    assert!(explain(explainer.as_ref()).0.is_ok());
    let body = server.seen()[0].body.to_string();
    assert!(body.contains("Write meaning in Burmese"), "{body}");
}

fn books(titles: &[&str]) -> Request {
    let loose = |(i, t): (usize, &&str)| Loose {
        key: i as u128 + 1,
        title: (*t).to_owned(),
        author: String::new(),
    };
    Request {
        books: titles.iter().enumerate().map(loose).collect(),
        shelves: Vec::new(),
        batch: 50,
        whole: true,
    }
}

/// Polls `run` to its end, or fails after ten seconds; partials kept.
fn finish(run: &mut Organizing) -> (Vec<Organized>, Result<Plan, Why>) {
    let start = Instant::now();
    let mut seen = Vec::new();
    loop {
        for event in run.poll() {
            match event {
                Organized::Done(plan) => return (seen, plan.map_err(|f| f.why)),
                Organized::Partial(_) => seen.push(event),
                _ => {}
            }
        }
        assert!(start.elapsed() < Duration::from_secs(10), "{seen:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_chatty_model_organizes_books_and_holds_its_model_meanwhile() {
    // Chat-only models wrap JSON in prose and fences; it's read anyway.
    let shelving = "Sure!\n```json\n{\"shelves\": [\"Poetry\"]}\n```";
    let filing = r#"{"placements":[{"book":"b1","shelf":1},{"book":"b2","shelf":0}]}"#;
    let answers = vec![Answer::stream(&[shelving]), Answer::stream(&[filing])];
    let (_rt, server) = server(answers);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let ai = settings(Provider::Ollama, &server.url);
    let id = preset(&ai).id;
    let _explainer = a.configure(&ai);
    let mut run = a.organize(books(&["Odes", "Taxes"]));
    let (progress, plan) = finish(&mut run);
    assert_eq!(progress.len(), 3, "a start and one step per request");
    let plan = plan.unwrap();
    assert_eq!(plan.shelves.len(), 1);
    assert_eq!(plan.placed, [(1, Spot::New(0))]);
    assert_eq!(plan.left, [2]);
    assert_eq!(server.seen().len(), 2);
    assert_eq!(a.in_use(id), 0, "the model is free again");
}

#[test]
fn organizing_waits_for_a_free_model_and_cancelling_frees_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    // A server that takes the request and never answers.
    let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/", silent.local_addr().unwrap());
    let ai = settings(Provider::Ollama, &url);
    let model = preset(&ai);
    let _explainer = a.configure(&ai);
    let explaining = a.slots().try_take(&model, TaskId::Explain);
    let mut run = a.organize(books(&["Odes"]));
    until(|| a.slots().queued(model.id) == 1);
    let queued = Organized::Queued { behind: 1 };
    let start = Instant::now();
    while !run.poll().contains(&queued) {
        assert!(start.elapsed() < Duration::from_secs(10), "told it waits");
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(explaining);
    until(|| a.in_use(model.id) == 1);
    run.cancel();
    until(|| a.in_use(model.id) == 0);
    // With nothing chosen, it says so.
    let mut none = ai.clone();
    none.tasks.choose(TaskId::Explain, None);
    let _ = a.configure(&none);
    let (_, plan) = finish(&mut a.organize(books(&["Odes"])));
    assert_eq!(plan.err(), Some(Why::NotHere));
}

#[test]
fn a_failed_run_is_kept_and_resumed_where_it_stopped() {
    let shelving = r#"{"shelves": ["Poetry"]}"#;
    let refused = Answer::json(400, r#"{"error": {"message": "no"}}"#);
    let answers = vec![
        Answer::stream(&[shelving]),
        Answer::stream(&[shelving]),
        refused,
    ];
    let (rt, server) = server(answers);
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(&dir);
    let _explainer = a.configure(&settings(Provider::Ollama, &server.url));
    let mut req = books(&["Odes", "Hymns"]);
    req.batch = 1;
    let (_, plan) = finish(&mut a.organize(req.clone()));
    assert!(plan.is_err());
    let paused = a.paused().expect("kept");
    assert_eq!((paused.shelved, paused.filing), (2, 0));
    // Undo of a discard brings it back.
    assert!(a.set_aside());
    assert!(a.paused().is_none());
    assert!(a.bring_back());
    // The server mends; only the filing is asked.
    let filing = r#"{"placements":[{"book":"b1","shelf":1}]}"#;
    let mended = rt.block_on(Server::start(vec![Answer::stream(&[filing])]));
    let _explainer = a.configure(&settings(Provider::Ollama, &mended.url));
    let (progress, plan) = finish(&mut a.resume(req.clone()));
    let plan = plan.unwrap();
    assert_eq!(plan.placed.len(), 2);
    assert_eq!(plan.shelves.len(), 1);
    assert_eq!(mended.seen().len(), 2);
    let start = Organized::Partial(Progress {
        stage: Stage::Filing,
        done: 2,
        total: 4,
    });
    assert_eq!(progress.first(), Some(&start));
    // The plan stays until it's applied or discarded.
    assert!(a.paused().is_some_and(|p| p.finished()));
    a.forget();
    assert!(a.paused().is_none());
}

#[cfg(unix)]
mod apple {
    use std::os::unix::fs::PermissionsExt;

    use fm_bridge::Bridge;
    use mareader_assist::{Apple, AppleState};

    use super::*;

    /// A stand-in for the Swift helper: probes ready, streams `quick`.
    const HELPER: &str = r#"#!/usr/bin/env bash
if [ "${1:-}" = "--probe" ]; then
  printf '%s\n' '{"ready":{"available":true,"contextSize":4096,"supportedLanguages":["en-US"]}}'
  exit 0
fi
read -r _request || true
printf '%s\n' '{"snapshot":{"pos":"adjective"}}'
printf '%s\n' '{"snapshot":{"pos":"adjective","meaning":"Fa"}}'
printf '%s\n' '{"structured":{"pos":"adjective","meaning":"Fast.","synonyms":["swift","rapid"],"usages":["Be quick."]}}'
printf '%s\n' '{"done":true,"usage":{"promptTokens":9,"completionTokens":9}}'
"#;

    #[test]
    fn apple_intelligence_streams_through_the_helper() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("FMBridge");
        std::fs::write(&path, HELPER).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut a = assist(&dir).with_apple(Apple::new(Bridge::new(&path)));
        let explainer = a.configure(&settings(Provider::Apple, ""));
        let (got, seen) = explain(explainer.as_ref());
        assert_eq!(got.map(|i| i.synonyms.len()), Ok(2));
        let meanings: Vec<&str> = seen.iter().map(|i| i.meaning.as_str()).collect();
        assert_eq!(meanings, ["", "Fa"]);
        a.send(Job::Probe);
        let english = AppleState::Ready(vec!["en-US".into()]);
        wait(&mut a, |b| b.apple == Load::Ready(english.clone()));
    }
}
