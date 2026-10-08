//! The transport's behaviour, against a loopback server: resume, refusal,
//! rotation, pause, cancel, remove.

use std::future::Future;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use download_core::{DownloadRequest, Downloads, Host, Phase, Progress, ProgressHook};

/// One request the server answered, as the transport sent it.
#[derive(Clone, Debug, Default)]
struct Seen {
    range: Option<String>,
    if_range: Option<String>,
}

/// How the server answers.
#[derive(Clone)]
struct Options {
    /// Served as the `ETag`, and matched against `If-Range`.
    etag: String,
    /// Answer every request with the whole body, ignoring `Range`.
    ignore_ranges: bool,
    /// Write the body in pieces this far apart, so a transfer can be
    /// caught in flight.
    slow_ms: u64,
    /// Send this many bytes, then drop the connection.
    cut_after: Option<usize>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            etag: "\"v1\"".into(),
            ignore_ranges: false,
            slow_ms: 0,
            cut_after: None,
        }
    }
}

/// A loopback HTTP server that serves one body with byte ranges.
struct Server {
    addr: SocketAddr,
    body: Arc<Vec<u8>>,
    seen: Arc<Mutex<Vec<Seen>>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    fn start(body: Vec<u8>) -> Self {
        Self::with(body, Options::default())
    }

    fn with(body: Vec<u8>, options: Options) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let body = Arc::new(body);
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        {
            let (body, seen, stop) = (body.clone(), seen.clone(), stop.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        return;
                    }
                    let Ok(stream) = stream else { continue };
                    let _ = serve(stream, &body, &options, &seen);
                }
            });
        }
        Self {
            addr,
            body,
            seen,
            stop,
        }
    }

    fn url(&self) -> String {
        format!("http://{}/file", self.addr)
    }

    fn body(&self) -> &[u8] {
        &self.body
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The accept loop is parked in accept(); one connection ends it.
        let _ = TcpStream::connect(self.addr);
    }
}

/// One connection: read the head, answer with a range or the whole body.
fn serve(
    mut stream: TcpStream,
    body: &[u8],
    options: &Options,
    seen: &Mutex<Vec<Seen>>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut request = Seen::default();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        let (name, value) = header.split_once(':').unwrap_or((&header, ""));
        match name.to_ascii_lowercase().as_str() {
            "range" => request.range = Some(value.trim().to_string()),
            "if-range" => request.if_range = Some(value.trim().to_string()),
            _ => {}
        }
    }
    seen.lock().unwrap().push(request.clone());

    let total = body.len();
    let start = request
        .range
        .as_deref()
        .and_then(|value| value.strip_prefix("bytes="))
        .and_then(|value| value.strip_suffix('-'))
        .and_then(|value| value.parse::<usize>().ok());
    let fresh = request.if_range.as_deref() == Some(options.etag.as_str());
    let ranged = start.is_some() && !options.ignore_ranges && fresh;

    if let Some(start) = start
        && start >= total
    {
        let head = format!(
            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\n\
             Content-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(head.as_bytes())?;
        return stream.flush();
    }

    let (status, slice, extra) = match (ranged, start) {
        (true, Some(start)) => (
            "206 Partial Content",
            &body[start..],
            format!("Content-Range: bytes {start}-{}/{total}\r\n", total - 1),
        ),
        _ => ("200 OK", body, String::new()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nETag: {}\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
        options.etag,
        slice.len(),
        extra,
    );
    stream.write_all(head.as_bytes())?;

    let cut = options.cut_after.unwrap_or(usize::MAX).min(slice.len());
    for piece in slice[..cut].chunks(16 * 1024) {
        stream.write_all(piece)?;
        if options.slow_ms > 0 {
            std::thread::sleep(Duration::from_millis(options.slow_ms));
        }
    }
    stream.flush()
}

/// The snapshots one test's hook saw, and where a terminal one lands.
type Hooked = Arc<Mutex<Vec<Progress>>>;

/// A host on a scratch directory: the bus records, the hook signals.
#[derive(Clone)]
struct TestHost {
    dir: PathBuf,
    bus: Hooked,
    terminal: Arc<Mutex<Option<tokio::sync::oneshot::Sender<Progress>>>>,
}

impl Host for TestHost {
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static) {
        tokio::spawn(task);
    }

    fn publish(&self, progress: &Progress) {
        self.bus.lock().unwrap().push(progress.clone());
    }

    fn data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.dir.clone())
    }
}

