//! The library with nothing in it: one import door.

use leptos::prelude::*;

use crate::features::library::add_menu::{AddFace, AddMenuButton};

#[component]
pub(crate) fn EmptyState(state: crate::context::LibraryContext) -> impl IntoView {
    view! {
        <div class="flex h-full w-full items-center justify-center pt-12 text-muted">
            <AddMenuButton state=state face=AddFace::Empty />
        </div>
    }
}
