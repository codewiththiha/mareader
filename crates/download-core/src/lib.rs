//! Background downloads: resumable, pausable, mirror-aware, by id.

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A download the caller asks for. `directory` is host-data-relative;
/// the file lands at `<data_dir>/<directory>/<file_name>`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadRequest {
    /// The key every later call and event uses. One download per id.
    pub id: String,
    /// The direct link to fetch.
    pub url: String,
    /// Fallback links to the same bytes, for strict networks.
    pub mirrors: Vec<String>,
    pub directory: Option<String>,
    /// A plain file name: no separators, no `.` or `..`.
    pub file_name: String,
    /// The size the file must reach; a file already that big is done.
    pub expected_size: Option<u64>,
}

impl DownloadRequest {
    /// Every link to the same bytes: primary first, then the mirrors.
    pub fn urls(&self) -> Vec<&str> {
        let mut urls = vec![self.url.as_str()];
        urls.extend(self.mirrors.iter().map(String::as_str));
        urls
    }
}

/// The wire snapshot of one download.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub id: String,
    /// `downloading` | `paused` | `done` | `failed` | `cancelled`.
    pub phase: String,
    pub received: u64,
    pub total: Option<u64>,
    pub message: Option<String>,
    /// The finished file's absolute path, on `done` only.
    pub path: Option<String>,
}

/// The per-client progress hook, called outside the slot lock.
pub type ProgressHook = Arc<dyn Fn(&Progress) + Send + Sync>;

impl Progress {
    fn terminal(&self) -> bool {
        matches!(self.phase.as_str(), "done" | "failed" | "cancelled")
    }
}

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
    /// Primary then mirrors; each attempt takes the next link.
    urls: Vec<String>,
    dest: PathBuf,
    expected_size: Option<u64>,
    /// The strong validator a resume re-validates against.
    validator: Option<String>,
    cancel: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    on_progress: Option<ProgressHook>,
}

/// One attempt's outcome.
enum Attempt {
    /// The file is complete on disk.
    Done,
    /// The partial was rejected (416): restart from zero, no retry spent.
    Restart,
    /// Stop for now; the partial stays and `resume` continues from it.
    Paused,
    /// The user asked to stop; the partial stays.
    Cancelled,
    /// A network or server failure; the loop retries from the bytes on
    /// disk.
    Retry(String),
}

/// What a stop flag asks a live download to do.
enum Stop {
    Cancelled,
    Paused,
}

/// The two stop flags of one download, cloned for the task.
type Flags = Option<(Arc<AtomicBool>, Arc<AtomicBool>)>;

/// Retry budget per URL; the rotation gives each link its own tries.
const MAX_ATTEMPTS_PER_URL: u32 = 4;

/// Progress snapshots are throttled to this pace; a phase change always
/// publishes.
const EMIT_EVERY: Duration = Duration::from_millis(100);

/// A connect that stays silent this long fails and rotates on.
const CONNECT_WAIT: Duration = Duration::from_secs(20);

/// A body that sends nothing this long is a stalled attempt.
const STALL_WAIT: Duration = Duration::from_secs(30);

/// How often a backoff sleep looks at the stop flags.
const SLEEP_SLICE: Duration = Duration::from_millis(200);

/// Retry backoff; doubles per attempt, capped.
fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64.saturating_mul(1 << attempt.min(4)))
}

/// The attempt a stop flag asks for, if any.
fn stop_attempt(flags: &Flags) -> Option<Attempt> {
    match stop_of(flags)? {
        Stop::Cancelled => Some(Attempt::Cancelled),
        Stop::Paused => Some(Attempt::Paused),
    }
}

/// The first stop the flags ask for; cancel wins over pause.
fn stop_of(flags: &Flags) -> Option<Stop> {
    let (cancel, pause) = flags.as_ref()?;
    if cancel.load(Ordering::SeqCst) {
        Some(Stop::Cancelled)
    } else if pause.load(Ordering::SeqCst) {
        Some(Stop::Paused)
    } else {
        None
    }
}

/// Backoff sleep that returns the moment a stop flag lands.
async fn wait_or_stop(flags: &Flags, total: Duration) -> Option<Stop> {
    let mut left = total;
    loop {
        if let Some(stop) = stop_of(flags) {
            return Some(stop);
        }
        if left == Duration::ZERO {
            return None;
        }
        let slice = left.min(SLEEP_SLICE);
        tokio::time::sleep(slice).await;
        left -= slice;
    }
}

/// Whether `name` is one safe path component.
fn is_plain_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\', '\0']) && name != "." && name != ".."
}

/// Whether `dir` is a relative path of plain components only.
fn is_relative(dir: &str) -> bool {
    !dir.is_empty()
        && !dir.contains('\0')
        && Path::new(dir)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
}

