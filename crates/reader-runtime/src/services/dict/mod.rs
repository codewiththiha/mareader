//! The dictionary's frontend: its realm state, [`wire`], [`route`].

pub mod route;
pub mod wire;

use std::cell::OnceCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

pub use route::{
    Asked, Pair, Route, answer_pack, built_packs, languages, named_packs, resolve,
};
pub use wire::{EntryMirror, PackMirror, ProgressMirror, lookup, request_pack, search};

/// The panel's ask. One for the window, so no pane owns it.
#[derive(Debug, Clone, Copy)]
pub struct Panel {
    /// The word the panel is holding.
    pub query: RwSignal<String>,
    /// The rows the packs answered with.
    pub results: RwSignal<Vec<EntryMirror>>,
    /// Whether an ask has been made since the box was emptied.
    pub asked: RwSignal<bool>,
}

thread_local! {
    /// This realm's pack rows, bound before any component reads.
    static PACKS: OnceCell<RwSignal<Vec<PackMirror>>> = const { OnceCell::new() };
    /// The panel's ask, minted beside the rows and kept as long.
    static PANEL: OnceCell<Panel> = const { OnceCell::new() };
    /// The backend tap is installed once per realm.
    static TAPPED: OnceCell<()> = const { OnceCell::new() };
}

/// The pack rows to read this realm.
pub fn packs() -> RwSignal<Vec<PackMirror>> {
    PACKS.with(|slot| *slot.get_or_init(|| RwSignal::new(Vec::new())))
}

/// The panel's ask, read by every pane's rail alike.
pub fn panel() -> Panel {
    PANEL.with(|slot| {
        *slot.get_or_init(|| Panel {
            query: RwSignal::new(String::new()),
            results: RwSignal::new(Vec::new()),
            asked: RwSignal::new(false),
        })
    })
}

/// Land one report of the rows: the answers' shelf watches it.
fn land(rows: Vec<PackMirror>) {
    wire::note_built(&rows);
    packs().set(rows);
}

/// Bind this realm's rows and tap the backend's progress.
pub fn install_dict_bridge() {
    // Realm-global: mint it in this owner, not the first transient reader's.
    let _ = packs();
    // And the panel's ask with it, so one owner holds both.
    let _ = panel();
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        if let Ok(value) = tauri_bridge::invoke("dict_packs_status", JsValue::UNDEFINED).await
            && let Ok(rows) = serde_wasm_bindgen::from_value::<Vec<PackMirror>>(value)
        {
            land(rows);
        }
    });
    if TAPPED.with(|slot| slot.set(())).is_err() {
        return;
    }
    crate::services::tauri_listen("dict-packs", |ev: web_sys::Event| {
        let value: &JsValue = ev.as_ref();
        let payload = js_sys::Reflect::get(value, &"payload".into()).unwrap_or(JsValue::UNDEFINED);
        match serde_wasm_bindgen::from_value::<Vec<PackMirror>>(payload) {
            Ok(rows) => land(rows),
            Err(e) => web_sys::console::warn_1(&format!("[dict] bad payload: {e:?}").into()),
        }
    });
}
