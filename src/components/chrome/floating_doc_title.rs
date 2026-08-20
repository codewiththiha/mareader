//! Difference-blend floating document name, parked TOP-LEFT.
//!
//! Overlap policy: the label may sit over the page canvas, but may cover at
//! most `MAX_CANVAS_OVERLAP` of the canvas width. Its budget is the blank gap
//! left of the page plus that overlap allowance (minus a safety margin); the
//! name shows in full only when its NATURAL width fits that budget, otherwise
//! it disappears entirely — it never truncates over the page.
//!
//! Shown only when a document is open, the sidebar is OFF (its identity row
//! already shows the name) AND the titlebar is not visible (the bar contains
//! the name). Blend note: `mix-blend-difference` must reach the page pixels,
//! so the wrapper carries NO z-index — a positioned wrapper with z-index forms
//! a stacking context that isolates the blend (the old centered label read as
//! plain white).

use leptos::html;
use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use pdf_viewer::state::SidebarMode;
use crate::core::state::AppState;
use super::titlebar_provider::TitleBarCtx;

/// Fraction of the canvas width the label may cover.
const MAX_CANVAS_OVERLAP: f64 = 0.25;
/// Safety margin subtracted from the budget (right inset + breather).
const SAFETY: f64 = 8.0;

#[component]
pub fn FloatingDocTitle(state: AppState) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    let label_ref: NodeRef<html::Span> = NodeRef::new();
    // Allowed total width in px, or None = hide.
    let budget = RwSignal::new(None::<f64>);
    // Natural (unclipped) width of the label; Infinity until first measured.
    let label_w = RwSignal::new(f64::INFINITY);

    let measure = move || {
        request_animation_frame(move || {
            let Some(doc_el) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector(".pdf-page").ok().flatten())
            else {
                budget.set(None);
                return;
            };
            let Some(viewer) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("viewer-slot"))
            else {
                budget.set(None);
                return;
            };
            let pr = doc_el.get_bounding_client_rect();
            let vr = viewer.get_bounding_client_rect();
            let canvas_w = pr.width();
            if canvas_w <= 0.0 {
                budget.set(None);
                return;
            }
            // Blank band left of the page + the overlap allowance.
            let gap = (pr.left() - vr.left()).max(0.0);
            budget.set(Some(gap + MAX_CANVAS_OVERLAP * canvas_w - SAFETY));
            // `scroll_width` is the NATURAL width even while `max-width`
            // constrains, so the disappear decision is correct.
            if let Some(span) = label_ref.get() {
                label_w.set(span.scroll_width() as f64);
            }
        });
    };

    // Re-measure whenever geometry or identity can change, and on resize.
    Effect::new(move |_| {
        _ = state.viewer.container_size.get();
        _ = state.viewer.display_scale.get();
        _ = state.viewer.page.get();
        _ = state.viewer.mode.get();
        _ = state.doc.title.get();
        _ = state.doc.path.get();
        measure();
        let h = window_event_listener_untyped("resize", move |_| measure());
        on_cleanup(move || h.remove());
    });

    let shown = move || {
        state.doc.status.get() == DocStatus::Ready
            && state.sidebar.get() == SidebarMode::None
            && ctx.map(|c| !c.visible.get()).unwrap_or(true)
            && budget.get().is_some_and(|b| label_w.get() <= b)
    };

    view! {
        // NO z-index on the wrapper: mix-blend-difference must reach page
        // pixels. opacity-0 (not `hidden`) keeps the span measurable.
        <div class="pointer-events-none absolute left-3 top-3">
            <span
                node_ref=label_ref
                class="block truncate text-sm font-medium text-white mix-blend-difference transition-opacity duration-200"
                class=("opacity-0", move || !shown())
                style:max-width=move || match budget.get() {
                    Some(b) => format!("{}px", b.max(0.0).floor()),
                    None => "0px".to_string(),
                }
            >
                {move || pdf_core::filename::display_name(
                    state.doc.title.get().as_deref(),
                    state.doc.path.get().as_deref(),
                )
                .unwrap_or_default()}
            </span>
        </div>
    }
}
