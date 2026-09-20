//! Independent Leptos roots. Host JavaScript, not a router, owns lifetimes.
mod bootstrap;
mod effects;
#[cfg(feature = "library")]
mod workspace;

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use crate::state::AppState;
use crate::components::app_overlays::toast::ToastHost;

#[cfg(feature = "library")]
pub fn mount_library() {
    when_connected(mount_connected_library);
}

#[cfg(feature = "library")]
fn mount_connected_library() {
    console_error_panic_hook::set_once();
    let root = document().get_element_by_id("library-root").expect("library mount target")
        .unchecked_into::<web_sys::HtmlElement>();
    let handle = leptos::mount::mount_to(root, || {
        let state = bootstrap::create_app_state();
        provide_context(state);
        let (appearance, typography) = bootstrap::provide_app_contexts(state);
        effects::install_library_effects(state, appearance, typography);
        crate::runtime::library::install(state);
        crate::memory::log_heap("boot");
        let drag_active = RwSignal::new(false);
        crate::effects::app::drag_drop::drag_drop(state, drag_active);
        view! {
            <crate::features::library::LibraryPage state=state />
            <ToastHost state=state />
            <div class="noise-overlay"></div>
            <Show when=move || drag_active.get()>
                <crate::components::app_overlays::drag_overlay::DragOverlay />
            </Show>
        }
    });
    crate::runtime::retain_mount(move || drop(handle));
}

pub fn mount_reader(format: &'static str) {
    when_connected(move || mount_connected_reader(format));
}

fn when_connected(mount: impl FnOnce() + 'static) {
    // If the host already connected (race where iframe load + postMessage beats WASM init),
    // mount immediately; otherwise wait for the connected event.
    let already_connected = js_sys::Reflect::get(&web_sys::window().unwrap(), &"__MAREADER_CONNECTED__".into())
        .map(|v| v.as_bool().unwrap_or(false))
        .unwrap_or(false);
    if already_connected {
        mount();
        return;
    }
    // Trunk initializes WASM before iframe load. Mount only once the host's
    // MessagePort and Tauri proxy exist, without delaying the load event.
    let callback = wasm_bindgen::closure::Closure::once_into_js(mount);
    let options = web_sys::AddEventListenerOptions::new();
    options.set_once(true);
    let _ = window().add_event_listener_with_callback_and_add_event_listener_options(
        crate::events::CONNECTED_EVENT, callback.unchecked_ref(), &options,
    );
}

fn mount_connected_reader(format: &'static str) {
    console_error_panic_hook::set_once();
    crate::runtime::mark_reader(format);
    let root = document().get_element_by_id("reader-app").expect("reader mount target")
        .unchecked_into::<web_sys::HtmlElement>();
    let handle = leptos::mount::mount_to(root, move || {
        let state = AppState::default();
        provide_context(state);
        let (appearance, typography) = bootstrap::provide_app_contexts(state);
        effects::install_reader_effects(state, appearance, typography);
        provide_context(crate::runtime::ChromeVisibility { bar: RwSignal::new(false), rail: RwSignal::new(false) });
        crate::runtime::reader::install(state, format);
        view! {
            <crate::features::reader::ReaderSurface state=state />
            <ToastHost state=state />
            <div class="noise-overlay"></div>
        }
    });
    crate::runtime::retain_mount(move || drop(handle));
}

#[cfg(feature = "library")]
pub fn mount_workspace() {
    console_error_panic_hook::set_once();
    crate::runtime::mark_workspace();
    let root = document().get_element_by_id("workspace-root").expect("workspace mount target")
        .unchecked_into::<web_sys::HtmlElement>();
    leptos::mount::mount_to(root, || {
        let state = AppState { settings: RwSignal::new(crate::storage::load_settings()), ..AppState::default() };
        provide_context(state);
        let (appearance, typography) = bootstrap::provide_app_contexts(state);
        effects::install_workspace_effects(state, appearance, typography);
        crate::runtime::workspace::install(state);
        view! { <workspace::WorkspaceShell state=state /><ToastHost state=state /><div class="noise-overlay" /> }
    }).forget();
}
