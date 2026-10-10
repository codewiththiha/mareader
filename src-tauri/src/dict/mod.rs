//! The dictionary backend: packs in, ranked entries out.
//! English bridges the gaps between shores.

pub mod build;
pub mod db;
pub mod fetch;
pub mod legacy;
pub mod packs;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
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

/// Where one conversion gets its rows.
#[derive(Debug, Clone)]
enum ConvertInput {
    Parquet,
    Legacy(PathBuf),
}

/// One entry on the wire to a hover card or sidebar search.
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

/// One carried row: the row, its pack, a bridge's word, and its fit.
type Carried = (RawRow, String, Option<String>, WordMatch);

/// The packs a search may ask: the filter over what is built.
fn wanted_packs(pack_ids: Option<Vec<String>>, built: &[String]) -> Vec<String> {
    match pack_ids {
        Some(ids) if !ids.is_empty() => ids.into_iter().filter(|id| built.contains(id)).collect(),
        _ => built.to_vec(),
    }
}

/// Every pack the planner may route through.
fn pack_refs() -> Vec<dict_core::PackRef> {
    PACKS
        .iter()
        .map(|pack| dict_core::PackRef {
            id: pack.id,
            source: pack.source,
            target: pack.target,
        })
        .collect()
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
            Some(Phase::Preparing | Phase::Downloading | Phase::Retrying | Phase::Verifying) => {
                ("downloading".to_string(), None)
            }
            Some(Phase::Paused) => ("paused".to_string(), None),
            Some(Phase::Failed) => (
                "failed".to_string(),
                progress.and_then(|progress| progress.message.clone()),
            ),
            _ => ("absent".to_string(), None),
        },
    }
}

/// Replace a database only after its complete replacement exists.
fn install_database(building: &Path, target: &Path) -> Result<(), String> {
    let backup = target.with_extension("db.previous");
    if backup.exists() {
        std::fs::remove_file(&backup).map_err(|error| error.to_string())?;
    }
    let had_target = target.exists();
    if had_target {
        std::fs::rename(target, &backup).map_err(|error| error.to_string())?;
    }
    if let Err(error) = std::fs::rename(building, target) {
        if had_target {
            let _ = std::fs::rename(&backup, target);
        }
        return Err(error.to_string());
    }
    if had_target {
        let _ = std::fs::remove_file(backup);
    }
    Ok(())
}

