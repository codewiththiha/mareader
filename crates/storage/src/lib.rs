//! Persisted app state over localStorage, plus `kept`.
pub mod kept;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use ai_core::gloss::GlossMark;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

use runtime_contract::covers::{CoverImage, CoverMap};
// The library's keys, shape and migration live in `library_core::blob`.
use library_core::blob::migrate::{BlobV2, LEGACY_KEY, RecentBook, V2_KEY, migrate_v1, migrate_v2};
use library_core::blob::sanitize as sanitize_library;
use library_core::blob::{LIBRARY_KEY, LibraryBlob, RETIRED_LIBRARY_KEY};
use reader_core::settings::{RETIRED_SETTINGS_KEY, SETTINGS_KEY, Settings, sanitize};

const COVERS_KEY: &str = "mareader.covers.v1";

/// The key the app read before it was renamed.
const RETIRED_COVERS_KEY: &str = "pdfreader.covers.v1";

/// Gloss highlights, keyed by the ROW ID the library holds.
const GLOSS_KEY: &str = "mareader.gloss.v2";

/// [`GLOSS_KEY`] before the rename.
const RETIRED_GLOSS_KEY: &str = "pdfreader.gloss.v2";

/// The address-keyed map this build migrated from.
const GLOSS_V1_KEY: &str = "pdfreader.gloss.v1";

/// One-shot gate for the address-to-row migration.
const GLOSS_V2_MIGRATED_KEY: &str = "mareader.gloss.v2.migrated";

/// The gate as the pre-rebrand build set it.
const RETIRED_GLOSS_V2_MIGRATED_KEY: &str = "pdfreader.gloss.v2.migrated";

/// A persistence failure: quota, blocked storage, serialization.
#[derive(Debug)]
pub struct StorageError {
    op: &'static str,
    detail: String,
}

impl StorageError {
    /// Surface the failure on the console without interrupting the UI.
    pub fn report(&self) {
        #[cfg(target_arch = "wasm32")]
        web_sys::console::warn_1(&JsValue::from_str(&format!("[storage] {self}")));
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.op, self.detail)
    }
}

fn warn(op: &'static str, detail: &str) {
    #[cfg(target_arch = "wasm32")]
    web_sys::console::warn_1(&JsValue::from_str(&format!("[storage] {op}: {detail}")));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (op, detail);
}

/// The browser's own key-value store, `None` off wasm.
fn local() -> Option<web_sys::Storage> {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()?.local_storage().ok()?
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// Read a raw JSON blob, if present and readable.
pub(crate) fn get(key: &str) -> Option<String> {
    local().and_then(|s| s.get_item(key).ok().flatten())
}

/// Write a raw JSON blob. Quota/security failures surface as an error.
pub(crate) fn set(key: &str, value: &str) -> Result<(), StorageError> {
    let storage = local().ok_or_else(|| StorageError {
        op: "set",
        detail: "localStorage unavailable".to_string(),
    })?;
    storage.set_item(key, value).map_err(|e| StorageError {
        op: "set",
        detail: e.as_string().unwrap_or_else(|| "unknown error".to_string()),
    })
}

/// Serialize for storage, naming the operation on failure.
fn encode<T: serde::Serialize + ?Sized>(
    op: &'static str,
    value: &T,
) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|e| StorageError {
        op,
        detail: format!("serialize failed: {e}"),
    })
}

/// Read a store under its current key, falling back to the retired one.
fn load_keyed<T: serde::de::DeserializeOwned + Default>(
    op: &'static str,
    key: &str,
    retired: &str,
) -> T {
    get(key)
        .or_else(|| get(retired))
        .map(|raw| parse(op, &raw))
        .unwrap_or_default()
}

fn parse<T: serde::de::DeserializeOwned + Default>(op: &'static str, raw: &str) -> T {
    match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            // Corrupt state must not brick the app, nor disappear silently.
            warn(op, &format!("invalid JSON, falling back to default ({e})"));
            T::default()
        }
    }
}

/// Load persisted settings; invalid values fall back to defaults + sanitize.
pub fn load_settings() -> Settings {
    let mut settings: Settings = load_keyed("settings", SETTINGS_KEY, RETIRED_SETTINGS_KEY);
    sanitize(&mut settings);
    settings
}

pub fn save_settings(settings: &Settings) -> Result<(), StorageError> {
    set(SETTINGS_KEY, &encode("save_settings", settings)?)
}

/// Load the library, migrating the previous schema's blob.
pub fn load_library() -> LibraryBlob {
    if let Some(raw) = get(LIBRARY_KEY) {
        let mut blob: LibraryBlob = parse("library", &raw);
        sanitize_library(&mut blob);
        return blob;
    }
    if let Some(raw) = get(RETIRED_LIBRARY_KEY) {
        let mut blob: LibraryBlob = parse("library", &raw);
        sanitize_library(&mut blob);
        return blob;
    }
    if let Some(raw) = get(V2_KEY) {
        let legacy: BlobV2 = parse("library v2", &raw);
        let mut blob = migrate_v2(legacy);
        sanitize_library(&mut blob);
        return blob;
    }
    let legacy: Vec<RecentBook> = get(LEGACY_KEY)
        .map(|raw| parse("library v1", &raw))
        .unwrap_or_default();
    if legacy.is_empty() {
        return LibraryBlob::default();
    }
    let mut blob = migrate_v1(legacy, runtime_contract::time::now_ms());
    sanitize_library(&mut blob);
    blob
}

