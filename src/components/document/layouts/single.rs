//! Single-page (`single`) layout: one centered PageCanvas in a scroll
//! container. `page` is remounted per page by a keyed `<For>` so each page
//! turn gets a fresh host (and the engine re-registers the canvas id); page
//! turns are instant — the reader's motion principles forbid entrance
//! animations on document content.

use leptos::prelude::*;

use crate::components::document::page_canvas::component::GlossOverlayProps;
use crate::components::document::shells::page_shell::PageShell;
use crate::components::document::PageCanvas;
use crate::components::primitives::hooks::dom::SINGLE_PAGE_CONTAINER_ID;
use crate::state::{ReaderState, TextureSignal};

#[component]
pub fn SingleLayout(
    state: ReaderState,
    #[prop(into)]
    progress_visible: Signal<bool>,
) -> impl IntoView {
    let texture =
        use_context::<TextureSignal>().expect("TextureSignal must be provided by app bootstrap");
    // Hosts live at the live display scale; the crisp raster follows
    // `render_scale`, which only moves when a zoom lands.
    let page_scale = state.viewer.zoom.display.read_only();
    let gesture_owns = state.viewer.gesture_owns();

    view! {
        <PageShell state=state scroller_id=SINGLE_PAGE_CONTAINER_ID progress_visible=progress_visible>
            <For
                each=move || std::iter::once(state.viewer.page.get())
                key=|p: &u32| *p
                children=move |page: u32| {
                    view! {
                        <PageCanvas
                            page=page
                            scale=page_scale
                            render_scale=state.viewer.zoom.committed
                            zoom_animating=state.viewer.zooming()
                            gesture_owns=gesture_owns
                            texture=texture
                            canvas_id=format!("sp-{page}-cv")
                            host_id=format!("sp-{page}-pg")
                            render_text=true
                            gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
                        />
                    }
                }
            />
        </PageShell>
    }
}
