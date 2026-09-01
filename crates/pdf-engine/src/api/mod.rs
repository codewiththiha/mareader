//! Typed wrappers over the JS engine (window.PDFReader). This is the ONLY
//! module that calls engine functions; views and effects never touch
//! wasm-bindgen types.
//!
//! Every engine fn resolves to `{ok:true, ...}` or `{ok:false, error:{name,message}}`
//! including the extraction calls behind the search index. We check `ok` here
//! and surface a `Result<T, EngineError>`.
//!
//! The module is split so each surface is independently readable:
//!   - [`document`]  — open / outline / destroy / covers / pending OS files
//!   - [`render`]    — page registration, live renders, thumbnails
//!   - [`search`]    — the Rust-owned full-text index + engine-side painting
//!   - [`paper`]     — the paper session's pixel plumbing
//!   - [`dialog`]    — the native open-file dialog
//!   - [`theme`]     — re-bake / scrub mode / advisory sweeps
//!   - [`window`]    — window chrome + AI word explanation
//!
//! [`resolve`] and the hoisted property keys live here: they are the one
//! parser for the `{ok,...}` envelope and the hottest allocations in the
//! crate, so they are shared rather than duplicated per surface.

use serde::de::DeserializeOwned;
use std::thread::LocalKey;
use wasm_bindgen::JsValue;

pub mod dialog;
pub mod document;
pub mod paper;
pub mod render;
pub mod search;
pub mod theme;
pub mod window;

pub use dialog::pick_pdf;
pub use document::{cover_data_url, destroy, open, outline, take_pending_file};
pub use paper::{cached_paper, persist_paper, sample_paper_page, set_paper, set_paper_active, take_paper_frame, CachedPaper, PaperFrame};
pub use render::{
    blit_thumb, cancel_thumb, has_thumb, prefetch_thumb, register_page, render_page, render_thumb,
    unregister_page,
};
pub use search::{build_search_index, clear_highlights, search, set_active_match};
pub use theme::{refresh_theme, set_live_pipeline, set_scrub_mode, sweep};
pub use window::{explain_word, set_traffic_lights};

/// Error returned by any engine call: the engine-side error `name` and
/// `message`, or a local failure to parse/communicate.
#[derive(Debug, Clone)]
pub struct EngineError {
    pub name: String,
    pub message: String,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.message)
    }
}

/// Engine errors are toast text on the UI side; converting without cloning
/// the inner strings keeps the retry/toast path allocation-free.
impl From<EngineError> for String {
    fn from(e: EngineError) -> Self {
        e.to_string()
    }
}

/// A non-string JS value is not a usable error field; show its debug form
/// instead of silently substituting an empty string (an empty pair read as
/// `: ` on screen and hid the real cause).
fn js_str(v: JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{v:?}"))
}

/// Hoisted property keys. `resolve` runs on EVERY engine call (each live
/// render, each thumbnail, each search), and `JsValue::from_str` allocates a
/// fresh JS string per key per call; these are created once. Every lookup in
/// this crate goes through one of these.
macro_rules! js_keys {
    ($($name:ident => $lit:literal),* $(,)?) => {
        // `thread_local!` emits `const NAME: LocalKey<JsValue>`, so `&NAME`
        // at a call site is a promoted `'static` reference — which is what
        // `LocalKey::with` requires.
        $(thread_local! {
            pub(crate) static $name: JsValue = JsValue::from_str($lit);
        })*
    };
}

js_keys! {
    KEY_OK => "ok",
    KEY_ERROR => "error",
    KEY_NAME => "name",
    KEY_MESSAGE => "message",
    KEY_PAGE => "page",
    KEY_WIDTH => "width",
    KEY_HEIGHT => "height",
    KEY_DATA => "data",
    KEY_VISIBLE => "visible",
    KEY_WORD => "word",
    KEY_CONTEXT => "context",
    KEY_RUN => "run",
    // Native dialog options (built once per dialog open — still hoisted so
    // the pattern is uniform and the picker never allocates a key twice).
    KEY_MULTIPLE => "multiple",
    KEY_DIRECTORY => "directory",
    KEY_FILTERS => "filters",
    KEY_EXTENSIONS => "extensions",
    KEY_PDF => "PDF",
    KEY_HEADER_HEIGHT => "headerHeight",
}

/// `obj[key]` using one of the hoisted keys.
pub(crate) fn reflect_get(obj: &JsValue, key: &'static LocalKey<JsValue>) -> Result<JsValue, JsValue> {
    key.with(|k| js_sys::Reflect::get(obj, k))
}

/// `obj[key] = value` using one of the hoisted keys.
pub(crate) fn reflect_set(obj: &JsValue, key: &'static LocalKey<JsValue>, value: &JsValue) -> bool {
    // `LocalKey::with` wants `&'static self`; the hoisted keys are `const`
    // items, so `&KEY_X` at the call site is a promoted `'static` reference.
    key.with(|k| js_sys::Reflect::set(obj, k, value)).unwrap_or(false)
}

/// True when `window.PDFReader` is attached; must be checked before any
/// engine call (a missing global makes the wasm-bindgen shim throw, which
/// panics the reactive owner and freezes menus / theme / open).
pub(crate) fn require_pdf_reader() -> Result<(), EngineError> {
    if crate::bridge::has_pdf_reader() {
        Ok(())
    } else {
        Err(EngineError {
            name: "no_engine".to_string(),
            message: "PDF engine is not loaded yet. Restart the app and try again.".to_string(),
        })
    }
}

/// Same probe as [`require_pdf_reader`] as a boolean, for the fire-and-forget
/// calls that are silent no-ops outside the engine.
pub(crate) fn guard_pdf_reader() -> bool {
    crate::bridge::has_pdf_reader()
}

/// Parses a `{ok:bool, error?:{name,message}, ...fields}` value into `T`.
/// Pure parsing — no JS awaits — so the whole engine-answer path that needs
/// no Promise (e.g. the synchronous paper-cache lookup) can use it too.
pub(crate) fn resolve<T: DeserializeOwned>(value: JsValue, what: &str) -> Result<T, EngineError> {
    let is_ok = reflect_get(&value, &KEY_OK)
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_ok {
        serde_wasm_bindgen::from_value(value).map_err(|e| EngineError {
            name: "parse".to_string(),
            message: format!("{what}: bad engine payload ({e})"),
        })
    } else {
        let err = reflect_get(&value, &KEY_ERROR).unwrap_or(JsValue::UNDEFINED);
        let name = reflect_get(&err, &KEY_NAME).map(js_str).unwrap_or_default();
        let message = reflect_get(&err, &KEY_MESSAGE)
            .map(js_str)
            .unwrap_or_else(|_| "unknown engine error".to_string());
        Err(EngineError { name, message })
    }
}
