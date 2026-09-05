//! Outline panel host: the absolutely-stacked panel wrapper with its
//! paint/outro toggles, around the reusable [`OutlinePanel`].

use leptos::prelude::*;

use super::OutlinePanel;
use crate::state::ReaderState;
use crate::state::app::SidebarMode;

#[component]
pub(crate) fn SidebarOutline(
    state: ReaderState,
    sidebar: RwSignal<SidebarMode>,
    shown: Signal<bool>,
    outro: Signal<bool>,
    intro: Signal<bool>,
) -> impl IntoView {
    view! {
        <div
            class="sidebar-panel absolute inset-0 flex flex-col"
            class=("invisible", move || !shown.get())
            class=("is-outro", move || outro.get())
            class=("is-intro", move || intro.get())
        >
            <OutlinePanel state=state sidebar=sidebar />
        </div>
    }
}
