//! The vocabulary feature's frontend: the dataset mirror and batch lookup.

use std::cell::Cell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

/// The dataset's lifecycle, as the backend reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DatasetPhase {
    /// Nothing on disk and nothing running.
    Absent,
    Downloading,
    Paused,
    /// The parquet is being rebuilt into the local database.
    Converting,
    Ready,
    Failed,
}

/// The dataset's state, as the backend reports it. One per realm.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetMirror {
    pub phase: DatasetPhase,
    pub received: u64,
    pub total: Option<u64>,
    pub words: Option<u64>,
    pub message: Option<String>,
    /// The POS model's own stage: the levels arrive first, and only the
    /// click needs the tagger.
    #[serde(default)]
    pub model: Option<DatasetPhase>,
}

impl DatasetMirror {
    /// The fraction downloaded, when the size is known.
    pub fn percent(&self) -> Option<u32> {
        let total = self.total?;
        (total > 0).then_some(((self.received.min(total) as f64 / total as f64) * 100.0) as u32)
    }

    /// Whether the level lookups can answer.
    pub fn is_ready(&self) -> bool {
        self.phase == DatasetPhase::Ready
    }
}

thread_local! {
    /// The realm's mirror. The session that binds it clears it on the way
    /// out; the realm outlives every session.
    static MIRROR: Cell<Option<RwSignal<Option<DatasetMirror>>>> = const { Cell::new(None) };
    /// The progress tap is registered once per realm, however many
    /// documents it opens.
    static TAPPED: Cell<bool> = const { Cell::new(false) };
    /// One warn per realm: a dead backend must not flood.
    static LOOKUP_WARNED: Cell<bool> = const { Cell::new(false) };
}

/// Hand a status snapshot to the realm's mirror, if one is bound.
fn publish(status: DatasetMirror) {
    MIRROR.with(|slot| {
        if let Some(mirror) = slot.get() {
            mirror.try_set(Some(status));
        }
    });
}

/// The dataset mirror to read; every ask in a realm agrees on one handle.
pub fn dataset() -> RwSignal<Option<DatasetMirror>> {
    MIRROR.with(|slot| match slot.get() {
        Some(mirror) => mirror,
        None => {
            let mirror = RwSignal::new(None);
            slot.set(Some(mirror));
            mirror
        }
    })
}

/// Bind this realm's mirror and tap the backend's progress. The ask
/// repeats per bind, so a document opened mid-download starts informed.
pub fn install_cefr_bridge() {
    if !tauri_bridge::has_tauri() {
        return;
    }
    let mirror = RwSignal::new(None);
    MIRROR.with(|slot| slot.set(Some(mirror)));
    on_cleanup(|| MIRROR.with(|slot| slot.set(None)));
    spawn_local(async move {
        if let Ok(value) = tauri_bridge::invoke("cefr_dataset_status", JsValue::UNDEFINED).await
            && let Ok(status) = serde_wasm_bindgen::from_value::<DatasetMirror>(value)
        {
            publish(status);
        }
    });
    if TAPPED.with(|tapped| tapped.replace(true)) {
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

/// Ask the backend for the POS model alone: the levels are already here,
/// and only a click needs the tagger.
pub fn request_model_download() {
    spawn_local(async move {
        if let Err(e) = tauri_bridge::invoke("cefr_model_download", JsValue::UNDEFINED).await {
            web_sys::console::warn_1(&format!("[cefr] model download failed: {e:?}").into());
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
            Err(e) => LOOKUP_WARNED.with(|warned| {
                if !warned.get() {
                    warned.set(true);
                    web_sys::console::warn_1(&format!("[cefr] lookup failed: {e:?}").into());
                }
            }),
        }
    });
}

/// The lookup command's wire shape: a named `words` field.
#[derive(Serialize)]
struct LevelsArgs {
    words: Vec<String>,
}

/// The backend's dataset POS answer for one clicked word.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PosAnswer {
    /// The readable word class: noun, verb, adjective, ...
    pub kind: String,
    /// The Penn tag behind it, when no class is named.
    pub pos: String,
}

impl PosAnswer {
    /// What a card prints for this word's role.
    pub fn label(&self) -> String {
        if self.kind.is_empty() {
            self.pos.clone()
        } else {
            self.kind.clone()
        }
    }
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
