//! The reflowable gloss stroke layer, mounted per page or once per stream.

use leptos::prelude::*;

use crate::components::ai::anchor::{layer_refresh, stroke_resolver};
use crate::components::ai::gloss::mark_layer::GlossMarkLayer;
use crate::state::ReaderState;

#[component]
pub fn ReflowGlossLayer(
    state: ReaderState,
    /// The host's own page, so the resolver places only this page's marks.
    #[prop(optional)]
    page: Option<u32>,
    /// The element id the strokes are positioned against: the page host, or the
    /// stream's scroller.
    #[prop(into)]
    host_id: String,
) -> impl IntoView {
    let gloss = state.gloss;
    let resolve = stroke_resolver(state, page, Some(host_id));
    let refresh = layer_refresh(state);
    let scale = state.viewer.zoom.display.read_only();

    view! {
        <GlossMarkLayer
            marks=gloss.marks.read_only().into()
            resolve=resolve
            refresh=refresh
            scale=scale
            processing=gloss.processing_id.read_only().into()
            selecting=gloss.selection_active
            selected=gloss.selected_marks
        />
    }
}
