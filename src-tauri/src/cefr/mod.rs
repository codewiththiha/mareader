//! The dataset's backend: download, parquet-to-sqlite rebuild, lookups.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::download::{AppDownloads, DownloadRequest, Phase as WirePhase, Progress, TauriHost};

/// The parquet's mirrors, preferred first: a client that blocks one
/// host still reaches the file.
const DATASET_URLS: [&str; 3] = [
    "https://raw.githubusercontent.com/codewiththiha/cefr-rs/main/data/cefr.zstd.parquet",
    "https://cdn.jsdelivr.net/gh/codewiththiha/cefr-rs@main/data/cefr.zstd.parquet",
    "https://github.com/codewiththiha/cefr-rs/raw/main/data/cefr.zstd.parquet",
];

/// The runtime tagger model's mirrors, in the same order.
const MODEL_URLS: [&str; 3] = [
    "https://raw.githubusercontent.com/codewiththiha/cefr-rs/main/models/en_tokenizer.bin.zst",
    "https://cdn.jsdelivr.net/gh/codewiththiha/cefr-rs@main/models/en_tokenizer.bin.zst",
    "https://github.com/codewiththiha/cefr-rs/raw/main/models/en_tokenizer.bin.zst",
];

/// The event the webview subscribes to for every dataset phase change.
pub const PROGRESS_EVENT: &str = "cefr-dataset-progress";

/// The generic downloader's key for this dataset's parquet.
const DOWNLOAD_ID: &str = "cefr-dataset";

/// The generic downloader's key for the tagger model.
const MODEL_ID: &str = "cefr-model";

/// The model file's name, under the dataset directory.
const MODEL_FILE: &str = "en_tokenizer.bin.zst";

/// One download's links, as the transport takes them.
fn links(mirrors: [&str; 3]) -> Vec<String> {
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
    /// The dataset sense that answered, when one did.
    pub sense: Option<String>,
    /// The answering sense's level.
    pub level: Option<f64>,
    /// Every POS sense the dataset lists for the word.
    pub senses: Vec<String>,
}

/// Progress payload, also the answer of the status command.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetStatus {
    /// `absent` | `downloading` | `paused` | `converting` | `ready` |
    /// `failed`.
    pub phase: String,
    pub received: u64,
    pub total: Option<u64>,
    pub words: Option<u64>,
    pub message: Option<String>,
    /// The POS engine's state: `absent` | `downloading` | `ready` |
    /// `failed`.
    pub model: String,
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
    /// The wire snapshot of this phase, before the model's own state is
    /// folded in.
    fn snapshot(&self) -> DatasetStatus {
        let (phase, received, total) = match self {
            Phase::Absent => ("absent", 0, None),
            Phase::Downloading { received, total } => ("downloading", *received, *total),
            Phase::Paused { received, total } => ("paused", *received, *total),
            Phase::Converting => ("converting", 0, None),
            Phase::Ready { .. } => ("ready", 0, None),
            Phase::Failed { .. } => ("failed", 0, None),
        };
        DatasetStatus {
            phase: phase.into(),
            received,
            total,
            words: match self {
                Phase::Ready { words } => Some(*words),
                _ => None,
            },
            message: match self {
                Phase::Failed { message } => Some(message.clone()),
                _ => None,
            },
            model: "absent".into(),
        }
    }
}

/// The largest batch one lookup may carry; the frontend's pages stay far
/// below it.
const MAX_BATCH: usize = 4_000;

/// The manager: one per process, shared by every pane's commands.
pub struct CefrManager {
    phase: Mutex<Phase>,
    /// The opened dataset, reused across lookups; dropped on remove.
    db: Mutex<Option<cefr::db::CefrDb>>,
    /// The loaded tagger model, reused across clicks.
    tagger: Mutex<Option<Arc<cefr::pos::Tagger>>>,
}

impl CefrManager {
    pub fn new() -> Self {
        Self {
            phase: Mutex::new(Phase::Absent),
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

    fn parquet_partial(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join("cefr.parquet.part"))
    }

    fn db_final(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join("cefr.db"))
    }

