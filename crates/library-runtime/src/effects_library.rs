//! The library's app-lifetime wiring: the startup pass, the folder
//! rescan, and the progress sink.

use std::cell::{Cell, RefCell};

use leptos::prelude::*;
use wasm_bindgen::JsValue;

use library_core::id::Cooldown;
use library_core::wire::ImportProgress;

use crate::services::{PROGRESS_CHANNEL, backfill_missing, migrate_store_layout, rescan_watched};

/// Focus events are not rare: a cooldown keeps alt-tabbing from
/// walking every watched folder.
const RESCAN_COOLDOWN_MS: u64 = 5_000;

// The rule lives in `library_core::id`; this is where it is held.
thread_local! {
    static RESCAN: RefCell<Cooldown> = RefCell::new(Cooldown::new(RESCAN_COOLDOWN_MS));
}

/// Called once from the app root; an incoming frame defers its
/// startup passes.
pub fn library_effects(state: crate::context::LibraryContext, defer_startup: bool) {
    install_progress_sink(state);
    if defer_startup {
        DEFERRED.with(|slot| slot.set(Some(state)));
        on_cleanup(|| DEFERRED.with(|slot| slot.set(None)));
        return;
    }
    startup_passes(state);
}

/// The passes a shown shelf owes its own state: migrate, measure,
/// backfill.
fn startup_passes(state: crate::context::LibraryContext) {
    // Migration first: measuring first would mark rows `missing`.
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

thread_local! {
    /// Startup work awaiting reveal; cleared on unmount.
    static DEFERRED: Cell<Option<crate::context::LibraryContext>> = const { Cell::new(None) };
}

/// Start deferred passes once, after reveal.
pub fn run_deferred_startup() {
    let Some(state) = DEFERRED.with(|slot| slot.take()) else {
        return;
    };
    startup_passes(state);
}

/// The realm's progress sink: shell beats folded straight into the
/// task list.
fn install_progress_sink(state: crate::context::LibraryContext) {
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

fn rescan_once(state: crate::context::LibraryContext) {
    let now = runtime_contract::time::now_ms();
    if !RESCAN.with(|gate| gate.borrow_mut().due(now)) {
        return;
    }
    rescan_watched(state);
}

/// Only an explicit `true` counts as the reader being back.
fn focused(ev: &web_sys::Event) -> bool {
    let value: &JsValue = ev.as_ref();
    js_sys::Reflect::get(value, &"payload".into())
        .ok()
        .and_then(|payload| payload.as_bool())
        .is_some_and(|focused| focused)
}
