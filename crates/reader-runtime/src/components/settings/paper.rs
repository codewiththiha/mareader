//! The paper section of the Theme tab: what colour the reader looks at.

use leptos::prelude::*;

use reader_core::settings::PaperArea;

use crate::components::settings::common::StyleSelect;
use app_ui::components::primitives::controls::switch::Switch;
use app_ui::components::primitives::form::row::Row;
use app_ui::components::primitives::menu::section_label::SectionLabel;
use app_ui::components::primitives::menu::separator::Separator;

/// The raster-only half of the Theme tab: the paper blend and its detection.
#[component]
pub(crate) fn PaperSection(state: crate::context::ReaderContext) -> impl IntoView {
    // PDF-only machinery, so the section hides while a reflowable
    // document is open.
    let reflowable = Signal::derive(move || state.reader.reflowable());
    let s = state.settings;
    let blend_off = Signal::derive(move || !s.with(|st| st.layout.blend_mode));

    view! {
        <Show when=move || !reflowable.get()>
        <Separator vertical=false spacing="mt-5" />
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
