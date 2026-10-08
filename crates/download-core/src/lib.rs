//! Background downloads: resumable, pausable, mirror-rotating.

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A download a feature asks for. `directory` is host-data-relative; the
/// file lands at `<data_dir>/<directory>/<file_name>`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    /// The key every later call and event uses. One download per id.
    pub id: String,
    /// Direct links to the one file, preferred first; a failure rotates.
    pub urls: Vec<String>,
    pub directory: Option<String>,
    pub file_name: String,
}

/// Where a download is; the wire vocabulary a frontend switches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Downloading,
    Paused,
    Done,
    Failed,
    Cancelled,
}

impl Phase {
    /// Whether the transport is finished with this download.
    pub fn terminal(self) -> bool {
        matches!(self, Self::Done | Self::Failed | Self::Cancelled)
    }
}

/// The wire snapshot of one download.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    pub phase: Phase,
    pub received: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
    /// The finished file's absolute path, on `done` only.
    pub path: Option<String>,
}

/// Every snapshot, terminal ones included: how a feature learns where
/// the file landed.
pub type ProgressHook = Arc<dyn Fn(&Progress) + Send + Sync>;

/// The environment a downloader lives in: threading, events, storage.
pub trait Host: Clone + Send + Sync + 'static {
    /// Run a task off the caller's thread, on a tokio-compatible runtime.
    fn spawn(&self, task: impl Future<Output = ()> + Send + 'static);
    /// Hand one snapshot to the feature's event channel.
    fn publish(&self, progress: &Progress);
    /// The directory every download lands under.
    fn data_dir(&self) -> Result<PathBuf, String>;
}

/// Per-download state: the wire snapshot plus the transport's flags.
struct Slot {
    progress: Progress,
    urls: Vec<String>,
    dest: PathBuf,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    on_progress: ProgressHook,
}

/// The transport's own copy of a slot's inputs, read once per attempt.
struct Target {
    urls: Vec<String>,
    dest: PathBuf,
}

/// One attempt's outcome.
enum Attempt {
    /// The file is complete on disk.
    Done,
    /// The partial was rejected (416): restart from zero, no retry spent.
    Restart,
    /// Stop for now; the partial stays and `resume` continues from it.
    Paused,
    /// The user asked to stop, or the record was dropped mid-flight.
    Cancelled,
    /// A network or server failure; the next attempt rotates mirrors.
    Retry(String),
}

/// Attempts per mirror; the backoff doubles per attempt, capped.
const ATTEMPTS_PER_URL: u32 = 2;

/// The whole budget: every mirror gets its own tries.
fn attempts_budget(mirrors: usize) -> u32 {
    (mirrors.max(1) as u32).saturating_mul(ATTEMPTS_PER_URL)
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64.saturating_mul(1 << attempt.min(4)))
}

/// Progress snapshots are throttled to this pace; a phase change always
/// publishes.
const EMIT_EVERY: Duration = Duration::from_millis(100);

/// A body that stops delivering for this long is a dead connection.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// The process-wide downloader, shared by every feature and host.
#[derive(Clone, Default)]
pub struct Downloads {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
}

impl Downloads {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin `req`; an active id is refused. The hook sees every
    /// snapshot, including the last.
    pub fn start<H: Host>(
        &self,
        host: &H,
        req: DownloadRequest,
        on_progress: ProgressHook,
    ) -> Result<(), String> {
        if req.urls.is_empty() {
            return Err(format!("download '{}' has no link", req.id));
        }
        let dest = Self::dest(host, &req)?;
        let mut slots = self.slots.lock().map_err(|_| "slots lock poisoned")?;
        if let Some(slot) = slots.get(&req.id)
            && !slot.progress.phase.terminal()
        {
            return Err(format!("download '{}' is already active", req.id));
        }
        let progress = Progress {
            id: req.id.clone(),
            phase: Phase::Downloading,
            received: 0,
            total: None,
            message: None,
            path: None,
        };
        slots.insert(
            req.id.clone(),
            Slot {
                progress,
                urls: req.urls,
                dest,
                cancel: Arc::new(AtomicBool::new(false)),
                pause: Arc::new(AtomicBool::new(false)),
                on_progress,
            },
        );
        drop(slots);
        self.emit(host, &req.id, None);
        let (downloads, task_host, id) = (self.clone(), host.clone(), req.id);
        host.spawn(async move {
            downloads.run(&task_host, id).await;
        });
        Ok(())
    }

