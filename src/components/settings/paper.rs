//! The paper section of the reader settings modal's Theme tab: what colour
//! the reader looks at.
//!
//! Both rows act on the PDF's bitmaps — blend sampling and edge detection —
//! so the component gates itself on the document being a raster one and the
//! tab can mount it unconditionally. The theme itself is always pre-rendered
//! into those bitmaps; the real-time compositing a tint drag needs is the
//! scrub window's business, not a reader-facing choice.

use leptos::prelude::*;

use reader_core::settings::PaperArea;

use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::settings::common::{Row, StyleSelect};
use crate::state::AppState;

/// The raster-only half of the Theme tab: the paper blend and its detection.
#[component]
pub(crate) fn PaperSection(state: AppState) -> impl IntoView {
    // Raster concerns, both of them: blend sampling and edge detection act on
    // the PDF's always-light bitmaps. A reflowable document paints its paper
    // and ink straight from the theme tokens, so the section is not merely
    // inert while one is open — it describes machinery that does not run.
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
        </Show>
    }
}
