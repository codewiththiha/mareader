use leptos::prelude::*;

use crate::components::organisms::toast::ToastHost;
use crate::components::views::reader_view::ReaderView;
use crate::core::state::AppState;
use crate::effects::link_nav::link_nav;
use crate::effects::selection_pages::selection_pages;
use crate::util::storage::{init_storage, load_covers, load_library, load_settings};
use pdf_core::appearance::TextureMode;
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
    provide_context(pdf_viewer::state::ViewerState::new(
        state.doc,
        state.viewer,
        state.search,
        state.sidebar,
    ));
    provide_context(texture as TextureSignal);

    // App-root hooks: global keyboard shortcuts + internal PDF link jumps +
    // text-selection page-range tracking (for virtualization pinning).
    shortcuts(state);
    link_nav(state);
    selection_pages(state);
    // OS file opening: double-click / "Open with" / default-app launch.
    // Pulls the pending path once (launch-time file) and subscribes to the
    // backend's `pdf-open-file` pings (files opened while running).
    crate::core::open_flow::init_open_file_handling(state);

    view! {
        <>
            <ReaderView state=state />
            // App-root toast host: fixed overlay, safe outside the toolbar's
            // backdrop-blur stacking context.
            <ToastHost state=state />
        </>
    }
}

/// Global keyboard shortcuts; the open-file action is injected from the app so
/// the viewer crate never depends on app chrome.
fn shortcuts(state: AppState) {
    let open_doc = {

        move || crate::core::open_flow::open_dialog(state)
    };
    pdf_viewer::effects::shortcuts::shortcuts(
        pdf_viewer::state::ViewerState::new(state.doc, state.viewer, state.search, state.sidebar),
        open_doc,
    );
}
