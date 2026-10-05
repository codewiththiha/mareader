//! The reflowable formats' gloss stroke layer — the same component the PDF
//! hosts mount, given the inputs only a document made of type needs.
//!
//! There are two mount points, because a reflowable document has two shapes:
//!
//! * a PAGE host (`.tx-page`, in single, spread and horizontal reading) mounts
//!   one layer inside itself, exactly like `.pdf-page` does, and its strokes are
//!   positioned against that host;
//! * the continuous STREAM mounts ONE layer for the whole reading surface,
//!   positioned against the scroller. Its blocks are virtualized individually
//!   and are not pages at all, so a per-page layer would have nothing to attach
//!   to — and a per-block layer would lose every mark whose block scrolled out
//!   of the window.
//!
//! Both are the same layer with the same `position:absolute; inset:0`, and the
//! only difference is which element the resolver subtracts. That element is
//! also what clips the strokes: a page's marks stop at the page edge, and the
//! stream's stop at the reader's edge, because `main#viewer-slot` hides its
//! overflow. Nothing here is `position:fixed`, so no ancestor's transform,
//! filter or containment can decide where a highlight lands.
//!
//! Which marks are on screen is the resolver's answer, not a page-number
//! filter: a re-cut moves blocks between pages, and the stream has no pages.

use leptos::prelude::*;

use crate::components::ai::anchor::{layer_refresh, stroke_resolver};
use crate::components::ai::gloss::mark_layer::GlossMarkLayer;
use crate::state::ReaderState;

#[component]
pub fn ReflowGlossLayer(
    state: ReaderState,
    /// The host's own page, so the resolver only places this page's marks.
    /// `None` for the stream's one layer, which has no page of its own.
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
