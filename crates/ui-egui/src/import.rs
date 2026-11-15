//! The one read of the store an older build wrote, and where a platform with
//! no files keeps the engine's two.
//!
//! A launch with a directory reads the engine's own two files, and the first
//! one after an older build takes the two keys that build left in
//! `eframe::Storage` and moves them there. This is the only module in the crate
//! that names `Storage` (definition of done 7), and nothing writes it: a
//! browser keeps the engine's two files in its page store, by their own names
//! and in the bytes `Session::blobs` gives, so what an older build left is read
//! once and left alone.

use std::path::{Path, PathBuf};

use eframe::Storage;
use mareader_engine::save::{self, Loaded};
use mareader_settings::Settings;

/// The key an older build's choices were written under.
pub const SETTINGS_KEY: &str = "settings";
/// Kept apart from settings: it says what was read, not how to read.
pub const LAST_DOCUMENT_KEY: &str = "last_document";

/// The store a browser keeps, named here so nothing else has to name it.
pub type Store<'a> = dyn Storage + 'a;

/// What a launch starts with: the engine's two files, or an older store.
pub fn once(storage: Option<&Store<'_>>, dir: Option<PathBuf>) -> Loaded {
    match dir {
        None => kept_or(storage),
        Some(dir) => files(storage, &dir),
    }
}

/// Whether the engine's two are kept beside a store, as a browser keeps them.
pub fn keeps() -> bool {
    cfg!(target_arch = "wasm32")
}

/// The store an older build left, read as it stands: the launch that imports.
pub fn browser(storage: Option<&Store<'_>>) -> Loaded {
    let mut notes = Vec::new();
    let settings = match storage.and_then(|s| s.get_string(SETTINGS_KEY)) {
        None => Settings::default(),
        Some(json) => {
            let loaded = mareader_settings::load(&json);
            notes.extend(loaded.notes);
            loaded.settings
        }
    };
    let held = storage.and_then(|s| s.get_string(LAST_DOCUMENT_KEY));
    let last = match held {
        None => None,
        Some(json) => match serde_json::from_str(&json) {
            Ok(last) => last,
            Err(why) => {
                notes.push(format!("{LAST_DOCUMENT_KEY} could not be read: {why}"));
                None
            }
        },
    };
    Loaded {
        settings,
        last,
        notes,
    }
}

/// The engine's files, with an older build's keys moved in when they are new.
fn files(storage: Option<&Store<'_>>, dir: &Path) -> Loaded {
    let fresh = !dir.join(save::SETTINGS_FILE).exists() && !dir.join(save::SESSION_FILE).exists();
    let held = storage.is_some_and(|s| {
        s.get_string(SETTINGS_KEY).is_some() || s.get_string(LAST_DOCUMENT_KEY).is_some()
    });
    if !fresh || !held {
        return save::load(Some(dir));
    }
    let moved = browser(storage);
    match save::store(dir, &moved.settings, moved.last.as_ref()) {
        Ok(()) => {
            let mut loaded = save::load(Some(dir));
            // A repair made on the way in is said once, at that launch.
            loaded.notes.extend(moved.notes);
            loaded
        }
        Err(why) => {
            let mut kept = moved;
            let note = format!("the stored settings did not move: {why}");
            kept.notes.push(note);
            kept
        }
    }
}

/// The page's own store, where a platform with no files keeps the two.
#[cfg(target_arch = "wasm32")]
fn page_store() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// The key one of the engine's two files is kept under.
#[cfg(target_arch = "wasm32")]
fn key(name: &str) -> String {
    format!("mareader/{name}")
}

/// One of the two files' text, as the page keeps it.
#[cfg(target_arch = "wasm32")]
fn read_kept(name: &str) -> Option<String> {
    page_store()?.get_item(&key(name)).ok()?
}

/// The two files as a page keeps them; none at the launch that imports.
#[cfg(target_arch = "wasm32")]
fn held() -> Option<Loaded> {
    let settings = read_kept(save::SETTINGS_FILE);
    let session = read_kept(save::SESSION_FILE);
    if settings.is_none() && session.is_none() {
        return None;
    }
    Some(save::parse(settings, session))
}

/// A page reads the two the engine keeps there, and only a launch that has
/// neither falls back to the store an older build left.
#[cfg(target_arch = "wasm32")]
fn kept_or(storage: Option<&Store<'_>>) -> Loaded {
    held().unwrap_or_else(|| browser(storage))
}

/// Anywhere with files, a launch with no directory has nowhere to read.
#[cfg(not(target_arch = "wasm32"))]
fn kept_or(storage: Option<&Store<'_>>) -> Loaded {
    browser(storage)
}

/// The engine's two files, kept where a platform with no files can keep them.
#[cfg(target_arch = "wasm32")]
pub fn keep(app: &crate::MareaderApp) {
    let Ok(blobs) = app.session().blobs() else {
        return;
    };
    let Some(store) = page_store() else {
        return;
    };
    for (name, text) in blobs {
        // A temp key first, so a write stopped halfway leaves the file before.
        let tmp = format!("{}.tmp", key(name));
        if store.set_item(&tmp, &text).is_err() {
            continue;
        }
        if store.set_item(&key(name), &text).is_err() {
            continue;
        }
        // A temp key the store kept would harm nothing, so a refusal stands.
        let _ = store.remove_item(&tmp);
    }
}

/// Anywhere with a state directory, the engine's two files are the whole of
/// what persists, and eframe's store is left alone after its one read.
#[cfg(not(target_arch = "wasm32"))]
pub fn keep(_app: &crate::MareaderApp) {}

#[cfg(test)]
mod tests;
