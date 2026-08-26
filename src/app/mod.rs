//! Application root: installs the storage backend, boots the persisted
//! state, provides the app contexts, installs the app-wide effects, and
//! mounts the routed shell.

mod bootstrap;
mod routes;
mod shell;

use leptos::prelude::*;
use leptos_router::components::Router;

use crate::components::overlays::toast::ToastHost;
use crate::effects::reader::link_navigation::link_navigation;
use crate::effects::reader::page_selection::page_selection;
use crate::effects::reader::text_selection::text_selection;
use crate::effects::app::theme::apply_theme;
use crate::state::AppState;
use bootstrap::{create_app_state, provide_app_contexts};
use shell::AppShell;

#[component]
pub fn App() -> impl IntoView {
    let state = create_app_state();
    provide_context(state);
    provide_app_contexts(state);

    // App-root hooks: theme (both pages), global keyboard shortcuts, internal
    // PDF link jumps, and text-selection tracking (page-range pinning for
    // virtualization, plus the AI selection detail).
    apply_theme(state);
    shortcuts(state);
    link_navigation(state);
    page_selection(state);
    text_selection(state);
    // ONE Tauri AI-chunk listener for the app's life; re-broadcasts as a
    // window event so the gloss popover never stacks/drops handlers across
    // document switches.
    crate::services::ai::install_ai_chunk_bridge();
    // OS file opening: double-click / "Open with" / default-app launch.
    crate::services::document::init_open_file_handling(state);

    view! {
        <Router>
            <AppShell state=state />
        </Router>
        // App-root toast host: fixed overlay, safe outside the toolbar's
        // backdrop-blur stacking context.
        <ToastHost state=state />
    }
}

/// Global keyboard shortcuts; the open-file action is injected from the app so
/// the viewer crate never depends on app chrome.
fn shortcuts(state: AppState) {
    let open_doc = {
        move || crate::services::document::open_dialog(state)
    };
    crate::effects::app::shortcuts::shortcuts(
        state.reader,
        open_doc,
        state.ui.sidebar,
    );
}
