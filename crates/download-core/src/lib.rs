//! Background downloads: resumable and pausable by id.

use std::collections::HashMap;
use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A download the frontend asks for. `directory` is host-data-relative;
/// the file lands at `<data_dir>/<directory>/<file_name>`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRequest {
    /// The key every later call and event uses. One download per id.
    pub id: String,
    pub url: String,
    pub directory: Option<String>,
    pub file_name: String,
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
    url: String,
    dest: PathBuf,
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

/// Retry budget; the backoff doubles per attempt, capped.
const MAX_ATTEMPTS: u32 = 4;
fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64.saturating_mul(1 << attempt.min(4)))
}

/// Progress snapshots are throttled to this pace; a phase change always
/// publishes.
const EMIT_EVERY: Duration = Duration::from_millis(100);

/// The process-wide downloader, shared by every feature and host.
#[derive(Clone, Default)]
pub struct Downloads {
    slots: Arc<Mutex<HashMap<String, Slot>>>,
}

impl Downloads {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin `req`; an active id is refused.
    pub fn start<H: Host>(
        &self,
        host: &H,
        req: DownloadRequest,
        on_progress: Option<ProgressHook>,
    ) -> Result<(), String> {
        let dest = Self::dest(host, &req)?;
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
                url: req.url,
                dest,
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
            let dest = slots.get(id).map(|s| s.dest.clone());
            if dest.is_some() {
                slots.remove(id);
            }
            dest
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
        let dir = match &req.directory {
            Some(sub) => host.data_dir()?.join(sub),
            None => host.data_dir()?,
        };
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
        let mut attempt: u32 = 0;
        loop {
            let slot_view = self
                .slots
                .lock()
                .ok()
                .and_then(|slots| slots.get(&id).map(|s| (s.url.clone(), s.dest.clone())));
            let Some((url, dest)) = slot_view else {
                return; // removed mid-flight
            };
            match attempt_once(self, host, &id, &url, &dest).await {
                Attempt::Done => {
                    self.finish(host, &id, "done");
                    return;
                }
                Attempt::Paused => {
                    self.publish(host, &id, Some("paused"));
                    return;
                }
                Attempt::Cancelled => {
                    self.publish(host, &id, Some("cancelled"));
                    return;
                }
                Attempt::Restart => continue,
                Attempt::Retry(message) => {
                    attempt += 1;
                    if attempt >= MAX_ATTEMPTS {
                        self.fail(host, &id, format!("download failed: {message}"));
                        return;
                    }
                    tokio::time::sleep(backoff(attempt)).await;
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
}

/// One HTTP attempt, resuming from whatever the partial already holds.
async fn attempt_once<H: Host>(
    downloads: &Downloads,
    host: &H,
    id: &str,
    url: &str,
    dest: &PathBuf,
) -> Attempt {
    let existing = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
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
        // The server ignored the range: the partial is overwritten.
        reqwest::StatusCode::OK => total = response.content_length(),
        reqwest::StatusCode::RANGE_NOT_SATISFIABLE => {
            if existing > 0 {
                let _ = std::fs::remove_file(dest);
                return Attempt::Restart;
            }
            return Attempt::Retry("range not satisfiable".into());
        }
        status => return Attempt::Retry(format!("server said {status}")),
    }

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

    let flags = downloads.flags(id);
    let mut last_emit = Instant::now();
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::with_capacity(64 * 1024);
    loop {
        if let Some((cancel, pause)) = &flags {
            if cancel.load(Ordering::SeqCst) {
                let _ = file.flush();
                return Attempt::Cancelled;
            }
            if pause.load(Ordering::SeqCst) {
                let _ = file.flush();
                return Attempt::Paused;
            }
        }
        buffer.clear();
        match stream.next().await {
            Some(Ok(chunk)) => buffer.extend_from_slice(&chunk),
            Some(Err(e)) => {
                // Keep what landed; the retry resumes from it.
                let _ = file.flush();
                return Attempt::Retry(format!("stream: {e}"));
            }
            None => break,
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
    Attempt::Done
}

impl Downloads {
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
        self.publish(host, id, None);
    }
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
}
