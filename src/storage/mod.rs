//! Persisted app state (settings, library, covers) over localStorage, plus the
//! one store that is not app state at all: what a removal kept of the
//! reader's own work ([`kept`]).
//!
//! Plain functions, not a trait: there is one localStorage backend, and a
//! second one can bring the abstraction back with it.
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
use library_core::blob::{LIBRARY_KEY, RETIRED_LIBRARY_KEY, LibraryBlob};
use library_core::blob::migrate::{BlobV2, LEGACY_KEY, RecentBook, V2_KEY, migrate_v1, migrate_v2};
use library_core::blob::sanitize as sanitize_library;
use reader_core::settings::{RETIRED_SETTINGS_KEY, SETTINGS_KEY, Settings, sanitize};

const COVERS_KEY: &str = "mareader.covers.v1";

/// The key the app read and wrote before it was renamed. Every store below
/// reads its retired key as a fallback and writes only the current one: the
/// contents are the same schema, so the first save after a load is what moves
/// a reader onto the new name, and a downgrade still finds the data it wrote.
const RETIRED_COVERS_KEY: &str = "pdfreader.covers.v1";

/// Gloss highlights, keyed by the ROW ID the library holds for a book.
///
/// A PDF's mark is a page-space rect in CSS px — stable across zoom and
/// sessions, but NOT across a change in how a page is laid out. If page
/// rendering metrics ever change, bump this rather than let old marks drift
/// onto the wrong words. A reflowable mark carries its identity in `context`
/// instead (a tagged envelope in `components::ai::reflow_anchor`), versioned
/// by its own tag, so a change there needs no new storage key.
///
/// The row id rather than the address is what makes this `v2`: a `v1` map is
/// keyed by address, and the two shapes cannot be told apart entry by entry,
/// so [`migrate_gloss_keys`] reads the old key and writes the new one rather
/// than overwriting a map this build cannot parse.
const GLOSS_KEY: &str = "mareader.gloss.v2";

/// [`GLOSS_KEY`] before the rename.
const RETIRED_GLOSS_KEY: &str = "pdfreader.gloss.v2";

/// The address-keyed map this build migrated from. Read once, left alone: a
/// reader who downgrades should still find the highlights the build they
/// downgraded to wrote.
const GLOSS_V1_KEY: &str = "pdfreader.gloss.v1";

/// One-shot gate for the address-to-row migration. The v1 data itself stays
/// in place so an older build can still read it after a downgrade.
const GLOSS_V2_MIGRATED_KEY: &str = "mareader.gloss.v2.migrated";

/// The gate as the pre-rebrand build set it. Read alongside the current one so
/// a reader who already carried their `v1` marks across is not made to do it
/// twice.
const RETIRED_GLOSS_V2_MIGRATED_KEY: &str = "pdfreader.gloss.v2.migrated";

/// A persistence failure (quota exceeded, storage blocked, serialization
/// error). The UI must never crash on these — but they must not vanish.
///
/// One handling rule, no per-call judgment: every save failure is reported
/// through [`StorageError::report`] at the call site. Covers could arguably
/// be dropped silently (they regenerate), but a single rule beats a
/// case-by-case call.
#[derive(Debug)]
pub struct StorageError {
    op: &'static str,
    detail: String,
}

impl StorageError {
    /// Surface the failure on the console without interrupting the UI.
    ///
    /// Off wasm there is no console and the wasm-bindgen stubs abort when
    /// called, so a failure is dropped rather than printed. That is what
    /// makes the library's services testable on the host at all: a placement
    /// writes the blob, and a write that aborted would take the test runner
    /// with it.
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

/// Serialize for storage, naming the operation a failure is reported against.
/// The four savers were four copies of this one `map_err`.
fn encode<T: serde::Serialize + ?Sized>(op: &'static str, value: &T) -> Result<String, StorageError> {
    serde_json::to_string(value).map_err(|e| StorageError {
        op,
        detail: format!("serialize failed: {e}"),
    })
}

/// Read a store under its current key, falling back to the name it wore before
/// the rename, then to the type's default when neither is present or parses.
///
/// The retired key is read and never written: the first save after a load is
/// what moves a reader onto the new name, and a downgrade still finds the data
/// it wrote.
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
            // Corrupt persisted state must not brick the app — but it must
            // not disappear either: the user just lost a saved value.
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

/// Load the library: the current blob when there is one, else the previous
/// schema's, migrated on the spot. Invalid values fall back to empty rather
/// than bricking the page, and every load is sanitised — a blob can arrive
/// with a shelf member naming no row, and the grid would render a hole.
///
/// Each migration leaves the key it read alone: a reader who downgrades
/// should still find the library the older build wrote, and the first save
/// after this load is what puts the new blob under its own key.
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
    let mut blob = migrate_v1(legacy, crate::time::now_ms());
    sanitize_library(&mut blob);
    blob
}

pub fn save_library(blob: &LibraryBlob) -> Result<(), StorageError> {
    set(LIBRARY_KEY, &encode("save_library", blob)?)
}

