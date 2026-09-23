//! Thumbnails panel host: the absolutely-stacked panel wrapper, and the empty
//! scroller the PDF instance fills. The host does not render cells. A cell
//! that called the engine would build engine state in this heap.

use leptos::prelude::*;

use crate::state::ReaderState;
use crate::state::app::SidebarMode;

#[component]
pub(crate) fn SidebarThumbs(
    state: ReaderState,
    sidebar: RwSignal<SidebarMode>,
    live: Signal<bool>,
    shown: Signal<bool>,
    outro: Signal<bool>,
    intro: Signal<bool>,
) -> impl IntoView {
    let _ = (sidebar, live);
    view! {
        <div
            class="sidebar-panel absolute inset-0 flex flex-col"
            class=("invisible", move || !shown.get())
            class=("is-outro", move || outro.get())
            class=("is-intro", move || intro.get())
        >
            <Show when=move || !state.reflowable()>
                <div
                    id="thumb-scroll"
                    data-thumb-mount=""
                    data-thumb-scale={super::geometry::THUMB_SCALE.to_string()}
                    class="relative flex-1 overflow-y-auto p-3"
                ></div>
            </Show>
        </div>
    }
}
