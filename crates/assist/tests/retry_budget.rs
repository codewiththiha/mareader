//! What a server that keeps failing costs: the bounded number of requests
//! written down in `docs/assist.md`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use mareader_assist::{Assist, Loose, Organized, Request, WAITED};
use mareader_settings::{AiSettings, Provider};
use mareader_study::Why;
use support::{Answer, Server};

/// A loopback server giving `status` to everything, on its own runtime.
fn server(status: u16) -> (tokio::runtime::Runtime, Server) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let server = rt.block_on(Server::start(vec![Answer::json(status, "{}")]));
    (rt, server)
}

/// The service, every task on model `m` of a keyless server at `url`.
fn assist(dir: &tempfile::TempDir, url: &str) -> (Assist, AiSettings) {
    let assist = Assist::new(dir.path().to_owned(), Arc::new(|| {})).unwrap();
    let mut ai = AiSettings {
        presets: Vec::new(),
        ..AiSettings::default()
    };
    let id = ai.add_preset(Provider::Ollama, "m");
    ai.tasks.explain.preset = Some(id);
    if let Some(e) = ai.endpoint_mut(Provider::Ollama) {
        e.url = url.to_owned();
    }
    (assist.without_keychain(), ai)
}

/// Chat requests that reached `server`.
fn chats(server: &Server) -> usize {
    let seen = server.seen();
    seen.iter().filter(|s| s.line.contains("chat")).count()
}

fn explain(a: &mut Assist, ai: &AiSettings) -> Option<Why> {
    let explainer = a.configure(ai);
    let got = explainer.explain("quick", "The quick fox", &|_| {});
    got.err().map(|f| f.why)
}

#[test]
fn a_failing_server_is_asked_four_times_by_explain() {
    let (_rt, server) = server(500);
    let dir = tempfile::tempdir().unwrap();
    let (mut a, ai) = assist(&dir, &server.url);
    assert_eq!(explain(&mut a, &ai), Some(Why::Busy));
    assert_eq!(chats(&server), 4, "ai-pool's 1 + 3, not waited on again");
}

#[test]
fn a_failing_server_is_asked_four_times_by_the_organizer() {
    let (_rt, server) = server(500);
    let dir = tempfile::tempdir().unwrap();
    let (mut a, ai) = assist(&dir, &server.url);
    let _explainer = a.configure(&ai);
    let loose = |key| Loose {
        key,
        title: format!("Book {key}"),
        author: String::new(),
    };
    let request = Request {
        books: (1..=2).map(loose).collect(),
        shelves: Vec::new(),
        batch: 50,
        whole: true,
    };
    let mut run = a.organize(request);
    let start = Instant::now();
    let plan = loop {
        let done = run.poll().into_iter().find_map(|event| match event {
            Organized::Done(plan) => Some(plan),
            Organized::Partial(_) => None,
        });
        if let Some(plan) = done {
            break plan;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "no end");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(plan.err().map(|f| f.why), Some(Why::Busy));
    assert_eq!(chats(&server), 4, "not (1 + 3) × (1 + 2) = 12");
}

#[test]
fn a_busy_server_is_asked_twice_by_explain() {
    let (_rt, server) = server(429);
    let dir = tempfile::tempdir().unwrap();
    let (mut a, ai) = assist(&dir, &server.url);
    let start = Instant::now();
    assert_eq!(explain(&mut a, &ai), Some(Why::Busy));
    assert_eq!(chats(&server), 2, "one wait, then it gives up");
    let waited = start.elapsed();
    assert!(waited >= WAITED.waits[0], "{waited:?}");
}
