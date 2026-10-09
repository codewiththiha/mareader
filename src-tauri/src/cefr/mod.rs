//! The dataset's backend: download, parquet-to-sqlite rebuild, lookups.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::download::{AppDownloads, Phase, Progress, Receipt, TauriHost};

pub mod fetch;
mod lookup;

pub use lookup::PosAnswer;

/// The event the webview subscribes to for every dataset phase change.
pub const PROGRESS_EVENT: &str = "cefr-dataset-progress";

/// The wire snapshot: the dataset's phase and the tagger model's own.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetStatus {
    /// `absent` | `downloading` | `paused` | `converting` | `ready` |
    /// `failed`.
    pub phase: String,
    pub received: u64,
    pub total: Option<u64>,
    pub speed: Option<f64>,
    pub eta_secs: Option<u64>,
    pub words: Option<u64>,
    pub message: Option<String>,
    /// The click-time tagger: `absent` | `downloading` | `ready` | `failed`.
    pub tagger: String,
    /// The tagger model's own percent, while it is inbound.
    pub tagger_percent: Option<u32>,
}

/// The dataset's stage, guarded so commands and the transport agree.
#[derive(Debug, Clone)]
enum Stage {
    Absent,
    Downloading {
        received: u64,
        total: Option<u64>,
        speed: Option<f64>,
        eta: Option<u64>,
    },
    Paused {
        received: u64,
        total: Option<u64>,
    },
    Converting,
    Ready {
        words: u64,
    },
    Failed {
        message: String,
    },
}

impl Stage {
    /// This stage's half of the wire snapshot.
    fn status(&self, tagger: TaggerState) -> DatasetStatus {
        let (phase, received, total, speed, eta_secs, words, message) = match self {
            Stage::Absent => ("absent", 0, None, None, None, None, None),
            Stage::Downloading {
                received,
                total,
                speed,
                eta,
            } => ("downloading", *received, *total, *speed, *eta, None, None),
            Stage::Paused { received, total } => {
                ("paused", *received, *total, None, None, None, None)
            }
            Stage::Converting => ("converting", 0, None, None, None, None, None),
            Stage::Ready { words } => ("ready", 0, None, None, None, Some(*words), None),
            Stage::Failed { message } => {
                ("failed", 0, None, None, None, None, Some(message.as_str()))
            }
        };
        DatasetStatus {
            phase: phase.into(),
            received,
            total,
            speed,
            eta_secs,
            words,
            message: message.map(str::to_string),
            tagger: tagger.phase.into(),
            tagger_percent: tagger.percent,
        }
    }
}

/// The tagger model's own state, beside the dataset's.
#[derive(Debug, Clone, Copy)]
struct TaggerState {
    /// `absent` | `downloading` | `ready` | `failed`.
    phase: &'static str,
    percent: Option<u32>,
}

impl Default for TaggerState {
    /// A derived default would be `""`, which the adoption check misses.
    fn default() -> Self {
        Self {
            phase: "absent",
            percent: None,
        }
    }
}

/// The files this feature owns, all under one directory.
struct Paths {
    dir: PathBuf,
    parquet: PathBuf,
    model: PathBuf,
    building: PathBuf,
    db: PathBuf,
}

/// The manager: one per process, shared by every pane's commands.
pub struct CefrManager {
    stage: Mutex<Stage>,
    tagger: Mutex<TaggerState>,
    /// The opened dataset, reused across lookups; dropped on remove.
    db: Mutex<Option<cefr::db::CefrDb>>,
    /// The loaded tagger model, reused across clicks.
    model: Mutex<Option<Arc<cefr::pos::Tagger>>>,
    /// One claim per landing; both waiters resolve at one ending.
    claimed: AtomicBool,
    /// Bumped when a landing is dropped; a build re-checks it.
    generation: AtomicU64,
}

