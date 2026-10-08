//! The crate's documentation is its README: one text, read by a dev here
//! and on the crate's front page, kept honest against the code by the
//! example it compiles.
#![doc = include_str!("../README.md")]

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A download a feature asks for. `directory` is host-data-relative and
/// `file_name` is a bare name, so the file lands at
/// `<data_dir>/<directory>/<file_name>` and the bytes in transit at
/// `<file_name>.part`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    /// The key every later call, event and hook uses. One download per id.
    pub id: String,
    /// Direct links to the one file, preferred first. A failed attempt
    /// moves to the next, so a blocked or dead host costs one round trip.
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

/// The wire snapshot of one download, one JSON object per event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    /// `downloading` | `paused` | `done` | `failed` | `cancelled`.
    pub phase: Phase,
    pub received: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
    /// The finished file's absolute path, on `done` only.
    pub path: Option<String>,
}

impl Progress {
    /// Whole-percent progress for a bar; `None` while the size is unknown.
    pub fn percent(&self) -> Option<u32> {
        let total = self.total?;
        (total > 0).then(|| ((self.received.min(total) as f64 / total as f64) * 100.0) as u32)
    }
}

/// Every snapshot, the terminal ones included: how a feature that would
/// rather be called than subscribed learns where the file landed.
pub type ProgressHook = Arc<dyn Fn(&Progress) + Send + Sync>;

/// The environment a downloader lives in: threading, events, storage.
pub trait Host: Clone + Send + Sync + 'static {
    /// Run a task off the caller's thread.
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
    on_progress: Option<ProgressHook>,
}

/// The transport's own copy of a slot's inputs, read once per attempt.
struct Target {
    urls: Vec<String>,
    dest: PathBuf,
}

/// The two flags a live stream watches.
#[derive(Clone)]
struct Flags {
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
}

impl Flags {
    /// Why the stream should stop, if any.
    fn raised(&self) -> Option<Attempt> {
        if self.cancel.load(Ordering::SeqCst) {
            return Some(Attempt::Cancelled);
        }
        self.pause.load(Ordering::SeqCst).then_some(Attempt::Paused)
    }
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
    /// A network, server or filesystem failure; the next attempt takes the
    /// bytes on disk — and the next mirror — from there.
    Retry(String),
}

/// Rounds over the link list; a round is one attempt per link.
const MIRROR_ROUNDS: u32 = 2;

/// The whole attempt budget for a request.
fn attempt_budget(mirrors: usize) -> u32 {
    (mirrors.max(1) as u32).saturating_mul(MIRROR_ROUNDS)
}

/// Backoff before attempt `n`; doubles, capped.
fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64.saturating_mul(1 << attempt.min(4)))
}

/// Progress snapshots are throttled to this pace; a phase change always
/// publishes.
const EMIT_EVERY: Duration = Duration::from_millis(100);

/// A stream loop wakes this often while no chunk is in flight, so pause
/// and cancel are read on a stalled connection.
const POLL_EVERY: Duration = Duration::from_millis(250);

/// A body that delivers nothing for this long is a dead connection.
const STALL_AFTER: Duration = Duration::from_secs(30);

/// The suffix of the file in transit, beside its destination.
const PART_SUFFIX: &str = ".part";

/// The suffix of the one-line validator a resume sends as `If-Range`.
const META_SUFFIX: &str = ".part.meta";

/// The process-wide downloader, shared by every feature and host.
#[derive(Clone, Default)]
pub struct Downloads {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
}

