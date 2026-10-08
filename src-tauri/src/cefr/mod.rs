//! The dataset's backend: resumable download, sqlite rebuild, lookups.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// The built dataset parquet, served straight from the cefr-rs repository.
pub const DATASET_URL: &str =
    "https://raw.githubusercontent.com/codewiththiha/cefr-rs/main/data/cefr.zstd.parquet";

/// The event the webview subscribes to for every dataset phase change.
pub const PROGRESS_EVENT: &str = "cefr-dataset-progress";

/// Progress payload, also the answer of the status command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetStatus {
    /// `absent` | `downloading` | `converting` | `ready` | `failed`.
    pub phase: String,
    pub received: u64,
    pub total: Option<u64>,
    pub words: Option<u64>,
    pub message: Option<String>,
}

/// The dataset's phase, guarded so commands and the download task agree.
enum Phase {
    Absent,
    Downloading { received: u64, total: Option<u64> },
    Converting,
    Ready { words: u64 },
    Failed { message: String },
}

impl Phase {
    /// The wire snapshot of this phase.
    fn snapshot(&self) -> DatasetStatus {
        match self {
            Phase::Absent => DatasetStatus {
                phase: "absent".into(),
                received: 0,
                total: None,
                words: None,
                message: None,
            },
            Phase::Downloading { received, total } => DatasetStatus {
                phase: "downloading".into(),
                received: *received,
                total: *total,
                words: None,
                message: None,
            },
            Phase::Converting => DatasetStatus {
                phase: "converting".into(),
                received: 0,
                total: None,
                words: None,
                message: None,
            },
            Phase::Ready { words } => DatasetStatus {
                phase: "ready".into(),
                received: 0,
                total: None,
                words: Some(*words),
                message: None,
            },
            Phase::Failed { message } => DatasetStatus {
                phase: "failed".into(),
                received: 0,
                total: None,
                words: None,
                message: Some(message.clone()),
            },
        }
    }
}

/// A download attempt's outcome.
enum Attempt {
    /// The parquet is complete on disk.
    Done,
    /// The partial was rejected (416); restart from zero without burning a
    /// retry.
    Restart,
    /// A network or server failure; the outer loop retries from the bytes
    /// already on disk.
    Retry(String),
    /// The user asked to stop.
    Cancelled,
}

/// Network retry budget; backoff doubles per attempt, capped.
const MAX_ATTEMPTS: u32 = 4;
fn backoff(attempt: u32) -> Duration {
    Duration::from_millis(500u64.saturating_mul(1 << attempt.min(4)))
}

/// Progress events are throttled to this pace; a phase change always emits.
const EMIT_EVERY: Duration = Duration::from_millis(100);
/// The largest batch one lookup may carry; the frontend's pages stay far
/// below it.
const MAX_BATCH: usize = 4_000;

/// The manager: one per process, shared by every pane's commands.
pub struct CefrManager {
    phase: Mutex<Phase>,
    cancel: AtomicBool,
    /// The opened dataset, reused across lookups; dropped on remove.
    db: Mutex<Option<cefr::db::CefrDb>>,
}

impl CefrManager {
    pub fn new() -> Self {
        Self {
            phase: Mutex::new(Phase::Absent),
            cancel: AtomicBool::new(false),
            db: Mutex::new(None),
        }
    }

