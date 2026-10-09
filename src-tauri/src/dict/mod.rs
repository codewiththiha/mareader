//! The dictionary backend: packs in, ranked entries out.
//! English bridges the gaps between shores.

pub mod build;
pub mod db;
pub mod fetch;
pub mod packs;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use dict_core::{
    CanonPos, DictEntry, WordMatch, classify, order_entries, parse_tags, penn_canon, plans,
};
use download_core::{Phase, Progress};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::download::{AppDownloads, TauriHost};
use packs::PACKS;

use self::db::{DictDb, RawRow};

/// The event carrying every pack's row to the download UI.
pub const DICT_EVENT: &str = "dict-packs";

/// A pack's row: where its download stands, and its phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackStatus {
    pub id: String,
    pub label: String,
    pub source: String,
    pub target: String,
    pub rows: u64,
    pub built: bool,
    pub progress: Option<Progress>,
    /// `absent` | `downloading` | `paused` | `converting` | `ready` |
    /// `failed`.
    pub phase: String,
    /// Why a phase is what it is: a failure's own words.
    pub message: Option<String>,
}

/// One pack's conversion: running, or the reason it stopped.
#[derive(Debug, Clone)]
enum BuildState {
    Converting,
    Failed(String),
}

/// One entry on the wire to a card, a route, or the overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictEntryWire {
    pub word: String,
    pub pos_raw: Option<String>,
    pub tags: Vec<CanonPos>,
    pub definition: String,
    pub romanization: Option<String>,
    pub sense: Option<String>,
    pub pack: String,
    pub via: Option<String>,
    pub word_match: WordMatch,
}

/// One carried row: the row, its pack, and a bridge's word.
type Carried = (RawRow, String, Option<(String, WordMatch)>);

/// The packs a search may ask: the filter over what is built.
fn wanted_packs(pack_ids: Option<Vec<String>>, built: &[String]) -> Vec<String> {
    match pack_ids {
        Some(ids) if !ids.is_empty() => ids.into_iter().filter(|id| built.contains(id)).collect(),
        _ => built.to_vec(),
    }
}

/// One pack's wire phase and why, from its three truths.
fn phase_of(
    built: bool,
    build: Option<&BuildState>,
    progress: Option<&Progress>,
) -> (String, Option<String>) {
    if built {
        return ("ready".to_string(), None);
    }
    match build {
        Some(BuildState::Converting) => ("converting".to_string(), None),
        Some(BuildState::Failed(message)) => ("failed".to_string(), Some(message.clone())),
        None => match progress.map(|progress| progress.phase) {
            Some(
                Phase::Preparing | Phase::Downloading | Phase::Retrying | Phase::Verifying,
            ) => ("downloading".to_string(), None),
            Some(Phase::Paused) => ("paused".to_string(), None),
            Some(Phase::Failed) => {
                ("failed".to_string(), progress.and_then(|progress| progress.message.clone()))
            }
            _ => ("absent".to_string(), None),
        },
    }
}

/// The dictionary system: open packs and the routes between them.
#[derive(Default)]
pub struct DictManager {
    dbs: Mutex<HashMap<String, Arc<Mutex<DictDb>>>>,
    builds: Mutex<HashMap<String, BuildState>>,
}

