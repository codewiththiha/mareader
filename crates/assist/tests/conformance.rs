//! Every AI task, held to the same contract: a spec, a route with its
//! fallback, a busy model waited out as its spec says, a cancel that frees
//! its model, and settings that reach it. A new task that forgets a piece
//! fails here, not in a reader's hands.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod support;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use mareader_assist::{Assist, Loose, PageJob, Request, Rewrite, Running};
use mareader_settings::{AiSettings, PresetId, Provider, SummaryStyle, TaskId};
use mareader_study::Why;
use mareader_study::mode::{Attempt, Key, Wanted};
use mareader_study::page::{Piece, Role};
use mareader_study::pending::{Answerers, Pending};
use support::{Answer, QUICK, Server};

const BACKS: &str =
    r#"{"cards":[{"n":1,"meaning":"Hidden.","synonyms":[],"usage":"An arcane rule."}]}"#;
const VERDICT: &str = r#"{"verdict":"close","feedback":"Nearly: here it means roamed."}"#;
const SHELVING: &str = r#"{"shelves":["Poetry"]}"#;
const FILING: &str = r#"{"placements":[{"book":"b1","shelf":1},{"book":"b2","shelf":0}]}"#;
const SUMMARY: &str = r#"{"paragraphs":["The fox ran home before the rain."]}"#;
const PIECES: &str = r#"{"pieces":[{"n":1,"text":"The fox went home."}]}"#;

/// What `id` answers with when all goes well, request by request.
fn answers(id: TaskId) -> Vec<&'static str> {
    match id {
        TaskId::Explain => vec![QUICK],
        TaskId::Organize => vec![SHELVING, FILING],
        TaskId::Summarize => vec![SUMMARY],
        TaskId::Translate | TaskId::Simplify => vec![PIECES],
        TaskId::StudyCards => vec![BACKS],
        TaskId::StudyJudge => vec![VERDICT],
    }
}

/// A job's end, once it came: polled, never waited on.
type Poll = Box<dyn FnMut() -> Option<Result<(), Why>>>;

fn ended<O: 'static, P: 'static>(mut job: Box<dyn Pending<O, P>>) -> Poll {
    Box::new(move || job.take().map(|done| done.map(drop).map_err(|f| f.why)))
}

fn run<O: Send + 'static, P: Send + 'static>(job: Running<O, P>) -> Poll {
    ended(Box::new(job))
}

fn page(rewrite: Rewrite) -> PageJob {
    PageJob {
        rewrite,
        pieces: vec![Piece::text(Role::Para, "The fox ran home.")],
        pages: 1,
    }
}

/// Starts one job of `id`; the study tools ask through `ports`.
///
/// Explain is a blocking call on a worker thread, as the word card makes.
/// Dropping its poll leaves the thread to finish; nothing cancels it.
fn start(a: &Assist, ports: &Answerers, id: TaskId) -> Poll {
    match id {
        TaskId::Explain => {
            let (explainer, (tx, rx)) = (Arc::clone(&ports.explainer), channel());
            std::thread::spawn(move || {
                let got = explainer.explain("quick", "The quick fox", &|_| {});
                let _ = tx.send(got.map(drop).map_err(|f| f.why));
            });
            polled(rx)
        }
        TaskId::Organize => run(a.organize(books())),
        TaskId::Summarize => {
            let summary = Rewrite::Summarize(SummaryStyle::Paragraphs);
            run(a.rewrite(page(summary)))
        }
        TaskId::Translate => {
            let french = Rewrite::Translate("French".to_owned());
            run(a.rewrite(page(french)))
        }
        TaskId::Simplify => run(a.rewrite(page(Rewrite::Simplify))),
        TaskId::StudyCards => ended(ports.cards.write(vec![wanted()])),
        TaskId::StudyJudge => ended(ports.judge.judge(attempt())),
    }
}

fn polled(rx: Receiver<Result<(), Why>>) -> Poll {
    Box::new(move || rx.try_recv().ok())
}

fn books() -> Request {
    let loose = |(key, title): (u128, &str)| Loose {
        key,
        title: title.to_owned(),
        author: String::new(),
    };
    Request {
        books: [(1, "Odes"), (2, "Taxes")].into_iter().map(loose).collect(),
        shelves: Vec::new(),
        batch: 50,
        whole: true,
    }
}

fn wanted() -> Wanted {
    Wanted {
        key: Key::new("arcane", None),
        form: "arcane".to_owned(),
        sentence: "An arcane rule.".to_owned(),
    }
}

fn attempt() -> Attempt {
    Attempt {
        form: "wandered".to_owned(),
        lemma: "wander".to_owned(),
        sentence: "She wandered the hills.".to_owned(),
        answer: "walked".to_owned(),
    }
}

