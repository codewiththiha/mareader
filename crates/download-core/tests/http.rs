//! One real socket, one real body: the endings the transport can have.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use download_core::{Downloads, Host, Job, Outcome, Phase, Progress};

/// One chunk the body is written in; a drip test needs many of them.
const CHUNK: usize = 8 * 1024;

/// What the socket answers one request with.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Mode {
    /// The whole body, at once.
    Serve,
    /// The whole body, one chunk per tick: enough boundaries to pause at.
    Drip,
    /// This many bytes, then the socket closes with the length unmet.
    Truncate(usize),
    /// This status and no body.
    Status(u16),
}

/// The socket's script and what a test reads back from it.
struct State {
    /// One entry per hit; the last one repeats.
    modes: Vec<Mode>,
    /// Which body the socket serves; a test moves it to change the resource.
    revision: Mutex<usize>,
    /// Replace the resource after this hit, so a held validator goes stale.
    bump_after: usize,
    /// The `Range` start each hit was asked for: proof a resume really resumed.
    ranges: Mutex<Vec<Option<usize>>>,
    hits: AtomicUsize,
    bodies: Vec<Vec<u8>>,
}

impl State {
    fn mode(&self, hit: usize) -> Mode {
        self.modes[hit.min(self.modes.len() - 1)]
    }

    fn body(&self) -> Vec<u8> {
        let revision = *self.revision.lock().unwrap();
        self.bodies[revision.min(self.bodies.len() - 1)].clone()
    }

    fn etag(&self) -> String {
        format!("\"v{}\"", self.revision.lock().unwrap())
    }

    fn bump_revision(&self) {
        *self.revision.lock().unwrap() += 1;
    }

    fn served(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    /// The range each hit was asked for, in order.
    fn ranges(&self) -> Vec<Option<usize>> {
        self.ranges.lock().unwrap().clone()
    }
}

/// A listening socket on `127.0.0.1`, serving `script` from its own thread.
struct Server {
    origin: String,
    state: Arc<State>,
}

impl Server {
    fn start(modes: Vec<Mode>, bodies: Vec<Vec<u8>>) -> Self {
        Self::scripted(modes, bodies, usize::MAX)
    }

    /// A server that replaces its resource after hit `bump_after`.
    fn changing_after(modes: Vec<Mode>, bodies: Vec<Vec<u8>>, bump_after: usize) -> Self {
        Self::scripted(modes, bodies, bump_after)
    }

    fn scripted(modes: Vec<Mode>, bodies: Vec<Vec<u8>>, bump_after: usize) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(State {
            modes,
            revision: Mutex::new(0),
            bump_after,
            ranges: Mutex::new(Vec::new()),
            hits: AtomicUsize::new(0),
            bodies,
        });
        let served = Arc::clone(&state);
        thread::spawn(move || accept(listener, served));
        Self {
            origin: format!("http://127.0.0.1:{port}"),
            state,
        }
    }

    fn url(&self, name: &str) -> String {
        format!("{}/{name}", self.origin)
    }
}

fn accept(listener: TcpListener, state: Arc<State>) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let Ok(head) = read_head(&mut stream) else {
            continue;
        };
        let hit = state.hits.fetch_add(1, Ordering::SeqCst);
        state.ranges.lock().unwrap().push(range_start(&head));
        respond(&mut stream, &state, state.mode(hit), &head);
        if hit == state.bump_after {
            state.bump_revision();
        }
    }
}

/// The request head, up to and including the blank line.
fn read_head(stream: &mut impl Read) -> std::io::Result<String> {
    let mut head = String::new();
    let mut byte = [0u8; 1];
    while !head.ends_with("\r\n\r\n") {
        match stream.read(&mut byte)? {
            0 => break,
            _ => head.push(byte[0] as char),
        }
        // A test never sends a body; a runaway read is a bug, not a wait.
        if head.len() > 16 * 1024 {
            break;
        }
    }
    Ok(head)
}

/// The `Range: bytes=N-` start, when the request carries one.
fn range_start(head: &str) -> Option<usize> {
    let line = header_line(head, "range")?;
    let value = line.strip_prefix("bytes=")?;
    value.split('-').next()?.trim().parse().ok()
}

/// The `If-Range` validator, when the request carries one.
fn if_range(head: &str) -> Option<String> {
    header_line(head, "if-range")
}

fn header_line(head: &str, name: &str) -> Option<String> {
    head.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.eq_ignore_ascii_case(name)).then(|| value.trim().to_string())
    })
}