pub fn save_library(blob: &LibraryBlob) -> Result<(), StorageError> {
    set(LIBRARY_KEY, &encode("save_library", blob)?)
}

/// A cheap identity for one store's current contents: a hash.
fn stamp_of(key: &str) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let raw = get(key)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    raw.len().hash(&mut hasher);
    raw.hash(&mut hasher);
    Some(hasher.finish())
}

/// The library blob's stamp (see [`stamp_of`]).
pub fn library_stamp() -> Option<u64> {
    stamp_of(LIBRARY_KEY)
}

/// The cover map's stamp (see [`stamp_of`]).
pub fn covers_stamp() -> Option<u64> {
    stamp_of(COVERS_KEY)
}

/// The settings blob's stamp (see [`stamp_of`]).
pub fn settings_stamp() -> Option<u64> {
    stamp_of(SETTINGS_KEY)
}

/// Load the cover-art map (path -> page-1 JPEG data URL).
pub fn load_covers() -> CoverMap {
    let stored: HashMap<String, CoverImage> = load_keyed("covers", COVERS_KEY, RETIRED_COVERS_KEY);
    stored
        .into_iter()
        .map(|(path, cover)| (path, Arc::new(cover)))
        .collect()
}

/// Save the cover-art map, through a map of BORROWED covers.
pub fn save_covers(covers: &CoverMap) -> Result<(), StorageError> {
    let borrowed: HashMap<&str, &CoverImage> = covers
        .iter()
        .map(|(path, cover)| (path.as_str(), cover.as_ref()))
        .collect();
    set(COVERS_KEY, &encode("save_covers", &borrowed)?)
}

/// Apply a read point to the persisted library blob.
pub fn apply_read_point(point: &runtime_contract::boundary::ReadPoint) {
    let mut blob = load_library();
    // The same recorder the reader's tail used: it mints a linked row.
    let lib_point = library_core::book::ReadPoint {
        page: point.page,
        num_pages: point.num_pages,
        fraction: point.fraction,
    };
    library_core::book::record_read(
        &mut blob.books,
        point.book_id.as_deref(),
        &point.path,
        point.title.clone(),
        point.author.clone(),
        lib_point,
        runtime_contract::time::now_ms(),
    );
    let _ = save_library(&blob);
}

/// Carry address-keyed highlights onto the rows that read them.
pub fn migrate_gloss_keys(books: &[library_core::book::Row]) {
    if get(GLOSS_V2_MIGRATED_KEY)
        .or_else(|| get(RETIRED_GLOSS_V2_MIGRATED_KEY))
        .is_some()
    {
        return;
    }
    let Some(raw) = get(GLOSS_V1_KEY) else {
        return;
    };
    let old: HashMap<String, Vec<GlossMark>> = parse("gloss v1", &raw);
    if old.is_empty() {
        return;
    }
    let mut carried = load_gloss();
    for (key, marks) in old {
        if marks.is_empty() {
            continue;
        }
        let id = match key.split_once("::") {
            // A private row's own list: re-keyed onto the id.
            Some((id, _)) => library_core::book::find_by_id(books, id)
                .map(|b| b.id.clone())
                .unwrap_or_else(|| id.to_string()),
            // The list every shared row at this address read.
            None => library_core::book::book_rows(books)
                .find(|b| b.path() == key && !b.independent)
                .map(|b| b.id.clone())
                .unwrap_or_default(),
        };
        if id.is_empty() {
            continue;
        }
        // Two old keys can land on one row: the marks are unioned.
        let existing = carried.entry(id).or_default();
        for mark in marks {
            if !existing.iter().any(|kept| kept.same_spot(&mark)) {
                existing.push(mark);
            }
        }
    }
    if let Err(e) = save_gloss(&carried) {
        e.report();
        return;
    }
    if let Err(e) = set(GLOSS_V2_MIGRATED_KEY, "1") {
        e.report();
    }
}

/// Load every book's gloss highlights, keyed by row id.
pub fn load_gloss() -> HashMap<String, Vec<GlossMark>> {
    load_keyed("gloss", GLOSS_KEY, RETIRED_GLOSS_KEY)
}

fn save_gloss(all: &HashMap<String, Vec<GlossMark>>) -> Result<(), StorageError> {
    set(GLOSS_KEY, &encode("save_gloss", all)?)
}

/// Drop one row's marks: the reader's data goes with the book.
pub fn remove_gloss(row_id: &str) {
    take_gloss(row_id);
}

/// Take one row's marks out of the store.
pub fn take_gloss(row_id: &str) -> Vec<GlossMark> {
    let mut all = load_gloss();
    let Some(marks) = all.remove(row_id) else {
        return Vec::new();
    };
    if let Err(e) = save_gloss(&all) {
        e.report();
    }
    marks
}