/// Whether the bus recorded `phase` for any download.
fn bus_saw(bus: &Hooked, phase: Phase) -> bool {
    bus.lock().unwrap().iter().any(|p| p.phase == phase)
}

/// Whether the record for `id` has reached `phase`.
fn reached(downloads: &Downloads, id: &str, phase: Phase) -> bool {
    downloads.status(id).is_some_and(|p| p.phase == phase)
}

/// A downloader, its host, the hook the dev keeps, and the last snapshot.
fn fixture(name: &str) -> (Downloads, TestHost, ProgressHook, Hooked, Receiver) {
    let dir = std::env::temp_dir().join(format!("download_core_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let hooked: Hooked = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = tokio::sync::oneshot::channel();
    let host = TestHost {
        dir,
        bus: Arc::new(Mutex::new(Vec::new())),
        terminal: Arc::new(Mutex::new(Some(tx))),
    };
    let hook: ProgressHook = {
        let (hooked, terminal) = (hooked.clone(), host.terminal.clone());
        Arc::new(move |progress: &Progress| {
            hooked.lock().unwrap().push(progress.clone());
            if progress.phase.terminal()
                && let Some(tx) = terminal.lock().unwrap().take()
            {
                let _ = tx.send(progress.clone());
            }
        })
    };
    (Downloads::new(), host, hook, hooked, rx)
}

type Receiver = tokio::sync::oneshot::Receiver<Progress>;

fn request(file_name: &str, urls: Vec<String>) -> DownloadRequest {
    DownloadRequest {
        id: "dl".into(),
        urls,
        directory: None,
        file_name: file_name.into(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
}

/// Poll `held` until it is true, or fail the test.
async fn wait_until(held: impl Fn() -> bool) {
    for _ in 0..2_000 {
        if held() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition never held");
}

fn body(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn part_of(dir: &Path, file_name: &str) -> PathBuf {
    dir.join(format!("{file_name}.part"))
}

#[test]
fn a_fresh_download_lands_and_reports_its_path() {
    let server = Server::start(body(64 * 1024));
    runtime().block_on(async {
        let (downloads, host, hook, hooked, rx) = fixture("fresh");
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("start");
        let last = rx.await.expect("terminal snapshot");
        assert_eq!(last.phase, Phase::Done);
        assert_eq!(last.percent(), Some(100));
        let path = PathBuf::from(last.path.expect("path"));
        assert_eq!(std::fs::read(&path).unwrap(), server.body());
        // The partial and its validator are gone; the record is settled.
        assert!(!part_of(&host.dir, "file.bin").exists());
        assert!(!host.dir.join("file.bin.part.meta").exists());
        // The dev's own hook saw the same last word as the bus.
        let hooked = hooked.lock().unwrap();
        assert_eq!(hooked.last().unwrap().phase, Phase::Done);
        assert!(hooked.iter().any(|p| p.received > 0));
        assert!(bus_saw(&host.bus, Phase::Done));
    });
}

#[test]
fn a_partial_without_a_validator_is_not_trusted() {
    let server = Server::start(body(48 * 1024));
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, rx) = fixture("unproven");
        std::fs::write(part_of(&host.dir, "file.bin"), b"not the same bytes").unwrap();
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("start");
        let last = rx.await.expect("terminal snapshot");
        assert_eq!(last.phase, Phase::Done);
        assert_eq!(
            std::fs::read(host.dir.join("file.bin")).unwrap(),
            server.body()
        );
        // No byte range was ever asked for: the junk partial was dropped.
        assert!(server.seen().iter().all(|s| s.range.is_none()));
    });
}

#[test]
fn a_proven_partial_resumes_from_its_last_byte() {
    let full = body(96 * 1024);
    let server = Server::start(full.clone());
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, rx) = fixture("resume");
        // Half arrived in an earlier run, with the validator that proves it.
        std::fs::write(part_of(&host.dir, "file.bin"), &full[..40 * 1024]).unwrap();
        std::fs::write(host.dir.join("file.bin.part.meta"), "\"v1\"\n").unwrap();
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("start");
        let last = rx.await.expect("terminal snapshot");
        assert_eq!(last.phase, Phase::Done, "{:?}", last.message);
        assert_eq!(std::fs::read(host.dir.join("file.bin")).unwrap(), full);
        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].range.as_deref(), Some("bytes=40960-"));
        assert_eq!(seen[0].if_range.as_deref(), Some("\"v1\""));
    });
}