impl Downloads {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin `req`; an active id is refused. A hook, when given, sees
    /// every snapshot, the last one included.
    pub fn start<H: Host>(
        &self,
        host: &H,
        req: DownloadRequest,
        on_progress: Option<ProgressHook>,
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

    /// Stop reading at the next chunk; the partial stays and `resume`
    /// continues from it.
    pub fn pause(&self, id: &str) {
        if let Some(pause) = self
            .slots
            .lock()
            .ok()
            .and_then(|slots| slots.get(id).map(|s| s.pause.clone()))
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

    /// Ask a download to stop; the partial stays for a later `resume`.
    /// A paused download has no live stream to read the flag, so this call
    /// settles its phase.
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
        if idle {
            self.emit(host, id, Some(Phase::Cancelled));
        }
    }

    /// Drop a download's record, its partial and its validator. A finished
    /// file stays: the feature owns what it asked for.
    pub fn remove(&self, id: &str) {
        let dest = self.slots.lock().ok().and_then(|mut slots| {
            let slot = slots.remove(id)?;
            // Flagged before the delete: the live stream closes its own
            // handle on the next chunk.
            slot.cancel.store(true, Ordering::SeqCst);
            Some(slot.dest)
        });
        if let Some(dest) = dest {
            drop_meta(&dest);
            let _ = std::fs::remove_file(part_of(&dest));
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

    /// The absolute destination of a request, with both path parts checked
    /// to stay inside the host's data directory.
    fn dest<H: Host>(host: &H, req: &DownloadRequest) -> Result<PathBuf, String> {
        let name = Path::new(&req.file_name);
        let mut parts = name.components();
        let bare = matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none();
        if !bare {
            return Err(format!("'{}' is not a file name", req.file_name));
        }
        let base = host.data_dir()?;
        let dir = match &req.directory {
            Some(sub) => base.join(relative(sub)?),
            None => base,
        };
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        Ok(dir.join(name))
    }

    /// The flags of a live download, or `None` once its record is gone.
    fn flags(&self, id: &str) -> Option<Flags> {
        let slots = self.slots.lock().ok()?;
        let slot = slots.get(id)?;
        Some(Flags {
            cancel: slot.cancel.clone(),
            pause: slot.pause.clone(),
        })
    }

    /// One snapshot out: the host's channel and the dev's hook, read under
    /// one lock and called without it.
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
        if let Some(hook) = hook {
            hook(&snapshot);
        }
    }

    /// Record a byte count and publish it.
    fn note_bytes<H: Host>(&self, host: &H, id: &str, received: u64, total: Option<u64>) {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.get_mut(id)
        {
            slot.progress.received = received;
            slot.progress.total = total;
        }
        self.emit(host, id, None);
    }

    /// The transport's inputs for one attempt; `None` once the record is
    /// gone.
    fn target(&self, id: &str) -> Option<Target> {
        let slots = self.slots.lock().ok()?;
        let slot = slots.get(id)?;
        Some(Target {
            urls: slot.urls.clone(),
            dest: slot.dest.clone(),
        })
    }

    /// Stamp failure on the slot, then publish it.
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

    /// One transport: attempts with backoff, rotating the links, then the
    /// completion publish.
    async fn run<H: Host>(&self, host: &H, id: String) {
        let mut attempt: u32 = 0;
        loop {
            let Some(target) = self.target(&id) else {
                return; // removed mid-flight
            };
            let budget = attempt_budget(target.urls.len());
            match attempt_once(self, host, &id, &target, attempt).await {
                Attempt::Done => {
                    self.finish(host, &id);
                    return;
                }
                Attempt::Restart => {
                    attempt += 1;
                }
                Attempt::Paused => {
                    self.emit(host, &id, Some(Phase::Paused));
                    return;
                }
                Attempt::Cancelled => {
                    self.emit(host, &id, Some(Phase::Cancelled));
                    return;
                }
                Attempt::Retry(message) => {
                    attempt += 1;
                    if attempt >= budget {
                        self.fail(host, &id, format!("download failed: {message}"));
                        return;
                    }
                    let Some(flags) = self.flags(&id) else {
                        return; // removed mid-flight
                    };
                    if let Some(stop) = backoff_wait(&flags, backoff(attempt)).await {
                        match stop {
                            Attempt::Cancelled => self.emit(host, &id, Some(Phase::Cancelled)),
                            _ => self.emit(host, &id, Some(Phase::Paused)),
                        }
                        return;
                    }
                }
            }
        }
    }
}

/// Sleep `total` between attempts, watching the flags so pause and cancel
/// answer inside the backoff too.
async fn backoff_wait(flags: &Flags, total: Duration) -> Option<Attempt> {
    let deadline = Instant::now() + total;
    loop {
        if let Some(stop) = flags.raised() {
            return Some(stop);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        tokio::time::sleep(left.min(POLL_EVERY)).await;
    }
}

/// One attempt, plus the cleanup a removed record owes its files.
async fn attempt_once<H: Host>(
    downloads: &Downloads,
    host: &H,
    id: &str,
    target: &Target,
    attempt: u32,
) -> Attempt {
    let outcome = attempt_body(downloads, host, id, target, attempt).await;
    // Nothing is left to resume: drop the bytes. The body's handle is
    // closed by now, which is what makes the delete land on Windows.
    if matches!(outcome, Attempt::Cancelled) && downloads.status(id).is_none() {
        let _ = std::fs::remove_file(part_of(&target.dest));
        drop_meta(&target.dest);
    }
    outcome
}

/// One attempt against one mirror; an on-disk partial is reused only when
/// the server proves it is the same bytes.
async fn attempt_body<H: Host>(
    downloads: &Downloads,
    host: &H,
    id: &str,
    target: &Target,
    attempt: u32,
) -> Attempt {
    let url = &target.urls[attempt as usize % target.urls.len()];
    let dest = &target.dest;
    let Some(flags) = downloads.flags(id) else {
        return Attempt::Cancelled;
    };
    if let Some(stop) = flags.raised() {
        return stop;
    }
    let part = part_of(dest);
    let validator = read_meta(&part);
    let mut existing = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if existing > 0 && validator.is_none() {
        // A partial by a different validator proves nothing, and a stale
        // one would splice two resources into one file.
        let _ = std::fs::remove_file(&part);
        drop_meta(dest);
        existing = 0;
    }
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(STALL_AFTER)
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
        // No range asked, or `If-Range` missed: the body is whole and the
        // partial is replaced.
        reqwest::StatusCode::OK => total = response.content_length(),
        reqwest::StatusCode::RANGE_NOT_SATISFIABLE => {
            let covered = content_range_total(
                response
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok()),
            );
            // The partial already IS the file: nothing left to fetch.
            if existing > 0 && covered == Some(existing) {
                return match finalize(dest) {
                    Ok(()) => Attempt::Done,
                    Err(e) => Attempt::Retry(e),
                };
            }
            if existing > 0 {
                drop_meta(dest);
                let _ = std::fs::remove_file(&part);
                return Attempt::Restart;
            }
            return Attempt::Retry("range not satisfiable".into());
        }
        status => return Attempt::Retry(format!("{url} said {status}")),
    }

    let opened = if append {
        std::fs::OpenOptions::new().append(true).open(&part)
    } else {
        std::fs::File::create(&part)
    };
    let Ok(mut file) = opened else {
        return Attempt::Retry("partial unavailable".into());
    };
    // Recorded before the body: a crash leaves a provable partial, and a
    // 200 from a mirror clears the previous resource's validator.
    match validator_of(response.headers()) {
        Some(fresh) => write_meta(dest, &fresh),
        None => drop_meta(dest),
    }
    let mut received = if append { existing } else { 0 };
    downloads.note_bytes(host, id, received, total);

    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut last_emit = Instant::now();
    let mut last_byte = Instant::now();
    loop {
        if let Some(stop) = flags.raised() {
            let _ = file.flush();
            return stop;
        }
        match tokio::time::timeout(POLL_EVERY, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                last_byte = Instant::now();
                received += chunk.len() as u64;
                if file.write_all(&chunk).is_err() {
                    return Attempt::Retry("write partial".into());
                }
            }
            Ok(Some(Err(e))) => {
                // Keep what landed; the retry resumes from it.
                let _ = file.flush();
                return Attempt::Retry(format!("stream: {e}"));
            }
            Ok(None) => break,
            Err(_elapsed) => {
                if last_byte.elapsed() >= STALL_AFTER {
                    let _ = file.flush();
                    return Attempt::Retry("stalled".into());
                }
            }
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
    // Size first (when declared), then completion.
    if let Some(total) = total
        && received != total
    {
        return Attempt::Retry(format!("size {received} of {total}"));
    }
    match finalize(dest) {
        Ok(()) => Attempt::Done,
        Err(e) => Attempt::Retry(e),
    }
}

/// Move the proven partial onto the destination, replacing whatever an
/// interrupted earlier run left there.
fn finalize(dest: &Path) -> Result<(), String> {
    let part = part_of(dest);
    if std::fs::rename(&part, dest).is_ok() {
        drop_meta(dest);
        return Ok(());
    }
    // Windows refuses a rename onto an existing file.
    let _ = std::fs::remove_file(dest);
    std::fs::rename(&part, dest).map_err(|e| format!("adopt {}: {e}", dest.display()))?;
    drop_meta(dest);
    Ok(())
}

/// A directory part of a request, refused unless it stays under the data
/// directory.
fn relative(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => return Err(format!("'{value}' is not a relative directory")),
        }
    }
    Ok(out)
}

