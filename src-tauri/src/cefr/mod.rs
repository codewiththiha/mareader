//! The dataset's backend: download, parquet-to-sqlite rebuild, lookups.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::download::{AppDownloads, DownloadRequest, TauriHost};

/// The built dataset parquet, served straight from the cefr-rs repository.
pub const DATASET_URL: &str =
    "https://raw.githubusercontent.com/codewiththiha/cefr-rs/main/data/cefr.zstd.parquet";

/// The event the webview subscribes to for every dataset phase change.
pub const PROGRESS_EVENT: &str = "cefr-dataset-progress";

/// The generic downloader's key for this dataset's parquet.
const DOWNLOAD_ID: &str = "cefr-dataset";

/// The runtime tagger model, served from the cefr-rs repository.
pub const MODEL_URL: &str =
    "https://raw.githubusercontent.com/codewiththiha/cefr-rs/main/models/en_tokenizer.bin.zst";

/// The generic downloader's key for the tagger model.
const MODEL_ID: &str = "cefr-model";

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
}

/// The dataset's phase, guarded so commands and the supervise task agree.
enum Phase {
    Absent,
    Downloading { received: u64, total: Option<u64> },
    Paused,
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
            Phase::Paused => DatasetStatus {
                phase: "paused".into(),
                received: 0,
                total: None,
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

    /// Hand the transport to the generic downloader, then supervise the
    /// dataset's own stages.
    pub fn begin_download(&self, app: AppHandle) -> Result<(), String> {
        {
            let mut guard = self.phase.lock().map_err(|_| "phase lock poisoned")?;
            match &*guard {
                Phase::Downloading { .. } | Phase::Paused | Phase::Converting => {
                    return Err("a download is already running".into());
                }
                _ => {}
            }
            *guard = Phase::Downloading {
                received: 0,
                total: None,
            };
        }
        let _ = app.emit(PROGRESS_EVENT, guard_snapshot(self));
        let on_progress = {
            let app = app.clone();
            Arc::new(move |p: &crate::download::Progress| {
                let phase = match p.phase.as_str() {
                    "downloading" => Phase::Downloading {
                        received: p.received,
                        total: p.total,
                    },
                    "paused" => Phase::Paused,
                    _ => return,
                };
                set_phase(&app, phase);
            })
        };
        let request = DownloadRequest {
            id: DOWNLOAD_ID.into(),
            url: DATASET_URL.into(),
            directory: Some("cefr".into()),
            file_name: "cefr.parquet.part".into(),
        };
        let host = TauriHost::new(app.clone());
        app.state::<AppDownloads>()
            .start(&host, request, Some(on_progress))?;
        // The click-time tagger model rides along; one tap equips both.
        let model = DownloadRequest {
            id: MODEL_ID.into(),
            url: MODEL_URL.into(),
            directory: Some("cefr".into()),
            file_name: "en_tokenizer.bin.zst".into(),
        };
        let _ = app.state::<AppDownloads>().start(&host, model, None);
        tauri::async_runtime::spawn(supervise(app));
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
        app.state::<AppDownloads>().cancel(DOWNLOAD_ID);
    }

    /// Drop the dataset and its partials; the next download starts over.
    pub fn remove(&self, app: &AppHandle) -> Result<(), String> {
        self.cancel(app);
        {
            let mut guard = self.phase.lock().map_err(|_| "phase lock poisoned")?;
            *guard = Phase::Absent;
        }
        app.state::<AppDownloads>().remove(DOWNLOAD_ID);
        if let Ok(db) = Self::db_final(app) {
            let _ = std::fs::remove_file(db);
        }
        if let Ok(tmp) = Self::db_building(app) {
            let _ = std::fs::remove_file(tmp);
        }
        if let Ok(mut slot) = self.db.lock() {
            *slot = None;
        }
        let _ = app.emit(PROGRESS_EVENT, guard_snapshot(self));
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
        let model = Self::dir(app)?.join("en_tokenizer.bin.zst");
        if !model.is_file() {
            // Self-heal: ask the downloader for it; a later click loads it.
            let request = DownloadRequest {
                id: MODEL_ID.into(),
                url: MODEL_URL.into(),
                directory: Some("cefr".into()),
                file_name: "en_tokenizer.bin.zst".into(),
            };
            let host = TauriHost::new(app.clone());
            let _ = app.state::<AppDownloads>().start(&host, request, None);
            return Ok(None);
        }
        let tagger = Arc::new(
            cefr::pos::Tagger::from_model_path(&model).map_err(|e| format!("tagger model: {e}"))?,
        );
        *slot = Some(tagger.clone());
        Ok(Some(tagger))
    }
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

/// Watch the transport until it settles, then run the dataset's stages.
async fn supervise(app: AppHandle) {
    let downloads = app.state::<AppDownloads>();
    loop {
        let Some(progress) = downloads.status(DOWNLOAD_ID) else {
            return; // the record went away (remove): nothing to stage
        };
        match progress.phase.as_str() {
            "downloading" | "paused" => {}
            "cancelled" => {
                set_phase(&app, Phase::Absent);
                return;
            }
            "failed" => {
                finish_failed(&app, &progress.message.unwrap_or_else(|| "failed".into()));
                return;
            }
            _ => break,
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let manager = app.state::<CefrManager>();
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
        let _ = std::fs::remove_file(&partial);
        app.state::<AppDownloads>().remove(DOWNLOAD_ID);
        finish_failed(&app, &message);
        return;
    }
    let (building, final_db) = match (CefrManager::db_building(&app), CefrManager::db_final(&app)) {
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
            // A parquet that parses but does not rebuild is bad data: drop it.
            let _ = std::fs::remove_file(&partial);
            let _ = std::fs::remove_file(&building);
            app.state::<AppDownloads>().remove(DOWNLOAD_ID);
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
    if let Ok(mut slot) = manager.db.lock() {
        *slot = None;
    }
    set_phase(&app, Phase::Ready { words });
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

/// One guarded write of a phase, then the event outside the lock.
fn set_phase(app: &AppHandle, phase: Phase) {
    let status = {
        let manager = app.state::<CefrManager>();
        let Ok(mut guard) = manager.phase.lock() else {
            return;
        };
        *guard = phase;
        guard.snapshot()
    };
    let _ = app.emit(PROGRESS_EVENT, &status);
}

fn finish_failed(app: &AppHandle, message: &str) {
    set_phase(
        app,
        Phase::Failed {
            message: message.to_string(),
        },
    );
}

/// Read the phase under guard, unlocked afterwards, for the emit.
fn guard_snapshot(manager: &CefrManager) -> DatasetStatus {
    manager
        .phase
        .lock()
        .map(|guard| guard.snapshot())
        .unwrap_or_else(|_| Phase::Absent.snapshot())
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

        let paused = serde_json::to_string(&Phase::Paused.snapshot()).unwrap();
        assert!(paused.contains("\"phase\":\"paused\""));

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
