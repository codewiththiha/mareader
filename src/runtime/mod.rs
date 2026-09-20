//! Runtime-local bridge. The host owns navigation and the browsing context.
#[cfg(feature = "library")]
pub mod library;
pub mod reader;
pub mod controls;
#[cfg(feature = "library")]
pub mod workspace;

use std::cell::{Cell, RefCell};
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use reader_core::settings::Settings;

pub use crate::events::{COMMAND_EVENT, OUTPUT_EVENT};

thread_local! {
    static LABEL: Cell<&'static str> = const { Cell::new("library") };
    static WORKSPACE: Cell<bool> = const { Cell::new(false) };
    static READER: Cell<bool> = const { Cell::new(false) };
    static UNMOUNT: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

pub fn mark_workspace() { WORKSPACE.with(|v| v.set(true)); }
pub fn is_workspace() -> bool { WORKSPACE.with(Cell::get) }
/// Window-owned visibility mirrored into a reader without mounting chrome.
#[derive(Clone, Copy)]
pub struct ChromeVisibility {
    pub bar: RwSignal<bool>,
    pub rail: RwSignal<bool>,
}

pub fn is_reader() -> bool { READER.with(Cell::get) }
pub fn mark_reader(format: &'static str) {
    READER.with(|r| r.set(true));
    LABEL.with(|label| label.set(match format { "pdf" => "reader-pdf", "txt" => "reader-txt", _ => "reader-md" }));
}
pub fn label() -> &'static str { LABEL.with(Cell::get) }
pub fn retain_mount(unmount: impl FnOnce() + 'static) {
    UNMOUNT.with(|slot| *slot.borrow_mut() = Some(Box::new(unmount)));
}
pub fn unmount() {
    let cleanup = UNMOUNT.with(|slot| slot.borrow_mut().take());
    if let Some(cleanup) = cleanup { cleanup(); }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderConfig {
    pub book_id: String,
    pub path: String,
    pub format: String,
    pub title: Option<String>,
    pub cover: Option<String>,
    pub resume_page: u32,
    pub resume_fraction: Option<f64>,
    pub settings: Settings,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadPoint {
    pub page: u32,
    pub num_pages: u32,
    pub fraction: Option<f64>,
}

pub fn emit(payload: serde_json::Value) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(detail) = payload.serialize(&serde_wasm_bindgen::Serializer::json_compatible()) {
            let init = web_sys::CustomEventInit::new();
            init.set_detail(&detail);
            if let Ok(event) = web_sys::CustomEvent::new_with_event_init_dict(OUTPUT_EVENT, &init) {
                let _ = window().dispatch_event(&event);
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = payload;
}

/// Local DOM listeners are owned by the same Leptos owner as their state.
pub fn listen(handler: impl Fn(serde_json::Value) + 'static) {
    let handle = window_event_listener(
        leptos::ev::Custom::new(COMMAND_EVENT),
        move |event: web_sys::CustomEvent| {
            if let Ok(value) = serde_wasm_bindgen::from_value(event.detail()) { handler(value); }
        },
    );
    on_cleanup(move || handle.remove());
}
