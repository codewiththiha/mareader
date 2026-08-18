//! More menu (⋯ overflow). OWNED BY U7 (phase 3).
//!
//! Fullscreen (Tauri window API with a browser fallback), Print, a
//! keyboard-shortcuts reference panel, and an About row. The popover carries
//! `.menu-popover` so it reverts the `.toolbar-glass` mix-blend glyph rule and
//! gets the shared popover entrance animation. Outside-click + Escape dismiss
//! it (self-contained window listeners, removed on cleanup) — the same pattern
//! the zoom/appearance popovers use, which also gives menu-exclusivity.

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsValue;

use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::kbd::Kbd;
use pdf_engine::bridge;
use crate::core::state::AppState;

/// One keyboard-shortcut reference row: label on the left, keycaps on the right.
#[component]
fn ShortcutRow(label: &'static str, keys: Vec<&'static str>) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between gap-2 px-1 py-0.5">
            <span class="text-xs text-muted">{label}</span>
            <span class="flex gap-0.5">
                {keys.into_iter().map(|k| view! { <Kbd>{k}</Kbd> }).collect_view()}
            </span>
        </div>
    }
}

#[component]
pub fn MoreMenu(state: AppState) -> impl IntoView {
    // AppState is kept for signature parity with the other toolbar menus (the
    // appearance menu consumes it); every item here is self-contained.
    _ = state;

    let open = RwSignal::new(false);
    let full = RwSignal::new(false);
    let show_keys = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();

    // Fullscreen toggle: prefer the Tauri window handle, fall back to the
    // browser Fullscreen API when running outside Tauri (`trunk serve`).
    // The Tauri global must be probed BEFORE calling bridge::tauri_get_current_window():
    // its wasm-bindgen shim evaluates `window.__TAURI__.window.getCurrentWindow()`,
    // which throws a TypeError when `window.__TAURI__` is absent — so the guard
    // on the returned JsValue alone would never reach the browser fallback.
    let has_tauri = pdf_engine::bridge::has_tauri();
    let toggle_fullscreen = move || {
        let next = !full.get();
        if has_tauri {
            let win = pdf_engine::bridge::tauri_get_current_window();
            if !(win.is_undefined() || win.is_null()) {
                if let Ok(f) = js_sys::Reflect::get(&win, &JsValue::from_str("setFullscreen"))
                                    && f.is_function()
                                {
                                    let f = js_sys::Function::from(f);
                                    let args = js_sys::Array::new();
                                    args.push(&JsValue::from_bool(next));
                                    _ = js_sys::Reflect::apply(&f, &win, &args);
                                }
                full.set(next);
                return;
            }
        }
        // Browser fallback. Read the ACTUAL fullscreen state (not `full`) so an
        // exit via browser Esc / window chrome keeps the toggle direction right.
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            let entering = doc.fullscreen_element().is_none();
            if entering {
                if let Some(el) = doc.document_element() {
                    _ = el.request_fullscreen();
                }
            } else {
                doc.exit_fullscreen();
            }
            full.set(entering);
        }
    };

    // Outside-click dismiss: while open, any pointerdown outside the root node
    // closes the popover. Re-registered per open-flip, removed on cleanup.
    Effect::new(move |_| {
        if open.get() {
            let handle = window_event_listener(
                leptos::ev::pointerdown,
                move |ev: leptos::ev::PointerEvent| {
                    let target: web_sys::Node = event_target(&ev);
                    let contains = root_ref
                        .get()
                        .as_ref()
                        .is_some_and(|c| c.contains(Some(&target)));
                    if !contains {
                        open.set(false);
                    }
                },
            );
            on_cleanup(move || handle.remove());
        }
    });

    // Escape dismiss: same window-listener lifecycle.
    Effect::new(move |_| {
        if open.get() {
            let handle = window_event_listener(
                leptos::ev::keydown,
                move |ev: leptos::ev::KeyboardEvent| {
                    if ev.key() == "Escape" {
                        open.set(false);
                    }
                },
            );
            on_cleanup(move || handle.remove());
        }
    });

    let item_class =
        "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink hover:bg-line";
    let icon_slot = "inline-flex w-4 shrink-0 justify-center text-muted";

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <button
                type="button"
                title="More"
                on:click=move |_| open.set(!open.get())
                class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-transparent bg-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
                <Icon name=IconName::More size=16 />
            </button>
            <Show when=move || open.get()>
                <div class="menu-popover absolute right-0 top-full z-50 mt-1 w-64 rounded-lg border border-line bg-surface p-1 shadow-lg">
                    <button
                        type="button"
                        on:click=move |_| toggle_fullscreen()
                        class=item_class
                    >
                        <span class=icon_slot><Icon name=IconName::Fullscreen size=14 /></span>
                        <span>"Fullscreen"</span>
                        {move || full.get().then(|| view! { <span class="ml-auto text-xs text-muted">"On"</span> })}
                    </button>
                    <button
                        type="button"
                        on:click=move |_| {
                            if let Some(w) = web_sys::window() {
                                _ = w.print();
                            }
                        }
                        class=item_class
                    >
                        <span class=icon_slot><Icon name=IconName::Print size=14 /></span>
                        <span>"Print…"</span>
                    </button>
                    <button
                        type="button"
                        on:click=move |_| show_keys.update(|v| *v = !*v)
                        class=item_class
                    >
                        <span class=icon_slot><Icon name=IconName::Keyboard size=14 /></span>
                        <span>"Keyboard Shortcuts"</span>
                        <svg
                            class="ml-auto text-muted"
                            width="12"
                            height="12"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                        >
                            <path d="m6 9 6 6 6-6"/>
                        </svg>
                    </button>
                    <Show when=move || show_keys.get()>
                        <div class="mt-1 max-h-56 overflow-y-auto border-t border-line pt-1">
                            <ShortcutRow label="Open…" keys=vec!["⌘", "O"] />
                            <ShortcutRow label="Search…" keys=vec!["⌘", "F"] />
                            <ShortcutRow label="Fit width" keys=vec!["⌘", "0"] />
                            <ShortcutRow label="Single view" keys=vec!["⌘", "1"] />
                            <ShortcutRow label="Continuous view" keys=vec!["⌘", "2"] />
                            <ShortcutRow label="Zoom in" keys=vec!["+"] />
                            <ShortcutRow label="Zoom out" keys=vec!["−"] />
                            <ShortcutRow label="Prev / Next page" keys=vec!["←", "→"] />
                            <ShortcutRow label="Page up / down (Single)" keys=vec!["↑", "↓"] />
                            <ShortcutRow label="Dismiss" keys=vec!["Esc"] />
                        </div>
                    </Show>
                    <div class="mt-1 flex items-center justify-between border-t border-line px-1 py-1">
                        <span class="text-xs text-muted">"PDF Reader"</span>
                        <span class="text-xs text-muted">{format!("v{}", bridge::version())}</span>
                    </div>
                </div>
            </Show>
        </div>
    }
}