#[test]
fn a_changed_resource_is_replaced_not_spliced() {
    let changed = body(64 * 1024);
    let mut server_body = changed.clone();
    server_body[..4 * 1024].fill(0xEE);
    let server = Server::start(server_body.clone());
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, rx) = fixture("changed");
        // An old partial under the OLD validator: the server's ETag has
        // moved on, so `If-Range` misses and the whole body comes back.
        std::fs::write(part_of(&host.dir, "file.bin"), &changed[..32 * 1024]).unwrap();
        std::fs::write(host.dir.join("file.bin.part.meta"), "\"stale\"\n").unwrap();
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("start");
        let last = rx.await.expect("terminal snapshot");
        assert_eq!(last.phase, Phase::Done);
        let landed = std::fs::read(host.dir.join("file.bin")).unwrap();
        assert_eq!(landed, server_body);
        // One request, and it proved the partial first.
        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].if_range.as_deref(), Some("\"stale\""));
    });
}

#[test]
fn a_complete_partial_finishes_on_a_refused_range() {
    let full = body(32 * 1024);
    let server = Server::start(full.clone());
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, rx) = fixture("complete");
        std::fs::write(part_of(&host.dir, "file.bin"), &full).unwrap();
        std::fs::write(host.dir.join("file.bin.part.meta"), "\"v1\"\n").unwrap();
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("start");
        let last = rx.await.expect("terminal snapshot");
        assert_eq!(last.phase, Phase::Done, "{:?}", last.message);
        assert_eq!(std::fs::read(host.dir.join("file.bin")).unwrap(), full);
        // The server never sent a byte of body.
        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].range.as_deref(), Some("bytes=32768-"));
    });
}

#[test]
fn a_dead_first_link_rotates_to_the_next() {
    let server = Server::start(body(16 * 1024));
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, rx) = fixture("rotate");
        // A port nothing listens on: the connection is refused at once.
        let dead = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            drop(listener);
            format!("http://{addr}/file")
        };
        downloads
            .start(
                &host,
                request("file.bin", vec![dead, server.url()]),
                Some(hook),
            )
            .expect("start");
        let last = rx.await.expect("terminal snapshot");
        assert_eq!(last.phase, Phase::Done, "{:?}", last.message);
        assert_eq!(
            std::fs::read(host.dir.join("file.bin")).unwrap(),
            server.body()
        );
        assert_eq!(server.seen().len(), 1);
    });
}

#[test]
fn a_cut_stream_retries_and_keeps_what_landed() {
    let full = body(64 * 1024);
    let server = Server::with(
        full.clone(),
        Options {
            cut_after: Some(1024),
            ..Default::default()
        },
    );
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, _rx) = fixture("cut");
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("start");
        // Every attempt ends one 1 KB in, so the transfer must exhaust
        // its budget and fail — with the bytes it did receive still on
        // disk for the next start.
        wait_until(|| reached(&downloads, "dl", Phase::Failed)).await;
        let held = std::fs::metadata(part_of(&host.dir, "file.bin"))
            .unwrap()
            .len();
        assert!(held > 0 && held < full.len() as u64);
        assert!(!host.dir.join("file.bin").exists());
        // A failed attempt is retried; the budget is finite.
        assert!(server.seen().len() > 1);
    });
}

