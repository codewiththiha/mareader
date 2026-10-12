//! The vocabulary feature's frontend: this realm's dataset mirror,
//! tapped from the backend.

use std::cell::OnceCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

mod ask;
mod dataset;

pub use ask::{
    PosAnswer, fetch_levels, fetch_pos, request_cancel, request_download, request_pause,
    request_remove, request_resume,
};
pub use dataset::DatasetMirror;

thread_local! {
    /// This realm's one mirror handle, bound before any component reads.
    static MIRROR: OnceCell<RwSignal<Option<DatasetMirror>>> = const { OnceCell::new() };
    /// The backend tap is installed once per realm.
    static TAPPED: OnceCell<()> = const { OnceCell::new() };
}

/// The dataset mirror to read this realm; `None` phases in from the bridge.
pub fn dataset() -> RwSignal<Option<DatasetMirror>> {
    MIRROR.with(|slot| *slot.get_or_init(|| RwSignal::new(None)))
}

/// Bind this realm's mirror and tap the backend's progress.
pub fn install_cefr_bridge() {
    // Realm-global: mint it in this owner, not the first transient reader's.
    let mirror = dataset();
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        if let Ok(value) = tauri_bridge::invoke("cefr_dataset_status", JsValue::UNDEFINED).await
            && let Ok(status) = serde_wasm_bindgen::from_value::<DatasetMirror>(value)
        {
            mirror.try_set(Some(status));
        }
    });
    if TAPPED.with(|slot| slot.set(())).is_err() {
        return;
    }
    crate::services::note_bridge_listener();
    crate::services::tauri_listen("cefr-dataset-progress", |ev: web_sys::Event| {
        let value: &JsValue = ev.as_ref();
        let payload = js_sys::Reflect::get(value, &"payload".into()).unwrap_or(JsValue::UNDEFINED);
        match serde_wasm_bindgen::from_value::<DatasetMirror>(payload) {
            Ok(status) => publish(status),
            Err(e) => web_sys::console::warn_1(&format!("[cefr] bad payload: {e:?}").into()),
        }
    });
}

/// Hand a status snapshot to this realm's mirror.
fn publish(status: DatasetMirror) {
    dataset().try_set(Some(status));
}