/// Replace one row's marks and write the whole map back.
pub fn persist_gloss(row_id: &str, marks: &[GlossMark]) {
    let mut all = load_gloss();
    all.insert(row_id.to_string(), marks.to_vec());
    if let Err(e) = save_gloss(&all) {
        e.report();
    }
}

/// One row's marks crossing the reader to Shell boundary, as JSON.
pub fn encode_gloss(marks: &[GlossMark]) -> Result<String, StorageError> {
    encode("encode_gloss", marks)
}

fn decode_gloss(encoded: &str) -> Result<Vec<GlossMark>, StorageError> {
    serde_json::from_str(encoded).map_err(|e| StorageError {
        op: "decode_gloss",
        detail: format!("parse failed: {e}"),
    })
}

/// The writer's half of `encode_gloss`: decode, then persist.
pub fn persist_encoded_gloss(row_id: &str, encoded: &str) {
    match decode_gloss(encoded) {
        Ok(marks) => persist_gloss(row_id, &marks),
        Err(e) => e.report(),
    }
}

/// One row's marks, copied onto another row.
pub fn copy_gloss(from_id: &str, to_id: &str) {
    let mut all = load_gloss();
    let Some(marks) = all.get(from_id) else {
        return;
    };
    if marks.is_empty() {
        return;
    }
    all.insert(
        to_id.to_string(),
        re_ided(marks, runtime_contract::time::now_ms()),
    );
    if let Err(e) = save_gloss(&all) {
        e.report();
    }
}

/// The marks a duplicate wears, under freshly minted ids.
fn re_ided(marks: &[GlossMark], now_ms: u64) -> Vec<GlossMark> {
    marks
        .iter()
        .enumerate()
        .map(|(at, mark)| GlossMark {
            id: ai_core::gloss::mark_id(mark.anchor.page, now_ms + at as u64),
            ..mark.clone()
        })
        .collect()
}

/// Build a reader launch descriptor for an in-session open.
pub fn resolve_launch(path: &str) -> Option<runtime_contract::boundary::LaunchDocument> {
    use library_core::book::resume_point;
    let blob = load_library();
    let book_id = library_core::book::book_rows(&blob.books)
        .find(|b| b.path() == path && !b.independent)
        .map(|b| b.id.clone());
    let (resume_page, saved_fraction) = resume_point(&blob.books, book_id.as_deref(), path);
    let display_name = book_id
        .as_deref()
        .and_then(|id| library_core::book::find_by_id(&blob.books, id))
        .map(|b| b.title());
    Some(runtime_contract::boundary::LaunchDocument {
        book_id,
        path: path.to_string(),
        resume_page,
        saved_fraction,
        blend_override: false,
        cover_data_url: None,
        display_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_core::gloss::{GlossBox, PageAnchor};

    fn mark(id: &str, word: &str, page: u32) -> GlossMark {
        GlossMark {
            id: id.to_string(),
            word: word.to_string(),
            context: "the sentence it stood in".to_string(),
            anchor: PageAnchor {
                page,
                rect: GlossBox {
                    x: 10.0,
                    y: 20.0,
                    w: 30.0,
                    h: 8.0,
                    r: 0.0,
                },
            },
        }
    }

    #[test]
    fn a_copied_list_keeps_its_spots_and_mints_its_own_ids() {
        let marks = vec![mark("g3-1", "palimpsest", 3), mark("g3-2", "sietch", 3)];
        let fresh = re_ided(&marks, 1_700);
        assert_eq!(fresh.len(), 2);
        for (old, new) in marks.iter().zip(&fresh) {
            assert_eq!(new.word, old.word, "the explained word travels");
            assert_eq!(new.context, old.context, "the context travels");
            assert_eq!(new.anchor, old.anchor, "the spot travels");
            assert_ne!(new.id, old.id, "the id does not");
        }
        assert_ne!(fresh[0].id, fresh[1].id, "two marks on one page differ");
        assert_eq!(
            fresh[0].id, "g3-1700",
            "the scheme the capture sites mint is the scheme the copy mints"
        );
        assert_eq!(fresh[1].id, "g3-1701", "the index keeps the stamps apart");
    }

    #[test]
    fn a_list_survives_the_boundary_encoding_and_garbage_does_not_decode() {
        let marks = vec![mark("g3-1", "palimpsest", 3), mark("g9-2", "sietch", 9)];
        let encoded = encode_gloss(&marks).expect("a mark list encodes");
        assert_eq!(decode_gloss(&encoded).expect("and decodes"), marks);
        assert!(
            decode_gloss("[]")
                .expect("an emptied list decodes")
                .is_empty()
        );
        assert!(
            decode_gloss("{\"not\":\"a list\"}").is_err(),
            "a malformed list is refused, so it can never be written"
        );
    }

    #[test]
    fn an_empty_list_never_reaches_storage() {
        // The guard the caller rides: nothing to copy writes nothing.
        assert!(re_ided(&[], 5).is_empty());
    }
}