    /// Stop reading; the partial stays and `resume` continues from it.
    pub fn pause(&self, id: &str) {
        if let Some(pause) = self
            .slots
            .lock()
            .ok()
            .and_then(|mut slots| slots.get_mut(id).map(|s| s.pause.clone()))
        {
            pause.store(true, Ordering::SeqCst);
        }
    }

    /// Continue a paused download from the bytes already on disk.
    pub fn resume<H: Host>(&self, host: &H, id: &str) -> Result<(), String> {
        {
            let mut slots = self.slots.lock().map_err(|_| "slots lock poisoned")?;
            let Some(slot) = slots.get_mut(id) else {
                return Err(format!("no download '{id}'"));
            };
            if slot.progress.phase != Phase::Paused {
                return Err(format!("download '{id}' is not paused"));
            }
            slot.pause.store(false, Ordering::SeqCst);
            slot.progress.message = None;
        }
        self.emit(host, id, Some(Phase::Downloading));
        let (downloads, task_host, id) = (self.clone(), host.clone(), id.to_string());
        host.spawn(async move {
            downloads.run(&task_host, id).await;
        });
        Ok(())
    }

    /// Ask a running download to stop; the partial stays for a later
    /// `start`'s range resume.
    pub fn cancel<H: Host>(&self, host: &H, id: &str) {
        let idle = self
            .slots
            .lock()
            .ok()
            .and_then(|mut slots| {
                let slot = slots.get_mut(id)?;
                slot.cancel.store(true, Ordering::SeqCst);
                Some(slot.progress.phase == Phase::Paused)
            })
            .unwrap_or(false);
        // A paused transport already returned, so this call settles
        // the phase.
        if idle {
            self.emit(host, id, Some(Phase::Cancelled));
        }
    }

    /// Drop a download's record, its partial and its resume sidecar.
    pub fn remove(&self, id: &str) {
        let dest = self.slots.lock().ok().and_then(|mut slots| {
            let slot = slots.remove(id)?;
            // Flag first: the next chunk check closes the handle on
            // the file this call deletes.
            slot.cancel.store(true, Ordering::SeqCst);
            Some(slot.dest)
        });
        if let Some(dest) = dest {
            drop_sidecar(&dest);
            let _ = std::fs::remove_file(dest);
        }
    }

    /// The current snapshot of one download.
    pub fn status(&self, id: &str) -> Option<Progress> {
        self.slots.lock().ok()?.get(id).map(|s| s.progress.clone())
    }

    /// Every download's snapshot, for a booting frontend.
    pub fn list(&self) -> Vec<Progress> {
        self.slots
            .lock()
            .map(|slots| slots.values().map(|s| s.progress.clone()).collect())
            .unwrap_or_default()
    }

    /// The absolute destination of a request.
    fn dest<H: Host>(host: &H, req: &DownloadRequest) -> Result<PathBuf, String> {
        let dir = match &req.directory {
            Some(sub) => host.data_dir()?.join(sub),
            None => host.data_dir()?,
        };
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        Ok(dir.join(&req.file_name))
    }

    /// One snapshot out: the host's bus and the dev's hook, from one lock.
    fn emit<H: Host>(&self, host: &H, id: &str, phase: Option<Phase>) {
        let (snapshot, hook) = {
            let Ok(mut slots) = self.slots.lock() else {
                return;
            };
            let Some(slot) = slots.get_mut(id) else {
                return;
            };
            if let Some(phase) = phase {
                slot.progress.phase = phase;
            }
            (slot.progress.clone(), slot.on_progress.clone())
        };
        host.publish(&snapshot);
        hook(&snapshot);
    }

    /// Stamp the failure on the slot, then publish it.
    fn fail<H: Host>(&self, host: &H, id: &str, message: String) {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.get_mut(id)
        {
            slot.progress.message = Some(message);
        }
        self.emit(host, id, Some(Phase::Failed));
    }

    /// Publish completion with the finished file's path.
    fn finish<H: Host>(&self, host: &H, id: &str) {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.get_mut(id)
        {
            slot.progress.path = Some(slot.dest.display().to_string());
        }
        self.emit(host, id, Some(Phase::Done));
    }

    /// The transport's inputs for one attempt, or `None` once removed.
    fn target(&self, id: &str) -> Option<Target> {
        let slots = self.slots.lock().ok()?;
        let slot = slots.get(id)?;
        Some(Target {
            urls: slot.urls.clone(),
            dest: slot.dest.clone(),
        })
    }

