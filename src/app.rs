use leptos::prelude::*;

use crate::components::organisms::toast::ToastHost;
use crate::components::views::reader_view::ReaderView;
use crate::core::state::AppState;
use crate::effects::shortcuts::shortcuts;
use crate::effects::theme_ui::theme_ui;
use crate::selftest::selftest;
use crate::util::storage::load_settings;

#[component]
pub fn App() -> impl IntoView {
    let state = AppState {
        settings: RwSignal::new(load_settings()),
        ..AppState::default()
    };
    provide_context(state.clone());

    // Fire the dev self-check after the first frame.
    Effect::new(move |_| {
        selftest();
    });

    // App-root hooks: global keyboard shortcuts + settings-UI glue.
    shortcuts(state);
    theme_ui(state);

    view! {
        <>
            <ReaderView state=state />
            // App-root toast host: fixed overlay, safe outside the toolbar's
            // backdrop-blur stacking context.
            <ToastHost state=state.clone() />
        </>
    }
}
