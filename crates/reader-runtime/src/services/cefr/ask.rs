//! The vocabulary backend's asks: the dataset's lifecycle and
//! the words' levels.

use std::cell::OnceCell;

use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

thread_local! {
    /// One warn per realm: a dead backend must not flood the console.
    static LOOKUP_WARNED: OnceCell<()> = const { OnceCell::new() };
}

/// Ask the backend to start the resumable download.
pub fn request_download() {
    spawn_local(async move {
        if let Err(e) = tauri_bridge::invoke("cefr_dataset_download", JsValue::UNDEFINED).await {
            web_sys::console::warn_1(&format!("[cefr] download kickoff failed: {e:?}").into());
        }
    });
}

/// Ask a running download to stop; the partial stays for a resume.
pub fn request_cancel() {
    spawn_local(async move {
        let _ = tauri_bridge::invoke("cefr_dataset_cancel", JsValue::UNDEFINED).await;
    });
}

/// Stop reading the stream; the partial stays for a resume.
pub fn request_pause() {
    spawn_local(async move {
        let _ = tauri_bridge::invoke("cefr_dataset_pause", JsValue::UNDEFINED).await;
    });
}

/// Continue a paused download from the bytes already on disk.
pub fn request_resume() {
    spawn_local(async move {
        if let Err(e) = tauri_bridge::invoke("cefr_dataset_resume", JsValue::UNDEFINED).await {
            web_sys::console::warn_1(&format!("[cefr] resume failed: {e:?}").into());
        }
    });
}

/// Delete the dataset and its partials.
pub fn request_remove() {
    spawn_local(async move {
        if let Err(e) = tauri_bridge::invoke("cefr_dataset_remove", JsValue::UNDEFINED).await {
            web_sys::console::warn_1(&format!("[cefr] remove failed: {e:?}").into());
        }
    });
}

/// The lookup command's wire shape: a named `words` field.
#[derive(Serialize)]
struct LevelsArgs {
    words: Vec<String>,
}

/// Levels for `keys`, aligned with the input.
///
/// `done` always runs; a failure releases its keys.
pub fn fetch_levels(keys: Vec<String>, done: impl FnOnce(Vec<Option<f64>>) + 'static) {
    if keys.is_empty() || !tauri_bridge::has_tauri() {
        done(Vec::new());
        return;
    }
    // Tauri binds command parameters by name: the array rides NAMED.
    let args = match serde_wasm_bindgen::to_value(&LevelsArgs { words: keys }) {
        Ok(args) => args,
        Err(_) => {
            done(Vec::new());
            return;
        }
    };
    spawn_local(async move {
        match tauri_bridge::invoke("cefr_levels", args).await {
            Ok(value) => match serde_wasm_bindgen::from_value::<Vec<Option<f64>>>(value) {
                Ok(levels) => done(levels),
                Err(_) => done(Vec::new()),
            },
            Err(e) => {
                if LOOKUP_WARNED.with(|slot| slot.set(())).is_ok() {
                    web_sys::console::warn_1(&format!("[cefr] lookup failed: {e:?}").into());
                }
                done(Vec::new());
            }
        }
    });
}

/// The backend's dataset POS answer for one clicked word.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PosAnswer {
    pub pos: String,
    pub kind: String,
    pub sense: Option<String>,
    pub level: Option<f64>,
    pub senses: Vec<String>,
}

/// The dataset's verdict for `word` in `sentence`; `None` if unequipped.
pub fn fetch_pos(word: String, context: String, done: impl FnOnce(Option<PosAnswer>) + 'static) {
    #[derive(Serialize)]
    struct PosArgs {
        word: String,
        context: String,
    }
    if !tauri_bridge::has_tauri() {
        done(None);
        return;
    }
    spawn_local(async move {
        let args =
            serde_wasm_bindgen::to_value(&PosArgs { word, context }).unwrap_or(JsValue::UNDEFINED);
        let parsed = match tauri_bridge::invoke("cefr_pos_of", args).await {
            Ok(value) => serde_wasm_bindgen::from_value::<Option<PosAnswer>>(value)
                .ok()
                .flatten(),
            Err(_) => None,
        };
        done(parsed);
    });
}
