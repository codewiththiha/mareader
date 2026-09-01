//! Persisted app state (settings, library, covers) over localStorage.
//!
//! Deliberately plain functions — a trait + `Box<dyn>` + `OnceLock` global
//! for a single localStorage backend was more architecture than the app
//! needs. If a second backend ever lands it can come back as a trait; until
//! then the boundary is simply "the app loads and saves here".
//!
//! Failures are NOT silent: loads warn about what was dropped, saves return
//! a [`StorageError`] the caller decides how to handle.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use wasm_bindgen::JsValue;

use crate::state::library::{sanitize as sanitize_library, CoverImage, CoverMap, RecentBook};
use pdf_core::gloss::GlossMark;
use pdf_core::settings::{sanitize, Settings, SETTINGS_KEY};

const LIBRARY_KEY: &str = "pdfreader.library.v1";
const COVERS_KEY: &str = "pdfreader.covers.v1";
/// Gloss highlights, keyed by document path.
///
/// Versioned like the rest: the rects are page-space CSS px, so they are
/// stable across zoom and sessions but NOT across a change in how a page is
/// laid out. If page rendering metrics ever change, bump this to `v2` rather
/// than letting old marks drift onto the wrong words.
const GLOSS_KEY: &str = "pdfreader.gloss.v1";

/// A persistence failure (quota exceeded, storage blocked, serialization
/// error). The UI should never crash on these — but they must not vanish.
///
/// Handling rule (one consistent decision, no per-call judgment): every
/// save failure is reported through [`StorageError::report`] at the call
/// site. Covers could arguably be dropped silently (they regenerate), but
/// a single rule beats a case-by-case call.
#[derive(Debug)]
pub struct StorageError {
    op: &'static str,
    detail: String,
}

impl StorageError {
    /// Surface the failure on the console without interrupting the UI.
    pub fn report(&self) {
        web_sys::console::warn_1(&JsValue::from_str(&format!("[storage] {self}")));
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.op, self.detail)
    }
}

fn warn(op: &'static str, detail: &str) {
    web_sys::console::warn_1(&JsValue::from_str(&format!("[storage] {op}: {detail}")));
}

fn local() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
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

/// Load the recent-books list; invalid values fall back to empty + sanitize.
pub fn load_library() -> Vec<RecentBook> {
    let mut recent = get(LIBRARY_KEY)
        .map(|raw| parse("library", &raw))
        .unwrap_or_default();
    sanitize_library(&mut recent);
    recent
}

pub fn save_library(recent: &[RecentBook]) -> Result<(), StorageError> {
    let json = serde_json::to_string(recent).map_err(|e| StorageError {
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

/// Save the cover-art map.
///
/// Serialized through a map of BORROWED covers: the images are the largest
/// thing the app persists, and going through an owned `HashMap` to hand serde
/// something it recognises would copy every data URL for no reason.
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

/// Load every document's gloss highlights (path -> marks).
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

/// Replace one document's marks and write the whole map back.
///
/// Read-modify-write rather than keeping the map in memory: marks change only
/// when the reader explains a word (a human-paced action), and re-reading
/// keeps a second window's marks from being clobbered.
pub fn persist_gloss(path: &str, marks: &[GlossMark]) {
    let mut all = load_gloss();
    all.insert(path.to_string(), marks.to_vec());
    if let Err(e) = save_gloss(&all) {
        e.report();
    }
}
