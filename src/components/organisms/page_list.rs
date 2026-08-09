//! Virtualized scroll container for the continuous layout. OWNED BY branch A
//! (viewer/continuous).

use leptos::prelude::*;

use crate::core::state::AppState;

#[component]
pub fn PageList(state: AppState) -> impl IntoView {
    let _ = state;
    // TODO(branch A): scroll container (#page-list) + spacer + keyed PageCanvas
    // window driven by effects::continuous_scroll.
    view! { <div id="page-list" class="relative h-full w-full overflow-y-auto" /> }
}
