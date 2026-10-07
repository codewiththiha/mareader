//! The shell's diagnostics surface, merging the manager's facts with the
//! active runtime's digest.

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

#[cfg(target_arch = "wasm32")]
fn install_web(state: ShellState) {
    use wasm_bindgen::prelude::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let probe = Closure::wrap(Box::new(move || {
        let mut value = serde_json::json!({
            // The boot surface: which state the host shows, and its failure.
            "bootState": state.manager.boot_state.lock().unwrap().clone(),
            "lastBootError": state.manager.boot_error.lock().unwrap().clone().unwrap_or(serde_json::Value::Null),
            // The manager's account: active runtime and session counts.
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
            // DOM residency includes incoming and retiring realms.
            "readerFramesResident": state.manager.reader_frames_resident(),
            "libraryFramesResident": crate::app::frame::resident(crate::app::frame::FrameKind::Library),
            "paneFramesResident": crate::app::frame::document_frames_resident(),
            "routeReturnPolicy": "unload-both",
            "routePrewarmAllowed": false,
            "routeArtifacts": 5,
            "rasterLane": raster_snapshot(),
            // The cover baker: whether its page is mounted, and its answers.
            "bakeFrameResident": crate::app::bake::resident(),
            "coversAnswered": crate::app::bake::answered(),
            "staleFramesSeen": state.manager.stale_frames_seen.load(std::sync::atomic::Ordering::Relaxed),
            "docStatus": state.manager.doc_status.lock().unwrap().clone(),
            "docError": state.manager.doc_error.lock().unwrap().clone(),
        });
        if let Some(digest) = state.manager.last_digest.lock().unwrap().clone() {
            if let (Some(obj), Some(d)) = (value.as_object_mut(), digest.as_object()) {
                merge_runtime_digest(obj, d);
            }
        }
        // The reported runtime generation is shell-owned identity.
        let sessions = &state.manager.reader_sessions_created;
        report_runtime_generation(
            &mut value,
            sessions.load(std::sync::atomic::Ordering::Relaxed),
        );
        // The phase's gate, decided by the SHELL from its own facts.
        let digest_drained = state
            .manager
            .last_digest
            .lock()
            .unwrap()
            .as_ref()
            .map(|d| d.get("atBaseline") == Some(&serde_json::Value::Bool(true)))
            .unwrap_or(true);
        value["atBaseline"] = serde_json::Value::Bool(
            state.manager.active() != Some(ActiveRuntime::Reader)
                && digest_drained
                && state.manager.reader_frames_resident() == 0
                && crate::app::frame::document_frames_resident() == 0
                && state
                    .manager
                    .reader_sessions_created
                    .load(std::sync::atomic::Ordering::Relaxed)
                    == state
                        .manager
                        .reader_disposes_completed
                        .load(std::sync::atomic::Ordering::Relaxed)
                && value["rasterLane"]["active"] == serde_json::json!(0)
                && value["rasterLane"]["queued"] == serde_json::json!(0)
                && value["rasterLane"]["owners"] == serde_json::json!(0),
        );
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
    }) as Box<dyn Fn() -> String>);
    let probe: wasm_bindgen::JsValue = probe.into_js_value();
    let name = wasm_bindgen::JsValue::from_str("__mareaderDiagnostics");
    let target: js_sys::Object = window.unchecked_into();
    _ = js_sys::Reflect::set(&target, &name, &probe);
}

#[cfg(target_arch = "wasm32")]
fn raster_snapshot() -> serde_json::Value {
    use wasm_bindgen::{JsCast, JsValue};

    let read = || {
        let window = web_sys::window()?;
        let lane =
            js_sys::Reflect::get(&window, &JsValue::from_str("__mareaderRasterLane")).ok()?;
        let snapshot = js_sys::Reflect::get(&lane, &JsValue::from_str("snapshot"))
            .ok()?
            .dyn_into::<js_sys::Function>()
            .ok()?;
        let value = snapshot.call0(&lane).ok()?;
        let json = js_sys::JSON::stringify(&value).ok()?.as_string()?;
        serde_json::from_str(&json).ok()
    };
    read().unwrap_or(serde_json::Value::Null)
}

/// Fold a runtime's last digest into the shell's diagnostics.
#[cfg(any(test, target_arch = "wasm32"))]
fn merge_runtime_digest(
    shell: &mut serde_json::Map<String, serde_json::Value>,
    digest: &serde_json::Map<String, serde_json::Value>,
) {
    for (key, value) in digest {
        if shell.contains_key(key) {
            continue;
        }
        shell.insert(key.clone(), value.clone());
    }
}

/// Overwrite the runtime generation with the shell's session count.
#[cfg(any(test, target_arch = "wasm32"))]
fn report_runtime_generation(value: &mut serde_json::Value, reader_sessions_created: u64) {
    if let Some(runtime) = value
        .get_mut("runtime")
        .and_then(serde_json::Value::as_object_mut)
    {
        runtime.insert(
            "generation".to_string(),
            serde_json::json!(reader_sessions_created),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{merge_runtime_digest, report_runtime_generation};

    /// The regression behind the rapid-transition CI failure.
    #[test]
    fn runtime_digest_never_overwrites_shell_owned_fields() {
        let mut shell: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            r#"{
                "activeRuntime": "library",
                "readerSessionsCreated": 2,
                "readerDisposesCompleted": 2,
                "librarySessionsCreated": 3,
                "libraryDisposesCompleted": 2
            }"#,
        )
        .expect("shell fixture parses");
        let digest: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            r#"{
                "readerRuntimesCreated": 1,
                "readerDisposesCompleted": 1,
                "virtualizerLive": 0
            }"#,
        )
        .expect("digest fixture parses");

        merge_runtime_digest(&mut shell, &digest);

        // The shell's cross-runtime accounting stays authoritative.
        assert_eq!(shell["readerSessionsCreated"], 2);
        assert_eq!(shell["readerDisposesCompleted"], 2);
        assert_eq!(shell["librarySessionsCreated"], 3);
        assert_eq!(shell["libraryDisposesCompleted"], 2);
        assert_eq!(shell["activeRuntime"], "library");
        // Digest-only keys still flow through the presentation boundary.
        assert_eq!(shell["readerRuntimesCreated"], 1);
        assert_eq!(shell["virtualizerLive"], 0);
        // And the digest still reports its own local counter as-is.
        assert_eq!(digest["readerDisposesCompleted"], 1);
    }

    /// The split-run identity rule for merged digests.
    #[test]
    fn reported_generation_is_the_shell_session_count() {
        let mut value: serde_json::Value = serde_json::from_str(
            r#"{
                "activeRuntime": "reader",
                "runtime": {"generation": 1, "state": "ready"}
            }"#,
        )
        .expect("value fixture parses");

        report_runtime_generation(&mut value, 7);

        assert_eq!(value["runtime"]["generation"], 7);
        assert_eq!(value["runtime"]["state"], "ready");
        assert_eq!(value["activeRuntime"], "reader");

        let mut no_runtime = serde_json::json!({"activeRuntime": "library"});
        let expected = serde_json::json!({"activeRuntime": "library"});
        report_runtime_generation(&mut no_runtime, 7);
        assert_eq!(no_runtime, expected);
    }
}