    fn db_building(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join("cefr.db.tmp"))
    }

    fn model_path(app: &AppHandle) -> Result<PathBuf, String> {
        Ok(Self::dir(app)?.join(MODEL_FILE))
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
        Ok(with_model(app, guard.snapshot()))
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

    /// Hand both files to the downloader; their snapshots drive the
    /// stages, so nothing here polls.
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
        republish(&app);
        let host = TauriHost::new(app.clone());
        let dataset = DownloadRequest {
            id: DOWNLOAD_ID.into(),
            urls: links(DATASET_URLS),
            directory: Some("cefr".into()),
            file_name: "cefr.parquet.part".into(),
        };
        app.state::<AppDownloads>()
            .start(&host, dataset, dataset_hook(app.clone()))?;
        // The tagger rides along; a model that lands late only delays
        // a click.
        let model = DownloadRequest {
            id: MODEL_ID.into(),
            urls: links(MODEL_URLS),
            directory: Some("cefr".into()),
            file_name: MODEL_FILE.into(),
        };
        let _ = app
            .state::<AppDownloads>()
            .start(&host, model, model_hook(app.clone()));
        Ok(())
    }

    /// Stop reading; the partial stays and resume continues from it.
    pub fn pause_download(&self, app: &AppHandle) {
        app.state::<AppDownloads>().pause(DOWNLOAD_ID);
    }

    /// Continue a paused download.
    pub fn resume_download(&self, app: &AppHandle) -> Result<(), String> {
        app.state::<AppDownloads>()
            .resume(&TauriHost::new(app.clone()), DOWNLOAD_ID)
    }

    /// Ask the transport to stop; the partial stays for the next start.
    pub fn cancel(&self, app: &AppHandle) {
        app.state::<AppDownloads>()
            .cancel(&TauriHost::new(app.clone()), DOWNLOAD_ID);
    }

    /// Drop the dataset and its partials; the next download starts over.
    pub fn remove(&self, app: &AppHandle) -> Result<(), String> {
        self.cancel(app);
        {
            let mut guard = self.phase.lock().map_err(|_| "phase lock poisoned")?;
            *guard = Phase::Absent;
        }
        app.state::<AppDownloads>().remove(DOWNLOAD_ID);
        for path in [
            Self::parquet_partial(app),
            Self::db_final(app),
            Self::db_building(app),
        ] {
            if let Ok(path) = path {
                let _ = std::fs::remove_file(path);
            }
        }
        if let Ok(mut slot) = self.db.lock() {
            *slot = None;
        }
        republish(app);
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
        // The batch key's tag half is always empty here, so the word
        // alone answers.
        let by_word: HashMap<&str, f64> = found
            .iter()
            .map(|((word, _), level)| (word.as_str(), *level))
            .collect();
        let mut out: Vec<Option<f64>> = normalized
            .iter()
            .map(|key| key.as_deref().and_then(|k| by_word.get(k).copied()))
            .collect();
        out.resize(words.len(), None);
        Ok(out)
    }

    /// The dataset's POS for `word` in `sentence`; `Ok(None)` if unequipped.
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
        let db_path = Self::db_final(app)?;
        let mut slot = self.db.lock().map_err(|_| "db lock poisoned")?;
        if slot.is_none() && db_path.is_file() {
            *slot = Some(cefr::db::CefrDb::open(&db_path).map_err(|e| format!("dataset: {e}"))?);
        }
        let Some(db) = slot.as_ref() else {
            return Ok(None);
        };
        let sense = db
            .sense_level(word, &found.pos)
            .map_err(|e| format!("pos lookup: {e}"))?;
        let senses = db
            .pos_senses(word)
            .map_err(|e| format!("senses: {e}"))?
            .into_iter()
            .map(|(pos, _)| pos)
            .collect();
        Ok(Some(PosAnswer {
            pos: found.pos.clone(),
            kind: penn_kind(&found.pos),
            // The sense that answered may be a tag-family neighbor; the
            // tag stays the tagger's.
            sense: sense.as_ref().map(|s| s.pos.clone()),
            level: sense.map(|s| s.level),
            senses,
        }))
    }

    /// The loaded tagger, or `None` while the model is still inbound.
    fn tagger(&self, app: &AppHandle) -> Result<Option<Arc<cefr::pos::Tagger>>, String> {
        let mut slot = self.tagger.lock().map_err(|_| "tagger lock poisoned")?;
        if let Some(tagger) = slot.as_ref() {
            return Ok(Some(tagger.clone()));
        }
        let model = Self::model_path(app)?;
        if !model.is_file() {
            // Self-heal: ask the downloader for it; a later click loads it.
            let request = DownloadRequest {
                id: MODEL_ID.into(),
                urls: links(MODEL_URLS),
                directory: Some("cefr".into()),
                file_name: MODEL_FILE.into(),
            };
            let host = TauriHost::new(app.clone());
            let _ = app
                .state::<AppDownloads>()
                .start(&host, request, model_hook(app.clone()));
            return Ok(None);
        }
        let tagger = Arc::new(
            cefr::pos::Tagger::from_model_path(&model).map_err(|e| format!("tagger model: {e}"))?,
        );
        *slot = Some(tagger.clone());
        Ok(Some(tagger))
    }
}