/// The process-wide downloader, shared by every feature and host.
#[derive(Clone, Default)]
pub struct Downloads {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
}

impl Downloads {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin `req`; an active id is refused. A file already at
    /// `expected_size` completes offline.
    pub fn start<H: Host>(
        &self,
        host: &H,
        req: DownloadRequest,
        on_progress: Option<ProgressHook>,
    ) -> Result<(), String> {
        let dest = Self::dest(host, &req)?;
        let urls = req.urls();
        if urls.iter().any(|u| u.is_empty()) {
            return Err("a download link is empty".into());
        }
        let mut slots = self.slots.lock().map_err(|_| "slots lock poisoned")?;
        if let Some(slot) = slots.get(&req.id)
            && !slot.progress.terminal()
        {
            return Err(format!("download '{}' is already active", req.id));
        }
        let progress = Progress {
            id: req.id.clone(),
            phase: "downloading".into(),
            received: 0,
            total: None,
            message: None,
            path: None,
        };
        slots.insert(
            req.id.clone(),
            Slot {
                progress,
                urls: urls.iter().map(|u| u.to_string()).collect(),
                dest,
                expected_size: req.expected_size,
                validator: None,
                cancel: Arc::new(AtomicBool::new(false)),
                pause: Arc::new(AtomicBool::new(false)),
                on_progress,
            },
        );
        drop(slots);
        self.publish(host, &req.id, None);
        let (downloads, task_host, id) = (self.clone(), host.clone(), req.id);
        host.spawn(async move {
            downloads.run(&task_host, id).await;
        });
        Ok(())
    }

    /// Stop reading; the partial stays and `resume` continues from it.
    pub fn pause(&self, id: &str) {
        if let Some(slot) = self
            .slots
            .lock()
            .ok()
            .and_then(|mut slots| slots.get_mut(id).map(|s| s.pause.clone()))
        {
            slot.store(true, Ordering::SeqCst);
        }
    }

    /// Continue a paused download from the bytes already on disk.
    pub fn resume<H: Host>(&self, host: &H, id: &str) -> Result<(), String> {
        {
            let mut slots = self.slots.lock().map_err(|_| "slots lock poisoned")?;
            let Some(slot) = slots.get_mut(id) else {
                return Err(format!("no download '{id}'"));
            };
            if slot.progress.phase != "paused" {
                return Err(format!("download '{id}' is not paused"));
            }
            slot.pause.store(false, Ordering::SeqCst);
            slot.progress.phase = "downloading".into();
            slot.progress.message = None;
        }
        self.publish(host, id, None);
        let (downloads, task_host, id) = (self.clone(), host.clone(), id.to_string());
        host.spawn(async move {
            downloads.run(&task_host, id).await;
        });
        Ok(())
    }

    /// Ask a running download to stop; the partial stays for a later
    /// `start`'s range resume.
    pub fn cancel(&self, id: &str) {
        if let Some(slot) = self
            .slots
            .lock()
            .ok()
            .and_then(|mut slots| slots.get_mut(id).map(|s| s.cancel.clone()))
        {
            slot.store(true, Ordering::SeqCst);
        }
    }

