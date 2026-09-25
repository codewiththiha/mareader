//! The shell's diagnostics surface: `window.__mareaderDiagnostics()` merges
//! the manager's facts (which runtime is active, session create/dispose
//! counts, doc status/error forwarded across the boundary) with the active
//! runtime's last pushed digest (engine stats, virtualizer gauges, heap).
//! The reader-side fields only exist while a reader session reports them —
//! and `atBaseline` fails closed: a reader that never reported a drained
//! digest keeps the baseline false.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;

#[cfg(target_arch = "wasm32")]
use crate::state::ActiveRuntime;
use crate::state::ShellState;

pub fn install(state: ShellState) {
    #[cfg(target_arch = "wasm32")]
    install_web(state);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = state;
}

/// The heap line the reload path logs (the reader logs its own on close).
pub fn log_reload_heap() {
    #[cfg(target_arch = "wasm32")]
    app_state::memory::log_heap("reload");
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("[mem] reload (host)");
}

#[cfg(target_arch = "wasm32")]
fn install_web(state: ShellState) {
    use wasm_bindgen::prelude::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let probe = Closure::wrap(Box::new(move || {
        let mut value = serde_json::json!({
            // The boot surface: which state the runtime host is showing, and
            // the last failure with its runtime + stage (§6). A headless run
            // (the browser suites) reads these instead of a screenshot.
            "bootState": state.manager.boot_state.lock().unwrap().clone(),
            "lastBootError": state.manager.boot_error.lock().unwrap().clone().unwrap_or(serde_json::Value::Null),
            // The manager's own account: which runtime is active and what
            // session identity has been created/disposed (§21's test hook).
            "activeRuntime": match state.manager.active() {
                Some(ActiveRuntime::Reader) => serde_json::json!("reader"),
                Some(ActiveRuntime::Library) => serde_json::json!("library"),
                None => serde_json::Value::Null,
            },
            "readerSessionsCreated": state.manager.reader_sessions_created.load(std::sync::atomic::Ordering::Relaxed),
            "readerDisposesCompleted": state.manager.reader_disposes_completed.load(std::sync::atomic::Ordering::Relaxed),
            "librarySessionsCreated": state.manager.library_sessions_created.load(std::sync::atomic::Ordering::Relaxed),
            "libraryDisposesCompleted": state.manager.library_disposes_completed.load(std::sync::atomic::Ordering::Relaxed),
            "readerRuntimeLive": state.manager.active() == Some(ActiveRuntime::Reader),
            "staleFramesSeen": state.manager.stale_frames_seen.load(std::sync::atomic::Ordering::Relaxed),
            "docStatus": state.manager.doc_status.lock().unwrap().clone(),
            "docError": state.manager.doc_error.lock().unwrap().clone(),
        });
        if let Some(digest) = state.manager.last_digest.lock().unwrap().clone() {
            if let (Some(obj), Some(d)) = (value.as_object_mut(), digest.as_object()) {
                for (k, v) in d {
                    // The manager's own keys are shell-authored and win: a
                    // runtime's digest may carry same-named facts about ITS
                    // frame (its own per-iframe lifecycle counters), and a
                    // merged overwrite would erase the sessions accounting
                    // the shell is the authority for (§21).
                    if !obj.contains_key(k) {
                        obj.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        // The phase's gate, decided by the SHELL from its own manager facts:
        // no active reader AND the last reader digest said drained (or there
        // never was one — nothing reader-owned to drain).
        let digest_drained = state
            .manager
            .last_digest
            .lock()
            .unwrap()
            .as_ref()
            .map(|d| d.get("atBaseline") == Some(&serde_json::Value::Bool(true)))
            .unwrap_or(true);
        value["atBaseline"] = serde_json::Value::Bool(
            state.manager.active() != Some(ActiveRuntime::Reader) && digest_drained,
        );
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
    }) as Box<dyn Fn() -> String>);
    let probe: wasm_bindgen::JsValue = probe.into_js_value();
    let name = wasm_bindgen::JsValue::from_str("__mareaderDiagnostics");
    let target: js_sys::Object = window.unchecked_into();
    _ = js_sys::Reflect::set(&target, &name, &probe);
}
