//! The dataset's backend: download, parquet-to-sqlite rebuild, lookups.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::download::{AppDownloads, DownloadRequest, Phase as WirePhase, ProgressHook, TauriHost};

/// The parquet's mirrors, preferred first: a client whose network blocks
/// one host still reaches the file.
const DATASET_URLS: [&str; 3] = [
    "https://raw.githubusercontent.com/codewiththiha/cefr-rs/main/data/cefr.zstd.parquet",
    "https://cdn.jsdelivr.net/gh/codewiththiha/cefr-rs@main/data/cefr.zstd.parquet",
    "https://github.com/codewiththiha/cefr-rs/raw/main/data/cefr.zstd.parquet",
];

/// The click-time tagger model's mirrors, in the same order.
const MODEL_URLS: [&str; 3] = [
    "https://raw.githubusercontent.com/codewiththiha/cefr-rs/main/models/en_tokenizer.bin.zst",
    "https://cdn.jsdelivr.net/gh/codewiththiha/cefr-rs@main/models/en_tokenizer.bin.zst",
    "https://github.com/codewiththiha/cefr-rs/raw/main/models/en_tokenizer.bin.zst",
];

/// The event the webview subscribes to for every dataset phase change.
pub const PROGRESS_EVENT: &str = "cefr-dataset-progress";

/// The generic downloader's keys: one download per file.
const DATASET_ID: &str = "cefr-dataset";
const MODEL_ID: &str = "cefr-model";

/// Both files, under the dataset directory. The downloader keeps the
/// bytes in transit beside them as `<name>.part`.
const DATASET_FILE: &str = "cefr.parquet";
const MODEL_FILE: &str = "en_tokenizer.bin.zst";

/// One download's links, as the transport takes them.
fn links(mirrors: &[&str]) -> Vec<String> {
    mirrors.iter().map(|url| url.to_string()).collect()
}

/// The dataset's POS verdict for one clicked word.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PosAnswer {
    /// The Penn Treebank tag at the word's position in its sentence.
    pub pos: String,
    /// The readable word class: noun, verb, adjective, ...
    pub kind: String,
}

/// The dataset's stage, the wire vocabulary a sheet switches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Nothing on disk and nothing running.
    Absent,
    Downloading,
    Paused,
    /// The parquet is being rebuilt into the local database.
    Converting,
    Ready,
    Failed,
}

/// Progress payload, also the answer of the status command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetStatus {
    pub phase: Stage,
    pub received: u64,
    pub total: Option<u64>,
    pub words: Option<u64>,
    pub message: Option<String>,
    /// The POS model's own stage, which moves on its own download.
    pub model: Stage,
}

/// The dataset's phase, guarded so commands and the transport's hooks
/// agree.
enum Phase {
    Absent,
    Downloading { received: u64, total: Option<u64> },
    Paused { received: u64, total: Option<u64> },
    Converting,
    Ready { words: u64 },
    Failed { message: String },
}

impl Phase {
    fn stage(&self) -> Stage {
        match self {
            Self::Absent => Stage::Absent,
            Self::Downloading { .. } => Stage::Downloading,
            Self::Paused { .. } => Stage::Paused,
            Self::Converting => Stage::Converting,
            Self::Ready { .. } => Stage::Ready,
            Self::Failed { .. } => Stage::Failed,
        }
    }

    /// The byte count a sheet prints; only the two download phases carry
    /// one, and a paused download carries the last.
    fn bytes(&self) -> (u64, Option<u64>) {
        match self {
            Self::Downloading { received, total } | Self::Paused { received, total } => {
                (*received, *total)
            }
            _ => (0, None),
        }
    }

    fn snapshot(&self, model: Stage) -> DatasetStatus {
        let (received, total) = self.bytes();
        DatasetStatus {
            phase: self.stage(),
            received,
            total,
            words: match self {
                Self::Ready { words } => Some(*words),
                _ => None,
            },
            message: match self {
                Self::Failed { message } => Some(message.clone()),
                _ => None,
            },
            model,
        }
    }
}

/// The largest batch one lookup may carry; the frontend's pages stay far
/// below it.
const MAX_BATCH: usize = 4_000;

