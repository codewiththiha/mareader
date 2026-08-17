//! localStorage-backed persistence for `Settings` and the library (via the
//! engine's wrapper so it works identically in a plain browser and in Tauri).

use std::collections::HashMap;

use crate::core::bridge;
use crate::core::library::{sanitize as sanitize_library, CoverImage, RecentBook};
use crate::core::settings::{sanitize, Settings, SETTINGS_KEY};

pub const LIBRARY_KEY: &str = "pdfreader.library.v1";
pub const COVERS_KEY: &str = "pdfreader.covers.v1";

fn get(key: &str) -> Option<String> {
    let v = bridge::storage_get(key);
    v.as_string()
}

fn set(key: &str, value: &str) {
    bridge::storage_set(key, value);
}

/// Load persisted settings; invalid values fall back to defaults + sanitize.
pub fn load_settings() -> Settings {
    let mut settings = get(SETTINGS_KEY)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    sanitize(&mut settings);
    settings
}

pub fn save_settings(settings: &Settings) {
    if let Ok(json) = serde_json::to_string(settings) {
        set(SETTINGS_KEY, &json);
    }
}

/// Load the recent-books list; invalid values fall back to empty + sanitize.
pub fn load_library() -> Vec<RecentBook> {
    let mut recent = get(LIBRARY_KEY)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    sanitize_library(&mut recent);
    recent
}

pub fn save_library(recent: &[RecentBook]) {
    if let Ok(json) = serde_json::to_string(recent) {
        set(LIBRARY_KEY, &json);
    }
}

/// Load the cover-art map (path -> page-1 JPEG data URL).
pub fn load_covers() -> HashMap<String, CoverImage> {
    get(COVERS_KEY)
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_covers(covers: &HashMap<String, CoverImage>) {
    if let Ok(json) = serde_json::to_string(covers) {
        set(COVERS_KEY, &json);
    }
}
