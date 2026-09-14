//! The library with nothing in it: one import door, and the drop hint beside it. The door is the
//! shared add trigger's empty face (`crate::features::library::add_menu::AddMenuButton`) — the same
//! menu the grid's last cell and the list's last row open.

use leptos::prelude::*;

use crate::features::library::add_menu::{AddFace, AddMenuButton};
use crate::state::AppState;

#[component]
pub(crate) fn EmptyState(state: AppState) -> impl IntoView {
    view! {
        <div class="flex h-full w-full items-center justify-center pt-12 text-muted">
            <AddMenuButton state=state face=AddFace::Empty />
        </div>
    }
}