fn respond(stream: &mut impl Write, state: &State, mode: Mode, head: &str) {
    if let Mode::Status(code) = mode {
        let _ = write!(
            stream,
            "HTTP/1.1 {code} Whatever\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        return;
    }
    let body = state.body();
    let etag = state.etag();
    // A range is honoured only against the validator the client holds.
    let resumable = match (range_start(head), if_range(head)) {
        (Some(_), Some(held)) => held == etag,
        (Some(_), None) => false,
        (None, _) => false,
    };
    let from = range_start(head).filter(|_| resumable).unwrap_or(0);
    if from >= body.len() {
        let _ = write!(
            stream,
            "HTTP/1.1 416 Bad\r\nContent-Range: bytes */{}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            body.len()
        );
        return;
    }
    let rest = &body[from..];
    let status = if resumable {
        format!(
            "206 Partial Content\r\nContent-Range: bytes {}-{}/{}",
            from,
            body.len() - 1,
            body.len()
        )
    } else {
        "200 OK".to_string()
    };
    let declared = if matches!(mode, Mode::Truncate(_)) {
        body.len()
    } else {
        rest.len()
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nETag: {etag}\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let cut = match mode {
        Mode::Truncate(bytes) => bytes.min(rest.len()),
        _ => rest.len(),
    };
    let mut sent = 0;
    while sent < cut {
        let end = (sent + CHUNK).min(cut);
        if stream.write_all(&rest[sent..end]).is_err() {
            return;
        }
        let _ = stream.flush();
        sent = end;
        if mode == Mode::Drip {
            thread::sleep(Duration::from_millis(20));
        }
    }
    // A truncated body closes short: that is the fault being reproduced.
}

/// A host that keeps every snapshot and runs tasks on the test's runtime.
#[derive(Clone, Default)]
struct TestHost {
    seen: Arc<Mutex<Vec<Progress>>>,
}

impl Host for TestHost {
    fn spawn(&self, task: impl std::future::Future<Output = ()> + Send + 'static) {
        tokio::spawn(task);
    }

    fn publish(&self, progress: &Progress) {
        self.seen.lock().unwrap().push(progress.clone());
    }
}

impl TestHost {
    fn phases(&self) -> Vec<Phase> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .map(|progress| progress.phase)
            .collect()
    }
}

/// The sidecar a run that wrote every byte and died before adopting leaves.
fn stage_complete_partial(dir: &std::path::Path, name: &str, body: &[u8]) {
    std::fs::write(dir.join(format!("{name}.part")), body).unwrap();
    std::fs::write(
        dir.join(format!("{name}.dmeta")),
        format!(
            "{{\"etag\":\"\\\"v0\\\"\",\"last_modified\":null,\"source\":0,\"total\":{}}}",
            body.len()
        ),
    )
    .unwrap();
}

/// A body of `bytes`, patterned so a shifted resume cannot look complete.
fn body(bytes: usize) -> Vec<u8> {
    (0..bytes)
        .map(|index| (index.wrapping_mul(31) % 251) as u8)
        .collect()
}

fn dir(tag: &str) -> PathBuf {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dl_http_{tag}_{}_{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Poll `registry` until `ready` answers or the budget runs out.
fn wait_for(registry: &Downloads, id: &str, ready: impl Fn(&Progress) -> bool) -> Option<Progress> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(progress) = registry.status(id)
            && ready(&progress)
        {
            return Some(progress);
        }
        thread::sleep(Duration::from_millis(5));
    }
    registry.status(id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_whole_body_is_verified_adopted_and_cleaned_up() {
    let server = Server::start(vec![Mode::Serve], vec![body(64 * 1024)]);
    let dir = dir("whole");
    let registry = Downloads::new();
    let host = TestHost::default();
    let expected = server.state.body();
    let wanted = expected.clone();

    let job = Job::new("whole", &dir, server.url("data.bin")).verified_by(move |path| {
        let held = std::fs::read(path).unwrap_or_default();
        (held == wanted)
            .then_some(())
            .ok_or_else(|| "the body is not the one that was served".to_string())
    });
    let outcome = registry
        .start(&host, job)
        .unwrap()
        .finished()
        .await
        .unwrap();

    assert!(!outcome.cached);
    assert_eq!(outcome.bytes as usize, expected.len());
    assert_eq!(outcome.path, dir.join("data.bin"));
    assert_eq!(std::fs::read(&outcome.path).unwrap(), expected);
    // Neither the partial nor its sidecar outlives the adoption.
    assert!(!dir.join("data.bin.part").exists());
    assert!(!dir.join("data.bin.dmeta").exists());
    let last = registry.status("whole").unwrap();
    assert_eq!(last.phase, Phase::Done);
    assert_eq!(last.percent(), Some(100));
    assert_eq!(last.path.as_deref(), Some(outcome.path.as_path()));
    assert!(host.phases().contains(&Phase::Verifying));
    assert_eq!(registry.outcome("whole"), Some(Ok(outcome)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_short_body_resumes_from_what_landed() {
    // First hit: 40 KiB of a 128 KiB body, then the socket closes short.
    let server = Server::start(
        vec![Mode::Truncate(40 * 1024), Mode::Serve],
        vec![body(128 * 1024)],
    );
    let dir = dir("resume");
    let registry = Downloads::new();
    let host = TestHost::default();
    let expected = server.state.body();

    let job = Job::new("resume", &dir, server.url("data.bin"));
    let outcome = registry
        .start(&host, job)
        .unwrap()
        .finished()
        .await
        .unwrap();

    assert_eq!(std::fs::read(&outcome.path).unwrap(), expected);
    // The second hit was asked for exactly the bytes the first one landed.
    assert_eq!(server.state.ranges(), vec![None, Some(40 * 1024)]);
    assert_eq!(outcome.bytes, 128 * 1024);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_resource_that_changed_restarts_instead_of_appending() {
    // The body is replaced after hit one, so the held etag goes stale.
    let server = Server::changing_after(
        vec![Mode::Truncate(16 * 1024), Mode::Serve],
        vec![body(96 * 1024), body(48 * 1024)],
        0,
    );
    let dir = dir("changed");
    let registry = Downloads::new();
    let host = TestHost::default();
    let expected = body(48 * 1024);

    let job = Job::new("changed", &dir, server.url("data.bin"));
    let outcome = registry
        .start(&host, job)
        .unwrap()
        .finished()
        .await
        .unwrap();

    assert_eq!(std::fs::read(&outcome.path).unwrap(), expected);
    // The stale validator was answered with the whole body, not a tail.
    assert_eq!(server.state.ranges(), vec![None, Some(16 * 1024)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_mirror_that_refuses_hands_the_file_to_the_next_one() {
    let server = Server::start(vec![Mode::Serve], vec![body(4096)]);
    let dir = dir("mirror");
    let registry = Downloads::new();
    let host = TestHost::default();
    let expected = server.state.body();
    // Port 9 is closed: the first mirror is spent without a byte moving.
    let job =
        Job::new("mirror", &dir, "http://127.0.0.1:9/data.bin").mirror(server.url("data.bin"));

    let outcome = registry
        .start(&host, job)
        .unwrap()
        .finished()
        .await
        .unwrap();
    assert_eq!(outcome.source, 1, "the second mirror served it");
    assert_eq!(std::fs::read(&outcome.path).unwrap(), expected);
    assert!(host.phases().contains(&Phase::Retrying));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_404_is_an_answer_not_an_outage() {
    let server = Server::start(vec![Mode::Status(404)], vec![body(1024)]);
    let dir = dir("gone");
    let registry = Downloads::new();
    let host = TestHost::default();

    let job = Job::new("gone", &dir, server.url("data.bin"));
    let started = Instant::now();
    let error = registry
        .start(&host, job)
        .unwrap()
        .finished()
        .await
        .unwrap_err();

    assert!(error.contains("404"), "{error}");
    // A permanent answer is not backed off into: one hit, no retry.
    assert_eq!(server.state.served(), 1);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(registry.status("gone").unwrap().phase, Phase::Failed);
    assert!(!dir.join("data.bin").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_busy_mirror_is_asked_again_after_a_backoff() {
    let server = Server::start(vec![Mode::Status(503), Mode::Serve], vec![body(2048)]);
    let dir = dir("busy");
    let registry = Downloads::new();
    let host = TestHost::default();
    let expected = server.state.body();

    let job = Job::new("busy", &dir, server.url("data.bin"));
    let outcome = registry
        .start(&host, job)
        .unwrap()
        .finished()
        .await
        .unwrap();

    assert_eq!(std::fs::read(&outcome.path).unwrap(), expected);
    assert_eq!(
        outcome.source, 0,
        "the same mirror answered the second time"
    );
    assert_eq!(server.state.served(), 2);
    assert!(host.phases().contains(&Phase::Retrying));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_body_that_fails_its_check_is_never_adopted() {
    let server = Server::start(vec![Mode::Serve], vec![body(4096)]);
    let dir = dir("reject");
    let registry = Downloads::new();
    let host = TestHost::default();

    let job = Job::new("reject", &dir, server.url("data.bin"))
        .verified_by(|_| Err("not a parquet".to_string()));
    let error = registry
        .start(&host, job)
        .unwrap()
        .finished()
        .await
        .unwrap_err();

    assert_eq!(error, "not a parquet");
    // Neither the destination nor the partial survives a rejected body.
    assert!(!dir.join("data.bin").exists());
    assert!(!dir.join("data.bin.part").exists());
    assert_eq!(registry.status("reject").unwrap().phase, Phase::Failed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_complete_file_needs_no_request_at_all() {
    let server = Server::start(vec![Mode::Serve], vec![body(1024)]);
    let dir = dir("cache");
    let expected = server.state.body();
    std::fs::write(dir.join("data.bin"), &expected).unwrap();

    let registry = Downloads::new();
    let host = TestHost::default();
    let job = Job::new("cache", &dir, server.url("data.bin"));
    let outcome = registry
        .start(&host, job)
        .unwrap()
        .finished()
        .await
        .unwrap();

    assert!(outcome.cached);
    assert_eq!(outcome.bytes as usize, expected.len());
    assert_eq!(server.state.served(), 0, "nothing was asked of the mirror");
    // `force` is what makes a cached file download again.
    let forced = Job::new("cache", &dir, server.url("data.bin")).force(true);
    let again: Outcome = registry
        .start(&host, forced)
        .unwrap()
        .finished()
        .await
        .unwrap();
    assert!(!again.cached);
    assert_eq!(server.state.served(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_partial_that_already_reaches_the_resource_is_adopted() {
    let server = Server::start(vec![Mode::Serve], vec![body(8192)]);
    let dir = dir("satisfied");
    let expected = server.state.body();
    // Every byte landed; the run died before the rename.
    stage_complete_partial(&dir, "data.bin", &expected);

    let registry = Downloads::new();
    let host = TestHost::default();
    let job = Job::new("satisfied", &dir, server.url("data.bin"));
    let outcome = registry
        .start(&host, job)
        .unwrap()
        .finished()
        .await
        .unwrap();

    // A range starting at the end answers 416; that is a whole file.
    assert_eq!(std::fs::read(&outcome.path).unwrap(), expected);
    assert_eq!(outcome.bytes as usize, expected.len());
    assert!(
        !outcome.cached,
        "it was the transport that proved it complete"
    );
    // One request: the 416. Wiping the partial would have refetched it.
    assert_eq!(server.state.served(), 1);
    assert_eq!(server.state.ranges(), vec![Some(8192)]);
    assert!(!dir.join("data.bin.part").exists());
    assert!(!dir.join("data.bin.dmeta").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pause_keeps_its_bytes_and_a_resume_finishes_it() {
    let server = Server::start(vec![Mode::Drip], vec![body(512 * 1024)]);
    let dir = dir("pause");
    let registry = Downloads::new();
    let host = TestHost::default();
    let expected = server.state.body();

    let job = Job::new("pause", &dir, server.url("data.bin"));
    let first = registry.start(&host, job).unwrap();
    // Wait for real bytes, then stop the read mid-body.
    let moving = wait_for(&registry, "pause", |progress| progress.received > 0).unwrap();
    assert_eq!(moving.phase, Phase::Downloading);
    registry.pause("pause");
    let paused = wait_for(&registry, "pause", |progress| {
        progress.phase == Phase::Paused
    })
    .unwrap();

    // A paused bar that reads zero looks like a lost download.
    assert_eq!(paused.phase, Phase::Paused);
    assert!(paused.received > 0, "the bytes stay reported");
    assert!(paused.received < expected.len() as u64, "not finished yet");
    assert_eq!(
        paused.percent(),
        Some((paused.received * 100 / expected.len() as u64) as u8)
    );
    let held = std::fs::metadata(dir.join("data.bin.part")).unwrap().len();
    assert_eq!(
        held, paused.received,
        "what was reported is what is on disk"
    );
    assert!(!dir.join("data.bin").exists());
    // Pausing is not an ending, so the first receipt is still waiting.
    assert!(registry.resume(&host, "pause").is_ok());
    let outcome = first.finished().await.unwrap();

    assert_eq!(std::fs::read(&outcome.path).unwrap(), expected);
    assert_eq!(outcome.bytes as usize, expected.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancel_stops_the_read_and_ends_the_receipt() {
    let server = Server::start(vec![Mode::Drip], vec![body(512 * 1024)]);
    let dir = dir("cancel");
    let registry = Downloads::new();
    let host = TestHost::default();

    let job = Job::new("cancel", &dir, server.url("data.bin"));
    let receipt = registry.start(&host, job).unwrap();
    wait_for(&registry, "cancel", |progress| progress.received > 0).unwrap();
    registry.cancel("cancel");
    let error = receipt.finished().await.unwrap_err();

    assert!(error.contains("cancelled"), "{error}");
    assert_eq!(registry.status("cancel").unwrap().phase, Phase::Cancelled);
    assert!(!dir.join("data.bin").exists());
    // The partial stays: a later start resumes from it rather than refetching.
    assert!(dir.join("data.bin.part").exists());

    let again = Job::new("cancel", &dir, server.url("data.bin"));
    let outcome = registry
        .start(&host, again)
        .unwrap()
        .finished()
        .await
        .unwrap();
    assert_eq!(outcome.bytes, 512 * 1024);
}
