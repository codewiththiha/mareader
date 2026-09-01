//! Wire drag-and-drop file opening.
//!
//! Two layers:
//!   1. DOM prevent-default listeners on `window` (`dragenter` / `dragover` /
//!      `drop`) so a dropped file never navigates the webview away — the
//!      plain-browser fallback (`trunk serve`). Inside Tauri a native file
//!      drag never reaches the DOM (no dragenter/dragover fire on macOS), so
//!      these are inert there and the overlay is driven by layer 2 instead.
//!   2. Tauri drag lifecycle subscriptions (`tauri://drag-enter`,
//!      `tauri://drag-leave`, `tauri://drag-drop`). These ARE the drag
//!      signals for a real file drag from Finder/Explorer: enter shows the
//!      drop-feedback overlay, leave hides it, drop opens the file (and hides
//!      it). Subscriptions go through `services::tauri_listen`, which parks
//!      each closure so the listener stays registered for the view's lifetime.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use web_sys::Event;

use crate::state::AppState;

pub(crate) fn drag_drop(state: AppState, drag_active: RwSignal<bool>) {
    // Drag-enter/leave DEPTH counter. Window-level `dragenter`/`dragleave`
    // fire for every child-element boundary crossed, not just the window edge,
    // so a naive "set true on enter, false on leave" would flicker the overlay
    // as the pointer moves over its own contents. Counting enter/leave pairs
    // makes the overlay appear on the first file dragenter and hold until the
    // drag genuinely leaves the window (or is dropped).
    let depth = Rc::new(Cell::new(0u32));

    // Layer 1: DOM prevent-default listeners. Parked in a StoredValue so they
    // (and the handles' removal closures) live for the component lifetime.
    let d_enter = depth.clone();
    let d_leave = depth.clone();
    let d_drop = depth;
    let da_enter = drag_active;
    let da_leave = drag_active;
    let da_drop = drag_active;
    let dom_handles = StoredValue::new_local(vec![
        window_event_listener(leptos::ev::dragenter, move |ev: leptos::ev::DragEvent| {
            let n = d_enter.get() + 1;
            d_enter.set(n);
            if is_file_drag(&ev) {
                ev.prevent_default();
                da_enter.set(true);
            }
        }),
        window_event_listener(leptos::ev::dragover, |ev: leptos::ev::DragEvent| {
            if is_file_drag(&ev) {
                ev.prevent_default();
            }
        }),
        window_event_listener(leptos::ev::dragleave, move |_ev: leptos::ev::DragEvent| {
            let n = d_leave.get().saturating_sub(1);
            d_leave.set(n);
            if n == 0 {
                da_leave.set(false);
            }
        }),
        window_event_listener(leptos::ev::drop, move |ev: leptos::ev::DragEvent| {
            if is_file_drag(&ev) {
                ev.prevent_default();
                d_drop.set(0);
                da_drop.set(false);
            }
        }),
    ]);
    // Leptos' window listener handles do NOT unregister when dropped — the
    // handle has to be called. Parking them without this cleanup left four
    // window listeners (and their closures) behind for every owner that ever
    // installed them.
    on_cleanup(move || {
        if let Some(handles) = dom_handles.try_update_value(std::mem::take) {
            for handle in handles {
                handle.remove();
            }
        }
    });

    // Layer 2: Tauri drag lifecycle events. Inside Tauri a native file drag
    // never reaches the DOM (no DOM dragenter/dragover fire on macOS — the
    // drag is handled at the window layer), so the overlay is driven by
    // Tauri's own events: drag-enter shows it, drag-leave hides it, and
    // drag-drop opens the file (and hides it); `tauri_listen` parks each
    // closure so the listener stays registered for the view's lifetime.
    if !pdf_engine::has_tauri() {
        return;
    }

    // drag-enter -> show the overlay; drag-leave -> hide it;
    // drag-drop -> hide the overlay and open the file.
    crate::services::tauri_listen("tauri://drag-enter", move |_ev: Event| drag_active.set(true));
    crate::services::tauri_listen("tauri://drag-leave", move |_ev: Event| drag_active.set(false));
    let drop_sig = drag_active;
    let st = state;
    crate::services::tauri_listen("tauri://drag-drop", move |ev: Event| {
        drop_sig.set(false);
        if let Some(path) = first_drop_path(&ev) {
            crate::services::document::open_path(st, path);
        }
    });
}

/// True when a DOM drag event carries files (vs. dragged text/HTML).
fn is_file_drag(ev: &leptos::ev::DragEvent) -> bool {
    ev.data_transfer()
        .map(|dt| {
            dt.types()
                .iter()
                .any(|t| t.as_string().as_deref() == Some("Files"))
        })
        .unwrap_or(false)
}

/// Extract `payload.paths[0]` from a Tauri v2 `tauri://drag-drop` event object.
///
/// Event shape: `{ event, id, payload: { paths: string[], position: {x,y} } }`.
/// Every access is guarded — a malformed or legacy-format event must not panic.
/// Returns the first dropped file path, or `None` when empty / unreadable.
fn first_drop_path(ev: &Event) -> Option<String> {
    let value: &wasm_bindgen::JsValue = ev.as_ref();
    let payload = js_sys::Reflect::get(value, &"payload".into()).ok()?;
    let paths = js_sys::Reflect::get(&payload, &"paths".into()).ok()?;
    // Reflect::get returns Ok(undefined) for a missing key; `Array::from` would
    // throw on that, so check it's actually an array first.
    if !paths.is_array() {
        return None;
    }
    let arr = js_sys::Array::from(&paths);
    arr.get(0).as_string().filter(|p| !p.is_empty())
}