impl DictManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// The cache folder every pack lives under.
    fn dir(app: &AppHandle) -> Result<PathBuf, String> {
        TauriHost::new(app.clone()).feature_dir(fetch::FEATURE)
    }

    /// Every pack's row, whatever its download is doing.
    pub fn status(&self, app: &AppHandle) -> Vec<PackStatus> {
        let dir = Self::dir(app).ok();
        let downloads = app.state::<AppDownloads>();
        let rows = PACKS
            .iter()
            .map(|pack| {
                let built = dir
                    .as_deref()
                    .map(|dir| fetch::db_file(dir, pack.id).exists())
                    .unwrap_or(false);
                let progress = downloads.status(pack.id);
                let build = self
                    .builds
                    .lock()
                    .ok()
                    .and_then(|builds| builds.get(pack.id).cloned());
                let (phase, message) = phase_of(built, build.as_ref(), progress.as_ref());
                PackStatus {
                    id: pack.id.to_string(),
                    label: pack.label.to_string(),
                    source: pack.source.to_string(),
                    target: pack.target.to_string(),
                    rows: pack.rows,
                    built,
                    progress,
                    phase,
                    message,
                }
            })
            .collect();
        self.adopt_landed(app, dir.as_deref());
        rows
    }

    /// A body landed in an earlier run converts itself into view.
    fn adopt_landed(&self, app: &AppHandle, dir: Option<&std::path::Path>) {
        let Some(dir) = dir else {
            return;
        };
        for pack in PACKS {
            if fetch::db_file(dir, pack.id).exists() {
                continue;
            }
            if fetch::parquet_file(dir, pack.id).is_file() {
                self.start_convert(app, pack.id);
            }
        }
    }

    /// Convert one landed body now; never two at once.
    fn start_convert(&self, app: &AppHandle, pack_id: &str) {
        {
            let mut builds = match self.builds.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if builds.contains_key(pack_id) {
                return;
            }
            builds.insert(pack_id.to_string(), BuildState::Converting);
        }
        self.emit(app);
        let app = app.clone();
        let pack_id = pack_id.to_string();
        tauri::async_runtime::spawn(async move {
            app.state::<DictManager>().convert(&app, &pack_id).await;
        });
    }

    /// A landed body becomes a sqlite, renamed into place when whole.
    async fn convert(&self, app: &AppHandle, pack_id: &str) {
        let result = self.convert_blocking(app, pack_id).await;
        let mut builds = match self.builds.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match result {
            Ok(_) => {
                builds.remove(pack_id);
            }
            Err(message) => {
                eprintln!("dict convert {pack_id}: {message}");
                builds.insert(
                    pack_id.to_string(),
                    BuildState::Failed(format!("conversion failed: {message}")),
                );
            }
        }
        drop(builds);
        self.emit(app);
    }

    /// The blocking half: build the table, then replace the old.
    async fn convert_blocking(&self, app: &AppHandle, pack_id: &str) -> Result<u64, String> {
        let dir = Self::dir(app)?;
        let source = fetch::parquet_file(&dir, pack_id);
        let target = fetch::db_file(&dir, pack_id);
        let building = fetch::db_building(&dir, pack_id);
        if let Ok(mut dbs) = self.dbs.lock() {
            dbs.remove(pack_id);
        }
        tauri::async_runtime::spawn_blocking(move || {
            let result = (|| {
                let rows = build::build_db(&source, &building).map_err(|e| e.to_string())?;
                if target.exists() {
                    std::fs::remove_file(&target).map_err(|e| e.to_string())?;
                }
                std::fs::rename(&building, &target).map_err(|e| e.to_string())?;
                Ok::<u64, String>(rows)
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&building);
            }
            result
        })
        .await
        .map_err(|e| format!("convert worker: {e}"))?
    }

    /// Start one pack's download, then convert it when the body lands.
    pub fn begin_download(&self, app: AppHandle, pack_id: &str) -> Result<(), String> {
        let dir = Self::dir(&app)?;
        self.clear_failed(pack_id);
        let watched = app.clone();
        let job = fetch::job_for(dir, pack_id)
            .ok_or_else(|| format!("no pack {pack_id}"))?
            .on_progress(move |progress| narrate(&watched, progress));
        let host = TauriHost::new(app.clone());
        let receipt = app.state::<AppDownloads>().start(&host, job)?;
        self.emit(&app);
        let pack_id = pack_id.to_string();
        tauri::async_runtime::spawn(async move {
            if receipt.finished().await.is_ok() {
                app.state::<DictManager>().start_convert(&app, &pack_id);
            }
        });
        Ok(())
    }

    /// A retry starts clean: the old failure goes with it.
    fn clear_failed(&self, pack_id: &str) {
        if let Ok(mut builds) = self.builds.lock()
            && matches!(builds.get(pack_id), Some(BuildState::Failed(_)))
        {
            builds.remove(pack_id);
        }
    }

    /// Stop reading; the partial stays and resume continues from it.
    pub fn pause(&self, app: &AppHandle, pack_id: &str) {
        app.state::<AppDownloads>().pause(pack_id);
        self.emit(app);
    }

    /// Continue a paused download and watch it like a first start.
    pub fn resume(&self, app: &AppHandle, pack_id: &str) -> Result<(), String> {
        let host = TauriHost::new(app.clone());
        let receipt = app.state::<AppDownloads>().resume(&host, pack_id)?;
        self.emit(app);
        let pack_id = pack_id.to_string();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if receipt.finished().await.is_ok() {
                app.state::<DictManager>().start_convert(&app, &pack_id);
            }
        });
        Ok(())
    }

    /// Drop the partial and the job.
    pub fn cancel(&self, app: &AppHandle, pack_id: &str) {
        app.state::<AppDownloads>().cancel(pack_id);
        app.state::<AppDownloads>().remove(pack_id);
        self.emit(app);
    }

    /// Take a pack out: files, partial, and any open handle.
    pub fn remove(&self, app: &AppHandle, pack_id: &str) -> Result<(), String> {
        self.cancel(app, pack_id);
        self.dbs
            .lock()
            .map_err(|_| "db lock poisoned")?
            .remove(pack_id);
        if let Ok(mut builds) = self.builds.lock() {
            builds.remove(pack_id);
        }
        let dir = Self::dir(app)?;
        for path in [
            fetch::parquet_file(&dir, pack_id),
            fetch::db_file(&dir, pack_id),
            fetch::db_building(&dir, pack_id),
        ] {
            let _ = std::fs::remove_file(path);
        }
        self.emit(app);
        Ok(())
    }

    /// The hover's ask: `word` from `from` to `to`.
    /// POS ranks senses; never gates them.
    pub fn lookup(
        &self,
        app: &AppHandle,
        word: &str,
        from: &str,
        to: &str,
        pos: Option<&str>,
        limit: usize,
    ) -> Vec<DictEntryWire> {
        let Ok(dir) = Self::dir(app) else {
            return Vec::new();
        };
        let built = self.built_packs(&dir);
        let refs: Vec<dict_core::PackRef> = PACKS
            .iter()
            .map(|p| dict_core::PackRef {
                id: p.id,
                source: p.source,
                target: p.target,
            })
            .collect();
        for plan in plans(from, to, &refs) {
            if !plan.hops.iter().all(|hop| built.contains(&hop.pack)) {
                continue;
            }
            let rows = self.run_plan(&dir, &plan, word, limit);
            if !rows.is_empty() {
                return finish(rows, word, pos, limit);
            }
        }
        // No route carried the ask: show the word's matches anyway.
        let mut rows = Vec::new();
        for pack_id in &built {
            if let Ok(db) = self.open(&dir, pack_id) {
                let db = db.lock().expect("db mutex");
                if let Ok(found) = db.lookup_word(word, limit) {
                    rows.extend(found.into_iter().map(|row| (row, pack_id.clone(), None)));
                }
            }
        }
        finish(rows, word, pos, limit)
    }

    /// The route's ask: the word on either side of a
    /// built pack.
    pub fn search(
        &self,
        app: &AppHandle,
        ask: &str,
        pack_ids: Option<Vec<String>>,
        limit: usize,
    ) -> Vec<DictEntryWire> {
        let Ok(dir) = Self::dir(app) else {
            return Vec::new();
        };
        let built = self.built_packs(&dir);
        let wanted = wanted_packs(pack_ids, &built);
        let mut rows = Vec::new();
        for pack_id in &wanted {
            if let Ok(db) = self.open(&dir, pack_id) {
                let db = db.lock().expect("db mutex");
                if let Ok(found) = db.search(ask, limit) {
                    rows.extend(found.into_iter().map(|row| (row, pack_id.clone(), None)));
                }
            }
        }
        finish(rows, ask, None, limit)
    }

    /// The packs whose sqlite stands built right now.
    fn built_packs(&self, dir: &std::path::Path) -> Vec<String> {
        PACKS
            .iter()
            .filter(|pack| fetch::db_file(dir, pack.id).exists())
            .map(|pack| pack.id.to_string())
            .collect()
    }

    /// One plan's rows: one hop, or two hops through the hub word.
    fn run_plan(
        &self,
        dir: &std::path::Path,
        plan: &dict_core::Plan,
        word: &str,
        limit: usize,
    ) -> Vec<Carried> {
        let [first, rest @ ..] = &plan.hops[..] else {
            return Vec::new();
        };
        // The first hop runs alone: two cross-pack locks
        // would deadlock opposite lookups.
        let rows = {
            let Ok(db) = self.open(dir, &first.pack) else {
                return Vec::new();
            };
            let db = db.lock().expect("db mutex");
            match hop_ask(&db, word, first.reverse, limit * 2) {
                Ok(rows) => rows,
                Err(_) => return Vec::new(),
            }
        };
        let Some(second) = rest.first() else {
            return rows
                .into_iter()
                .map(|row| (row, first.pack.to_string(), None))
                .collect();
        };
        // The bridge word: the side the second hop will ask.
        let mids: Vec<(String, WordMatch)> = rows
            .iter()
            .map(|row| {
                let mid = if second.reverse {
                    row.definition.clone()
                } else {
                    row.word.clone()
                };
                let fit = classify(word, &mid).unwrap_or(WordMatch::Fuzzy);
                (mid, fit)
            })
            .collect();
        let finals = {
            let Ok(second_db) = self.open(dir, &second.pack) else {
                return Vec::new();
            };
            let second_db = second_db.lock().expect("db mutex");
            let asks: Vec<String> = mids.iter().map(|(mid, _)| mid.clone()).collect();
            match if second.reverse {
                lookup_defs_any(&second_db, &asks, limit)
            } else {
                second_db.lookup_words_any(&asks, limit)
            } {
                Ok(finals) => finals,
                Err(_) => return Vec::new(),
            }
        };
        finals
            .into_iter()
            .map(|row| {
                let mid = if second.reverse {
                    row.definition.clone()
                } else {
                    row.word.clone()
                };
                let fit = mids
                    .iter()
                    .find(|(name, _)| *name == mid)
                    .map(|(_, fit)| *fit)
                    .unwrap_or(WordMatch::Fuzzy);
                (row, second.pack.to_string(), Some((mid, fit)))
            })
            .collect()
    }

    /// A pack's open database, kept for the next ask.
    fn open(&self, dir: &std::path::Path, pack_id: &str) -> Result<Arc<Mutex<DictDb>>, String> {
        if let Some(db) = self
            .dbs
            .lock()
            .map_err(|_| "db lock poisoned")?
            .get(pack_id)
        {
            return Ok(db.clone());
        }
        let db = DictDb::open(&fetch::db_file(dir, pack_id))
            .map_err(|e| format!("open {pack_id}: {e}"))?;
        let db = Arc::new(Mutex::new(db));
        self.dbs
            .lock()
            .map_err(|_| "db lock poisoned")?
            .insert(pack_id.to_string(), db.clone());
        Ok(db)
    }

    /// Tell the UI every pack's row.
    pub fn emit(&self, app: &AppHandle) {
        let _ = app.emit(DICT_EVENT, self.status(app));
    }
}

