//! The realm-level half of the engine surface, plus the envelope parser
//! every engine call shares. Views and effects never touch wasm-bindgen
//! types.
//!
//! Document work is NOT here: every call that touches a document goes
//! through the [`crate::session::PdfSession`] that owns it. What remains is
//! what names no document — the appearance broadcast ([`theme`]: each live
//! session re-derives its own raster theme), the diagnostics read side
//! ([`diagnostics`]), and the paper frame parser ([`paper`]) the session's
//! paper state machine reads through.
//!
//! Every engine fn resolves to `{ok:true, ...}` or
//! `{ok:false, error:{name,message}}`; we check `ok` here and surface a
//! `Result<T, EngineError>`.
//!
//! [`resolve`] and the hoisted property keys live here: the one parser for the
//! `{ok,...}` envelope and the hottest allocations in the crate, shared rather
//! than duplicated per surface.

use serde::de::DeserializeOwned;
use std::thread::LocalKey;
use wasm_bindgen::JsValue;

pub mod diagnostics;
pub mod paper;
pub mod theme;

pub use diagnostics::{EngineStats, engine_stats, set_lifecycle_log};
pub use paper::PaperFrame;
pub use theme::{refresh_theme, set_appearance_menu_open, set_scrub_mode};

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
}

/// `obj[key]` using one of the hoisted keys.
pub(crate) fn reflect_get(
    obj: &JsValue,
    key: &'static LocalKey<JsValue>,
) -> Result<JsValue, JsValue> {
    key.with(|k| js_sys::Reflect::get(obj, k))
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
/// no Promise can use it too.
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
