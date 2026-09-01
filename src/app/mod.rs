//! Application root: installs the storage backend, boots the persisted
//! state, provides the app contexts, installs the app-wide effects, and
//! mounts the routed shell.

mod bootstrap;
mod effects;
mod routes;
mod shell;

use leptos::prelude::*;
use leptos_router::components::Router;

use crate::components::overlays::toast::ToastHost;
use bootstrap::{create_app_state, provide_app_contexts};
use effects::install_app_effects;
use shell::AppShell;

#[component]
pub fn App() -> impl IntoView {
    let state = create_app_state();
    provide_context(state);
    let appearance = provide_app_contexts(state);

    // Every app-lifetime effect, in one ordered place: see `app::effects` for
    // the order and what depends on it.
    install_app_effects(state, appearance);

    view! {
        <Router>
            <AppShell state=state />
        </Router>
        // App-root toast host: fixed overlay, safe outside the toolbar's
        // backdrop-blur stacking context.
        <ToastHost state=state />
    }
}
