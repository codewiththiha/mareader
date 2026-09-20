//! The library's app-lifetime wiring: the measurement pass at startup, the rescan of every
//! watched folder when the window comes back, and the sink that folds the shell's progress
//! beats into the dock's task list. All three are installed once at the app root.

use std::cell::RefCell;

use leptos::prelude::*;
use wasm_bindgen::JsValue;

use library_core::id::Cooldown;
use library_core::wire::ImportProgress;

use crate::services::library::{backfill_missing, migrate_store_layout, rescan_watched, PROGRESS_CHANNEL};
use crate::state::AppState;

/// Focus events are not rare: alt-tabbing back and forth would otherwise
/// walk every watched folder once per flick of the switcher.
const RESCAN_COOLDOWN_MS: u64 = 5_000;

// The rule is `library_core::id`'s to hold and test; this file only says where the answer lives.
thread_local! {
    static RESCAN: RefCell<Cooldown> = RefCell::new(Cooldown::new(RESCAN_COOLDOWN_MS));
}

/// Called once from the app root, after the theme and the AI bridge and
/// before the OS file handoff: a double-clicked book must not land in the
/// middle of the library's first measurement pass.
pub(crate) fn library_effects(state: AppState) {
    install_progress_sink(state);
    // The store migration changes the address rows hold, so measuring first
    // would mark books `missing` for files this pass is about to move.
    migrate_store_layout(state);
    rescan_watched(state);
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

/// App-lifetime rather than page-lifetime: an import started on the library page is still
/// running after the reader has opened a book. A beat for a task the list does not hold is
/// dropped — which is how a quiet rescan stays quiet.
///
/// The listener is this sink's own, on the shell's channel, folded straight
/// into the task list: one parse between the shell and the state, where the
/// window-event re-broadcast it replaced was two.
fn install_progress_sink(state: AppState) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    crate::services::tauri_listen(PROGRESS_CHANNEL, move |ev: web_sys::Event| {
        let value: &JsValue = ev.as_ref();
        let Ok(payload) = js_sys::Reflect::get(value, &"payload".into()) else {
            return;
        };
        match serde_wasm_bindgen::from_value::<ImportProgress>(payload) {
            Ok(beat) => state.library.tasks.update(|tasks| {
                if let Some(task) = tasks.iter_mut().find(|t| t.id == beat.task) {
                    task.beat(&beat);
                }
            }),
            Err(e) => {
                web_sys::console::warn_1(&format!("[library] bad progress payload: {e}").into());
            }
        }
    });
}

fn rescan_once(state: AppState) {
    let now = crate::time::now_ms();
    if !RESCAN.with(|gate| gate.borrow_mut().due(now)) {
        return;
    }
    rescan_watched(state);
}

/// Only an explicit `true` counts as the reader being back: a focus event whose payload is
/// missing or not a boolean is not a reason to walk every watched folder.
fn focused(ev: &web_sys::Event) -> bool {
    let value: &JsValue = ev.as_ref();
    js_sys::Reflect::get(value, &"payload".into())
        .ok()
        .and_then(|payload| payload.as_bool())
        .is_some_and(|focused| focused)
}
