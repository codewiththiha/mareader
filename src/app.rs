use leptos::prelude::*;
use leptos_router::components::{Route, Router, Routes};
use leptos_router::hooks::{use_location, use_navigate};

use crate::components::molecules::drag_overlay::DragOverlay;
use crate::components::organisms::toast::ToastHost;
use crate::components::views::library_page::LibraryPage;
use crate::components::views::reader_view::ReaderView;
use crate::core::state::AppState;
use crate::effects::drag_drop::drag_drop;
use crate::effects::link_nav::link_nav;
use crate::effects::selection_pages::selection_pages;
use crate::effects::theme_applier::theme_applier;
use crate::util::storage::{init_storage, load_covers, load_library, load_settings};
use pdf_core::appearance::TextureMode;
use pdf_engine::types::DocStatus;
use pdf_viewer::state::TextureSignal;

#[component]
pub fn App() -> impl IntoView {
    // Storage backend: localStorage today; a SQLite impl lives behind the
    // `sqlite` feature of pdf-storage. One line to swap.
    init_storage(Box::new(pdf_storage::LocalStorage));

    let state = AppState {
        settings: RwSignal::new(load_settings()),
        library: RwSignal::new(load_library()),
        covers: RwSignal::new(load_covers()),
        ..AppState::default()
    };
    provide_context(state);

    // Viewer context: the viewer slice of app state + the texture signal the
    // page hosts need (derived from settings; the viewer never touches
    // settings itself).
    let texture = RwSignal::new(TextureMode::None);
    Effect::new(move || {
        let t = state.settings.get().appearance.texture;
        texture.set(t);
    });
    provide_context(state.viewer_state());
    provide_context(texture as TextureSignal);

    // App-root hooks: theme (both pages), global keyboard shortcuts, internal
    // PDF link jumps, and text-selection page-range tracking.
    theme_applier(state);
    shortcuts(state);
    link_nav(state);
    selection_pages(state);
    // OS file opening: double-click / "Open with" / default-app launch.
    crate::core::open_flow::init_open_file_handling(state);

    view! {
        <Router>
            <AppShell state=state />
        </Router>
        // App-root toast host: fixed overlay, safe outside the toolbar's
        // backdrop-blur stacking context.
        <ToastHost state=state />
    }
}

/// The routed shell: syncs URL with document state, renders the current page,
/// and hosts the app-global overlays (noise + drag feedback).
#[component]
fn AppShell(state: AppState) -> impl IntoView {
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
                    view=move || view! { <ReaderView state=state /> }
                />
            </Routes>
            <div class="noise-overlay"></div>
            <Show when=move || drag_active.get()>
                <DragOverlay />
            </Show>
        </>
    }
}

/// URL follows document state: Ready ⇒ /reader, otherwise /.
///
/// The guard compares the current pathname before navigating, so a completed
/// navigation makes the effect a no-op and it can never loop.
#[component]
fn RouteSync(state: AppState) -> impl IntoView {
    let navigate = use_navigate();
    let loc = use_location();
    Effect::new(move |_| {
        let ready = state.doc.status.get() == DocStatus::Ready;
        let path = loc.pathname.get();
        if ready && path != "/reader" {
            navigate("/reader", Default::default());
        } else if !ready && path == "/reader" {
            navigate("/", Default::default());
        }
    });
}

/// Fallback for unmatched paths: bounce to the library.
#[component]
fn RedirectHome() -> impl IntoView {
    let navigate = use_navigate();
    Effect::new(move |_| navigate("/", Default::default()));
}

/// Global keyboard shortcuts; the open-file action is injected from the app so
/// the viewer crate never depends on app chrome.
fn shortcuts(state: AppState) {
    let open_doc = {

        move || crate::core::open_flow::open_dialog(state)
    };
    pdf_viewer::effects::shortcuts::shortcuts(
        state.viewer_state(),
        open_doc,
    );
}
