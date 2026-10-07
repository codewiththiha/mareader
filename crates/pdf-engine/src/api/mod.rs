//! The realm-level half of the engine surface and the shared envelope parser.
use serde::de::DeserializeOwned;
use std::thread::LocalKey;
use wasm_bindgen::JsValue;

pub mod diagnostics;
pub mod paper;
pub mod theme;

pub use diagnostics::{EngineStats, engine_stats, set_lifecycle_log};
pub use paper::PaperFrame;
pub use theme::{refresh_theme, set_appearance_menu_open, set_scrub_mode};

/// Error from any engine call: its name and message, or a parse failure.
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

/// Engine errors are toast text; the conversion avoids cloning.
impl From<EngineError> for String {
    fn from(e: EngineError) -> Self {
        e.to_string()
    }
}

/// A non-string error field shows its debug form.
fn js_str(v: JsValue) -> String {
    v.as_string().unwrap_or_else(|| format!("{v:?}"))
}

/// Hoisted property keys, created once for the hottest path.
macro_rules! js_keys {
    ($($name:ident => $lit:literal),* $(,)?) => {
        // `&NAME` on a `thread_local!` is a promoted `'static` reference.
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

/// True when `window.PDFReader` is attached; check before any call.
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

/// [`require_pdf_reader`] as a boolean, for the silent calls.
pub(crate) fn guard_pdf_reader() -> bool {
    crate::bridge::has_pdf_reader()
}

/// Parses a `{ok, error?, ...}` value into `T`.
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
