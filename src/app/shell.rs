//! The two page shells.
//!
//! The library page and the reader page are not routes of one mounted tree.
//! Each is the whole document. `public/session/boot.ts` picks one before wasm
//! starts, and leaving one for the other is a navigation that drops the
//! previous page's wasm instance. `src/boot.rs` owns that handoff.

use leptos::prelude::*;

use crate::components::app_overlays::drag_overlay::DragOverlay;
use crate::components::app_overlays::toast::ToastHost;
use crate::effects::app::drag_drop::drag_drop;
use crate::features::library::LibraryPage;
use crate::features::reader::ReaderPage;
use crate::state::AppState;

#[component]
pub(crate) fn LibrarySession(state: AppState) -> impl IntoView {
    let drag_active = RwSignal::new(false);
    drag_drop(state, drag_active);

    view! {
        <>
            <LibraryPage state=state />
            <div class="noise-overlay"></div>
            <Show when=move || drag_active.get()>
                <DragOverlay />
            </Show>
            <ToastHost state=state />
        </>
    }
}

#[component]
pub(crate) fn ReaderSession(state: AppState) -> impl IntoView {
    // The handoff mounts from ReaderPage, after `#viewer-slot` is in the
    // document. Applying here raced that div.
    let drag_active = RwSignal::new(false);
    drag_drop(state, drag_active);

    view! {
        <>
            <ReaderPage state=state />
            <div class="noise-overlay"></div>
            <Show when=move || drag_active.get()>
                <DragOverlay />
            </Show>
            <ToastHost state=state />
        </>
    }
}