    /// The dataset directory, created on demand.
    fn dir(app: &AppHandle) -> Result<PathBuf, String> {
        let base = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("app data dir: {e}"))?;
        let dir = base.join("cefr");
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        Ok(dir)
    }

    fn parquet_partial(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join("cefr.parquet.part"))
    }

    fn db_final(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join("cefr.db"))
    }

    fn db_building(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join("cefr.db.tmp"))
    }

    /// The live phase, or the disk when this process has not touched it yet.
    pub fn status(&self, app: &AppHandle) -> Result<DatasetStatus, String> {
        let mut guard = self.phase.lock().map_err(|_| "phase lock poisoned")?;
        if matches!(*guard, Phase::Absent) && Self::db_final(app)?.is_file() {
            // A dataset from an earlier run: adopt it without a rebuild.
            *guard = match Self::probe_db(&Self::db_final(app)?) {
                Ok(words) => Phase::Ready { words },
                Err(message) => Phase::Failed { message },
            };
        }
        Ok(guard.snapshot())
    }

    /// A finished dataset answers a count and the version mark; anything
    /// else reads as corrupt.
    fn probe_db(path: &Path) -> Result<u64, String> {
        let db = cefr::db::CefrDb::open(path).map_err(|e| format!("dataset: {e}"))?;
        let version: i64 = db
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| format!("dataset unreadable: {e}"))?;
        if version != 1 {
            return Err("dataset was never finished".into());
        }
        let words: i64 = db
            .conn()
            .query_row("SELECT COUNT(*) FROM cefr", [], |r| r.get(0))
            .map_err(|e| format!("dataset unreadable: {e}"))?;
        Ok(words.max(0) as u64)
    }

    /// Whether a download may start: not while one runs.
    pub fn begin_download(&self, app: AppHandle) -> Result<(), String> {
        {
            let mut guard = self.phase.lock().map_err(|_| "phase lock poisoned")?;
            match &*guard {
                Phase::Downloading { .. } | Phase::Converting => {
                    return Err("a download is already running".into());
                }
                _ => {}
            }
            *guard = Phase::Downloading {
                received: 0,
                total: None,
            };
            self.cancel.store(false, Ordering::SeqCst);
        }
        let _ = app.emit(PROGRESS_EVENT, guard_snapshot(self));
        tauri::async_runtime::spawn(async move {
            run_download(app).await;
        });
        Ok(())
    }

    /// Ask the running download to stop; the partial stays for a resume.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Drop the dataset and its partials; the next download starts over.
    pub fn remove(&self, app: &AppHandle) -> Result<(), String> {
        self.cancel.store(true, Ordering::SeqCst);
        let mut guard = self.phase.lock().map_err(|_| "phase lock poisoned")?;
        // A mid-flight download re-checks the flag and stands down, silent.
        *guard = Phase::Absent;
        drop(guard);
        if let Ok(db) = Self::db_final(app) {
            let _ = std::fs::remove_file(db);
        }
        if let Ok(tmp) = Self::db_building(app) {
            let _ = std::fs::remove_file(tmp);
        }
        if let Ok(part) = Self::parquet_partial(app) {
            let _ = std::fs::remove_file(part);
        }
        if let Ok(mut slot) = self.db.lock() {
            *slot = None;
        }
        Ok(())
    }

    /// Levels for `words`, aligned with the input; the db opens lazily.
    pub fn levels(&self, app: &AppHandle, words: &[String]) -> Result<Vec<Option<f64>>, String> {
        let db_path = Self::db_final(app)?;
        let mut slot = self.db.lock().map_err(|_| "db lock poisoned")?;
        if slot.is_none() && db_path.is_file() {
            *slot = Some(cefr::db::CefrDb::open(&db_path).map_err(|e| format!("dataset: {e}"))?);
        }
        let Some(db) = slot.as_ref() else {
            return Ok(vec![None; words.len()]);
        };
        // One normalize per word; unanswerable words keep their place as
        // `None` in the aligned answer.
        let normalized: Vec<Option<String>> = words
            .iter()
            .take(MAX_BATCH)
            .map(|w| cefr_core::is_english_ascii(w).then(|| cefr_core::normalize(w)))
            .collect();
        let pairs: Vec<(String, String)> = normalized
            .iter()
            .flatten()
            .map(|key| (key.clone(), String::new()))
            .collect();
        let found = db
            .lookup_batch(&pairs)
            .map_err(|e| format!("lookup: {e}"))?;
        let mut out: Vec<Option<f64>> = normalized
            .iter()
            .map(|key| {
                key.as_ref()
                    .and_then(|k| found.get(&(k.clone(), String::new())).copied())
            })
            .collect();
        out.resize(words.len(), None);
        Ok(out)
    }
}

impl Default for CefrManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Read the phase under guard, unlocked afterwards, for the emit.
fn guard_snapshot(manager: &CefrManager) -> DatasetStatus {
    manager
        .phase
        .lock()
        .map(|guard| guard.snapshot())
        .unwrap_or_else(|_| Phase::Absent.snapshot())
}

