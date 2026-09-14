//! The library's app-lifetime wiring: the measurement pass at startup, the rescan of every
//! watched folder when the window comes back, and the sink that folds the shell's progress
//! beats into the dock's task list. All three are installed once at the app root.

use std::sync::atomic::{AtomicU64, Ordering};

// The prelude is what puts `update` and `with_untracked` on a signal: they are trait methods, and a file that only names `RwSignal` gets a struct with no methods on it.
use leptos::prelude::*;
use wasm_bindgen::JsValue;

use library_core::wire::ImportProgress;

use crate::components::primitives::hooks::use_custom_event::use_typed_event;
use crate::events::IMPORT_PROGRESS_EVENT;
use crate::services::library::{
    backfill_missing, migrate_store_layout, rescan_watched, verify_library,
};
use crate::state::AppState;

/// Focus events are not rare: alt-tabbing back and forth would otherwise walk every watched folder once per flick of the switcher.
const RESCAN_COOLDOWN_MS: u64 = 5_000;

/// Relaxed ordering: the webview is single-threaded, so this only has to be a stamp, never a fence.
static LAST_RESCAN: AtomicU64 = AtomicU64::new(0);

/// Called once from the app root, after the theme and the AI bridge and before the OS file handoff — a double-clicked book must not land in the middle of the library's first measurement pass.
pub(crate) fn library_effects(state: AppState) {
    install_progress_sink(state);
    // The move changes the address the row holds, so measuring first would mark the book `missing` for a file this pass is about to put somewhere else.
    migrate_store_layout(state);
    verify_library(state);
    backfill_missing(state);

    if !tauri_bridge::has_tauri() {
        return;
    }
    crate::services::tauri_listen("tauri://focus", move |ev: web_sys::Event| {
        if focused(&ev) {
            rescan_once(state);
            backfill_missing(state);
        }
    });
}

/// App-lifetime rather than page-lifetime: an import started on the library page is still running after the reader has opened a book. A beat for a task the list does not hold is dropped — which is how a quiet rescan stays quiet.
fn install_progress_sink(state: AppState) {
    use_typed_event::<ImportProgress>(IMPORT_PROGRESS_EVENT, move |beat| {
        state.library.tasks.update(|tasks| {
            if let Some(task) = tasks.iter_mut().find(|t| t.id == beat.task) {
                task.beat(&beat);
            }
        });
    });
}

fn rescan_once(state: AppState) {
    let now = crate::time::now_ms();
    let last = LAST_RESCAN.load(Ordering::Relaxed);
    if now.saturating_sub(last) < RESCAN_COOLDOWN_MS {
        return;
    }
    LAST_RESCAN.store(now, Ordering::Relaxed);
    rescan_watched(state);
}

fn focused(ev: &web_sys::Event) -> bool {
    let value: &JsValue = ev.as_ref();
    js_sys::Reflect::get(value, &"payload".into())
        .ok()
        .and_then(|payload| payload.as_bool())
        .unwrap_or(true)
}