    /// One transport: attempts with backoff, then the completion publish.
    async fn run<H: Host>(&self, host: &H, id: String) {
        let mut attempt: u32 = 0;
        loop {
            let Some(target) = self.target(&id) else {
                return; // removed mid-flight
            };
            match attempt_once(self, host, &id, &target, attempt).await {
                Attempt::Done => {
                    self.finish(host, &id);
                    return;
                }
                Attempt::Paused => {
                    self.emit(host, &id, Some(Phase::Paused));
                    return;
                }
                Attempt::Cancelled => {
                    self.emit(host, &id, Some(Phase::Cancelled));
                    return;
                }
                Attempt::Restart => continue,
                Attempt::Retry(message) => {
                    attempt += 1;
                    if attempt >= attempts_budget(target.urls.len()) {
                        self.fail(host, &id, format!("download failed: {message}"));
                        return;
                    }
                    tokio::time::sleep(backoff(attempt)).await;
                }
            }
        }
    }

    /// The two stop flags of one download, cloned for the stream loop.
    fn flags(&self, id: &str) -> Option<(Arc<AtomicBool>, Arc<AtomicBool>)> {
        self.slots
            .lock()
            .ok()?
            .get(id)
            .map(|s| (s.cancel.clone(), s.pause.clone()))
    }

    fn note_bytes<H: Host>(&self, host: &H, id: &str, received: u64, total: Option<u64>) {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.get_mut(id)
        {
            slot.progress.received = received;
            slot.progress.total = total;
        }
        self.emit(host, id, None);
    }
}

/// One attempt against one mirror; a partial is reused only when
/// proven.
async fn attempt_once<H: Host>(
    downloads: &Downloads,
    host: &H,
    id: &str,
    target: &Target,
    attempt: u32,
) -> Attempt {
    let url = &target.urls[attempt as usize % target.urls.len()];
    let dest = &target.dest;
    // Read first: a cancel during a backoff must not pay another
    // round trip.
    let Some((cancel, pause)) = downloads.flags(id) else {
        return Attempt::Cancelled;
    };
    // An unprovable partial cannot be appended to, so this attempt
    // starts over.
    let validator = read_validator(dest);
    let mut existing = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    if existing > 0 && validator.is_none() {
        existing = 0;
    }
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(READ_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(e) => return Attempt::Retry(format!("client: {e}")),
    };
    let mut request = client.get(url);
    if let (Some(validator), true) = (validator.as_deref(), existing > 0) {
        request = request
            .header("Range", format!("bytes={existing}-"))
            .header("If-Range", validator);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(e) => return Attempt::Retry(format!("connect: {e}")),
    };

    let mut append = false;
    let total: Option<u64>;
    match response.status() {
        reqwest::StatusCode::PARTIAL_CONTENT => {
            append = true;
            total = content_range_total(
                response
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok()),
            );
        }
        // No range asked, or `If-Range` missed: the partial and its
        // validator are both replaced.
        reqwest::StatusCode::OK => total = response.content_length(),
        reqwest::StatusCode::RANGE_NOT_SATISFIABLE => {
            if existing > 0 {
                drop_sidecar(dest);
                let _ = std::fs::remove_file(dest);
                return Attempt::Restart;
            }
            return Attempt::Retry("range not satisfiable".into());
        }
        status => return Attempt::Retry(format!("{url} said {status}")),
    }

    let opened = if append {
        std::fs::OpenOptions::new().append(true).open(dest)
    } else {
        std::fs::File::create(dest)
    };
    let Ok(mut file) = opened else {
        return Attempt::Retry("partial unavailable".into());
    };
    // Written before the body: a crash still leaves a provable
    // partial.
    match validator_of(response.headers()) {
        Some(fresh) => write_validator(dest, &fresh),
        None => drop_sidecar(dest),
    }
    let mut received = if append { existing } else { 0 };
    downloads.note_bytes(host, id, received, total);

    let mut last_emit = Instant::now();
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = file.flush();
            return Attempt::Cancelled;
        }
        if pause.load(Ordering::SeqCst) {
            let _ = file.flush();
            return Attempt::Paused;
        }
        match stream.next().await {
            Some(Ok(chunk)) => {
                received += chunk.len() as u64;
                if file.write_all(&chunk).is_err() {
                    return Attempt::Retry("write partial".into());
                }
            }
            Some(Err(e)) => {
                // Keep what landed; the retry resumes from it.
                let _ = file.flush();
                return Attempt::Retry(format!("stream: {e}"));
            }
            None => break,
        }
        if last_emit.elapsed() >= EMIT_EVERY {
            last_emit = Instant::now();
            downloads.note_bytes(host, id, received, total);
        }
    }
    if file.flush().is_err() || file.sync_all().is_err() {
        return Attempt::Retry("partial sync".into());
    }
    drop(file);
    downloads.note_bytes(host, id, received, total);
    // Size check first (when declared), then completion.
    if let Some(total) = total
        && received != total
    {
        return Attempt::Retry(format!("size {received} of {total}"));
    }
    drop_sidecar(dest);
    Attempt::Done
}

