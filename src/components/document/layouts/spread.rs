//! Spread (`spread`) layout: two pages side by side with no gap.

use leptos::prelude::*;

use crate::components::document::page_canvas::component::GlossOverlayProps;
use crate::components::document::shells::page_shell::PageShell;
use crate::components::document::PageCanvas;
use crate::components::primitives::hooks::dom::DUAL_PAGE_CONTAINER_ID;
use crate::state::{ReaderState, TextureSignal};

#[component]
pub fn SpreadLayout(
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
        <PageShell state=state scroller_id=DUAL_PAGE_CONTAINER_ID progress_visible=progress_visible>
            <For
                each=move || std::iter::once(pdf_core::layout::spread_index(state.viewer.page.get()))
                key=|s: &u32| *s
                children=move |spread: u32| {
                    let n = state.document.num_pages.get();
                    let p1 = spread * 2 + 1;
                    let p2 = p1 + 1;
                    view! {
                        <div class="flex items-start justify-center gap-0">
                                <PageCanvas
                                    page=p1
                                    scale=page_scale
                                    render_scale=state.viewer.zoom.committed
                                    zoom_animating=state.viewer.zooming()
                                    gesture_owns=gesture_owns
                                    texture=texture
                                    canvas_id=format!("dp-{p1}-cv")
                                    host_id=format!("dp-{p1}-pg")
                                    render_text=true
                                    gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
                                />
                                {if p2 <= n {
                                    view! {
                                        <PageCanvas
                                            page=p2
                                            scale=page_scale
                                            render_scale=state.viewer.zoom.committed
                                            zoom_animating=state.viewer.zooming()
                                            gesture_owns=gesture_owns
                                            texture=texture
                                            canvas_id=format!("dp-{p2}-cv")
                                            host_id=format!("dp-{p2}-pg")
                                            render_text=true
                                            gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
                                        />
                                    }.into_any()
                                } else {
                                    ().into_any()
                                }}
                            </div>
                    }
                }
            />
        </PageShell>
    }
}