/// Polls `job` to its end, or fails after thirty seconds.
fn finish(job: &mut Poll, id: TaskId) -> Result<(), Why> {
    let (start, limit) = (Instant::now(), Duration::from_secs(30));
    loop {
        if let Some(done) = job() {
            return done;
        }
        assert!(start.elapsed() < limit, "{id:?} never ended");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn until(id: TaskId, what: &str, done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(10), "{id:?}: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Settings with no saved model; `bare().add_preset` adds them.
fn bare() -> AiSettings {
    let mut ai = AiSettings {
        presets: Vec::new(),
        ..AiSettings::default()
    };
    ai.tasks.models.clear();
    ai
}

/// One keyless model at `url` explaining words; every task follows it.
fn settings(url: &str) -> (AiSettings, PresetId) {
    let mut ai = bare();
    let id = ai.add_preset(Provider::Ollama, "m");
    ai.tasks.choose(TaskId::Explain, Some(id));
    if let Some(e) = ai.endpoint_mut(Provider::Ollama) {
        e.url = url.to_owned();
    }
    (ai, id)
}

/// Chat requests that reached `server`.
fn chats(server: &Server) -> usize {
    let seen = server.seen();
    seen.iter().filter(|s| s.line.contains("chat")).count()
}

/// A loopback server giving `answers` in turn, on its own runtime.
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
    let a = Assist::new(dir.path().to_owned(), Arc::new(|| {})).unwrap();
    a.without_keychain()
}

/// Runs `check` for every task at once, each on a thread of its own.
fn each_task(check: fn(TaskId)) {
    let threads: Vec<_> = TaskId::ALL
        .into_iter()
        .map(|id| (id, std::thread::spawn(move || check(id))))
        .collect();
    for (id, thread) in threads {
        assert!(thread.join().is_ok(), "{id:?} broke the contract");
    }
}

#[test]
fn every_task_has_a_spec_and_its_follows_end() {
    for id in TaskId::ALL {
        let spec = id.spec();
        assert!(!spec.label.is_empty() && !spec.doing.is_empty(), "{id:?}");
        // Following `follows` reaches a task that follows none.
        let mut at = id;
        for _ in 0..TaskId::ALL.len() {
            match at.spec().follows {
                Some(next) => at = next,
                None => break,
            }
        }
        assert_eq!(at.spec().follows, None, "{id:?} follows in a loop");
    }
}

#[test]
fn every_task_routes_through_its_fallback_and_loses_a_removed_model() {
    for id in TaskId::ALL {
        let (mut ai, first) = settings("http://127.0.0.1:9/v1/");
        let b = ai.add_preset(Provider::Ollama, "b");
        let c = ai.add_preset(Provider::Ollama, "c");
        ai.tasks.models.insert(id, vec![b, c]);
        let route: Vec<PresetId> = ai.route(id).iter().map(|p| p.id).collect();
        assert_eq!(route[..2], [b, c], "{id:?}: its own, the fallback second");
        let (has, follows) = (route.contains(&first), id.spec().follows.is_some());
        assert_eq!(has, follows, "{id:?}: then those it follows");
        ai.remove_preset(b);
        assert_eq!(ai.tasks.choice(id), Some(c), "{id:?}: fallback first");
        ai.remove_preset(c);
        assert_eq!(ai.tasks.choice(id), None, "{id:?}: unassigned");
        let left: Vec<PresetId> = ai.route(id).iter().map(|p| p.id).collect();
        assert!(!left.contains(&b) && !left.contains(&c), "{id:?}: {left:?}");
    }
}

#[test]
fn every_task_waits_out_a_busy_model_as_its_spec_says() {
    each_task(|id| {
        let mut script = vec![Answer::json(429, "{}")];
        script.extend(answers(id).into_iter().map(|a| Answer::stream(&[a])));
        let (_rt, server) = server(script);
        let dir = tempfile::tempdir().unwrap();
        let mut a = assist(&dir);
        let (ai, model) = settings(&server.url);
        let _ = a.configure(&ai);
        let started = Instant::now();
        let mut job = start(&a, &a.answerers(), id);
        assert_eq!(finish(&mut job, id), Ok(()), "{id:?}");
        let waited = started.elapsed();
        let wait = id.spec().retry.waits[0];
        assert!(waited >= wait, "{id:?} waited {waited:?}, not {wait:?}");
        let asked = chats(&server);
        assert_eq!(asked, 1 + answers(id).len(), "{id:?}: busy, then asked");
        let free = || a.slots().in_use(model) == 0;
        until(id, "the answer frees its model", free);
    });
}

#[test]
fn cancelling_any_task_frees_its_model() {
    each_task(|id| {
        // A server that takes the request and never answers.
        let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/", silent.local_addr().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let mut a = assist(&dir);
        let (ai, model) = settings(&url);
        let _ = a.configure(&ai);
        if id == TaskId::Explain {
            // A blocking call, with no handle; its answer frees the model.
            return;
        }
        let job = start(&a, &a.answerers(), id);
        let held = |n| a.slots().in_use(model) == n;
        until(id, "it holds its model", || held(1));
        drop(job);
        until(id, "cancelling frees it", || held(0));
    });
}

#[test]
fn a_settings_change_reaches_every_task_without_new_ports() {
    each_task(|id| {
        let (_old_rt, old) = server(vec![Answer::json(500, "{}")]);
        let script = answers(id).into_iter().map(|a| Answer::stream(&[a]));
        let (_rt, fresh) = server(script.collect());
        let dir = tempfile::tempdir().unwrap();
        let mut a = assist(&dir);
        let (ai, model) = settings(&old.url);
        let _ = a.configure(&ai);
        // The study tools' ports, made once, before the change.
        let ports = a.answerers();
        let (mut ai, _) = settings(&fresh.url);
        if let Some(p) = ai.preset_mut(model) {
            p.name = "Fresh".to_owned();
        }
        let _ = a.configure(&ai);
        let mut job = start(&a, &ports, id);
        assert_eq!(finish(&mut job, id), Ok(()), "{id:?}");
        assert_eq!(chats(&old), 0, "{id:?} asked the old model");
        assert!(chats(&fresh) > 0, "{id:?} never asked the new one");
        let by = match id {
            TaskId::Explain => ports.explainer.by(),
            TaskId::StudyCards => ports.cards.by(),
            TaskId::StudyJudge => ports.judge.by(),
            _ => "Fresh".to_owned(),
        };
        assert_eq!(by, "Fresh", "{id:?} names the model it asks now");
    });
}
