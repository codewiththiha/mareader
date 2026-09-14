//! Persisted app state (settings, library, covers) over localStorage, and the one store that is
//! not app state at all: what a removal kept of the reader's own work ([`kept`]).
//!
//! Deliberately plain functions — a trait + `Box<dyn>` + `OnceLock` global for
//! a single localStorage backend was more architecture than the app needs. If
//! a second backend ever lands it can come back as a trait.
//!
//! Failures are NOT silent: loads warn about what was dropped, saves return a
//! [`StorageError`] the caller decides how to handle.

pub mod kept;

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use ai_core::gloss::GlossMark;
use leptos::prelude::*;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

use crate::state::library::{CoverImage, CoverMap, LibraryState};
// The library's key names, its persisted shape and the migration from the shape
// it replaced all live in `library_core::blob`, so the schema and the rules that
// keep it valid are one crate's business rather than two.
use library_core::blob::{LIBRARY_KEY, LibraryBlob};
use library_core::blob::migrate::{BlobV2, LEGACY_KEY, RecentBook, V2_KEY, migrate_v1, migrate_v2};
use library_core::blob::sanitize as sanitize_library;
use reader_core::settings::{SETTINGS_KEY, Settings, sanitize};

const COVERS_KEY: &str = "pdfreader.covers.v1";
/// Gloss highlights, keyed by the ROW ID the library holds for a book.
///
/// Versioned like the rest: a PDF's mark is a page-space rect in CSS px — stable
/// across zoom and sessions, but NOT across a change in how a page is laid out.
/// If page rendering metrics ever change, bump this rather than let old marks
/// drift onto the wrong words.
///
/// A reflowable mark carries its identity in `context` instead — a tagged
/// envelope holding a block index and a character range
/// (`components::ai::reflow_anchor`) — because its pages are re-cut whenever
/// the typography or column width moves. The envelope is versioned by its own
/// tag, so a change there needs no new storage key.
///
/// The row id rather than the address is what makes this `v2`: a `v1` map is
/// keyed by address, and the two shapes cannot be told apart by looking at one
/// entry, so [`migrate_gloss_keys`] reads the old key and writes the new one
/// rather than overwriting a map this build cannot parse.
const GLOSS_KEY: &str = "pdfreader.gloss.v2";

/// The address-keyed map this build migrated from. Read once, left alone: a
/// reader who downgrades should still find the highlights the build they
/// downgraded to wrote.
const GLOSS_V1_KEY: &str = "pdfreader.gloss.v1";

/// A persistence failure (quota exceeded, storage blocked, serialization
/// error). The UI must never crash on these — but they must not vanish.
///
/// Handling rule (one consistent decision, no per-call judgment): every save
/// failure is reported through [`StorageError::report`] at the call site.
/// Covers could arguably be dropped silently (they regenerate), but a single
/// rule beats a case-by-case call.
#[derive(Debug)]
pub struct StorageError {
    op: &'static str,
    detail: String,
}

impl StorageError {
    /// Surface the failure on the console without interrupting the UI.
    ///
    /// Off wasm — the host test this crate gets from `cargo test --workspace`
    /// — there is no console to warn on and the wasm-bindgen stubs abort when
    /// called, so a failure is dropped rather than printed. That is what makes
    /// the library's services testable on the host at all: a placement writes
    /// the blob, and a write that aborted would take the test runner with it.
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

/// The browser's own key-value store, and `None` wherever there is not one —
/// which is every host test this crate runs, and the reason a save off wasm is
/// a reported no-op rather than a panic.
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
pub fn get(key: &str) -> Option<String> {
    local().and_then(|s| s.get_item(key).ok().flatten())
}

/// Write a raw JSON blob. Quota/security failures surface as an error.
pub fn set(key: &str, value: &str) -> Result<(), StorageError> {
    let storage = local().ok_or_else(|| StorageError {
        op: "set",
        detail: "localStorage unavailable".to_string(),
    })?;
    storage.set_item(key, value).map_err(|e| StorageError {
        op: "set",
        detail: e.as_string().unwrap_or_else(|| "unknown error".to_string()),
    })
}

fn parse<T: serde::de::DeserializeOwned + Default>(op: &'static str, raw: &str) -> T {
    match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            // Corrupt persisted state must not brick the app — but it must
            // not disappear either: the user just lost a saved value.
            warn(op, &format!("invalid JSON, falling back to default ({e})"));
            T::default()
        }
    }
}