/// The manager: one per process, shared by every pane's commands.
pub struct CefrManager {
    phase: Mutex<Phase>,
    /// The POS model's stage, unprobed until the first status ask.
    model: Mutex<Option<Stage>>,
    /// The opened dataset, reused across lookups; dropped on remove.
    db: Mutex<Option<cefr::db::CefrDb>>,
    /// The loaded tagger model, reused across clicks.
    tagger: Mutex<Option<Arc<cefr::pos::Tagger>>>,
}

impl CefrManager {
    pub fn new() -> Self {
        Self {
            phase: Mutex::new(Phase::Absent),
            model: Mutex::new(None),
            db: Mutex::new(None),
            tagger: Mutex::new(None),
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

    fn dataset_file(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join(DATASET_FILE))
    }

    fn model_file(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join(MODEL_FILE))
    }

    fn db_final(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join("cefr.db"))
    }

    fn db_building(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join("cefr.db.tmp"))
    }

    /// The model's stage: probed from disk once, then kept by its own
    /// download's hook.
    fn model_stage(&self, app: &AppHandle) -> Stage {
        if let Ok(probe) = self.model.lock()
            && let Some(stage) = *probe
        {
            return stage;
        }
        let stage = match Self::model_file(app) {
            Ok(path) if path.is_file() => Stage::Ready,
            _ => Stage::Absent,
        };
        if let Ok(mut probe) = self.model.lock() {
            *probe = Some(stage);
        }
        stage
    }

    /// The live phase, or the disk when this process has not touched it yet.
    pub fn status(&self, app: &AppHandle) -> Result<DatasetStatus, String> {
        let model = self.model_stage(app);
        let mut guard = self.phase.lock().map_err(|_| "phase lock poisoned")?;
        if matches!(*guard, Phase::Absent) && Self::db_final(app)?.is_file() {
            // A dataset from an earlier run: adopt it without a rebuild.
            *guard = match Self::probe_db(&Self::db_final(app)?) {
                Ok(words) => Phase::Ready { words },
                Err(message) => Phase::Failed { message },
            };
        }
        Ok(guard.snapshot(model))
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

    /// Hand both files to the downloader. Every later stage is driven by
    /// their snapshots: nothing here polls.
    pub fn begin_download(&self, app: AppHandle) -> Result<(), String> {
        {
            let mut guard = self.phase.lock().map_err(|_| "phase lock poisoned")?;
            match &*guard {
                Phase::Downloading { .. } | Phase::Paused { .. } | Phase::Converting => {
                    return Err("a download is already running".into());
                }
                Phase::Ready { .. } => {
                    return Err("the dataset is already installed".into());
                }
                _ => {}
            }
            *guard = Phase::Downloading {
                received: 0,
                total: None,
            };
        }
        republish(self, &app);
        let downloads = app.state::<AppDownloads>();
        let host = TauriHost::new(app.clone());
        let dataset = DownloadRequest {
            id: DATASET_ID.into(),
            urls: links(&DATASET_URLS),
            directory: Some("cefr".into()),
            file_name: DATASET_FILE.into(),
        };
        if let Err(e) = downloads.start(&host, dataset, Some(dataset_hook(app.clone()))) {
            // A refused start must not leave the sheet on a bar that
            // never moves.
            set_phase(&app, Phase::Absent);
            return Err(e);
        }
        // The tagger rides along; a model that lands late only delays a
        // click, and its own stage rides the same snapshot.
        let _ = self.begin_model_download(app);
        Ok(())
    }

    /// The POS model on its own: the levels are already here, and only a
    /// click needs the tagger. A paused transfer continues from its bytes;
    /// a live one is already doing what this asks for.
    pub fn begin_model_download(&self, app: AppHandle) -> Result<(), String> {
        let downloads = app.state::<AppDownloads>();
        if let Some(progress) = downloads.status(MODEL_ID)
            && !progress.phase.terminal()
        {
            if progress.phase != WirePhase::Paused {
                return Err("the POS model is already downloading".into());
            }
            let host = TauriHost::new(app.clone());
            return downloads.resume(&host, MODEL_ID);
        }
        if Self::model_file(&app)?.is_file() {
            return Err("the POS model is already installed".into());
        }
        let request = DownloadRequest {
            id: MODEL_ID.into(),
            urls: links(&MODEL_URLS),
            directory: Some("cefr".into()),
            file_name: MODEL_FILE.into(),
        };
        let host = TauriHost::new(app.clone());
        downloads.start(&host, request, Some(model_hook(app.clone())))
    }

    /// Stop reading; the partial stays and resume continues from it.
    pub fn pause_download(&self, app: &AppHandle) {
        app.state::<AppDownloads>().pause(DATASET_ID);
    }

    /// Continue a paused download.
    pub fn resume_download(&self, app: &AppHandle) -> Result<(), String> {
        let host = TauriHost::new(app.clone());
        app.state::<AppDownloads>().resume(&host, DATASET_ID)
    }

    /// Ask the transport to stop; the partial stays for the next start.
    pub fn cancel(&self, app: &AppHandle) {
        let host = TauriHost::new(app.clone());
        app.state::<AppDownloads>().cancel(&host, DATASET_ID);
    }

    /// Drop both files, their partials and everything loaded from them.
    pub fn remove(&self, app: &AppHandle) -> Result<(), String> {
        let downloads = app.state::<AppDownloads>();
        downloads.remove(DATASET_ID);
        downloads.remove(MODEL_ID);
        let files = [
            Self::db_final(app),
            Self::db_building(app),
            Self::dataset_file(app),
            Self::model_file(app),
        ];
        for path in files.into_iter().flatten() {
            let _ = std::fs::remove_file(path);
        }
        if let Ok(mut guard) = self.phase.lock() {
            *guard = Phase::Absent;
        }
        if let Ok(mut slot) = self.db.lock() {
            *slot = None;
        }
        if let Ok(mut slot) = self.tagger.lock() {
            *slot = None;
        }
        if let Ok(mut probe) = self.model.lock() {
            *probe = Some(Stage::Absent);
        }
        Ok(())
    }

    /// Levels for `words`, aligned with the input; the db opens lazily.
    pub fn levels(&self, app: &AppHandle, words: &[String]) -> Result<Vec<Option<f64>>, String> {
        let db_path = Self::db_final(app)?;
        let mut slot = self.db.lock().map_err(|_| "db lock poisoned")?;
        if slot.is_none() && db_path.is_file() {
            let opened = cefr::db::CefrDb::open(&db_path);
            *slot = Some(opened.map_err(|e| format!("dataset: {e}"))?);
        }
        let Some(db) = slot.as_ref() else {
            return Ok(vec![None; words.len()]);
        };
        // Every English word gets one slot; the answers land back in the
        // positions they were asked for.
        let mut asked: Vec<(usize, String)> = Vec::new();
        for (index, word) in words.iter().take(MAX_BATCH).enumerate() {
            if cefr_core::is_english_ascii(word) {
                asked.push((index, cefr_core::normalize(word)));
            }
        }
        let mut out: Vec<Option<f64>> = vec![None; words.len()];
        if asked.is_empty() {
            return Ok(out);
        }
        let pairs: Vec<(String, String)> = asked
            .iter()
            .map(|(_, key)| (key.clone(), String::new()))
            .collect();
        let found = db
            .lookup_batch(&pairs)
            .map_err(|e| format!("lookup: {e}"))?;
        for (index, key) in asked {
            out[index] = found.get(&(key, String::new())).copied();
        }
        Ok(out)
    }

    /// The dataset's POS for `word` in `sentence`; `Ok(None)` if unequipped.
    /// Blocking work: the caller leaves the async pool.
    pub fn pos_of(
        &self,
        app: &AppHandle,
        word: &str,
        sentence: &str,
    ) -> Result<Option<PosAnswer>, String> {
        let Some(tagger) = self.tagger(app)? else {
            return Ok(None);
        };
        let Some(found) = tagger.pos_in_context(word, sentence) else {
            return Ok(None);
        };
        Ok(Some(PosAnswer {
            pos: found.pos.clone(),
            kind: cefr::pos::kind_of(&found.pos).to_string(),
        }))
    }

    /// The loaded tagger, or `None` while the model is still inbound.
    fn tagger(&self, app: &AppHandle) -> Result<Option<Arc<cefr::pos::Tagger>>, String> {
        let path = Self::model_file(app)?;
        let mut slot = self.tagger.lock().map_err(|_| "tagger lock poisoned")?;
        if let Some(tagger) = slot.as_ref() {
            return Ok(Some(tagger.clone()));
        }
        if !path.is_file() {
            // Nothing to load: the settings panel owns asking for the
            // model, so a click never starts a multi-megabyte fetch.
            return Ok(None);
        }
        let tagger = Arc::new(
            cefr::pos::Tagger::from_model_path(&path).map_err(|e| format!("tagger model: {e}"))?,
        );
        *slot = Some(tagger.clone());
        Ok(Some(tagger))
    }
}