/// The header a later resume can send back as `If-Range`.
fn validator_of(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get("etag")
        .or_else(|| headers.get("last-modified"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// The partial's validator, one line, sent verbatim as `If-Range`.
fn sidecar(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_os_string();
    name.push(".mareader-resume");
    PathBuf::from(name)
}

fn read_validator(dest: &Path) -> Option<String> {
    let text = std::fs::read_to_string(sidecar(dest)).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

fn write_validator(dest: &Path, validator: &str) {
    let _ = std::fs::write(sidecar(dest), format!("{validator}\n"));
}

fn drop_sidecar(dest: &Path) {
    let _ = std::fs::remove_file(sidecar(dest));
}

/// The total from a `Content-Range: bytes a-b/total` header.
fn content_range_total(header: Option<&str>) -> Option<u64> {
    header?
        .rsplit('/')
        .next()?
        .parse::<u64>()
        .ok()
        .filter(|t| *t > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_totals_parse_and_reject_junk() {
        assert_eq!(content_range_total(Some("bytes 100-199/1234")), Some(1234));
        assert_eq!(content_range_total(Some("bytes 100-199/*")), None);
        assert_eq!(content_range_total(Some("garbage")), None);
        assert_eq!(content_range_total(None), None);
    }

    #[test]
    fn backoff_grows_and_saturates() {
        assert_eq!(backoff(1), Duration::from_millis(1000));
        assert_eq!(backoff(2), Duration::from_millis(2000));
        assert_eq!(backoff(99), Duration::from_millis(8000));
    }

    #[test]
    fn the_wire_shape_is_camel_case_with_lowercase_phases() {
        let json = serde_json::to_string(&Progress {
            id: "x".into(),
            phase: Phase::Downloading,
            received: 1,
            total: Some(2),
            message: None,
            path: None,
        })
        .unwrap();
        assert!(json.contains("\"phase\":\"downloading\""));
        assert!(json.contains("\"received\":1"));
        assert!(json.contains("\"total\":2"));
        assert!(!json.contains("file_name"));
    }

    #[test]
    fn only_the_last_three_phases_are_terminal() {
        assert!(!Phase::Downloading.terminal());
        assert!(!Phase::Paused.terminal());
        assert!(Phase::Done.terminal());
        assert!(Phase::Failed.terminal());
        assert!(Phase::Cancelled.terminal());
    }

    #[test]
    fn every_mirror_gets_its_own_attempts() {
        assert_eq!(attempts_budget(1), 2);
        assert_eq!(attempts_budget(3), 6);
        // A linkless request is refused earlier; the budget still
        // must not.
        assert_eq!(attempts_budget(0), 2);
    }

    #[test]
    fn attempts_walk_the_mirror_list_and_wrap() {
        let urls = ["a", "b", "c"];
        let pick = |attempt: u32| urls[attempt as usize % urls.len()];
        assert_eq!(pick(0), "a");
        assert_eq!(pick(1), "b");
        assert_eq!(pick(3), "a");
        assert_eq!(pick(5), "c");
    }

    #[test]
    fn the_sidecar_round_trips_one_validator() {
        let dir = std::env::temp_dir().join(format!("dl_core_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let dest = dir.join("file.parquet.part");
        assert_eq!(read_validator(&dest), None);
        write_validator(&dest, "\"68a7c2-1e5\"");
        assert_eq!(read_validator(&dest).as_deref(), Some("\"68a7c2-1e5\""));
        write_validator(&dest, "Wed, 21 Oct 2015 07:28:00 GMT");
        assert_eq!(
            read_validator(&dest).as_deref(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
        drop_sidecar(&dest);
        assert_eq!(read_validator(&dest), None);
        let _ = std::fs::remove_dir(&dir);
    }
}
