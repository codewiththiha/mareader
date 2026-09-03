//! Drag-and-drop file opening, and the feedback overlay that goes with it.
//!
//! The overlay is shown for exactly one thing: a drag that came from OUTSIDE
//! the window and is carrying a document the reader can open. Two things
//! used to fool it and are ruled out here —
//!
//!   * A drag that STARTED inside the window (a text selection, a page image,
//!     a book card). Both the DOM and Tauri report it as a drag entering the
//!     window; the `dragstart`/`dragend` pair on the window is what tells the
//!     two apart, and while it is open every enter is ignored.
//!   * A drag of something else (a PNG, a folder, a URL). The DOM drag names
//!     its items' kinds and MIME types up front; Tauri names the paths. Each
//!     is checked against `pdf_core::documents`, the one registry of what the
//!     reader opens, so a new format is a new row there and nothing here.
//!
//! Two transports feed the same decision:
//!   1. DOM `dragenter` / `dragover` / `dragleave` / `drop` on `window` — the
//!      plain-browser path (`trunk serve`), which also has to prevent the
//!      default navigation on drop. Inside Tauri a native file drag never
//!      reaches the DOM, so these only ever see internal drags there.
//!   2. Tauri's `tauri://drag-enter` / `drag-leave` / `drag-drop`, the real
//!      signals for a file dragged in from Finder / Explorer.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use pdf_core::documents;
use web_sys::Event;

use crate::state::AppState;

pub(crate) fn drag_drop(state: AppState, drag_active: RwSignal<bool>) {
    // A drag that began in this window is never a drop candidate. `dragstart`
    // fires for every internal drag, native or not, and `dragend` closes it
    // even when the item is released outside the window.
    let internal = Rc::new(Cell::new(false));
    // DOM `dragenter`/`dragleave` fire for every child boundary crossed, not
    // just the window edge, so the overlay tracks a depth count rather than
    // the last event.
    let depth = Rc::new(Cell::new(0u32));

    let dom_handles = StoredValue::new_local({
        let (i_start, i_end, i_enter) = (internal.clone(), internal.clone(), internal.clone());
        let (d_enter, d_leave, d_drop) = (depth.clone(), depth.clone(), depth);
        vec![
            window_event_listener(leptos::ev::dragstart, move |_| i_start.set(true)),
            window_event_listener(leptos::ev::dragend, move |_| i_end.set(false)),
            window_event_listener(leptos::ev::dragenter, move |ev: leptos::ev::DragEvent| {
                if i_enter.get() || !carries_supported_file(&ev) {
                    return;
                }
                ev.prevent_default();
                d_enter.set(d_enter.get() + 1);
                drag_active.set(true);
            }),
            window_event_listener(leptos::ev::dragover, |ev: leptos::ev::DragEvent| {
                if carries_supported_file(&ev) {
                    ev.prevent_default();
                }
            }),
            window_event_listener(leptos::ev::dragleave, move |_| {
                if d_leave.get() == 0 {
                    return; // a drag we never admitted
                }
                let n = d_leave.get() - 1;
                d_leave.set(n);
                if n == 0 {
                    drag_active.set(false);
                }
            }),
            window_event_listener(leptos::ev::drop, move |ev: leptos::ev::DragEvent| {
                // Always swallow the navigation; only an admitted drag was
                // ever shown, so the reset is harmless otherwise.
                ev.prevent_default();
                d_drop.set(0);
                drag_active.set(false);
            }),
        ]
    });
    // Leptos' window listener handles do NOT unregister when dropped — the
    // handle has to be called, or every owner that installed them leaks a set.
    on_cleanup(move || {
        if let Some(handles) = dom_handles.try_update_value(std::mem::take) {
            for handle in handles {
                handle.remove();
            }
        }
    });

    if !tauri_bridge::has_tauri() {
        return;
    }

    // Tauri reports an internal drag as an enter with NO paths, and a foreign
    // one with the paths it carries; both go through the same admission.
    let admit = {
        let internal = internal.clone();
        move |ev: &Event| !internal.get() && drop_paths(ev).iter().any(|p| documents::is_supported_path(p))
    };
    let enter_admit = admit.clone();
    crate::services::tauri_listen("tauri://drag-enter", move |ev: Event| {
        if enter_admit(&ev) {
            drag_active.set(true);
        }
    });
    crate::services::tauri_listen("tauri://drag-leave", move |_ev: Event| drag_active.set(false));
    crate::services::tauri_listen("tauri://drag-drop", move |ev: Event| {
        drag_active.set(false);
        if internal.get() {
            return;
        }
        let paths = drop_paths(&ev);
        if let Some(path) = documents::first_supported(paths.iter().map(String::as_str)) {
            crate::services::document::open_path(state, path.to_string());
        }
    });
}

/// Whether a DOM drag carries at least one FILE whose advertised type may be
/// a supported document. Dragged text, links and markup have no file items
/// and are refused outright; a file of a known-unsupported type (an image)
/// is refused by its MIME type. The path is not known until the drop, so a
/// blank type is admitted and the drop is what decides.
fn carries_supported_file(ev: &leptos::ev::DragEvent) -> bool {
    let Some(items) = ev.data_transfer().map(|dt| dt.items()) else {
        return false;
    };
    (0..items.length())
        .filter_map(|i| items.get(i))
        .filter(|item| item.kind() == "file")
        .any(|item| documents::is_supported_mime(&item.type_()))
}

/// `payload.paths` of a Tauri v2 drag event (`drag-enter` and `drag-drop`
/// both carry it). Every access is guarded — a malformed or legacy event
/// yields an empty list rather than a panic.
fn drop_paths(ev: &Event) -> Vec<String> {
    let value: &wasm_bindgen::JsValue = ev.as_ref();
    let Ok(payload) = js_sys::Reflect::get(value, &"payload".into()) else {
        return Vec::new();
    };
    let Ok(paths) = js_sys::Reflect::get(&payload, &"paths".into()) else {
        return Vec::new();
    };
    if !paths.is_array() {
        return Vec::new();
    }
    js_sys::Array::from(&paths)
        .iter()
        .filter_map(|p| p.as_string())
        .filter(|p| !p.is_empty())
        .collect()
}
