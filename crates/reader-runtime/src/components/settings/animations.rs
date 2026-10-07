//! The Animations tab: one switch per motion the reader interpolates.

use leptos::prelude::*;

use app_ui::components::primitives::controls::switch::Switch;
use app_ui::components::primitives::form::row::Row;
use app_ui::components::primitives::menu::section_label::SectionLabel;

#[component]
pub(crate) fn AnimationsTab(state: crate::context::ReaderContext) -> impl IntoView {
    let s = state.settings;
    view! {
        <SectionLabel text="Reader motion" />
        <div class="divide-y divide-line rounded-xl border border-line">
            <Row label="Sidebar Animation">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.animations.sidebar_slide))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.animations.sidebar_slide = v);
                    })
                    title="The docked rail slides its width open and closed; the floating rail \
                           fades. Off, either appears in one step."
                        .to_string()
                />
            </Row>
            <Row label="Canvas Follows Window">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.animations.canvas_resize))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.animations.canvas_resize = v);
                    })
                    title="Re-fit the page while the window is dragged. Off, it re-fits once, when \
                           the drag ends."
                        .to_string()
                />
            </Row>
            <Row label="Zoom In / Out">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.animations.zoom))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.animations.zoom = v);
                    })
                    title="Ease a zoom to its new scale. Off, every zoom lands on the first frame."
                        .to_string()
                />
            </Row>
            <Row label="Scroll To Page">
                <Switch
                    checked=Signal::derive(move || s.with(|st| st.animations.scroll_jumps))
                    on_change=Callback::new(move |v| {
                        s.update(|st| st.animations.scroll_jumps = v);
                    })
                    title="Glide the column to a page or a search hit. Off, it lands there."
                        .to_string()
                />
            </Row>
        </div>
        <p class="mt-2 text-xs text-muted">
            "Anything switched off still changes — it just changes in one frame. The master switch \
             is Animations, in the Layout tab, and it outranks everything here."
        </p>
    }
}