/// The dataset's own stage driver: one call per transport snapshot.
fn dataset_hook(app: AppHandle) -> crate::download::ProgressHook {
    Arc::new(move |p: &Progress| match p.phase {
        WirePhase::Downloading => set_phase(
            &app,
            Phase::Downloading {
                received: p.received,
                total: p.total,
            },
        ),
        WirePhase::Paused => set_phase(
            &app,
            Phase::Paused {
                received: p.received,
                total: p.total,
            },
        ),
        WirePhase::Done => {
            let app = app.clone();
            tauri::async_runtime::spawn(build_dataset(app));
        }
        WirePhase::Failed => {
            finish_failed(&app, p.message.as_deref().unwrap_or("the download failed"));
        }
        WirePhase::Cancelled => set_phase(&app, Phase::Absent),
    })
}

/// The sheet shows the engine's phase, so only its endings emit.
fn model_hook(app: AppHandle) -> crate::download::ProgressHook {
    Arc::new(move |p: &Progress| {
        if p.phase.terminal() {
            republish(&app);
        }
    })
}

/// A readable word class for a Penn tag.
fn penn_kind(tag: &str) -> String {
    let kind = if tag.starts_with("VB") {
        "verb"
    } else if tag.starts_with("NN") {
        "noun"
    } else if tag.starts_with("JJ") {
        "adjective"
    } else if tag.starts_with("RB") {
        "adverb"
    } else if tag == "PRP" || tag.starts_with("WP") {
        "pronoun"
    } else if tag == "IN" || tag == "TO" {
        "preposition"
    } else if tag == "CC" {
        "conjunction"
    } else if tag == "CD" {
        "number"
    } else if tag == "MD" {
        "modal verb"
    } else if tag == "DT" || tag == "PDT" || tag == "WDT" {
        "determiner"
    } else {
        "other"
    };
    kind.to_string()
}

impl Default for CefrManager {
    fn default() -> Self {
        Self::new()
    }
}