/// The dictionary system: open packs and the routes between them.
#[derive(Default)]
pub struct DictManager {
    dbs: Mutex<HashMap<String, Arc<Mutex<DictDb>>>>,
    builds: Mutex<HashMap<String, BuildState>>,
    legacy_checked: Mutex<HashSet<String>>,
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
        self.adopt_landed(app, dir.as_deref());
        let downloads = app.state::<AppDownloads>();
        PACKS
            .iter()
            .map(|pack| {
                let built = dir
                    .as_deref()
                    .map(|dir| db::is_usable(&fetch::db_file(dir, pack.id)))
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
            .collect()
    }

    /// Adopt database or parquet files left by an earlier run.
    fn adopt_landed(&self, app: &AppHandle, dir: Option<&std::path::Path>) {
        let Some(dir) = dir else {
            return;
        };
        let downloads = app.state::<AppDownloads>();
        for pack in PACKS {
            if db::is_usable(&fetch::db_file(dir, pack.id)) || self.has_build_record(pack.id) {
                continue;
            }
            let progress = downloads.status(pack.id);
            if matches!(
                progress.as_ref().map(|progress| progress.phase),
                Some(
                    Phase::Preparing
                        | Phase::Downloading
                        | Phase::Retrying
                        | Phase::Verifying
                        | Phase::Paused
                )
            ) {
                continue;
            }
            if fetch::parquet_file(dir, pack.id).is_file() {
                self.start_convert(app, pack.id);
                continue;
            }
            if legacy::is_marked(dir, pack.id) || !self.claim_legacy_scan(pack.id) {
                continue;
            }
            let pair = format!("{}-{}", pack.source, pack.target);
            let old = legacy::candidates(app, dir, pack.id, &pair)
                .into_iter()
                .find(|path| build::legacy_db_matches(path, pack.id, &pair).unwrap_or(false));
            if let Some(path) = old {
                self.start_conversion(app, pack.id, ConvertInput::Legacy(path));
            }
        }
    }

    /// Whether this run already checked old paths for the pack.
    fn claim_legacy_scan(&self, pack_id: &str) -> bool {
        self.legacy_checked
            .lock()
            .map(|mut checked| checked.insert(pack_id.to_string()))
            .unwrap_or(false)
    }

    /// A new download can finish before old files are checked again.
    fn reset_legacy_scan(&self, pack_id: &str) {
        if let Ok(mut checked) = self.legacy_checked.lock() {
            checked.remove(pack_id);
        }
    }

    /// Whether this run already owns or rejected the pack's conversion.
    fn has_build_record(&self, pack_id: &str) -> bool {
        self.builds
            .lock()
            .map(|builds| builds.contains_key(pack_id))
            .unwrap_or(true)
    }

    /// Convert a landed parquet now.
    fn start_convert(&self, app: &AppHandle, pack_id: &str) {
        self.start_conversion(app, pack_id, ConvertInput::Parquet);
    }

    /// Convert one source now; never two builds for a pack.
    fn start_conversion(&self, app: &AppHandle, pack_id: &str, input: ConvertInput) {
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
            app.state::<DictManager>()
                .convert(&app, &pack_id, input)
                .await;
        });
    }

    /// A landed body becomes a sqlite, renamed into place when whole.
    async fn convert(&self, app: &AppHandle, pack_id: &str, input: ConvertInput) {
        let legacy_input = matches!(&input, ConvertInput::Legacy(_));
        let result = self.convert_blocking(app, pack_id, input).await;
        let mut builds = match self.builds.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match result {
            Ok(_) => {
                builds.remove(pack_id);
                if legacy_input && let Ok(dir) = Self::dir(app) {
                    legacy::mark(&dir, pack_id);
                }
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
    async fn convert_blocking(
        &self,
        app: &AppHandle,
        pack_id: &str,
        input: ConvertInput,
    ) -> Result<u64, String> {
        let dir = Self::dir(app)?;
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("create {}: {error}", dir.display()))?;
        let parquet = fetch::parquet_file(&dir, pack_id);
        let target = fetch::db_file(&dir, pack_id);
        let building = fetch::db_building(&dir, pack_id);
        let pair = packs::pack(pack_id)
            .map(|pack| format!("{}-{}", pack.source, pack.target))
            .ok_or_else(|| format!("no pack {pack_id}"))?;
        if let Ok(mut dbs) = self.dbs.lock() {
            dbs.remove(pack_id);
        }
        let pack_id = pack_id.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            let built = match input {
                ConvertInput::Parquet => {
                    build::build_db(&parquet, &building).map_err(|error| error.to_string())
                }
                ConvertInput::Legacy(source) => {
                    build::build_legacy_db(&source, &building, &pack_id, &pair)
                        .map_err(|error| error.to_string())?
                        .ok_or_else(|| "legacy database has no rows for this pack".to_string())
                }
            };
            let result = built.and_then(|rows| {
                install_database(&building, &target)?;
                Ok(rows)
            });
            if result.is_err() {
                let _ = std::fs::remove_file(&building);
            }
            result
        })
        .await
        .map_err(|error| format!("convert worker: {error}"))?
    }

    /// Start one pack's download, then convert it when the body lands.
    pub fn begin_download(&self, app: AppHandle, pack_id: &str) -> Result<(), String> {
        let dir = Self::dir(&app)?;
        let watched = app.clone();
        let job = fetch::job_for(dir.clone(), pack_id)
            .ok_or_else(|| format!("no pack {pack_id}"))?
            .on_progress(move |progress| narrate(&watched, progress));
        let host = TauriHost::new(app.clone());
        let receipt = app.state::<AppDownloads>().start(&host, job)?;
        self.clear_failed(pack_id);
        self.reset_legacy_scan(pack_id);
        legacy::unmark(&dir, pack_id);
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
        legacy::mark(&dir, pack_id);
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
        let refs = pack_refs();
        for plan in plans(from, to, &refs) {
            if !plan.hops.iter().all(|hop| built.contains(&hop.pack)) {
                continue;
            }
            let rows = self.run_plan(&dir, &plan, word, limit, false);
            if !rows.is_empty() {
                return finish(rows, pos, limit);
            }
        }
        // No route carried the ask: the pair's own packs, and no
        // other language's.
        let mut rows = Vec::new();
        for pack_id in &built {
            let Some(def) = packs::pack(pack_id) else {
                continue;
            };
            let forward = def.source == from && def.target == to;
            let backward = def.source == to && def.target == from;
            if !forward && !backward {
                continue;
            }
            if let Ok(db) = self.open(&dir, pack_id) {
                let db = db.lock().expect("db mutex");
                let found = if backward {
                    db.lookup_definition(word, limit)
                } else {
                    db.lookup_word(word, limit)
                };
                if let Ok(found) = found {
                    rows.extend(found.into_iter().map(|row| {
                        let door = if backward {
                            row.definition.clone()
                        } else {
                            row.word.clone()
                        };
                        let door = if backward {
                            row.definition.clone()
                        } else {
                            row.word.clone()
                        };
                        let fit = classify(word, &door).unwrap_or(WordMatch::Fuzzy);
                        (row, pack_id.clone(), None, fit)
                    }));
                }
            }
        }
        finish(rows, pos, limit)
    }

    /// The route's ask: a word on either side, or between two shores.
    pub fn search(
        &self,
        app: &AppHandle,
        ask: &str,
        pack_ids: Option<Vec<String>>,
        from: Option<&str>,
        to: Option<&str>,
        limit: usize,
    ) -> Vec<DictEntryWire> {
        let Ok(dir) = Self::dir(app) else {
            return Vec::new();
        };
        // A pair asks only its own routes; no bridge otherwise.
        if let (Some(from), Some(to)) = (from, to)
            && from != to
            && !from.is_empty()
            && !to.is_empty()
        {
            return self.search_route(&dir, from, to, ask, limit);
        }
        let built = self.built_packs(&dir);
        let wanted = wanted_packs(pack_ids, &built);
        let mut rows = Vec::new();
        for pack_id in &wanted {
            if let Ok(db) = self.open(&dir, pack_id) {
                let db = db.lock().expect("db mutex");
                if let Ok(found) = db.search(ask, limit) {
                    rows.extend(found.into_iter().map(|row| {
                        // Either side may have matched; the tighter fit stands.
                        let fit = classify(ask, &row.word)
                            .into_iter()
                            .chain(classify(ask, &row.definition))
                            .min()
                            .unwrap_or(WordMatch::Fuzzy);
                        (row, pack_id.clone(), None, fit)
                    }));
                }
            }
        }
        finish(rows, None, limit)
    }

    /// One pair's routes, best first: the first plan that answers wins.
    fn search_route(
        &self,
        dir: &std::path::Path,
        from: &str,
        to: &str,
        ask: &str,
        limit: usize,
    ) -> Vec<DictEntryWire> {
        let built = self.built_packs(dir);
        let refs = pack_refs();
        for plan in plans(from, to, &refs) {
            if !plan.hops.iter().all(|hop| built.contains(&hop.pack)) {
                continue;
            }
            let rows = self.run_plan(dir, &plan, ask, limit, true);
            if !rows.is_empty() {
                return finish(rows, None, limit);
            }
        }
        Vec::new()
    }

    /// The packs whose sqlite stands built right now.
    fn built_packs(&self, dir: &std::path::Path) -> Vec<String> {
        PACKS
            .iter()
            .filter(|pack| db::is_usable(&fetch::db_file(dir, pack.id)))
            .map(|pack| pack.id.to_string())
            .collect()
    }

    /// One plan's rows: one hop, or two through the hub.
    /// `search` is loose.
    fn run_plan(
        &self,
        dir: &std::path::Path,
        plan: &dict_core::Plan,
        word: &str,
        limit: usize,
        search: bool,
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
            match hop_ask(&db, word, first.reverse, search, limit * 2) {
                Ok(rows) => rows,
                Err(_) => return Vec::new(),
            }
        };
        // The door: the column this hop was asked through.
        let door = |row: &RawRow| {
            if first.reverse {
                row.definition.clone()
            } else {
                row.word.clone()
            }
        };
        let Some(second) = rest.first() else {
            return rows
                .into_iter()
                .map(|row| {
                    let fit = classify(word, &door(&row)).unwrap_or(WordMatch::Fuzzy);
                    (row, first.pack.to_string(), None, fit)
                })
                .collect();
        };
        // The bridge word: what the second hop asks. The fit is
        // the first hop's.
        let mids: Vec<(String, WordMatch)> = rows
            .iter()
            .map(|row| {
                let mid = if second.reverse {
                    row.definition.clone()
                } else {
                    row.word.clone()
                };
                let fit = classify(word, &door(row)).unwrap_or(WordMatch::Fuzzy);
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
                (row, second.pack.to_string(), Some(mid), fit)
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

/// A hop's rows: the side the hop opens, tightly or loosely.
fn hop_ask(
    db: &DictDb,
    word: &str,
    reverse: bool,
    search: bool,
    limit: usize,
) -> anyhow::Result<Vec<RawRow>> {
    match (reverse, search) {
        (false, _) => db.lookup_word(word, limit),
        (true, false) => db.lookup_definition(word, limit),
        // A panel's ask on the far shore: prefixes and near spellings too.
        (true, true) => db.search_definition(word, limit),
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
fn finish(rows: Vec<Carried>, pos: Option<&str>, limit: usize) -> Vec<DictEntryWire> {
    let mut entries: Vec<DictEntry> = rows
        .into_iter()
        .map(|(row, pack, via, word_match)| DictEntry {
            word: row.word,
            tags: parse_tags(row.pos.as_deref().unwrap_or_default()),
            pos_raw: row.pos,
            definition: row.definition,
            romanization: row.romanization,
            sense: row.sense,
            pack,
            via,
            word_match,
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
    fn a_finished_database_replaces_the_old_file() {
        let dir = std::env::temp_dir().join(format!("dict_replace_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("pack.db");
        let building = dir.join("pack.db.tmp");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&building, b"new").unwrap();
        install_database(&building, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new".to_vec());
        assert!(!building.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

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

    /// One row on its way to the wire: a pack, a bridge, a fit.
    fn carried(word: &str, definition: &str, via: Option<&str>, fit: WordMatch) -> Carried {
        (
            RawRow {
                word: word.into(),
                pos: Some("n".into()),
                definition: definition.into(),
                romanization: None,
                sense: None,
            },
            "mcfnlp-en-my".to_string(),
            via.map(str::to_string),
            fit,
        )
    }

    #[test]
    fn a_row_is_judged_by_the_door_it_came_in_by() {
        // A reversed hop matched the far side: the fit says so.
        let got = finish(
            vec![carried("light", "မီး", None, WordMatch::Exact)],
            None,
            10,
        );
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].word_match, WordMatch::Exact);
        assert_eq!(got[0].via, None);
    }

    #[test]
    fn a_bridge_row_names_the_word_it_rode() {
        let got = finish(
            vec![carried("light", "光", Some("light"), WordMatch::Prefix)],
            None,
            10,
        );
        assert_eq!(got[0].via.as_deref(), Some("light"));
        assert_eq!(got[0].word_match, WordMatch::Prefix);
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
