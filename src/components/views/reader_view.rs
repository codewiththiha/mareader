//! The `/reader` route: sidebar + viewer slot + the shared titlebar (via the
//! chrome `TitleBarProvider`), plus the floating doc title, page pill, bottom
//! bar and floating search. The viewer slot switches on viewer.mode.
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

use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::segmented::{Segmented, SegmentedLabel};
use pdf_viewer::components::atoms::separator::Separator;
use pdf_viewer::components::atoms::tooltip::Tooltip;

use pdf_engine::types::DocStatus;
use pdf_core::layout::ViewMode;
use crate::components::chrome::floating_doc_title::FloatingDocTitle;
use crate::components::chrome::titlebar_provider::TitleBarProvider;
use crate::components::molecules::appearance_menu::AppearanceMenu;
use crate::components::molecules::doc_title::DocTitle;
use crate::components::molecules::more_menu::MoreMenu;
use crate::components::molecules::zoom_controls::ZoomControls;
use crate::core::open_flow::{close_document, open_dialog};
use crate::core::state::AppState;
use crate::effects::reading_progress::reading_progress;
use pdf_viewer::effects::fit::{fit_effect, zoom_system};
use pdf_viewer::effects::page_tracking::page_tracking;
use pdf_viewer::state::SidebarMode;

#[component]
pub fn ReaderView(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reusable viewer components
    // and effects (all field paths match the app-level state).
    let vs = state.viewer_state();
    fit_effect(vs);
    zoom_system(vs);
    page_tracking(vs);
    reading_progress(state);
    pdf_viewer::effects::search_effects::dismissed_search_watch(vs);

    let status = state.doc.status;
    let mode = state.viewer.mode;
    let state_sidebar = state;
    let is_ready = move || status.get() == DocStatus::Ready;

    // LEFT slot: sidebar toggle (only while the sidebar is closed — the
    // sidebar's own chrome row owns it otherwise), Library, Open, DocTitle.
    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id="toolbar-left-pre"
                    data-tauri-drag-region="true"
                    class="flex shrink-0 items-center gap-1"
                >
                    <Show when=move || state.sidebar.get() == SidebarMode::None>
                        <Tooltip text="Toggle sidebar".to_string()>
                            <Button
                                on_click=move |_| state.sidebar.set(SidebarMode::Thumbs)
                                kind=ButtonKind::Ghost
                                icon=IconName::Sidebar
                                title="Toggle sidebar".to_string()
                            />
                        </Tooltip>
                    </Show>
                    <Show when=move || {
                        matches!(
                            state.doc.status.get(),
                            DocStatus::Ready | DocStatus::Opening
                        )
                    }>
                        <Tooltip text="Library".to_string()>
                            <Button
                                on_click=move |_| close_document(state)
                                kind=ButtonKind::Ghost
                                icon=IconName::Library
                                title="Close this book and return to the library".to_string()
                            />
                        </Tooltip>
                    </Show>
                    <Tooltip text="Open PDF (Cmd/Ctrl+O)".to_string()>
                        <Button
                            on_click=move |_| open_dialog(state)
                            kind=ButtonKind::Toolbar
                            icon=IconName::Open
                            label="Open".to_string()
                            title="Open PDF (Cmd/Ctrl+O)".to_string()
                        />
                    </Tooltip>
                </div>
                <DocTitle state=state />
            </div>
        }
    };

    // RIGHT slot: search, view mode, zoom, appearance, more.
    let right = move || {
        view! {
            <div
                id="toolbar-right"
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
                <Tooltip text="Search (Cmd/Ctrl+F)".to_string()>
                    <button
                        type="button"
                        data-search-chrome="true"
                        title="Search (Cmd/Ctrl+F)"
                        on:pointerdown=move |ev| ev.stop_propagation()
                        on:click=move |_| {
                            if state.search.visible.get() {
                                pdf_viewer::effects::search_effects::dismiss_search(vs);
                            } else {
                                pdf_viewer::effects::search_effects::resume_search(vs);
                            }
                        }
                        class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-transparent bg-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    >
                        <Icon name=IconName::Search size=18 />
                    </button>
                </Tooltip>
                <Tooltip text="View mode".to_string()>
                    <Segmented
                        options=vec![
                            (
                                ViewMode::Single,
                                SegmentedLabel::Icon(IconName::SinglePage),
                                "Single page view",
                            ),
                            (
                                ViewMode::Continuous,
                                SegmentedLabel::Icon(IconName::Continuous),
                                "Continuous scroll view",
                            ),
                        ]
                        value={mode.read_only()}
                        on_change=move |m: ViewMode| state.viewer.mode.set(m)
                    />
                </Tooltip>
                <ZoomControls state=state />
                <Separator vertical=true />
                <AppearanceMenu state=state />
                <MoreMenu state=state />
            </div>
        }
    };

    view! {
        <TitleBarProvider state=state left=left right=right>
            // overflow-hidden clips the hidden BottomBar's slide-down translate
            // so it can never leak a phantom scrollbar onto the window.
            <div class="relative flex h-full w-full flex-col overflow-hidden bg-paper text-ink">
                <div class="flex min-h-0 flex-1">
                    <crate::components::organisms::sidebar::Sidebar state=state_sidebar />
                    <main id="viewer-slot" class="relative min-w-0 flex-1 overflow-hidden">
                        <Show when=is_ready>
                            {move || match mode.get() {
                                ViewMode::Single => view! {
                                    <pdf_viewer::components::single_page_view::SinglePageView state=vs />
                                }
                                .into_any(),
                                ViewMode::Continuous => view! {
                                    <pdf_viewer::components::continuous_view::ContinuousView state=vs />
                                }
                                .into_any(),
                            }}
                        </Show>
                        <FloatingDocTitle state=state />
                        <crate::components::molecules::page_pill::PagePill state=state />
                        <crate::components::molecules::bottom_bar::BottomBar state=state />
                        // Floating search overlay (U4): mounted at the viewer
                        // slot; its top-14 offset clears the titlebar.
                        <pdf_viewer::components::floating_search::FloatingSearch state=vs />
                    </main>
                </div>
            </div>
        </TitleBarProvider>
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
pub(crate) fn DragOverlay() -> impl IntoView {
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
///      plain-browser fallback (`trunk serve`). Inside Tauri a native file
///      drag never reaches the DOM (no dragenter/dragover fire on macOS), so
///      these are inert there and the overlay is driven by layer 2 instead.
///   2. Tauri drag lifecycle subscriptions (`tauri://drag-enter`,
///      `tauri://drag-leave`, `tauri://drag-drop`). These ARE the drag
///      signals for a real file drag from Finder/Explorer: enter shows the
///      drop-feedback overlay, leave hides it, drop opens the file (and hides
///      it). Each Closure is parked in a StoredValue so the listener stays
///      registered for the view's lifetime.
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
    if !pdf_engine::bridge::has_tauri() {
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
        _ = pdf_engine::bridge::listen("tauri://drag-enter", f_enter).await;
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
        _ = pdf_engine::bridge::listen("tauri://drag-leave", f_leave).await;
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
                crate::core::open_flow::open_path(st, path);
            }
        }) as Box<dyn FnMut(Event)>,
    );
    let f_drop: js_sys::Function = cb_drop.as_ref().unchecked_ref::<js_sys::Function>().clone();
    spawn_local(async move {
        _ = pdf_engine::bridge::listen("tauri://drag-drop", f_drop).await;
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