impl Default for CefrManager {
    fn default() -> Self {
        Self::new()
    }
}

/// The dataset download's watcher: every snapshot becomes a phase change,
/// and the end of the transfer starts the rebuild.
fn dataset_hook(app: AppHandle) -> ProgressHook {
    Arc::new(move |progress: &crate::download::Progress| {
        match progress.phase {
            WirePhase::Downloading => {
                let received = progress.received;
                let total = progress.total;
                set_phase(&app, Phase::Downloading { received, total });
            }
            WirePhase::Paused => {
                let received = progress.received;
                let total = progress.total;
                set_phase(&app, Phase::Paused { received, total });
            }
            WirePhase::Cancelled => set_phase(&app, Phase::Absent),
            WirePhase::Failed => {
                let message = progress.message.as_deref().unwrap_or("the download failed");
                finish_failed(&app, message);
            }
            // The bytes are whole: rebuild, then the parquet's job is done.
            WirePhase::Done => {
                set_phase(&app, Phase::Converting);
                tauri::async_runtime::spawn(convert(app.clone()));
            }
        }
    })
}

/// The model download's watcher: its stage rides the dataset's snapshot.
fn model_hook(app: AppHandle) -> ProgressHook {
    Arc::new(move |progress: &crate::download::Progress| {
        let stage = match progress.phase {
            WirePhase::Downloading => Stage::Downloading,
            WirePhase::Paused => Stage::Paused,
            WirePhase::Done => Stage::Ready,
            WirePhase::Failed | WirePhase::Cancelled => Stage::Absent,
        };
        if let Ok(mut probe) = app.state::<CefrManager>().model.lock() {
            *probe = Some(stage);
        }
        republish(&app.state::<CefrManager>(), &app);
    })
}