impl Default for CefrManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CefrManager {
    pub fn new() -> Self {
        Self {
            stage: Mutex::new(Stage::Absent),
            tagger: Mutex::new(TaggerState::default()),
            db: Mutex::new(None),
            model: Mutex::new(None),
            claimed: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    /// The feature's directory, created on demand.
    fn dir(app: &AppHandle) -> Result<PathBuf, String> {
        let dir = TauriHost::new(app.clone()).feature_dir(fetch::FEATURE)?;
        std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
        Ok(dir)
    }

    fn paths(app: &AppHandle) -> Result<Paths, String> {
        let dir = Self::dir(app)?;
        Ok(Paths {
            parquet: fetch::dataset_file(&dir),
            model: fetch::tagger_file(&dir),
            building: fetch::db_building(&dir),
            db: fetch::db_final(&dir),
            dir,
        })
    }

    /// The live stage, or the disk when this process has not touched it yet.
    pub fn status(&self, app: &AppHandle) -> Result<DatasetStatus, String> {
        let paths = Self::paths(app)?;
        {
            let mut guard = self.stage.lock().map_err(|_| "stage lock poisoned")?;
            // A dataset from an earlier run: adopt it without a rebuild.
            if matches!(*guard, Stage::Absent) && paths.db.is_file() {
                *guard = match Self::probe_db(&paths.db) {
                    Ok(words) => Stage::Ready { words },
                    Err(message) => Stage::Failed { message },
                };
            }
        }
        // A model from an earlier run is likewise already there.
        if paths.model.is_file()
            && let Ok(mut slot) = self.tagger.lock()
            && slot.phase == "absent"
        {
            slot.phase = "ready";
        }
        Ok(self.snapshot())
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

    /// The wire snapshot. Neither lock is held while the other is taken.
    fn snapshot(&self) -> DatasetStatus {
        let tagger = self.tagger.lock().map(|slot| *slot).unwrap_or_default();
        let stage = self
            .stage
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or(Stage::Absent);
        stage.status(tagger)
    }

    /// Publish the snapshot both guards describe.
    fn emit(&self, app: &AppHandle) {
        let _ = app.emit(PROGRESS_EVENT, self.snapshot());
    }

    /// Hand both files to the downloader, then await the dataset's landing.
    pub fn begin_download(&self, app: AppHandle) -> Result<(), String> {
        let paths = Self::paths(&app)?;
        {
            let mut guard = self.stage.lock().map_err(|_| "stage lock poisoned")?;
            match &*guard {
                Stage::Downloading { .. } | Stage::Paused { .. } | Stage::Converting => {
                    return Err("a download is already running".into());
                }
                _ => {}
            }
            *guard = Stage::Downloading {
                received: 0,
                total: None,
                speed: None,
                eta: None,
            };
        }
        // A new landing is its own build; a stale one must stand down.
        self.claimed.store(false, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
        // The click-time tagger rides along; one tap equips both.
        self.request_tagger(&app, &paths.dir);

        let host = TauriHost::new(app.clone());
        let watched = app.clone();
        let job = fetch::dataset_job(paths.dir.clone())
            .on_progress(move |progress| dataset_progress(&watched, progress));
        let receipt = match app.state::<AppDownloads>().start(&host, job) {
            Ok(receipt) => receipt,
            Err(message) => {
                // A refused start must not leave the stage downloading.
                set_stage(
                    &app,
                    Stage::Failed {
                        message: message.clone(),
                    },
                );
                return Err(message);
            }
        };
        self.emit(&app);
        Self::spawn_landing(app, receipt);
        Ok(())
    }

    /// Own one receipt's landing: rebuild on a file, report on an ending.
    fn spawn_landing(app: AppHandle, receipt: Receipt) {
        tauri::async_runtime::spawn(async move {
            match receipt.finished().await {
                Ok(_) => convert(app.clone()).await,
                Err(_) => settled(&app),
            }
        });
    }

    /// Ask for the tagger model, reporting its progress beside the dataset's.
    fn request_tagger(&self, app: &AppHandle, dir: &Path) {
        let watched = app.clone();
        let job = fetch::tagger_job(dir.to_path_buf())
            .on_progress(move |progress| tagger_progress(&watched, progress));
        let host = TauriHost::new(app.clone());
        let _ = app.state::<AppDownloads>().start(&host, job);
    }

    /// Stop reading; the partial stays and resume continues from it.
    pub fn pause_download(&self, app: &AppHandle) {
        app.state::<AppDownloads>().pause(fetch::DATASET_ID);
    }

    /// Continue a paused download, and own its landing like a first start.
    pub fn resume_download(&self, app: &AppHandle) -> Result<(), String> {
        let host = TauriHost::new(app.clone());
        let receipt = app
            .state::<AppDownloads>()
            .resume(&host, fetch::DATASET_ID)?;
        Self::spawn_landing(app.clone(), receipt);
        Ok(())
    }

    /// Ask the transport to stop; the partial stays for the next start.
    pub fn cancel(&self, app: &AppHandle) {
        app.state::<AppDownloads>().cancel(fetch::DATASET_ID);
    }

    /// Drop both files, the database and every cached handle.
    pub fn remove(&self, app: &AppHandle) -> Result<(), String> {
        // A build in flight must see this and stay deleted.
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.claimed.store(false, Ordering::SeqCst);
        let downloads = app.state::<AppDownloads>();
        downloads.remove(fetch::DATASET_ID);
        downloads.remove(fetch::TAGGER_ID);
        let paths = Self::paths(app)?;
        {
            let mut guard = self.stage.lock().map_err(|_| "stage lock poisoned")?;
            *guard = Stage::Absent;
        }
        if let Ok(mut slot) = self.tagger.lock() {
            *slot = TaggerState::default();
        }
        if let Ok(mut slot) = self.db.lock() {
            *slot = None;
        }
        if let Ok(mut slot) = self.model.lock() {
            *slot = None;
        }
        // Records from an earlier run own no files; the paths do.
        for file in [&paths.parquet, &paths.model, &paths.db, &paths.building] {
            let _ = std::fs::remove_file(file);
        }
        download_core::discard(&paths.parquet);
        download_core::discard(&paths.model);
        self.emit(app);
        Ok(())
    }

    /// The loaded tagger, or `None` while the model is still inbound.
    pub(super) fn tagger(&self, app: &AppHandle) -> Result<Option<Arc<cefr::pos::Tagger>>, String> {
        let paths = Self::paths(app)?;
        let mut slot = self.model.lock().map_err(|_| "model lock poisoned")?;
        if let Some(tagger) = slot.as_ref() {
            return Ok(Some(tagger.clone()));
        }
        if !paths.model.is_file() {
            // Self-heal: ask for it; a later click loads what lands.
            drop(slot);
            self.request_tagger(app, &paths.dir);
            return Ok(None);
        }
        let loaded = Arc::new(
            cefr::pos::Tagger::from_model_path(&paths.model)
                .map_err(|e| format!("tagger model: {e}"))?,
        );
        *slot = Some(loaded.clone());
        Ok(Some(loaded))
    }
}

/// Mirror one transport snapshot into the dataset's own stage.
fn dataset_progress(app: &AppHandle, progress: &Progress) {
    // The receipt owns the endings; this hook only narrates the middle.
    let stage = match progress.phase {
        Phase::Done | Phase::Failed | Phase::Cancelled => return,
        Phase::Verifying => Stage::Converting,
        Phase::Paused => Stage::Paused {
            received: progress.received,
            total: progress.total,
        },
        _ => Stage::Downloading {
            received: progress.received,
            total: progress.total,
            speed: progress.speed,
            eta: progress.eta_secs,
        },
    };
    set_stage(app, stage);
}

/// Mirror one transport snapshot into the tagger model's own state.
fn tagger_progress(app: &AppHandle, progress: &Progress) {
    let manager = app.state::<CefrManager>();
    let state = match progress.phase {
        Phase::Done => TaggerState {
            phase: "ready",
            percent: Some(100),
        },
        Phase::Failed => TaggerState {
            phase: "failed",
            percent: None,
        },
        Phase::Cancelled => TaggerState::default(),
        _ => TaggerState {
            phase: "downloading",
            percent: progress.percent().map(u32::from),
        },
    };
    if let Ok(mut slot) = manager.tagger.lock() {
        *slot = state;
    }
    manager.emit(app);
}

/// The receipt ended without a file: the record says which ending it was.
fn settled(app: &AppHandle) {
    let record = app.state::<AppDownloads>().status(fetch::DATASET_ID);
    // remove() woke this receipt and already spoke for the stage.
    if record.is_none() {
        return;
    }
    let cancelled = record
        .as_ref()
        .is_some_and(|progress| progress.phase == Phase::Cancelled);
    let message = record
        .and_then(|progress| progress.message)
        .unwrap_or_else(|| "the download did not finish".to_string());
    let manager = app.state::<CefrManager>();
    let stage = if cancelled {
        Stage::Absent
    } else {
        Stage::Failed { message }
    };
    {
        let Ok(mut guard) = manager.stage.lock() else {
            return;
        };
        *guard = stage;
    }
    manager.emit(app);
}

/// One guarded write of a stage, then the event outside the lock.
fn set_stage(app: &AppHandle, stage: Stage) {
    let manager = app.state::<CefrManager>();
    if let Ok(mut guard) = manager.stage.lock() {
        *guard = stage;
    }
    manager.emit(app);
}

/// The dataset's own stages, after the transport landed the parquet.
async fn convert(app: AppHandle) {
    let manager = app.state::<CefrManager>();
    // A start's receipt and a resume's both land here; one claim wins.
    if manager.claimed.swap(true, Ordering::SeqCst) {
        return;
    }
    let generation = manager.generation.load(Ordering::SeqCst);
    set_stage(&app, Stage::Converting);
    let paths = match CefrManager::paths(&app) {
        Ok(paths) => paths,
        Err(message) => return land(&app, failed(&message)),
    };
    let (source, target) = (paths.parquet.clone(), paths.building.clone());
    let built =
        tauri::async_runtime::spawn_blocking(move || cefr::db::build_db(&source, &target)).await;
    match built {
        Ok(Ok(_stats)) => {}
        Ok(Err(e)) => {
            // A parquet that will not rebuild is not usable; fetch again.
            let _ = std::fs::remove_file(&paths.parquet);
            let _ = std::fs::remove_file(&paths.building);
            return land(&app, failed(&format!("rebuild failed: {e}")));
        }
        Err(e) => {
            let _ = std::fs::remove_file(&paths.building);
            return land(&app, failed(&format!("rebuild worker: {e}")));
        }
    }
    // A remove mid-build has already deleted the answer; stay deleted.
    if manager.generation.load(Ordering::SeqCst) != generation {
        let _ = std::fs::remove_file(&paths.building);
        return;
    }
    if let Err(e) = std::fs::rename(&paths.building, &paths.db) {
        let _ = std::fs::remove_file(&paths.building);
        // A remove that landed mid-adopt already owns the story.
        if manager.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        return land(&app, failed(&format!("adopt dataset: {e}")));
    }
    if manager.generation.load(Ordering::SeqCst) != generation {
        // A remove landed around the rename; its deletion must stay.
        let _ = std::fs::remove_file(&paths.db);
        return;
    }
    // The parquet's job is done; only the database is kept.
    let _ = std::fs::remove_file(&paths.parquet);
    match CefrManager::probe_db(&paths.db) {
        Ok(words) => {
            if let Ok(mut slot) = manager.db.lock() {
                *slot = None;
            }
            land(&app, Stage::Ready { words });
        }
        Err(message) => {
            let _ = std::fs::remove_file(&paths.db);
            land(
                &app,
                Stage::Failed {
                    message: message.clone(),
                },
            );
        }
    }
}

/// Land a rebuild's ending, unless a remove already took the stage back.
fn land(app: &AppHandle, stage: Stage) {
    let manager = app.state::<CefrManager>();
    {
        let Ok(mut guard) = manager.stage.lock() else {
            return;
        };
        if !matches!(*guard, Stage::Converting) {
            return;
        }
        *guard = stage;
    }
    manager.emit(app);
}

fn failed(message: &str) -> Stage {
    Stage::Failed {
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(stage: &Stage, tagger: TaggerState) -> String {
        serde_json::to_string(&stage.status(tagger)).unwrap()
    }

    #[test]
    fn the_snapshot_uses_the_camel_case_wire() {
        let downloading = Stage::Downloading {
            received: 1024,
            total: Some(2048),
            speed: Some(512.0),
            eta: Some(3),
        };
        let json = json(&downloading, TaggerState::default());
        assert!(json.contains("\"phase\":\"downloading\""));
        assert!(json.contains("\"received\":1024"));
        assert!(json.contains("\"total\":2048"));
        assert!(json.contains("\"etaSecs\":3"));
        assert!(json.contains("\"tagger\":\"absent\""));
        assert!(!json.contains("eta_secs"));
    }

    #[test]
    fn a_pause_keeps_the_bytes_it_had() {
        let paused = Stage::Paused {
            received: 900,
            total: Some(2048),
        };
        let json = json(&paused, TaggerState::default());
        // A paused bar that reads zero looks like a lost download.
        assert!(json.contains("\"phase\":\"paused\""));
        assert!(json.contains("\"received\":900"));
        assert!(json.contains("\"total\":2048"));
    }

    #[test]
    fn only_a_ready_stage_carries_a_word_count() {
        assert!(
            json(&Stage::Ready { words: 248_447 }, TaggerState::default())
                .contains("\"words\":248447")
        );
        let failed = Stage::Failed {
            message: "no net".into(),
        };
        let json = json(&failed, TaggerState::default());
        assert!(json.contains("\"message\":\"no net\""));
        // An `Option` field is null on the wire, never absent.
        assert!(json.contains("\"words\":null"), "{json}");
    }

    #[test]
    fn a_default_tagger_is_absent_not_an_empty_word() {
        // Pinned: the disk-adoption check compares against this string.
        assert_eq!(TaggerState::default().phase, "absent");
        assert_eq!(TaggerState::default().percent, None);
        assert!(json(&Stage::Absent, TaggerState::default()).contains("\"tagger\":\"absent\""));
    }

    #[test]
    fn the_tagger_model_reports_beside_the_dataset() {
        let inbound = TaggerState {
            phase: "downloading",
            percent: Some(42),
        };
        let json = json(&Stage::Absent, inbound);
        assert!(json.contains("\"tagger\":\"downloading\""));
        assert!(json.contains("\"taggerPercent\":42"));
        assert!(!json.contains("tagger_percent"));
    }
}