#[test]
fn a_paused_download_resumes_where_it_stopped() {
    let full = body(512 * 1024);
    let server = Server::with(
        full.clone(),
        Options {
            slow_ms: 4,
            ..Default::default()
        },
    );
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, rx) = fixture("pause");
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("start");
        wait_until(|| downloads.status("dl").is_some_and(|p| p.received > 0)).await;
        downloads.pause("dl");
        wait_until(|| {
            downloads
                .status("dl")
                .is_some_and(|p| p.phase == Phase::Paused)
        })
        .await;
        let paused = downloads.status("dl").unwrap();
        assert!(paused.received > 0 && paused.received < full.len() as u64);
        assert_eq!(paused.percent().map(|p| p < 100), Some(true));
        // The phase carries its byte count, so a sheet can say where.
        assert_eq!(paused.total, Some(full.len() as u64));

        downloads.resume(&host, "dl").expect("resume");
        let last = rx.await.expect("terminal snapshot");
        assert_eq!(last.phase, Phase::Done, "{:?}", last.message);
        assert_eq!(std::fs::read(host.dir.join("file.bin")).unwrap(), full);
        // The resume asked from the bytes on disk, proving them first.
        let resumed = server
            .seen()
            .into_iter()
            .find(|s| s.range.is_some())
            .expect("a ranged request");
        assert_eq!(resumed.if_range.as_deref(), Some("\"v1\""));
        let offset: u64 = resumed
            .range
            .unwrap()
            .trim_start_matches("bytes=")
            .trim_end_matches('-')
            .parse()
            .expect("range offset");
        assert!(offset >= paused.received && offset < full.len() as u64);
    });
}

#[test]
fn a_cancelled_download_keeps_its_partial() {
    let full = body(512 * 1024);
    let server = Server::with(
        full.clone(),
        Options {
            slow_ms: 4,
            ..Default::default()
        },
    );
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, rx) = fixture("cancel");
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("start");
        wait_until(|| downloads.status("dl").is_some_and(|p| p.received > 0)).await;
        downloads.cancel(&host, "dl");
        let last = rx.await.expect("terminal snapshot");
        assert_eq!(last.phase, Phase::Cancelled);
        let part = part_of(&host.dir, "file.bin");
        let held = std::fs::metadata(&part).unwrap().len();
        assert!(held > 0 && held < full.len() as u64);
        assert!(!host.dir.join("file.bin").exists());
    });
}

#[test]
fn cancelling_a_paused_download_settles_its_phase() {
    let full = body(512 * 1024);
    let server = Server::with(
        full.clone(),
        Options {
            slow_ms: 4,
            ..Default::default()
        },
    );
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, _rx) = fixture("cancel-paused");
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("start");
        wait_until(|| downloads.status("dl").is_some_and(|p| p.received > 0)).await;
        downloads.pause("dl");
        wait_until(|| {
            downloads
                .status("dl")
                .is_some_and(|p| p.phase == Phase::Paused)
        })
        .await;
        // Nothing is left to read the flag, so cancel must land itself.
        downloads.cancel(&host, "dl");
        wait_until(|| reached(&downloads, "dl", Phase::Cancelled)).await;
    });
}

#[test]
fn remove_stops_the_stream_and_drops_the_partial() {
    let full = body(512 * 1024);
    let server = Server::with(
        full.clone(),
        Options {
            slow_ms: 4,
            ..Default::default()
        },
    );
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, _rx) = fixture("remove");
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("start");
        wait_until(|| downloads.status("dl").is_some_and(|p| p.received > 0)).await;
        downloads.remove("dl");
        assert!(downloads.status("dl").is_none());
        assert!(!part_of(&host.dir, "file.bin").exists());
        // The stream notices its record is gone and stops writing.
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!part_of(&host.dir, "file.bin").exists());
        assert!(!host.dir.join("file.bin").exists());
    });
}

#[test]
fn an_active_id_is_refused_and_a_settled_one_is_restartable() {
    let server = Server::with(
        body(64 * 1024),
        Options {
            slow_ms: 4,
            ..Default::default()
        },
    );
    runtime().block_on(async {
        let (downloads, host, hook, _hooked, rx) = fixture("busy");
        downloads
            .start(
                &host,
                request("file.bin", vec![server.url()]),
                Some(hook.clone()),
            )
            .expect("start");
        let refused = downloads.start(
            &host,
            request("file.bin", vec![server.url()]),
            Some(hook.clone()),
        );
        assert!(refused.is_err());
        let last = rx.await.expect("terminal snapshot");
        assert_eq!(last.phase, Phase::Done);
        // A settled record does not block a new one: a feature retries.
        downloads
            .start(&host, request("file.bin", vec![server.url()]), Some(hook))
            .expect("restart");
        assert_eq!(downloads.list().len(), 1);
    });
}
