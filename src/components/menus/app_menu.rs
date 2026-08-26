//! More menu (⋯ overflow).
//!
//! Fullscreen (Tauri window API with a browser fallback), Print, a
//! keyboard-shortcuts reference panel, and an About row. The panel renders
//! through the shared window-aware `Popover`, so outside-click/Escape
//! dismissal, viewport clamping, upward flipping and the "keep the titlebar
//! open" hold all come from there.
//!
//! Takes no state: every action here is self-contained, so the menu needs
//! nothing from the app.

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsValue;

use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::icon_button::IconButton;
use crate::components::primitives::menu_item::MenuItem;
use crate::components::app_shell::toolbar_popover::MenuPopover;
use crate::components::primitives::shortcut_row::ShortcutRow;

#[component]
pub fn MoreMenu() -> impl IntoView {
    let open = RwSignal::new(false);
    let (full, set_full) = signal(false);
    let (show_keys, set_show_keys) = signal(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();

    // Fullscreen toggle: prefer the Tauri window handle, fall back to the
    // browser Fullscreen API when running outside Tauri (`trunk serve`).
    // The Tauri global must be probed BEFORE calling bridge::tauri_get_current_window():
    // its wasm-bindgen shim evaluates `window.__TAURI__.window.getCurrentWindow()`,
    // which throws a TypeError when `window.__TAURI__` is absent — so the guard
    // on the returned JsValue alone would never reach the browser fallback.
    let has_tauri = pdf_engine::has_tauri();
    let toggle_fullscreen = move || {
        let next = !full.get();
        if has_tauri {
            let win = pdf_engine::tauri_get_current_window();
            if !(win.is_undefined() || win.is_null()) {
                if let Ok(f) = js_sys::Reflect::get(&win, &JsValue::from_str("setFullscreen"))
                                    && f.is_function()
                                {
                                    let f = js_sys::Function::from(f);
                                    let args = js_sys::Array::new();
                                    args.push(&JsValue::from_bool(next));
                                    _ = js_sys::Reflect::apply(&f, &win, &args);
                                }
                set_full.set(next);
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
            set_full.set(entering);
        }
    };

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <IconButton
                icon=IconName::More
                title="More"
                on_click=move || open.set(!open.get())
            />
            <MenuPopover open=open anchor=root_ref width=256 hold_titlebar=false coordinate_space="toolbar-row" class="p-1".to_string()>
                <MenuItem
                    icon=IconName::Fullscreen
                    label="Fullscreen"
                    on_click=move || toggle_fullscreen()
                >
                    {move || full.get().then(|| view! { <span class="ml-auto text-xs text-muted">"On"</span> })}
                </MenuItem>
                <MenuItem
                    icon=IconName::Print
                    label="Print…"
                    on_click=move || {
                        if let Some(w) = web_sys::window() {
                            _ = w.print();
                        }
                    }
                />
                <MenuItem
                    icon=IconName::Keyboard
                    label="Keyboard Shortcuts"
                    on_click=move || set_show_keys.update(|v| *v = !*v)
                >
                    <Icon name=IconName::ChevronDown size=12 class="ml-auto text-muted" />
                </MenuItem>
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
                    <span class="text-xs text-muted">{
                        if pdf_engine::has_pdf_reader() {
                            format!("v{}", pdf_engine::version())
                        } else {
                            String::new()
                        }
                    }</span>
                </div>
            </MenuPopover>
        </div>
    }
}
