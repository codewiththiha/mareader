//! Toast notification host. Stub only — the body is filled in a later phase; it
//! must simply compile with zero warnings until then.

use leptos::prelude::*;

use crate::core::state::AppState;

#[allow(dead_code)] // consumed in phase 5
#[component]
pub fn ToastHost(state: AppState) -> impl IntoView {
    // Param intentionally unused until the toast host body lands.
    let _ = state;
    view! {}
}