/// A hop's rows: the word side or the definition side.
fn hop_ask(db: &DictDb, word: &str, reverse: bool, limit: usize) -> anyhow::Result<Vec<RawRow>> {
    if reverse {
        db.lookup_definition(word, limit)
    } else {
        db.lookup_word(word, limit)
    }
}

/// The batched way home a reversed second hop needs.
fn lookup_defs_any(db: &DictDb, asks: &[String], limit: usize) -> anyhow::Result<Vec<RawRow>> {
    let mut out = Vec::new();
    for ask in asks {
        out.extend(db.lookup_definition(ask, limit)?);
    }
    Ok(out)
}

/// Rows to wire, ranked: role fit first, word fit next.
fn finish(rows: Vec<Carried>, ask: &str, pos: Option<&str>, limit: usize) -> Vec<DictEntryWire> {
    let mut entries: Vec<DictEntry> = rows
        .into_iter()
        .map(|(row, pack, bridge)| {
            let (via, fit) = match bridge {
                Some((mid, fit)) => (Some(mid), fit),
                None => (None, classify(ask, &row.word).unwrap_or(WordMatch::Fuzzy)),
            };
            DictEntry {
                word: row.word,
                tags: parse_tags(row.pos.as_deref().unwrap_or_default()),
                pos_raw: row.pos,
                definition: row.definition,
                romanization: row.romanization,
                sense: row.sense,
                pack,
                via,
                word_match: fit,
            }
        })
        .collect();
    order_entries(&mut entries, pos.map(penn_canon));
    entries.truncate(limit);
    entries
        .into_iter()
        .map(|entry| DictEntryWire {
            word: entry.word,
            pos_raw: entry.pos_raw,
            tags: entry.tags,
            definition: entry.definition,
            romanization: entry.romanization,
            sense: entry.sense,
            pack: entry.pack,
            via: entry.via,
            word_match: entry.word_match,
        })
        .collect()
}

