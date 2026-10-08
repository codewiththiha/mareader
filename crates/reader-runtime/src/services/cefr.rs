//! The vocabulary feature's frontend: the dataset mirror and batch lookup.

use std::sync::{Mutex, OnceLock};

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

/// The dataset's lifecycle, as the backend reports it. One per realm.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetMirror {
    /// `absent` | `downloading` | `converting` | `ready` | `failed`.
    pub phase: String,
    pub received: u64,
    pub total: Option<u64>,
    pub words: Option<u64>,
    pub message: Option<String>,
}

impl DatasetMirror {
    /// The fraction downloaded, when the size is known.
    pub fn percent(&self) -> Option<u32> {
        let total = self.total?;
        (total > 0).then_some(((self.received.min(total) as f64 / total as f64) * 100.0) as u32)
    }

    pub fn is_ready(&self) -> bool {
        self.phase == "ready"
    }
}

/// The current session's mirror; a session's end takes its own signal down.
static MIRROR: Mutex<Option<RwSignal<Option<DatasetMirror>>>> = Mutex::new(None);

/// The backend tap, installed once per process; it writes whoever is bound.
static TAP: OnceLock<()> = OnceLock::new();

/// Hand a status snapshot to the live session's mirror, if any survives.
fn publish(status: DatasetMirror) {
    if let Some(mirror) = MIRROR.lock().ok().and_then(|slot| *slot) {
        mirror.try_set(Some(status));
    }
}

/// The dataset mirror to read this session; `None` phases in from the bridge.
pub fn dataset() -> RwSignal<Option<DatasetMirror>> {
    MIRROR
        .lock()
        .ok()
        .and_then(|slot| *slot)
        .unwrap_or_else(|| RwSignal::new(None))
}

/// Bind this realm's mirror and tap the backend's progress; the ask
/// repeats per bind.
pub fn install_cefr_bridge() {
    if !tauri_bridge::has_tauri() {
        return;
    }
    let mirror = RwSignal::new(None);
    if let Ok(mut slot) = MIRROR.lock() {
        *slot = Some(mirror);
    }
    spawn_local(async move {
        if let Ok(value) = tauri_bridge::invoke("cefr_dataset_status", JsValue::UNDEFINED).await
            && let Ok(status) = serde_wasm_bindgen::from_value::<DatasetMirror>(value)
        {
            publish(status);
        }
    });
    if TAP.set(()).is_err() {
        return;
    }
    crate::services::tauri_listen("cefr-dataset-progress", |ev: web_sys::Event| {
        let value: &JsValue = ev.as_ref();
        let payload = js_sys::Reflect::get(value, &"payload".into()).unwrap_or(JsValue::UNDEFINED);
        match serde_wasm_bindgen::from_value::<DatasetMirror>(payload) {
            Ok(status) => publish(status),
            Err(e) => web_sys::console::warn_1(&format!("[cefr] bad payload: {e:?}").into()),
        }
    });
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

/// Levels for `words`, aligned with the input; a web build never calls
/// back.
pub fn fetch_levels(words: Vec<String>, done: impl FnOnce(Vec<Option<f64>>) + 'static) {
    if words.is_empty() {
        return;
    }
    // Tauri binds command parameters by name: the array rides NAMED.
    let args = match serde_wasm_bindgen::to_value(&LevelsArgs { words }) {
        Ok(args) => args,
        Err(_) => return,
    };
    spawn_local(async move {
        match tauri_bridge::invoke("cefr_levels", args).await {
            Ok(value) => {
                if let Ok(levels) = serde_wasm_bindgen::from_value::<Vec<Option<f64>>>(value) {
                    done(levels);
                }
            }
            Err(e) => {
                // One warn per realm: a dead backend must not flood.
                LOOKUP_WARNED.with(|warned| {
                    if !warned.get() {
                        warned.set(true);
                        web_sys::console::warn_1(&format!("[cefr] lookup failed: {e:?}").into());
                    }
                });
            }
        }
    });
}

thread_local! {
    static LOOKUP_WARNED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The lookup command's wire shape: a named `words` field.
#[derive(Serialize)]
struct LevelsArgs {
    words: Vec<String>,
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
    spawn_local(async move {
        if !tauri_bridge::has_tauri() {
            return;
        }
        let args = serde_wasm_bindgen::to_value(&PosArgs { word, context })
            .unwrap_or(JsValue::UNDEFINED);
        let parsed = match tauri_bridge::invoke("cefr_pos_of", args).await {
            Ok(value) => serde_wasm_bindgen::from_value::<Option<PosAnswer>>(value)
                .ok()
                .flatten(),
            Err(_) => None,
        };
        done(parsed);
    });
}
