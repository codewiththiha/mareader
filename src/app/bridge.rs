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
        if let Ok(launch) =
            serde_json::from_str::<runtime_contract::boundary::LaunchDocument>(&json)
        {
            st.manager.open_document(&st, launch);
        }
    });
    let st = state.clone();
    register_void(&bridge, "navigateLibrary", move || {
        st.manager.navigate_library(&st);
    });
    register(&bridge, "readPoint", move |json: String| {
        if let Ok(point) = serde_json::from_str::<runtime_contract::boundary::ReadPoint>(&json) {
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
        if let Ok(covers) = serde_json::from_str::<runtime_contract::covers::CoverMap>(&json) {
            let _ = storage::save_covers(&covers);
        }
    });
    register(&bridge, "saveCover", move |json: String| {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct One {
            path: String,
            image: runtime_contract::covers::CoverImage,
        }
        if let Ok(one) = serde_json::from_str::<One>(&json) {
            crate::services::save_cover(&one.path, one.image);
        }
    });
    // The shelf's bake queue has no engine: it asks, the Shell bakes, the
    // answer re-enters through the library session's command surface —
    // generation-checked, so a bake that outlived its frame is dropped.
    let st = state.clone();
    register(&bridge, "bakeCover", move |json: String| {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Bake {
            path: String,
        }
        let Ok(bake) = serde_json::from_str::<Bake>(&json) else {
            return;
        };
        let st = st.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let path = bake.path;
            let image =
                pdf_engine::api::cover_data_url(&path, runtime_contract::covers::COVER_WIDTH)
                    .await
                    .ok()
                    .map(|cover| runtime_contract::covers::CoverImage {
                        data_url: cover.data_url,
                        width: cover.width,
                        height: cover.height,
                    });
            st.manager
                .deliver_library_frame(&runtime_contract::protocol::ShellFrame::CoverBaked {
                    path,
                    image,
                });
        });
    });
    let st = state.clone();
    register(&bridge, "docStatus", move |json: String| {
        if let Ok(report) =
            serde_json::from_str::<runtime_contract::boundary::DocStatusReport>(&json)
        {
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