/// The stages after the parquet lands: verify, rebuild, adopt, probe.
async fn build_dataset(app: AppHandle) {
    set_phase(&app, Phase::Converting);
    let partial = match CefrManager::parquet_partial(&app) {
        Ok(path) => path,
        Err(message) => {
            finish_failed(&app, &message);
            return;
        }
    };
    if let Err(message) = verify_parquet(&partial) {
        // Bad data, not bad luck: the next attempt re-downloads.
        drop_dataset_files(&app, &partial);
        finish_failed(&app, &message);
        return;
    }
    let (building, final_db) = match (
        CefrManager::db_building(&app),
        CefrManager::db_final(&app),
    ) {
        (Ok(building), Ok(ready)) => (building, ready),
        (Err(message), _) | (_, Err(message)) => {
            finish_failed(&app, &message);
            return;
        }
    };
    let build_path = building.clone();
    let built = {
        let parquet = partial.clone();
        tauri::async_runtime::spawn_blocking(move || cefr::db::build_db(&parquet, &build_path))
            .await
    };
    match built {
        Ok(Ok(_stats)) => {}
        Ok(Err(e)) => {
            // A parquet that parses but does not rebuild is bad data.
            let _ = std::fs::remove_file(&building);
            drop_dataset_files(&app, &partial);
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
        finish_failed(&app, &format!("adopt dataset: {e}"));
        return;
    }
    let words = match CefrManager::probe_db(&final_db) {
        Ok(words) => words,
        Err(message) => {
            finish_failed(&app, &message);
            return;
        }
    };
    // The parquet's job is done; only the db is kept.
    let _ = std::fs::remove_file(&partial);
    if let Ok(mut slot) = app.state::<crate::cefr::CefrManager>().db.lock() {
        *slot = None;
    }
    set_phase(&app, Phase::Ready { words });
}

/// Drop a rejected partial and the download record that points at it.
fn drop_dataset_files(app: &AppHandle, partial: &Path) {
    let _ = std::fs::remove_file(partial);
    app.state::<AppDownloads>().remove(DOWNLOAD_ID);
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

/// The POS engine's state, which the dataset's phase does not carry.
fn model_state(app: &AppHandle) -> &'static str {
    let present = CefrManager::model_path(app)
        .map(|path| path.is_file())
        .unwrap_or(false);
    if present {
        return "ready";
    }
    match app.state::<AppDownloads>().status(MODEL_ID).map(|p| p.phase) {
        Some(WirePhase::Downloading) | Some(WirePhase::Paused) => "downloading",
        Some(WirePhase::Failed) => "failed",
        _ => "absent",
    }
}

/// The snapshot plus the engine's own state, which needs the app.
fn with_model(app: &AppHandle, mut status: DatasetStatus) -> DatasetStatus {
    status.model = model_state(app).into();
    status
}

/// One guarded write of a phase, then the event outside the lock.
fn set_phase(app: &AppHandle, phase: Phase) {
    let status = {
        let manager = app.state::<crate::cefr::CefrManager>();
        let Ok(mut guard) = manager.phase.lock() else {
            return;
        };
        *guard = phase;
        guard.snapshot()
    };
    let _ = app.emit(PROGRESS_EVENT, with_model(app, status));
}

fn finish_failed(app: &AppHandle, message: &str) {
    set_phase(
        app,
        Phase::Failed {
            message: message.to_string(),
        },
    );
}

/// Re-publish the phase as it stands: the engine moved, not the dataset.
fn republish(app: &AppHandle) {
    let status = app
        .state::<crate::cefr::CefrManager>()
        .phase
        .lock()
        .map(|guard| guard.snapshot());
    if let Ok(status) = status {
        let _ = app.emit(PROGRESS_EVENT, with_model(app, status));
    }
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

    #[test]
    fn a_pause_keeps_the_bytes_it_had() {
        // The sheet reads "Paused at N%", so the count must survive.
        let paused = Phase::Paused {
            received: 700,
            total: Some(1000),
        }
        .snapshot();
        assert_eq!(paused.phase, "paused");
        assert_eq!(paused.received, 700);
        assert_eq!(paused.total, Some(1000));
    }

    #[test]
    fn every_mirror_list_reaches_the_same_file() {
        assert_eq!(links(DATASET_URLS).len(), 3);
        assert_eq!(links(MODEL_URLS).len(), 3);
        for url in links(DATASET_URLS).iter().chain(&links(MODEL_URLS)) {
            assert!(url.starts_with("https://"), "{url}");
            assert!(url.contains("cefr-rs"), "{url}");
        }
        assert!(links(DATASET_URLS)[0].ends_with("cefr.zstd.parquet"));
        assert!(links(MODEL_URLS)[0].ends_with(MODEL_FILE));
    }
}
