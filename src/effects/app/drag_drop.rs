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
//!      it). Each Closure is parked in a StoredValue so the listener stays
//!      registered for the view's lifetime.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
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
    let _dom_handles = StoredValue::new_local(vec![
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

    // Layer 2: Tauri drag lifecycle events. Inside Tauri a native file drag
    // never reaches the DOM (no DOM dragenter/dragover fire on macOS — the
    // drag is handled at the window layer), so the overlay is driven by
    // Tauri's own events: drag-enter shows it, drag-leave hides it, and
    // drag-drop opens the file (and hides it). Each Closure is parked so the
    // listener stays registered for the view's lifetime.
    if !pdf_engine::has_tauri() {
        return;
    }

    // drag-enter -> show the overlay.
    let enter_handle = StoredValue::new_local(None::<Closure<dyn FnMut(Event)>>);
    let enter_sig = drag_active;
    let cb_enter: Closure<dyn FnMut(Event)> = Closure::wrap(
        Box::new(move |_ev: Event| enter_sig.set(true)) as Box<dyn FnMut(Event)>,
    );
    let f_enter: js_sys::Function = cb_enter.as_ref().unchecked_ref::<js_sys::Function>().clone();
    spawn_local(async move {
        // The unlisten handle is intentionally discarded: Tauri keeps the
        // listener registered until that fn is called (we never do), and the
        // view lives for the whole app window.
        _ = pdf_engine::listen("tauri://drag-enter", f_enter).await;
    });
    enter_handle.set_value(Some(cb_enter));

    // drag-leave -> hide the overlay.
    let leave_handle = StoredValue::new_local(None::<Closure<dyn FnMut(Event)>>);
    let leave_sig = drag_active;
    let cb_leave: Closure<dyn FnMut(Event)> = Closure::wrap(
        Box::new(move |_ev: Event| leave_sig.set(false)) as Box<dyn FnMut(Event)>,
    );
    let f_leave: js_sys::Function = cb_leave.as_ref().unchecked_ref::<js_sys::Function>().clone();
    spawn_local(async move {
        _ = pdf_engine::listen("tauri://drag-leave", f_leave).await;
    });
    leave_handle.set_value(Some(cb_leave));

    // drag-drop -> hide the overlay and open the file.
    let drop_handle = StoredValue::new_local(None::<Closure<dyn FnMut(Event)>>);
    let drop_sig = drag_active;
    let st = state;
    let cb_drop: Closure<dyn FnMut(Event)> = Closure::wrap(
        Box::new(move |ev: Event| {
            drop_sig.set(false);
            if let Some(path) = first_drop_path(&ev) {
                crate::services::document::open_path(st, path);
            }
        }) as Box<dyn FnMut(Event)>,
    );
    let f_drop: js_sys::Function = cb_drop.as_ref().unchecked_ref::<js_sys::Function>().clone();
    spawn_local(async move {
        _ = pdf_engine::listen("tauri://drag-drop", f_drop).await;
    });
    drop_handle.set_value(Some(cb_drop));
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