/// The file in transit, beside its destination.
fn part_of(dest: &Path) -> PathBuf {
    with_suffix(dest, PART_SUFFIX)
}

/// The validator's one-line file, beside the partial.
fn meta_of(dest: &Path) -> PathBuf {
    with_suffix(dest, META_SUFFIX)
}

fn with_suffix(dest: &Path, suffix: &str) -> PathBuf {
    let mut name = dest.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// The header a later resume sends back as `If-Range`.
fn validator_of(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get("etag")
        .or_else(|| headers.get("last-modified"))
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn read_meta(dest: &Path) -> Option<String> {
    let text = std::fs::read_to_string(meta_of(dest)).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_string())
}

fn write_meta(dest: &Path, validator: &str) {
    let _ = std::fs::write(meta_of(dest), format!("{validator}\n"));
}

fn drop_meta(dest: &Path) {
    let _ = std::fs::remove_file(meta_of(dest));
}

/// The total from a `Content-Range: bytes a-b/total` header, and from the
/// `bytes */total` a 416 carries.
fn content_range_total(header: Option<&str>) -> Option<u64> {
    // `bytes 0-99/1234` and `bytes */1234` both end in the total.
    let tail = header?.rsplit('/').next()?;
    let total = tail.parse::<u64>().ok()?;
    (total > 0).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(phase: Phase, received: u64, total: Option<u64>) -> Progress {
        Progress {
            id: "x".into(),
            phase,
            received,
            total,
            message: None,
            path: None,
        }
    }

    #[test]
    fn content_range_totals_parse_and_reject_junk() {
        assert_eq!(content_range_total(Some("bytes 100-199/1234")), Some(1234));
        assert_eq!(content_range_total(Some("bytes */1234")), Some(1234));
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
    fn every_mirror_gets_a_round() {
        assert_eq!(attempt_budget(1), 2);
        assert_eq!(attempt_budget(3), 6);
        // A linkless request is refused earlier; the budget still must
        // not be zero.
        assert_eq!(attempt_budget(0), 2);
    }

    #[test]
    fn attempts_walk_the_links_and_wrap() {
        let urls = ["a", "b", "c"];
        let pick = |attempt: u32| urls[attempt as usize % urls.len()];
        assert_eq!(pick(0), "a");
        assert_eq!(pick(1), "b");
        assert_eq!(pick(2), "c");
        assert_eq!(pick(3), "a");
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
    fn percent_is_whole_and_bounded() {
        assert_eq!(progress(Phase::Downloading, 50, Some(200)).percent(), Some(25));
        assert_eq!(progress(Phase::Downloading, 0, None).percent(), None);
        assert_eq!(progress(Phase::Downloading, 0, Some(0)).percent(), None);
        // A server that over-delivers never reads past 100.
        assert_eq!(progress(Phase::Downloading, 9, Some(2)).percent(), Some(100));
    }

    #[test]
    fn the_wire_shape_is_camel_case_with_lowercase_phases() {
        let mut json = serde_json::to_string(&progress(Phase::Downloading, 1, Some(2))).unwrap();
        assert!(json.contains("\"phase\":\"downloading\""));
        assert!(json.contains("\"received\":1"));
        assert!(json.contains("\"total\":2"));
        assert!(!json.contains("file_name"));
        json = serde_json::to_string(&progress(Phase::Paused, 1, Some(2))).unwrap();
        assert!(json.contains("\"phase\":\"paused\""));
    }

    #[test]
    fn a_file_name_must_be_bare_and_a_directory_relative() {
        assert!(relative("cefr").is_ok());
        assert!(relative("a/b").is_ok());
        assert!(relative("../escape").is_err());
        assert!(relative("a/../../escape").is_err());
        assert!(relative("/etc").is_err());
        assert!(relative("").is_err());

        let request = |file_name: &str| DownloadRequest {
            id: "x".into(),
            urls: vec!["https://example.invalid/a".into()],
            directory: None,
            file_name: file_name.into(),
        };
        assert!(request("cefr.parquet").file_name == "cefr.parquet");
        for bad in ["../db", "a/b", "", ".", "..", "/abs"] {
            let path = Path::new(bad);
            let mut parts = path.components();
            let bare =
                matches!(parts.next(), Some(Component::Normal(_))) && parts.next().is_none();
            assert!(!bare, "'{bad}' should not pass as a file name");
        }
    }

    #[test]
    fn the_meta_round_trips_one_validator() {
        let dir = std::env::temp_dir().join(format!("download_core_meta_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let dest = dir.join("file.parquet");
        assert_eq!(read_meta(&dest), None);
        write_meta(&dest, "\"68a7c2-1e5\"");
        assert_eq!(read_meta(&dest).as_deref(), Some("\"68a7c2-1e5\""));
        write_meta(&dest, "Wed, 21 Oct 2015 07:28:00 GMT");
        assert_eq!(
            read_meta(&dest).as_deref(),
            Some("Wed, 21 Oct 2015 07:28:00 GMT")
        );
        // The partial and its validator sit beside the destination.
        assert_eq!(part_of(&dest), dir.join("file.parquet.part"));
        assert_eq!(meta_of(&dest), dir.join("file.parquet.part.meta"));
        drop_meta(&dest);
        assert_eq!(read_meta(&dest), None);
        let _ = std::fs::remove_dir(&dir);
    }
}
