//! localStorage-backed persistence for `Settings` (via the engine's wrapper so it
//! works identically in a plain browser and inside Tauri).

use crate::core::bridge;
use crate::core::settings::{sanitize, Settings, SETTINGS_KEY};

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
