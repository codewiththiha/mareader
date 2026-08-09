//! Film-grain noise toggle + intensity. OWNED BY branch D (panels/settings).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn NoiseToggle(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch D): Toggle for settings.noise_enabled + Slider for intensity.
    view! { <div class="inline-flex" /> }
}