/// Verify the parquet, rebuild the database, adopt it, drop the parquet.
async fn convert(app: AppHandle) {
    let manager = app.state::<CefrManager>();
    let parquet = match CefrManager::dataset_file(&app) {
        Ok(path) => path,
        Err(message) => {
            finish_failed(&app, &message);
            return;
        }
    };
    if let Err(message) = verify_parquet(&parquet) {
        // Bad data, not bad luck: the next attempt re-downloads.
        discard(&app, &parquet);
        finish_failed(&app, &message);
        return;
    }
    let paths = (CefrManager::db_building(&app), CefrManager::db_final(&app));
    let (building, final_db) = match paths {
        (Ok(building), Ok(ready)) => (building, ready),
        (Err(message), _) | (_, Err(message)) => {
            finish_failed(&app, &message);
            return;
        }
    };
    let source = parquet.clone();
    let target = building.clone();
    let build = move || cefr::db::build_db(&source, &target);
    let built = tauri::async_runtime::spawn_blocking(build).await;
    match built {
        Ok(Ok(_stats)) => {}
        Ok(Err(e)) => {
            let _ = std::fs::remove_file(&building);
            discard(&app, &parquet);
            finish_failed(&app, &format!("rebuild failed: {e}"));
            return;
        }
        Err(e) => {
            let _ = std::fs::remove_file(&building);
            finish_failed(&app, &format!("rebuild worker: {e}"));
            return;
        }
    }
    if let Err(e) = std::fs::rename(&building, &final_db) {
        let _ = std::fs::remove_file(&building);
        discard(&app, &parquet);
        finish_failed(&app, &format!("adopt dataset: {e}"));
        return;
    }
    let words = match CefrManager::probe_db(&final_db) {
        Ok(words) => words,
        Err(message) => {
            let _ = std::fs::remove_file(&final_db);
            discard(&app, &parquet);
            finish_failed(&app, &message);
            return;
        }
    };
    // The parquet's job is done; only the db is kept.
    let _ = std::fs::remove_file(&parquet);
    app.state::<AppDownloads>().remove(DATASET_ID);
    if let Ok(mut slot) = manager.db.lock() {
        *slot = None;
    }
    set_phase(&app, Phase::Ready { words });
}