/// One HTTP attempt, resuming from whatever the partial already holds.
async fn attempt_once(
    app: &AppHandle,
    manager: &CefrManager,
    partial: &Path,
) -> Result<Attempt, String> {
    let existing = std::fs::metadata(partial).map(|m| m.len()).unwrap_or(0);
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("client: {e}"))?;
    let mut request = client.get(DATASET_URL);
    if existing > 0 {
        request = request.header("Range", format!("bytes={existing}-"));
    }
    let response = request.send().await.map_err(|e| format!("connect: {e}"))?;

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
        reqwest::StatusCode::OK => {
            // The server ignored the range: whatever the partial held is
            // about to be overwritten.
            total = response.content_length();
        }
        reqwest::StatusCode::RANGE_NOT_SATISFIABLE => {
            // The partial outruns the file: one clean restart, no retry spent.
            if existing > 0 {
                let _ = std::fs::remove_file(partial);
                return Ok(Attempt::Restart);
            }
            return Ok(Attempt::Retry("range not satisfiable".into()));
        }
        status => {
            return Ok(Attempt::Retry(format!("server said {status}")));
        }
    }

    let mut file = if append {
        std::fs::OpenOptions::new()
            .append(true)
            .open(partial)
            .map_err(|e| format!("open partial: {e}"))?
    } else {
        std::fs::File::create(partial).map_err(|e| format!("create partial: {e}"))?
    };
    let mut received = if append { existing } else { 0 };
    publish(app, manager, received, total);

    let mut last_emit = std::time::Instant::now();
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::with_capacity(64 * 1024);
    loop {
        if manager.cancel.load(Ordering::SeqCst) {
            return Ok(Attempt::Cancelled);
        }
        buffer.clear();
        match stream.next().await {
            Some(Ok(chunk)) => buffer.extend_from_slice(&chunk),
            Some(Err(e)) => {
                // Keep what landed; the retry resumes from it.
                let _ = file.flush();
                return Ok(Attempt::Retry(format!("stream: {e}")));
            }
            None => break,
        }
        received += buffer.len() as u64;
        file.write_all(&buffer)
            .map_err(|e| format!("write partial: {e}"))?;
        if last_emit.elapsed() >= EMIT_EVERY {
            last_emit = std::time::Instant::now();
            publish(app, manager, received, total);
        }
    }
    file.flush().map_err(|e| format!("flush partial: {e}"))?;

    // Size check, then the parquet magic: truncation must not reach it.
    if let Some(total) = total
        && received != total
    {
        return Ok(Attempt::Retry(format!("size {received} of {total}")));
    }
    let _ = file.sync_all();
    drop(file);
    verify_parquet(partial)?;
    Ok(Attempt::Done)
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

/// The four-byte signature a real parquet carries at both ends.
fn verify_parquet(path: &Path) -> Result<(), String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("reopen partial: {e}"))?;
    let len = file
        .metadata()
        .map_err(|e| format!("stat partial: {e}"))?
        .len();
    if len < 8 {
        return Err(format!("dataset too small ({len} bytes)"));
    }
    let mut head = [0u8; 4];
    file.read_exact(&mut head)
        .map_err(|e| format!("read head: {e}"))?;
    if &head != b"PAR1" {
        return Err("not a parquet file".into());
    }
    let mut tail = [0u8; 4];
    file.seek(SeekFrom::Start(len - 4))
        .map_err(|e| format!("seek tail: {e}"))?;
    file.read_exact(&mut tail)
        .map_err(|e| format!("read tail: {e}"))?;
    if &tail != b"PAR1" {
        return Err("dataset is truncated".into());
    }
    Ok(())
}

/// Publish a progress snapshot: read the caller's numbers, write the phase,
/// emit outside the lock.
fn publish(app: &AppHandle, manager: &CefrManager, received: u64, total: Option<u64>) {
    let status = {
        let Ok(mut guard) = manager.phase.lock() else {
            return;
        };
        match &*guard {
            Phase::Downloading { .. } => {
                *guard = Phase::Downloading { received, total };
                guard.snapshot()
            }
            // A remove or cancel owns the phase now; this task stays silent.
            _ => return,
        }
    };
    let _ = app.emit(PROGRESS_EVENT, &status);
}

