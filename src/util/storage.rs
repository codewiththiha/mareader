//! Persisted app state (settings, library, covers) over a [`PdfStorage`]
//! backend. The backend is chosen once at startup (`init_storage`); the
//! load/save helpers below are backend-agnostic.

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::core::library::{sanitize as sanitize_library, CoverImage, RecentBook};
use pdf_core::settings::{sanitize, Settings, SETTINGS_KEY};
use pdf_storage::PdfStorage;

pub const LIBRARY_KEY: &str = "pdfreader.library.v1";
pub const COVERS_KEY: &str = "pdfreader.covers.v1";

static STORAGE: OnceLock<Box<dyn PdfStorage>> = OnceLock::new();

/// Install the storage backend. Must be called once, before any load/save.
/// Swapping localStorage for SQLite is a one-line change here.
pub fn init_storage(storage: Box<dyn PdfStorage>) {
    _ = STORAGE.set(storage);
}

fn storage() -> &'static (dyn PdfStorage + 'static) {
    // `&**` instead of `&` to avoid the `borrowed_box` clippy lint: we want
    // `&dyn PdfStorage`, not `&Box<dyn PdfStorage>`.
    &**STORAGE.get_or_init(|| Box::new(pdf_storage::LocalStorage))
}

/// Load persisted settings; invalid values fall back to defaults + sanitize.
pub fn load_settings() -> Settings {
    let mut settings = storage()
        .get(SETTINGS_KEY)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    sanitize(&mut settings);
    settings
}

pub fn save_settings(settings: &Settings) {
    if let Ok(json) = serde_json::to_string(settings) {
        storage().set(SETTINGS_KEY, &json);
    }
}

/// Load the recent-books list; invalid values fall back to empty + sanitize.
pub fn load_library() -> Vec<RecentBook> {
    let mut recent = storage()
        .get(LIBRARY_KEY)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    sanitize_library(&mut recent);
    recent
}

pub fn save_library(recent: &[RecentBook]) {
    if let Ok(json) = serde_json::to_string(recent) {
        storage().set(LIBRARY_KEY, &json);
    }
}

/// Load the cover-art map (path -> page-1 JPEG data URL).
pub fn load_covers() -> HashMap<String, CoverImage> {
    storage()
        .get(COVERS_KEY)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_covers(covers: &HashMap<String, CoverImage>) {
    if let Ok(json) = serde_json::to_string(covers) {
        storage().set(COVERS_KEY, &json);
    }
}