/// Forget a dataset that failed to rebuild, so a retry starts over.
fn discard(app: &AppHandle, parquet: &Path) {
    let _ = std::fs::remove_file(parquet);
    app.state::<AppDownloads>().remove(DATASET_ID);
}

/// The four-byte signature a real parquet carries at both ends.
fn verify_parquet(path: &Path) -> Result<(), String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("reopen dataset: {e}"))?;
    let len = match file.metadata() {
        Ok(meta) => meta.len(),
        Err(e) => return Err(format!("stat dataset: {e}")),
    };
    if len < 8 {
        return Err(format!("dataset too small ({len} bytes)"));
    }
    let mut head = [0u8; 4];
    read_at(&mut file, 0, &mut head)?;
    if &head != b"PAR1" {
        return Err("not a parquet file".into());
    }
    let mut tail = [0u8; 4];
    read_at(&mut file, len - 4, &mut tail)?;
    if &tail != b"PAR1" {
        return Err("dataset is truncated".into());
    }
    Ok(())
}

/// Read exactly `buffer.len()` bytes at `offset`, or say what failed.
fn read_at(file: &mut std::fs::File, offset: u64, buffer: &mut [u8]) -> Result<(), String> {
    file.seek(SeekFrom::Start(offset))
        .map_err(|e| format!("seek: {e}"))?;
    file.read_exact(buffer).map_err(|e| format!("read: {e}"))?;
    Ok(())
}

/// One guarded write of a phase, then the event outside the lock.
fn set_phase(app: &AppHandle, phase: Phase) {
    {
        let manager = app.state::<CefrManager>();
        let Ok(mut guard) = manager.phase.lock() else {
            return;
        };
        *guard = phase;
    }
    republish(&app.state::<CefrManager>(), app);
}

fn finish_failed(app: &AppHandle, message: &str) {
    set_phase(
        app,
        Phase::Failed {
            message: message.to_string(),
        },
    );
}

/// Publish the current snapshot, model stage included.
fn republish(manager: &CefrManager, app: &AppHandle) {
    let model = manager.model_stage(app);
    let status = manager
        .phase
        .lock()
        .map(|guard| guard.snapshot(model))
        .unwrap_or_else(|_| Phase::Absent.snapshot(model));
    let _ = app.emit(PROGRESS_EVENT, &status);
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let downloading = Phase::Downloading {
            received: 1024,
            total: Some(2048),
        };
        let status = downloading.snapshot(Stage::Downloading);
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"phase\":\"downloading\""));
        assert!(json.contains("\"received\":1024"));
        assert!(json.contains("\"total\":2048"));
        assert!(json.contains("\"model\":\"downloading\""));

        let paused = Phase::Paused {
            received: 512,
            total: Some(1024),
        };
        let status = paused.snapshot(Stage::Absent);
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"phase\":\"paused\""));
        // The sheet prints where a paused download stopped.
        assert!(json.contains("\"received\":512"));

        let failed = Phase::Failed {
            message: "no net".into(),
        };
        let status = failed.snapshot(Stage::Absent);
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"phase\":\"failed\""));
        assert!(json.contains("\"message\":\"no net\""));

        let ready = Phase::Ready { words: 248_447 };
        let status = ready.snapshot(Stage::Ready);
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"phase\":\"ready\""));
        assert!(json.contains("\"words\":248447"));
    }
}
