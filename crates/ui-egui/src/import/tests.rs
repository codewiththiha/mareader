//! The importer's fixture: a store faked, a directory borrowed, both read back.

use std::sync::Arc;

use mareader_engine::host::{Fake, Machine, Shared};
use mareader_engine::save::LastDocument;

use super::*;
use crate::MareaderApp;

/// The app a launch builds on a host with no machine, as a browser's.
fn built(loaded: Loaded) -> MareaderApp {
    let host: Shared = Arc::new(Fake::default());
    MareaderApp::from_loaded(loaded, &egui::Context::default(), host, Machine::default())
}

/// The store a browser keeps, as a map of two keys.
#[derive(Default)]
struct Memory(std::collections::HashMap<String, String>);

impl Memory {
    fn with(key: &str, value: &str) -> Self {
        let mut disk = Self::default();
        disk.put(key, value);
        disk
    }

    fn put(&mut self, key: &str, value: &str) {
        self.0.insert(key.to_owned(), value.to_owned());
    }
}

impl eframe::Storage for Memory {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }

    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }

    fn flush(&mut self) {}
}

/// A directory that goes when the test does.
fn dir() -> Result<tempfile::TempDir, String> {
    tempfile::tempdir().map_err(|e| e.to_string())
}

fn written(value: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}

#[test]
fn the_pre_crate_settings_blob_still_loads() {
    let v0 = r#"{"titlebar_pinned":true,"animations":false,"sidebar_overlay":true}"#;
    let disk = Memory::with(SETTINGS_KEY, v0);
    let app = built(once(Some(&disk), None));
    assert!(app.pinned() && app.settings().chrome.sidebar_overlay);
    assert!(!app.settings().motion.enabled);
    assert!(app.settings().chrome.glass, "glass is new, so it starts on");
}

#[test]
fn repaired_settings_say_so() {
    let bad = r#"{"version":1,"motion":{"zoom":"often"}}"#;
    let disk = Memory::with(SETTINGS_KEY, bad);
    let app = built(once(Some(&disk), None));
    let tone = app.toast().map(|t| t.tone);
    assert_eq!(tone, Some(mareader_engine::toast::Tone::Error));
}

/// A launch with no directory and no page store reads the store an older build
/// left as it stands, at every launch, and its keys are still in it after.
#[test]
fn a_launch_with_nowhere_to_read_keeps_reading_the_store() {
    let mut settings = Settings::default();
    settings.text.font_size = 21.0;
    let json = mareader_settings::to_json(&settings);
    let disk = Memory::with(SETTINGS_KEY, &json);
    for launch in 0..2 {
        let loaded = once(Some(&disk), None);
        assert_eq!(loaded.settings.text.font_size, 21.0, "launch {launch}");
        assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    }
    assert_eq!(
        disk.get_string(SETTINGS_KEY).as_deref(),
        Some(json.as_str())
    );
}

#[test]
fn the_two_keys_move_into_the_engine_files_once() -> Result<(), String> {
    let dir = dir()?;
    let mut settings = Settings::default();
    settings.text.font_size = 21.0;
    let path = PathBuf::from("/a-sample.pdf");
    let last = LastDocument {
        path,
        page: 12,
        place: Some((3, 4)),
        spine: None,
        pages: 30,
        book: None,
    };
    let json = written(&serde_json::json!(last))?;
    let mut disk = Memory::with(SETTINGS_KEY, &mareader_settings::to_json(&settings));
    disk.put(LAST_DOCUMENT_KEY, &json);
    let moved = once(Some(&disk), Some(dir.path().to_path_buf()));
    assert_eq!(moved.settings.text.font_size, 21.0);
    assert_eq!(moved.last.as_ref(), Some(&last));
    assert!(dir.path().join(save::SETTINGS_FILE).exists(), "written");
    assert!(dir.path().join(save::SESSION_FILE).exists(), "written");
    // The store is not read again: an empty one finds what the files hold.
    let empty = Memory::default();
    let again = once(Some(&empty), Some(dir.path().to_path_buf()));
    assert_eq!(again.settings.text.font_size, 21.0);
    assert_eq!(again.last.as_ref(), Some(&last));
    assert!(again.notes.is_empty(), "{:?}", again.notes);
    Ok(())
}

#[test]
fn a_store_that_will_not_move_leaves_what_it_held() -> Result<(), String> {
    let dir = dir()?;
    let blocked = dir.path().join("a-file");
    std::fs::write(&blocked, "not a directory").map_err(|e| e.to_string())?;
    let mut settings = Settings::default();
    settings.text.font_size = 21.0;
    let disk = Memory::with(SETTINGS_KEY, &mareader_settings::to_json(&settings));
    let into = blocked.join("state");
    let loaded = once(Some(&disk), Some(into));
    assert_eq!(loaded.settings.text.font_size, 21.0, "the store is kept");
    assert!(
        loaded.notes.iter().any(|n| n.contains("did not move")),
        "{:?}",
        loaded.notes
    );
    Ok(())
}
