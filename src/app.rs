use leptos::prelude::*;

use crate::components::organisms::toast::ToastHost;
use crate::components::views::reader_view::ReaderView;
use crate::core::state::AppState;
use crate::effects::shortcuts::shortcuts;
use crate::effects::link_nav::link_nav;
use crate::util::storage::load_settings;

#[component]
pub fn App() -> impl IntoView {
    let state = AppState {
        settings: RwSignal::new(load_settings()),
        ..AppState::default()
    };
    provide_context(state.clone());

    // App-root hooks: global keyboard shortcuts + internal PDF link jumps.
    // (The old `theme_ui` hook was removed with the appearance refactor — it
    // only logged a sidebar transition and held a subscription to settings
    // fields that no longer exist.)
    shortcuts(state);
    link_nav(state);

    view! {
        <>
            <ReaderView state=state />
            // App-root toast host: fixed overlay, safe outside the toolbar's
            // backdrop-blur stacking context.
            <ToastHost state=state.clone() />
        </>
    }
}