/// Progress arrives; the UI gets every pack's row to redraw.
fn narrate(app: &AppHandle, progress: &Progress) {
    // The receipt owns the endings; this hook only narrates.
    match progress.phase {
        Phase::Done | Phase::Failed | Phase::Cancelled => return,
        _ => {}
    }
    let statuses = app.state::<DictManager>().status(app);
    let _ = app.emit(DICT_EVENT, statuses);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_filter_still_answers_with_every_built_pack() {
        let built = vec!["jmdict-en-jp".to_string(), "mcfnlp-en-my".to_string()];
        // No filter and an empty one mean the same thing: all built.
        assert_eq!(wanted_packs(None, &built), built);
        assert_eq!(wanted_packs(Some(Vec::new()), &built), built);
        // A filter keeps only what is built and asked.
        assert_eq!(
            wanted_packs(
                Some(vec!["jmdict-en-jp".to_string(), "blorp".to_string()]),
                &built
            ),
            vec!["jmdict-en-jp".to_string()]
        );
    }

    #[test]
    fn the_wire_shape_is_three_values_and_a_via() {
        let entry = DictEntryWire {
            word: "light".into(),
            pos_raw: Some("v,n".into()),
            tags: parse_tags("v,n"),
            definition: "光".into(),
            romanization: Some("ひかり".into()),
            sense: Some("illumination".into()),
            pack: "jmdict-en-jp".into(),
            via: Some("light".into()),
            word_match: WordMatch::Exact,
        };
        assert_eq!(entry.tags.len(), 2);
        let text = serde_json::to_string(&entry).unwrap();
        assert!(text.contains("wordMatch"));
    }

    #[test]
    fn a_pack_row_reports_where_it_stands() {
        let status = PackStatus {
            id: "mcfnlp-en-my".into(),
            label: "MCF NLP English–Myanmar".into(),
            source: "en".into(),
            target: "my".into(),
            rows: 110_640,
            built: false,
            progress: None,
            phase: "absent".into(),
            message: None,
        };
        let text = serde_json::to_string(&status).unwrap();
        assert!(text.contains("\"built\":false"));
        assert!(text.contains("\"phase\":\"absent\""));
    }

    #[test]
    fn a_pack_says_where_it_stands_in_words() {
        // A built pack is ready, whatever the progress says.
        assert_eq!(phase_of(true, None, None).0, "ready");
        // A conversion under way says so, not "not downloaded".
        let (phase, _) = phase_of(false, Some(&BuildState::Converting), None);
        assert_eq!(phase, "converting");
        // A failed conversion keeps its own reason on the wire.
        let (phase, message) =
            phase_of(false, Some(&BuildState::Failed("bad parquet".into())), None);
        assert_eq!(phase, "failed");
        assert_eq!(message.as_deref(), Some("bad parquet"));
        // A live download reads as downloading.
        let progress = Progress {
            id: "x".into(),
            phase: Phase::Downloading,
            received: 0,
            total: None,
            source: None,
            attempt: 1,
            speed: None,
            eta_secs: None,
            message: None,
            path: None,
            cached: false,
        };
        let (phase, _) = phase_of(false, None, Some(&progress));
        assert_eq!(phase, "downloading");
        // And nothing at all reads as not downloaded.
        assert_eq!(phase_of(false, None, None).0, "absent");
    }
}
