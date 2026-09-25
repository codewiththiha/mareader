//! The Shell side of the boundary: `window.__mareaderShell`, the typed
//! methods runtimes call with JSON strings (§14). The shell's responses are
//! persistence writes, navigation intents, and diagnostics merges — never
//! state handles.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

use crate::state::ShellState;

/// Install the bridge. Called once from the shell root, before any runtime
/// loads.
pub fn install(state: ShellState) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let bridge = js_sys::Object::new();
    let st = state.clone();
    register(&bridge, "openDocument", move |json: String| {
        if let Ok(launch) = serde_json::from_str::<app_state::boundary::LaunchDocument>(&json) {
            st.manager.open_document(&st, launch);
        }
    });
    let st = state.clone();
    register_void(&bridge, "navigateLibrary", move || {
        st.manager.navigate_library(&st);
    });
    register(&bridge, "readPoint", move |json: String| {
        if let Ok(point) = serde_json::from_str::<app_state::boundary::ReadPoint>(&json) {
            crate::services::apply_read_point(&point);
        }
    });
    register(&bridge, "saveSettings", move |json: String| {
        if let Ok(settings) = serde_json::from_str::<reader_core::settings::Settings>(&json) {
            crate::services::save_settings(&settings);
        }
    });
    register(&bridge, "saveLibrary", move |json: String| {
        if let Ok(blob) = serde_json::from_str::<library_core::blob::LibraryBlob>(&json) {
            crate::services::save_library(&blob);
        }
    });
    register(&bridge, "saveCovers", move |json: String| {
        if let Ok(covers) = serde_json::from_str::<app_state::state::covers::CoverMap>(&json) {
            let _ = storage::save_covers(&covers);
        }
    });
    register(&bridge, "saveCover", move |json: String| {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct One {
            path: String,
            image: app_state::state::covers::CoverImage,
        }
        if let Ok(one) = serde_json::from_str::<One>(&json) {
            crate::services::save_cover(&one.path, one.image);
        }
    });
    let st = state.clone();
    register(&bridge, "docStatus", move |json: String| {
        if let Ok(report) = serde_json::from_str::<app_state::boundary::DocStatusReport>(&json) {
            if report.status != "Ready" && st.manager.active().is_some() {
                let live = st.manager.active() == Some(crate::state::ActiveRuntime::Reader);
                if live && report.status == "Idle" {
                    st.manager.navigate_library(&st);
                }
            }
            *st.manager.doc_status.lock().unwrap() = report.status;
            *st.manager.doc_error.lock().unwrap() = report.error;
        }
    });
    let st = state.clone();
    register(&bridge, "publishDigest", move |json: String| {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
            *st.manager.last_digest.lock().unwrap() = Some(value);
        }
    });
    register_void(&bridge, "reload", move || {
        crate::diagnostics::log_reload_heap();
        app_chrome::window::api::reload_window();
    });
    // resolveLaunch: a synchronous query answered from the persisted store.
    let resolver = Closure::wrap(Box::new(move |path: String| -> wasm_bindgen::JsValue {
        match crate::services::resolve_launch(&path) {
            Some(launch) => {
                serde_wasm_bindgen::to_value(&launch).unwrap_or(wasm_bindgen::JsValue::NULL)
            }
            None => wasm_bindgen::JsValue::NULL,
        }
    }) as Box<dyn Fn(String) -> wasm_bindgen::JsValue>);
    let _ = js_sys::Reflect::set(
        &bridge,
        &wasm_bindgen::JsValue::from_str("resolveLaunch"),
        &resolver.into_js_value(),
    );

    let target: js_sys::Object = window.unchecked_into();
    let _ = js_sys::Reflect::set(
        &target,
        &wasm_bindgen::JsValue::from_str("__mareaderShell"),
        &bridge.into(),
    );
}

/// Register a command whose payload is nothing: the runtimes send no argument
/// for these (a close intent needs no data, a reload carries none), and a
/// handler that demanded a string would REJECT that call — at the
/// wasm-bindgen boundary, where the caller's `_ =` would swallow it.
fn register_void(bridge: &js_sys::Object, name: &str, f: impl Fn() + 'static) {
    let closure = Closure::wrap(Box::new(f) as Box<dyn Fn()>);
    let _ = js_sys::Reflect::set(
        bridge,
        &wasm_bindgen::JsValue::from_str(name),
        &closure.into_js_value(),
    );
}

fn register(bridge: &js_sys::Object, name: &str, f: impl Fn(String) + 'static) {
    let closure = Closure::wrap(Box::new(f) as Box<dyn Fn(String)>);
    let _ = js_sys::Reflect::set(
        bridge,
        &wasm_bindgen::JsValue::from_str(name),
        &closure.into_js_value(),
    );
}
