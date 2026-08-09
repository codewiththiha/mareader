use leptos::prelude::*;

use crate::components::views::reader_view::ReaderView;
use crate::core::state::AppState;
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

    view! { <ReaderView state=state /> }
}
