//! Thumbnails panel host: the absolutely-stacked panel wrapper with its
//! paint/outro toggles, around the reusable `ThumbnailsPanel`.

use leptos::prelude::*;

use crate::components::sidebar::thumbnails::ThumbnailsPanel;
use crate::state::ReaderState;
use crate::state::ui::SidebarMode;
use leptos::prelude::RwSignal;

#[component]
pub(crate) fn SidebarThumbs(
    state: ReaderState,
    sidebar: RwSignal<SidebarMode>,
    live: Signal<bool>,
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
            <ThumbnailsPanel state=state live=live sidebar=sidebar />
        </div>
    }
}
