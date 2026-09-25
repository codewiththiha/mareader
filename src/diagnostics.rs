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
                merge_runtime_digest(obj, d);
            }
        }
        // After the merge: the reported runtime generation is shell-owned
        // identity (§21), so the shell's reader-session count wins over the
        // digest's per-frame copy.
        let sessions = &state.manager.reader_sessions_created;
        report_runtime_generation(
            &mut value,
            sessions.load(std::sync::atomic::Ordering::Relaxed),
        );
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

/// Fold a runtime's last digest into the shell's own diagnostics object.
///
/// Shell-authored keys win the merge, unconditionally: a digest reports its
/// frame's local counters, and one of them shares `readerDisposesCompleted`
/// with the shell's cross-runtime session accounting. The invariant this
/// function exists to hold is that a runtime digest may never overwrite a
/// Shell-owned diagnostic field — the digest keeps its own local value on
/// its side of the boundary, the shell keeps its authority on its side.
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

/// Overwrite the merged runtime section's generation with the shell's
/// reader-session count — the identity the application observes across
/// frames. The digest's copy is seeded per frame from a wasm-static ordinal
/// that restarts with every iframe, so it only proves "not the first
/// runtime inside this frame"; the count below is bumped once per reader
/// boot, never for the library, and never resets while the page lives. The
/// runtime keeps its ordinal as its own internal lifetime stamp (§21).
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

    /// The regression behind the rapid-transition CI failure: a reader
    /// digest reporting its frame-local `readerDisposesCompleted = 1`
    /// merged over the shell's authoritative `2` and the browser suite
    /// then read "a session outlived its close". Shell-owned fields must
    /// survive any digest, and the digest's own counters must stay intact
    /// on its side of the merge.
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

    /// The split-run identity rule: a merged digest's per-frame generation
    /// (every iframe's wasm world starts its counter over) is overwritten by
    /// the shell's reader-session count, and a payload without a runtime
    /// section is left untouched.
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
