//! Merged appearance menu (theme/texture/noise). Stub only — the body is filled
//! in a later phase; it must simply compile with zero warnings until then.

use leptos::prelude::*;

use crate::core::state::AppState;

#[allow(dead_code)] // consumed in phase 4
#[component]
pub fn AppearanceMenu(state: AppState) -> impl IntoView {
    // Param intentionally unused until the menu body lands.
    let _ = state;
    view! {}
}