/// Load persisted settings; invalid values fall back to defaults + sanitize.
pub fn load_settings() -> Settings {
    let mut settings = get(SETTINGS_KEY)
        .map(|raw| parse("settings", &raw))
        .unwrap_or_default();
    sanitize(&mut settings);
    settings
}

pub fn save_settings(settings: &Settings) -> Result<(), StorageError> {
    let json = serde_json::to_string(settings).map_err(|e| StorageError {
        op: "save_settings",
        detail: format!("serialize failed: {e}"),
    })?;
    set(SETTINGS_KEY, &json)
}

/// Load the library: the current blob when there is one, else the previous
/// schema's — one book per row, then a recent-books list — migrated on the spot.
/// Invalid values fall back to empty rather than bricking the page, and every
/// load is sanitised — a blob can arrive with a shelf member naming no row, and
/// the grid would render a hole.
///
/// Two migrations rather than one, and each is one-way in effect but leaves the
/// key it read alone: a reader who downgrades should still find the library the
/// build they downgraded to wrote, and the first save after this load is what
/// puts the new blob under its own key. The step from `v2` is the row list
/// gaining a kind — every book becomes a book row and nothing else moves — so
/// a library written before links existed loads as a library with no links,
/// which is exactly what it was.
pub fn load_library() -> LibraryBlob {
    if let Some(raw) = get(LIBRARY_KEY) {
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
    let mut blob = migrate_v1(legacy, crate::time::now_ms());
    sanitize_library(&mut blob);
    blob
}

pub fn save_library(blob: &LibraryBlob) -> Result<(), StorageError> {
    let json = serde_json::to_string(blob).map_err(|e| StorageError {
        op: "save_library",
        detail: format!("serialize failed: {e}"),
    })?;
    set(LIBRARY_KEY, &json)
}

/// Load the cover-art map (path -> page-1 JPEG data URL).
pub fn load_covers() -> CoverMap {
    let stored: HashMap<String, CoverImage> = get(COVERS_KEY)
        .map(|raw| parse("covers", &raw))
        .unwrap_or_default();
    stored
        .into_iter()
        .map(|(path, cover)| (path, Arc::new(cover)))
        .collect()
}

/// Save the cover-art map. Serialized through a map of BORROWED covers: the
/// images are the largest thing the app persists, and going through an owned
/// `HashMap` to hand serde something it recognises would copy every data URL
/// for no reason.
pub fn save_covers(covers: &CoverMap) -> Result<(), StorageError> {
    let borrowed: HashMap<&str, &CoverImage> = covers
        .iter()
        .map(|(path, cover)| (path.as_str(), cover.as_ref()))
        .collect();
    let json = serde_json::to_string(&borrowed).map_err(|e| StorageError {
        op: "save_covers",
        detail: format!("serialize failed: {e}"),
    })?;
    set(COVERS_KEY, &json)
}

/// Write the library's current blob, reporting a failure instead of returning
/// it.
///
/// The shelf is written from four moments and three want exactly this: a
/// document opening (the shelf record), a document closing (the last known
/// page), a book leaving the shelf. None can do anything with a
/// `StorageError` — a shelf that will not write is still a shelf the reader
/// can use, and the next write carries the same books again — so all three
/// spelled the same `if let Err(e) = ... { e.report() }` around the same
/// untracked read.
///
/// The fourth is the reading-progress debounce, which keeps [`save_library`]:
/// it snapshots the VALUE and hands it to a timer, because a timer firing
/// during teardown that reached into a disposed signal would panic where a
/// dropped save would not.
///
/// The read here is untracked — the only relationship this module has with the
/// reactive graph: storage takes a value and writes it, never subscribes.
/// Writes are immediate rather than debounced on purpose: two of the three are
/// the last thing before a teardown or window close, and a debounced save is a
/// save that may never land.
pub fn persist_library(library: LibraryState) {
    if let Err(e) = save_library(&library.snapshot()) {
        e.report();
    }
}

/// [`persist_library`] for the cover cache, which is budgeted on its own key:
/// the cap in `crate::services::library::covers::COVER_CAP` is only a real quota if the
/// images are written back after a prune, not just dropped from memory.
pub fn persist_covers(library: LibraryState) {
    if let Err(e) = library.covers.with_untracked(|covers| save_covers(covers)) {
        e.report();
    }
}

/// Carry the address-keyed highlights a previous build wrote onto the rows that
/// were reading them.
///
/// The key changed from an address to a row id, and the two shapes are not
/// distinguishable entry by entry, so this runs once at load with the row list in
/// hand: an `"<id>::<address>"` entry is a private row's own list and is re-keyed
/// onto that id, and a bare address is the list every shared row at it read,
/// which goes to the first such row — the one a reader opening that file by
/// address would have been given.
///
/// An entry no row answers for is left where it is rather than dropped. It is a
/// list belonging to a book the library no longer holds, and the honest answer to
/// "whose marks are these" is that nobody knows; a later load that finds the row
/// again picks them up, and a removal that never comes costs one localStorage
/// entry rather than a reader's highlights.
///
/// Skip throughout: a highlight list no row answers for costs that list, not a
/// library that refuses to load.
pub fn migrate_gloss_keys(books: &[library_core::book::Row]) {
    let Some(raw) = get(GLOSS_V1_KEY) else {
        return;
    };
    let old: HashMap<String, Vec<GlossMark>> = parse("gloss v1", &raw);
    if old.is_empty() {
        return;
    }
    let mut carried = load_gloss();
    let mut moved = 0usize;
    for (key, marks) in old {
        if marks.is_empty() {
            continue;
        }
        let id = match key.split_once("::") {
            // A private row's own list: the id is the half in front of the seam,
            // and it is re-keyed onto that row whether or not the address it wore
            // is still the one it reads.
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
        // Two old keys can land on one row — an address and a private row of it —
        // so the marks are unioned rather than overwritten.
        let existing = carried.entry(id).or_default();
        for mark in marks {
            if !existing.iter().any(|kept| kept.same_spot(&mark)) {
                existing.push(mark);
            }
        }
        moved += 1;
    }
    if moved == 0 {
        return;
    }
    if let Err(e) = save_gloss(&carried) {
        e.report();
    }
}

/// Load every book's gloss highlights, keyed by row id.
pub fn load_gloss() -> HashMap<String, Vec<GlossMark>> {
    get(GLOSS_KEY)
        .map(|raw| parse("gloss", &raw))
        .unwrap_or_default()
}

fn save_gloss(all: &HashMap<String, Vec<GlossMark>>) -> Result<(), StorageError> {
    let json = serde_json::to_string(all).map_err(|e| StorageError {
        op: "save_gloss",
        detail: format!("serialize failed: {e}"),
    })?;
    set(GLOSS_KEY, &json)
}

/// Drop one row's marks: the reader's data goes with the book.
///
/// A removal that left them behind would leave the largest half of what a
/// reader put into a book sitting in localStorage under a row nothing points at
/// any more.
pub fn remove_gloss(row_id: &str) {
    take_gloss(row_id);
}

/// Take one row's marks out of the store. The row id is never reused, so the
/// entry goes whatever the answer was — and a removal that KEEPS the reader's
/// data carries the marks away with it (`crate::storage::kept::remember`)
/// rather than dropping them.
///
/// Read-modify-write rather than keeping the map in memory: marks change only
/// when the reader explains a word (human-paced), and re-reading keeps a second
/// window's marks from being clobbered by a write in this one.
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
/// Read-modify-write for [`take_gloss`]'s reason: a second window's marks must
/// not be clobbered by a write in this one.
pub fn persist_gloss(row_id: &str, marks: &[GlossMark]) {
    let mut all = load_gloss();
    all.insert(row_id.to_string(), marks.to_vec());
    if let Err(e) = save_gloss(&all) {
        e.report();
    }
}
