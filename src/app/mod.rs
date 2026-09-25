//! Application root: installs the storage backend, boots the persisted
//! state, provides the app contexts, installs the app-wide effects, and
//! mounts the routed shell.

mod bootstrap;
mod effects;
mod routes;
mod shell;

use leptos::prelude::*;
use leptos_router::components::Router;

use crate::components::app_overlays::toast::ToastHost;
use bootstrap::{create_app_state, provide_app_contexts};
use effects::install_app_effects;
use shell::AppShell;

#[component]
pub fn App() -> impl IntoView {
    let state = create_app_state();
    provide_context(state);
    let (appearance, typography) = provide_app_contexts(state);

    // The dev diagnostics surface (window.__mareaderDiagnostics): one probe
    // for the lifecycle/memory baseline. The "is a reader live" half reads
    // the document status, the one authoritative bit.
    crate::diagnostics::install(
        move || state.reader.document.status.get_untracked() != pdf_engine::types::DocStatus::Idle,
        move || format!("{:?}", state.reader.document.status.get_untracked()),
        move || state.reader.document.error.get_untracked(),
    );

    // Every app-lifetime effect, in one ordered place: see `app::effects` for
    // the order and what depends on it.
    install_app_effects(state, appearance, typography);

    // The browser-only test hook (`?open=`, `?blend=`): inert in the
    // packaged app, so it can sit behind the effects it rides.
    crate::services::web_params::init(state);

    view! {
        <Router>
            <AppShell state=state />
        </Router>
        // App-root toast host: fixed overlay, safe outside the toolbar's
        // backdrop-blur stacking context.
        <ToastHost state=state />
    }
}