/// The whole download: attempts with backoff, then the rebuild, then the
/// parquet's removal.
async fn run_download(app: AppHandle) {
    let partial = match CefrManager::parquet_partial(&app) {
        Ok(path) => path,
        Err(message) => {
            if let Some(manager) = app.try_state::<CefrManager>() {
                finish_failed(&app, &manager, message);
            }
            return;
        }
    };
    let manager = app.state::<CefrManager>();

    let mut attempt: u32 = 0;
    let outcome = loop {
        if manager.cancel.load(Ordering::SeqCst) {
            break Ok(());
        }
        match attempt_once(&app, &manager, &partial).await {
            Ok(Attempt::Done) => break Ok(()),
            Ok(Attempt::Cancelled) => break Ok(()),
            Ok(Attempt::Restart) => continue,
            Ok(Attempt::Retry(message)) => {
                attempt += 1;
                if attempt >= MAX_ATTEMPTS {
                    break Err(format!("download failed: {message}"));
                }
                tokio::time::sleep(backoff(attempt)).await;
            }
            Err(message) => break Err(message),
        }
    };

    if let Err(message) = outcome {
        finish_failed(&app, &manager, message);
        return;
    }
    if manager.cancel.load(Ordering::SeqCst) {
        // A cancel during the tail: stand down, keep the partial.
        if let Ok(mut guard) = manager.phase.lock() {
            *guard = Phase::Absent;
        }
        return;
    }

    // Rebuild off the async pool, to a temp name, then adopt atomically.
    set_phase(&app, &manager, Phase::Converting);
    let (building, final_db) = match (CefrManager::db_building(&app), CefrManager::db_final(&app)) {
        (Ok(building), Ok(ready)) => (building, ready),
        (Err(message), _) | (_, Err(message)) => {
            finish_failed(&app, &manager, message);
            return;
        }
    };
    let build_path = building.clone();
    let parquet = partial.clone();
    let built =
        tauri::async_runtime::spawn_blocking(move || cefr::db::build_db(&parquet, &build_path))
            .await;
    match built {
        Ok(Ok(_stats)) => {}
        Ok(Err(e)) => {
            // A parquet that parses but does not rebuild is bad data: drop it.
            let _ = std::fs::remove_file(&partial);
            let _ = std::fs::remove_file(&building);
            finish_failed(&app, &manager, format!("rebuild failed: {e}"));
            return;
        }
        Err(e) => {
            let _ = std::fs::remove_file(&building);
            finish_failed(&app, &manager, format!("rebuild worker: {e}"));
            return;
        }
    }
    if let Err(e) = std::fs::rename(&building, &final_db) {
        let _ = std::fs::remove_file(&building);
        finish_failed(&app, &manager, format!("adopt dataset: {e}"));
        return;
    }
    let words = match CefrManager::probe_db(&final_db) {
        Ok(words) => words,
        Err(message) => {
            finish_failed(&app, &manager, message);
            return;
        }
    };
    // The parquet's job is done; only the db is kept.
    let _ = std::fs::remove_file(&partial);
    if let Ok(mut slot) = manager.db.lock() {
        *slot = None;
    }
    set_phase(&app, &manager, Phase::Ready { words });
}

/// One guarded write of a phase, then the event outside the lock.
fn set_phase(app: &AppHandle, manager: &CefrManager, phase: Phase) {
    let status = {
        let Ok(mut guard) = manager.phase.lock() else {
            return;
        };
        *guard = phase;
        guard.snapshot()
    };
    let _ = app.emit(PROGRESS_EVENT, &status);
}

fn finish_failed(app: &AppHandle, manager: &CefrManager, message: String) {
    set_phase(app, manager, Phase::Failed { message });
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
    fn the_parquet_signature_check_rejects_html_error_bodies() {
        let dir = std::env::temp_dir().join(format!("cefr_mgr_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("fake.parquet");
        std::fs::write(&path, b"<html>Not Found</html>").unwrap();
        assert!(verify_parquet(&path).is_err());
        // Truncated real header: magic passes, tail cannot.
        std::fs::write(&path, b"PAR1").unwrap();
        assert!(verify_parquet(&path).is_err());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn phase_snapshots_use_the_camel_case_wire() {
        let json = serde_json::to_string(
            &Phase::Downloading {
                received: 1024,
                total: Some(2048),
            }
            .snapshot(),
        )
        .unwrap();
        assert!(json.contains("\"received\":1024"));
        assert!(json.contains("\"total\":2048"));
        assert!(!json.contains("received_"));

        let failed = serde_json::to_string(
            &Phase::Failed {
                message: "no net".into(),
            }
            .snapshot(),
        )
        .unwrap();
        assert!(failed.contains("\"phase\":\"failed\""));
        assert!(failed.contains("\"message\":\"no net\""));
    }
}
