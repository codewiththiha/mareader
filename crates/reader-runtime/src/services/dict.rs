//! The dictionary's frontend: pack rows and ranked lookups.

use std::cell::OnceCell;

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

/// One download's middle, as download-core draws it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProgressMirror {
    pub phase: String,
    pub received: u64,
    pub total: Option<u64>,
    pub speed: Option<f64>,
    pub eta_secs: Option<u64>,
    pub message: Option<String>,
}

/// A pack's row: what it is and where it stands.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PackMirror {
    pub id: String,
    pub label: String,
    pub source: String,
    pub target: String,
    pub rows: u64,
    pub built: bool,
    pub progress: Option<ProgressMirror>,
}

impl PackMirror {
    /// `absent` | `downloading` | `paused` | `ready` | `failed`.
    pub fn phase(&self) -> &'static str {
        if self.built {
            return "ready";
        }
        match self.progress.as_ref().map(|p| p.phase.as_str()) {
            Some("downloading") | Some("preparing") | Some("retrying")
            | Some("verifying") => "downloading",
            Some("paused") => "paused",
            Some("failed") => "failed",
            _ => "absent",
        }
    }

    /// The fraction downloaded, when the size is known.
    pub fn percent(&self) -> Option<u32> {
        let progress = self.progress.as_ref()?;
        let total = progress.total?;
        (total > 0)
            .then_some(((progress.received.min(total) as f64 / total as f64) * 100.0) as u32)
    }
}

/// One ranked entry on the wire to a card or a search list.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EntryMirror {
    pub word: String,
    pub pos_raw: Option<String>,
    /// Canon tags as snake_case words: `verb`, `noun`, ...
    pub tags: Vec<String>,
    pub definition: String,
    pub romanization: Option<String>,
    pub sense: Option<String>,
    pub pack: String,
    /// The bridge word, when two packs carried the reader here.
    pub via: Option<String>,
    /// `exact` | `prefix` | `substring` | `fuzzy`.
    pub word_match: String,
}

thread_local! {
    /// This realm's pack rows, bound before any component reads.
    static PACKS: OnceCell<RwSignal<Vec<PackMirror>>> = const { OnceCell::new() };
    /// The backend tap is installed once per realm.
    static TAPPED: OnceCell<()> = const { OnceCell::new() };
}

/// The pack rows to read this realm.
pub fn packs() -> RwSignal<Vec<PackMirror>> {
    PACKS.with(|slot| *slot.get_or_init(|| RwSignal::new(Vec::new())))
}

/// Bind this realm's rows and tap the backend's progress.
pub fn install_dict_bridge() {
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        if let Ok(value) = tauri_bridge::invoke("dict_packs_status", JsValue::UNDEFINED).await
            && let Ok(rows) = serde_wasm_bindgen::from_value::<Vec<PackMirror>>(value)
        {
            packs().set(rows);
        }
    });
    if TAPPED.with(|slot| slot.set(())).is_err() {
        return;
    }
    crate::services::tauri_listen("dict-packs", |ev: web_sys::Event| {
        let value: &JsValue = ev.as_ref();
        let payload = js_sys::Reflect::get(value, &"payload".into()).unwrap_or(JsValue::UNDEFINED);
        match serde_wasm_bindgen::from_value::<Vec<PackMirror>>(payload) {
            Ok(rows) => packs().set(rows),
            Err(e) => web_sys::console::warn_1(&format!("[dict] bad payload: {e:?}").into()),
        }
    });
}

/// One lifecycle verb per pack: download | pause | resume | cancel | remove.
pub fn request_pack(pack_id: &str, verb: &str) {
    #[derive(Serialize)]
    struct PackArgs {
        #[serde(rename = "packId")]
        pack_id: String,
    }
    let name = format!("dict_pack_{verb}");
    let args = serde_wasm_bindgen::to_value(&PackArgs {
        pack_id: pack_id.to_string(),
    })
    .unwrap_or(JsValue::UNDEFINED);
    spawn_local(async move {
        if let Err(e) = tauri_bridge::invoke(&name, args).await {
            web_sys::console::warn_1(&format!("[dict] {verb} failed: {e:?}").into());
        }
    });
}

/// The hover's ask: `word` between two languages.
/// POS ranks senses; never gates them.
pub fn lookup(
    word: String,
    from: String,
    to: String,
    pos: Option<String>,
    done: impl FnOnce(Vec<EntryMirror>) + 'static,
) {
    #[derive(Serialize)]
    struct LookupArgs {
        word: String,
        from: String,
        to: String,
        pos: Option<String>,
    }
    if !tauri_bridge::has_tauri() {
        done(Vec::new());
        return;
    }
    spawn_local(async move {
        let args = serde_wasm_bindgen::to_value(&LookupArgs {
            word,
            from,
            to,
            pos,
        })
        .unwrap_or(JsValue::UNDEFINED);
        let parsed = match tauri_bridge::invoke("dict_lookup", args).await {
            Ok(value) => serde_wasm_bindgen::from_value::<Vec<EntryMirror>>(value)
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        done(parsed);
    });
}

/// The route's ask: a word on either side of a built
/// pack.
pub fn search(
    ask: String,
    pack_ids: Option<Vec<String>>,
    done: impl FnOnce(Vec<EntryMirror>) + 'static,
) {
    #[derive(Serialize)]
    struct SearchArgs {
        ask: String,
        #[serde(rename = "packIds")]
        pack_ids: Option<Vec<String>>,
    }
    if !tauri_bridge::has_tauri() {
        done(Vec::new());
        return;
    }
    spawn_local(async move {
        let args = serde_wasm_bindgen::to_value(&SearchArgs { ask, pack_ids })
            .unwrap_or(JsValue::UNDEFINED);
        let parsed = match tauri_bridge::invoke("dict_search", args).await {
            Ok(value) => serde_wasm_bindgen::from_value::<Vec<EntryMirror>>(value)
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        done(parsed);
    });
}