/// Load the cover-art map (path -> page-1 JPEG data URL).
pub fn load_covers() -> CoverMap {
    let stored: HashMap<String, CoverImage> =
        load_keyed("covers", COVERS_KEY, RETIRED_COVERS_KEY);
    stored
        .into_iter()
        .map(|(path, cover)| (path, Arc::new(cover)))
        .collect()
}

/// Save the cover-art map. Serialized through a map of BORROWED covers: the
/// images are the largest thing the app persists, and an owned `HashMap`
/// would copy every data URL for no reason.
pub fn save_covers(covers: &CoverMap) -> Result<(), StorageError> {
    let borrowed: HashMap<&str, &CoverImage> = covers
        .iter()
        .map(|(path, cover)| (path.as_str(), cover.as_ref()))
        .collect();
    set(COVERS_KEY, &encode("save_covers", &borrowed)?)
}

/// Write the library's current blob, reporting a failure instead of returning
/// it.
///
/// None of the callers can do anything with a `StorageError`: a shelf that
/// will not write is still a shelf the reader can use, and the next write
/// carries the same books again.
///
/// The read is untracked: storage takes a value and writes it, never
/// subscribes. Writes are immediate rather than debounced on purpose — a
/// debounced save ahead of a teardown or window close may never land — which
/// is why the reading-progress debounce keeps [`save_library`] instead: it
/// snapshots the value and hands it to a timer, because a timer firing during
/// teardown that reached into a disposed signal would panic where a dropped
/// save would not.
pub fn persist_library(library: LibraryState) {
    if crate::boot::in_format() {
        return;
    }
    if let Err(e) = save_library(&library.snapshot()) {
        e.report();
    }
}

/// [`persist_library`] for the cover cache, which is budgeted on its own key:
/// the cap in `crate::services::library::covers::COVER_CAP` is only a real quota if the
/// images are written back after a prune, not just dropped from memory.
pub fn persist_covers(library: LibraryState) {
    if crate::boot::in_format() {
        return;
    }
    if let Err(e) = library.covers.with_untracked(save_covers) {
        e.report();
    }
}

/// Carry the address-keyed highlights a previous build wrote onto the rows
/// that were reading them.
///
/// Runs once at load with the row list in hand: an `"<id>::<address>"` entry
/// is a private row's own list and is re-keyed onto that id, and a bare
/// address is the list every shared row at it read, which goes to the first
/// such row.
///
/// An entry no row answers for is left where it is rather than dropped: a
/// later load that finds the row again picks it up, and a removal that never
/// comes costs one localStorage entry rather than a reader's highlights.
pub fn migrate_gloss_keys(books: &[library_core::book::Row]) {
    if get(GLOSS_V2_MIGRATED_KEY).or_else(|| get(RETIRED_GLOSS_V2_MIGRATED_KEY)).is_some() {
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
            // A private row's own list: re-keyed onto the id in front of the
            // seam whether or not the address it wore is still the one it
            // reads.
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

/// Drop one row's marks: the reader's data goes with the book, not into
/// localStorage under a row nothing points at any more.
pub fn remove_gloss(row_id: &str) {
    take_gloss(row_id);
}

/// Take one row's marks out of the store. The row id is never reused, so the
/// entry goes whatever the answer was — and a removal that KEEPS the reader's
/// data carries the marks away with it (`crate::storage::kept::remember`)
/// rather than dropping them.
///
/// Read-modify-write rather than a cached map: marks change at human pace,
/// and re-reading keeps a second window's marks from being clobbered by a
/// write in this one.
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

/// One row's marks, copied onto another row: the duplicate's highlights are
/// its own list under its own id, each mark wearing a freshly minted id, so
/// nothing about the two lists is shared.
///
/// The source list stays where it is, because the original keeps its
/// highlights; a source with no list, or an empty one, costs nothing and
/// writes nothing. Read-modify-write for [`persist_gloss`]'s reason: a second
/// window's marks must not be clobbered by a write in this one.
pub fn copy_gloss(from_id: &str, to_id: &str) {
    let mut all = load_gloss();
    let Some(marks) = all.get(from_id) else {
        return;
    };
    if marks.is_empty() {
        return;
    }
    all.insert(to_id.to_string(), re_ided(marks, crate::time::now_ms()));
    if let Err(e) = save_gloss(&all) {
        e.report();
    }
}

/// The marks a duplicate wears: the same words at the same spots, under ids
/// minted for the copy rather than carried from the original. An id only keys
/// a list's own toggles and answer cache, so carrying the old ones would work
/// today — and be one future rule away from two books sharing a stroke, which
/// is the sharing this copy exists to end.
///
/// The stamp is the caller's (`copy_gloss` reads the clock once): every mark
/// in one list mints at the same millisecond, so the index is folded in to
/// keep two marks on one page of one list from colliding.
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
                rect: GlossBox { x: 10.0, y: 20.0, w: 30.0, h: 8.0, r: 0.0 },
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
    fn an_empty_list_never_reaches_storage() {
        // The guard the caller rides: copy_gloss with nothing to copy writes
        // nothing, which on wasm is the difference between a duplicate that
        // leaves the store alone and one that serializes the whole map for
        // nothing.
        assert!(re_ided(&[], 5).is_empty());
    }
}
