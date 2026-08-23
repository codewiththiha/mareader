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

use wasm_bindgen::JsValue;

use crate::state::library::{sanitize as sanitize_library, CoverImage, RecentBook};
use pdf_core::settings::{sanitize, Settings, SETTINGS_KEY};

pub const LIBRARY_KEY: &str = "pdfreader.library.v1";
pub const COVERS_KEY: &str = "pdfreader.covers.v1";

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
pub fn load_covers() -> HashMap<String, CoverImage> {
    get(COVERS_KEY)
        .map(|raw| parse("covers", &raw))
        .unwrap_or_default()
}

pub fn save_covers(covers: &HashMap<String, CoverImage>) -> Result<(), StorageError> {
    let json = serde_json::to_string(covers).map_err(|e| StorageError {
        op: "save_covers",
        detail: format!("serialize failed: {e}"),
    })?;
    set(COVERS_KEY, &json)
}