    /// Drop a download's record and its partial file.
    pub fn remove(&self, id: &str) {
        let dest = self.slots.lock().ok().and_then(|mut slots| {
            let slot = slots.remove(id);
            // A live task stops at its next flag read; the file goes now.
            if let Some(slot) = &slot {
                slot.cancel.store(true, Ordering::SeqCst);
            }
            slot.map(|s| s.dest)
        });
        if let Some(dest) = dest {
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
        if !is_plain_name(&req.file_name) {
            return Err(format!("bad file name '{}'", req.file_name));
        }
        let mut dir = host.data_dir()?;
        if let Some(sub) = &req.directory {
            if !is_relative(sub) {
                return Err(format!("bad directory '{sub}'"));
            }
            dir = dir.join(sub);
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        Ok(dir.join(&req.file_name))
    }

    /// Update a slot's phase, then publish outside the lock.
    fn publish<H: Host>(&self, host: &H, id: &str, phase: Option<&str>) {
        let snapshot = {
            let Ok(mut slots) = self.slots.lock() else {
                return;
            };
            let Some(slot) = slots.get_mut(id) else {
                return;
            };
            if let Some(phase) = phase {
                slot.progress.phase = phase.to_string();
            }
            slot.progress.clone()
        };
        host.publish(&snapshot);
        if let Some(cb) = self
            .slots
            .lock()
            .ok()
            .and_then(|slots| slots.get(id).and_then(|s| s.on_progress.clone()))
        {
            cb(&snapshot);
        }
    }

    /// One transport: attempts with backoff, then the completion publish.
    async fn run<H: Host>(&self, host: &H, id: String) {
        let Some((urls, dest, flags)) = self.slot_view(&id) else {
            return;
        };
        let mut attempt: u32 = 0;
        loop {
            if !self.live(&id) {
                // A remove owns the file; a racing write cannot keep it.
                let _ = std::fs::remove_file(&dest);
                return;
            }
            let url = urls[attempt as usize % urls.len()].clone();
            match attempt_once(self, host, &id, &url, &dest, &flags).await {
                Attempt::Done => {
                    if self.live(&id) {
                        self.finish(host, &id, "done");
                    } else {
                        let _ = std::fs::remove_file(&dest);
                    }
                    return;
                }
                Attempt::Paused => {
                    self.publish(host, &id, Some("paused"));
                    return;
                }
                Attempt::Cancelled => {
                    if !self.live(&id) {
                        let _ = std::fs::remove_file(&dest);
                    }
                    self.publish(host, &id, Some("cancelled"));
                    return;
                }
                Attempt::Restart => continue,
                Attempt::Retry(message) => {
                    attempt += 1;
                    if attempt >= MAX_ATTEMPTS_PER_URL * urls.len() as u32 {
                        self.fail(host, &id, format!("download failed: {message}"));
                        return;
                    }
                    if let Some(stop) = wait_or_stop(&flags, backoff(attempt)).await {
                        let phase = match stop {
                            Stop::Cancelled => "cancelled",
                            Stop::Paused => "paused",
                        };
                        self.publish(host, &id, Some(phase));
                        return;
                    }
                }
            }
        }
    }

    fn fail<H: Host>(&self, host: &H, id: &str, message: String) {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.get_mut(id)
        {
            slot.progress.phase = "failed".into();
            slot.progress.message = Some(message);
        }
        self.publish(host, id, None);
    }

    /// Stamp `phase` on the slot and publish it with the final path.
    fn finish<H: Host>(&self, host: &H, id: &str, phase: &'static str) {
        let snapshot = {
            let Ok(mut slots) = self.slots.lock() else {
                return;
            };
            let Some(slot) = slots.get_mut(id) else {
                return;
            };
            slot.progress.phase = phase.into();
            slot.progress.path = Some(slot.dest.display().to_string());
            slot.progress.clone()
        };
        host.publish(&snapshot);
    }

    /// The slot's transport handles, cloned out of the lock.
    fn slot_view(&self, id: &str) -> Option<(Vec<String>, PathBuf, Flags)> {
        self.slots.lock().ok()?.get(id).map(|s| {
            (
                s.urls.clone(),
                s.dest.clone(),
                Some((s.cancel.clone(), s.pause.clone())),
            )
        })
    }

    /// Whether the record still exists (no `remove` has claimed it).
    fn live(&self, id: &str) -> bool {
        self.slots
            .lock()
            .map(|slots| slots.contains_key(id))
            .unwrap_or(false)
    }

    fn expected_size(&self, id: &str) -> Option<u64> {
        self.slots.lock().ok()?.get(id)?.expected_size
    }

    /// The strong validator a resume re-validates against.
    fn validator(&self, id: &str) -> Option<String> {
        self.slots.lock().ok()?.get(id)?.validator.clone()
    }

    /// Replace the resume validator from a response's headers.
    fn note_validator(&self, id: &str, validator: Option<String>) {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.get_mut(id)
        {
            slot.validator = validator;
        }
    }

    fn note_bytes<H: Host>(&self, host: &H, id: &str, received: u64, total: Option<u64>) {
        if let Ok(mut slots) = self.slots.lock()
            && let Some(slot) = slots.get_mut(id)
        {
            slot.progress.received = received;
            slot.progress.total = total;
        }
        self.publish(host, id, None);
    }
}

/// One HTTP attempt, resuming from whatever the partial already holds.
async fn attempt_once<H: Host>(
    downloads: &Downloads,
    host: &H,
    id: &str,
    url: &str,
    dest: &Path,
    flags: &Flags,
) -> Attempt {
    if let Some(stop) = stop_attempt(flags) {
        return stop;
    }
    let existing = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
    if downloads.expected_size(id) == Some(existing) {
        // The cache already holds exactly this file.
        return Attempt::Done;
    }
    let client = match reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(e) => return Attempt::Retry(format!("client: {e}")),
    };
    let mut request = client.get(url);
    if existing > 0 {
        request = request.header("Range", format!("bytes={existing}-"));
        if let Some(validator) = downloads.validator(id) {
            // A changed remote must restart clean, never append blind.
            request = request.header("If-Range", validator);
        }
    }
    let response = match tokio::time::timeout(CONNECT_WAIT, request.send()).await {
        Ok(Ok(response)) => response,
        Ok(Err(e)) => return Attempt::Retry(format!("connect: {e}")),
        Err(_) => return Attempt::Retry("connect: timed out".into()),
    };

    let mut append = false;
    let total: Option<u64>;
    match response.status() {
        reqwest::StatusCode::PARTIAL_CONTENT => {
            append = true;
            total = content_range_total(range_header(&response));
        }
        // The server ignored the range: the partial is overwritten.
        reqwest::StatusCode::OK => total = response.content_length(),
        reqwest::StatusCode::RANGE_NOT_SATISFIABLE => {
            if content_range_total(range_header(&response)) == Some(existing) && existing > 0 {
                return Attempt::Done;
            }
            if existing > 0 {
                let _ = std::fs::remove_file(dest);
                return Attempt::Restart;
            }
            return Attempt::Retry("range not satisfiable".into());
        }
        status => return Attempt::Retry(format!("server said {status}")),
    }
    if let (Some(expected), Some(total)) = (downloads.expected_size(id), total)
        && total != expected
    {
        // A mirror serving other bytes is a failed link, not this file.
        return Attempt::Retry(format!("served {total} bytes, expected {expected}"));
    }
    downloads.note_validator(
        id,
        response
            .headers()
            .get("etag")
            .or_else(|| response.headers().get("last-modified"))
            .and_then(|v| v.to_str().ok())
            .map(String::from),
    );

    let opened = if append {
        std::fs::OpenOptions::new().append(true).open(dest)
    } else {
        std::fs::File::create(dest)
    };
    let Ok(mut file) = opened else {
        return Attempt::Retry("partial unavailable".into());
    };
    let mut received = if append { existing } else { 0 };
    downloads.note_bytes(host, id, received, total);

    let mut last_emit = Instant::now();
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::with_capacity(64 * 1024);
    loop {
        if let Some(stop) = stop_attempt(flags) {
            let _ = file.flush();
            return stop;
        }
        buffer.clear();
        match tokio::time::timeout(STALL_WAIT, stream.next()).await {
            Ok(Some(Ok(chunk))) => buffer.extend_from_slice(&chunk),
            Ok(Some(Err(e))) => {
                // Keep what landed; the retry resumes from it.
                let _ = file.flush();
                return Attempt::Retry(format!("stream: {e}"));
            }
            Ok(None) => break,
            Err(_) => {
                let _ = file.flush();
                return Attempt::Retry("stream: stalled".into());
            }
        }
        // Checked before the write too: a remove must win over this write.
        if let Some(stop) = stop_attempt(flags) {
            let _ = file.flush();
            return stop;
        }
        received += buffer.len() as u64;
        if file.write_all(&buffer).is_err() {
            return Attempt::Retry("write partial".into());
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
    if let Some(expected) = downloads.expected_size(id)
        && received != expected
    {
        return Attempt::Retry(format!("size {received} of {expected}"));
    }
    Attempt::Done
}

/// The `Content-Range` header of a response.
fn range_header(response: &reqwest::Response) -> Option<&str> {
    response.headers().get("content-range")?.to_str().ok()
}

/// The total in a `Content-Range` header; the 416 form (`bytes */total`)
/// reads too.
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
    fn the_wire_shape_is_camel_case() {
        let json = serde_json::to_string(&Progress {
            id: "x".into(),
            phase: "downloading".into(),
            received: 1,
            total: Some(2),
            message: None,
            path: None,
        })
        .unwrap();
        assert!(json.contains("\"received\":1"));
        assert!(json.contains("\"total\":2"));
        assert!(!json.contains("file_name"));
    }

    #[test]
    fn names_and_directories_stay_inside_the_data_dir() {
        assert!(is_plain_name("cefr.parquet.part"));
        assert!(!is_plain_name(""));
        assert!(!is_plain_name("."));
        assert!(!is_plain_name(".."));
        assert!(!is_plain_name("a/b"));
        assert!(!is_plain_name("a\\b"));
        assert!(is_relative("cefr"));
        assert!(is_relative("a/b"));
        assert!(!is_relative(""));
        assert!(!is_relative("/abs"));
        assert!(!is_relative("../cefr"));
        assert!(!is_relative("a/../b"));
    }

    #[test]
    fn the_link_list_is_primary_then_mirrors() {
        let req = DownloadRequest {
            id: "x".into(),
            url: "https://a".into(),
            mirrors: vec!["https://b".into(), "https://c".into()],
            ..Default::default()
        };
        assert_eq!(req.urls(), ["https://a", "https://b", "https://c"]);
        // Rotation wraps back to the primary.
        assert_eq!(req.urls()[3 % req.urls().len()], "https://a");
    }

    #[test]
    fn a_request_wire_defaults_to_no_mirrors() {
        let req: DownloadRequest =
            serde_json::from_str(r#"{"id":"x","url":"u","fileName":"f"}"#).unwrap();
        assert_eq!(req.file_name, "f");
        assert!(req.mirrors.is_empty());
        assert!(req.expected_size.is_none());
        assert_eq!(req.urls(), ["u"]);
    }
}
