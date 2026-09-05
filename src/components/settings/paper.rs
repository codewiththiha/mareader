//! The paper and rendering sections of the reader settings modal's Theme tab:
//! what colour the reader looks at, and which pipeline paints it.
//!
//! Both sections act on the PDF's bitmaps — blend sampling, edge detection and
//! the live-vs-baked choice — so the component gates itself on the document
//! being a raster one and the tab can mount it unconditionally.

use leptos::prelude::*;

use reader_core::settings::{PaperArea, RenderPipeline};

use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::settings::common::{Row, StyleSelect};
use crate::state::AppState;

/// The raster-only half of the Theme tab: paper blend and the theme pipeline.
#[component]
pub(crate) fn PaperSection(state: AppState) -> impl IntoView {
    // Raster concerns, all of them: blend sampling, edge detection and the
    // live-vs-baked choice act on the PDF's always-light bitmaps. A reflowable
    // document paints its paper and ink straight from the theme tokens, so the
    // sections are not merely inert while one is open — they describe machinery
    // that does not run.
    let reflowable = Signal::derive(move || state.reader.reflowable());
    let s = state.settings;
    let blend_off = Signal::derive(move || !s.with(|st| st.layout.blend_mode));

    view! {
        <Show when=move || !reflowable.get()>
        <div class="mt-5"><Separator vertical=false /></div>
        <SectionLabel text="Paper" />
        <div class="divide-y divide-line rounded-xl border border-line">
            <Row label="Blend Mode">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.layout.blend_mode))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.blend_mode = v);
                    })
                    title="Paint the reader background with the page's own paper \
                           colour, following the scroll page by page, through the \
                           same filter the pages use"
                        .to_string()
                />
            </Row>
            <Row label="Detection">
                <StyleSelect
                    value=Signal::derive(move || s.with(|st| st.layout.blend_area))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.layout.blend_area = v);
                    })
                    options=vec![
                        (PaperArea::WholePage, "Whole Page"),
                        (PaperArea::Edges, "Edges"),
                    ]
                    label_of=|v: &PaperArea| v.label()
                    disabled=blend_off
                />
            </Row>
        </div>
        <div class="mt-5"><Separator vertical=false /></div>
        <SectionLabel text="Rendering" />
        // Which pipeline paints the theme onto the pages. Live keeps the page
        // and the backdrop in one compositor pass — the only way the two match
        // exactly in blend mode — at the cost of a filter intermediate per
        // mounted page. Baked burns the look into each raster instead: lighter
        // to composite, but a re-bake on every appearance change and a page
        // that can never be bit-identical to the backdrop.
        <div class="divide-y divide-line rounded-xl border border-line">
            <Row label="Theme Pipeline">
                <StyleSelect
                    value=Signal::derive(move || s.with(|st| st.render_pipeline))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.render_pipeline = v);
                    })
                    options=vec![
                        (RenderPipeline::Live, "Live"),
                        (RenderPipeline::Baked, "Baked"),
                    ]
                    label_of=|v: &RenderPipeline| v.label()
                    disabled=Signal::derive(move || false)
                />
            </Row>
        </div>
        <p class="mt-2 text-xs text-muted">
            "Live filters each page in the compositor, so pages and the blend \
             backdrop share one pass. Baked burns the look into the rasters — \
             lighter to composite, slightly slower to change."
        </p>
        </Show>
    }
}
