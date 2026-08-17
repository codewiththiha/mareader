//! Top-level app view: toolbar + sidebar + viewer slot + status bar + noise
//! overlay. The viewer slot switches on viewer.mode. The real mode match is
//! wired during integration; until then it renders the placeholder.
//!
//! Slot wiring is the SINGLE coordinator's job — branches
//! must not edit this file.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::Event;

use crate::components::atoms::icon::{Icon, IconName};

use crate::core::document::DocStatus;
use crate::core::layout::ViewMode;
use crate::core::state::AppState;
use crate::effects::fit::{fit_effect, zoom_system};
use crate::effects::page_tracking::page_tracking;
use crate::effects::reading_progress::reading_progress;
use crate::effects::theme_applier::theme_applier;

#[component]
pub fn ReaderView(state: AppState) -> impl IntoView {
    theme_applier(state.clone());
    // Fit width / fit page recompute in BOTH view modes (each view reports its
    // container size into the same signal).
    fit_effect(state.clone());
    // Owns display_scale/render_scale during a zoom; every zoom control posts
    // to it via `request_zoom`. Must be wired alongside fit_effect.
    zoom_system(state.clone());
    // Keep `viewer.page` and the scroll position in sync in continuous mode
    // (status-bar counter, page jumps, mode-switch position).
    page_tracking(state.clone());
    // Persist the current book's reading position into the library.
    reading_progress(state.clone());

    // Ends the grace period after a dismissed search: the next scroll, click or
    // keypress drops the muted highlights and empties the query.
    crate::effects::search_effects::dismissed_search_watch(state.clone());
    // Drag-and-drop file open: DOM prevent-default fallback + the authoritative
    // `tauri://drag-drop` subscription. Also drives the drop-feedback overlay
    // via `drag_active`.
    let drag_active = RwSignal::new(false);
    drag_drop(state.clone(), drag_active);

    // Hoist signal handles + owned state clones BEFORE the view! macro. Each
    // `move` closure below captures exactly one owned value, so there is no
    // double-move of `state`.
    let status = state.doc.status;
    let mode = state.viewer.mode;
    let state_toolbar = state.clone();
    let state_sidebar = state.clone();
    let state_status = state.clone();
    let state_single = state.clone();
    let state_cont = state.clone();
    let state_placeholder = state.clone();
    let state_floating = state.clone();

    let is_ready = move || status.get() == DocStatus::Ready;

    view! {
        <div class="relative flex h-full w-full flex-col bg-paper text-ink">
            <header
                class="toolbar-glass absolute inset-x-0 top-0 z-50 border-b border-line/60"
            >
                <crate::components::molecules::toolbar::Toolbar state=state_toolbar />
            </header>
            // No top margin: the viewer starts at the very top so page content
            // scrolls UNDER the translucent header, which is what gives the
            // glass something to refract. Each view pads its own scroller by
            // the toolbar height so the first page is not born behind the bar.
            <div class="flex min-h-0 flex-1">
                <crate::components::organisms::sidebar::Sidebar state=state_sidebar />
                <main id="viewer-slot" class="relative min-w-0 flex-1">
                    <Show
                        when=is_ready
                        fallback=move || {
                            view! {
                                <crate::components::views::library_view::LibraryView state=state_placeholder.clone() />
                            }
                        }
                    >
                        {move || match mode.get() {
                            ViewMode::Single => view! {
                                <crate::components::views::single_page_view::SinglePageView state=state_single.clone() />
                            }
                            .into_any(),
                            ViewMode::Continuous => view! {
                                <crate::components::views::continuous_view::ContinuousView state=state_cont.clone() />
                            }
                            .into_any(),
                        }}
                    </Show>
                    // Floating search overlay (U4): mounted at the viewer slot,
                    // not inside the backdrop-blur header (which would trap
                    // fixed descendants). The slot now starts at the window top
                    // so pages can scroll under the glass, so the panel carries
                    // its own top-14 offset to clear the toolbar.
                    <crate::components::organisms::floating_search::FloatingSearch state=state_floating />
                </main>
            </div>
            <footer class="pointer-events-none absolute inset-x-0 bottom-0 z-50 mix-blend-difference">
                <crate::components::organisms::status_bar::StatusBar state=state_status />
            </footer>
            <div class="noise-overlay"></div>
            // Drop-feedback overlay: appears only while a file drag is over the
            // window (drag_active), themed to follow the current appearance.
            <Show when=move || drag_active.get()>
                <DragOverlay />
            </Show>
        </div>
    }
}

/// Full-screen feedback shown while a file is being dragged over the window.
///
/// Purely visual (`pointer-events: none`), so the window-level drop handlers
/// keep receiving the drag. Themed entirely through the design tokens
/// (`--color-accent`, `--color-paper`, `--color-surface`, `--color-ink`,
/// `--color-muted`), so it follows the active base mode and tint with no extra
/// wiring.
#[component]
fn DragOverlay() -> impl IntoView {
    view! {
        <div class="drag-overlay" role="presentation" aria-hidden="true">
            <div class="drag-dropzone">
                <div class="drag-dropzone-icon">
                    <Icon name=IconName::Drop size=40 />
                </div>
                <p class="drag-dropzone-title">"Drop to open"</p>
                <p class="drag-dropzone-sub">"Release your PDF to start reading"</p>
            </div>
        </div>
    }
}

/// Wire drag-and-drop file opening.
///
/// Two layers:
///   1. DOM prevent-default listeners on `window` (`dragenter` / `dragover` /
///      `drop`) so a dropped file never navigates the webview away — the
///      plain-browser fallback. Dragging from Finder onto the WKWebView only
///      fires these DOM events if Tauri forwards them; the authoritative open
///      comes from layer 2. Scoped to file drags so dragging text/HTML inside
///      the app keeps working. The same listeners drive `drag_active`, which
///      shows the drop-feedback overlay.
///   2. A `tauri://drag-drop` subscription via `bridge::listen`. The handler
///      reads `payload.paths[0]` and routes it through the shared open-flow
///      (`open_flow::open_path`). The Closure is parked in a StoredValue:
///      dropping the JS function would unregister the listener.
fn drag_drop(state: AppState, drag_active: RwSignal<bool>) {
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

    // Layer 2: Tauri drag-drop subscription. Only inside Tauri — the
    // wasm-bindgen shim for `window.__TAURI__.event.listen` throws a TypeError
    // when the global is absent (`trunk serve`), so probe first.
    if !crate::core::bridge::has_tauri() {
        return;
    }
    let handler_handle = StoredValue::new_local(None::<Closure<dyn FnMut(Event)>>);
    let st = state;
    let cb: Closure<dyn FnMut(Event)> = Closure::wrap(
        Box::new(move |ev: Event| {
            if let Some(path) = first_drop_path(&ev) {
                crate::core::open_flow::open_path(st, path);
            }
        }) as Box<dyn FnMut(Event)>,
    );
    let handler: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    spawn_local(async move {
        // The unlisten handle is intentionally discarded: Tauri keeps the
        // listener registered until that fn is called (we never do), and the
        // view lives for the whole app window.
        let _ = crate::core::bridge::listen("tauri://drag-drop", handler).await;
    });
    handler_handle.set_value(Some(cb));
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


