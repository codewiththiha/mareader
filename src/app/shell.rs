//! The routed shell: syncs URL with document state, renders the current page,
//! and hosts the app-global overlays (noise + drag feedback).

use leptos::prelude::*;
use leptos_router::components::{Route, Routes};

use crate::components::app_overlays::drag_overlay::DragOverlay;
use crate::effects::app::drag_drop::drag_drop;
use crate::features::library::LibraryPage;
use crate::features::reader::ReaderPage;
use crate::state::AppState;
use super::routes::{RedirectHome, RouteSync};

#[component]
pub(crate) fn AppShell(state: AppState) -> impl IntoView {
    let drag_active = RwSignal::new(false);
    drag_drop(state, drag_active);

    view! {
        <>
            <RouteSync state=state />
            <Routes fallback=move || view! { <RedirectHome /> }>
                <Route
                    path=leptos_router::path!("/")
                    view=move || view! { <LibraryPage state=state /> }
                />
                <Route
                    path=leptos_router::path!("/reader")
                    view=move || view! { <ReaderPage state=state /> }
                />
            </Routes>
            <div class="noise-overlay"></div>
            <Show when=move || drag_active.get()>
                <DragOverlay />
            </Show>
        </>
    }
}
