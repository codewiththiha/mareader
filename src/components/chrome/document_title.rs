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
use crate::state::SidebarMode;
use crate::state::AppState;
use super::title_bar::TitleBarCtx;

/// Fraction of the canvas width the label may cover.
const MAX_CANVAS_OVERLAP: f64 = 0.25;
/// Safety margin subtracted from the budget (right inset + breather).
const SAFETY: f64 = 8.0;
/// Minimum budget to even attempt showing the label.
const MIN_LABEL_W: f64 = 40.0;

#[component]
pub fn FloatingDocumentTitle(state: AppState) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    let label_ref: NodeRef<html::Span> = NodeRef::new();
    // Allowed total width in px, or None = hide.
    let budget = RwSignal::new(None::<f64>);
    // Natural (unclipped) width of the label; Infinity until first measured.
    let label_w = RwSignal::new(f64::INFINITY);

    let measure = move || {
        request_animation_frame(move || {
            // Mid-zoom relayout: geometry is moving; the effect re-runs when
            // zoom_animating drops, so skipping here loses nothing.
            if state.reader.viewer.zoom_animating.get_untracked() { return; }

            // THE page under the eyes, by id — never an arbitrary mounted page.
            let page = state.reader.viewer.page.get_untracked().max(1);
            let host_id = if state.reader.viewer.mode.get_untracked() == pdf_core::layout::ViewMode::Single {
                format!("sp-{page}-pg")
            } else {
                format!("cont-{}-pg", page.saturating_sub(1))
            };
            let Some(doc_el) = crate::components::document::dom_helpers::by_id(&host_id) else { return };
            let Some(viewer) = crate::components::document::dom_helpers::by_id("viewer-slot") else { return };

            let pr = doc_el.get_bounding_client_rect();
            let vr = viewer.get_bounding_client_rect();
            let canvas_w = pr.width();
            if canvas_w <= 0.0 { return; } // not laid out yet: keep last budget

            let gap = (pr.left() - vr.left()).max(0.0);
            let new_budget = gap + MAX_CANVAS_OVERLAP * canvas_w - SAFETY;

            // Only write on a real change — avoids class/style closure churn
            // every rAF during the sidebar slide.
            if budget.get_untracked().is_none_or(|b| (b - new_budget).abs() > 0.5) {
                budget.set(Some(new_budget));
            }
            if let Some(span) = label_ref.get() {
                let w = span.scroll_width() as f64;
                if w > 0.0 && (label_w.get_untracked() - w).abs() > 0.5 {
                    label_w.set(w);
                }
            }
        });
    };

    // Re-measure whenever geometry or identity can change, and on resize.
    Effect::new(move |_| {
        _ = state.reader.viewer.container_size.get();
        _ = state.reader.viewer.page.get();
        _ = state.reader.viewer.mode.get();
        _ = state.reader.document.title.get();
        _ = state.reader.document.path.get();
        measure();
        let h = window_event_listener_untyped("resize", move |_| measure());
        on_cleanup(move || h.remove());
    });

    let shown = move || {
        state.reader.document.status.get() == DocStatus::Ready
            && state.ui.sidebar.get() == SidebarMode::None
            && ctx.map(|c| !c.visible.get()).unwrap_or(true)
            && budget.get().is_none_or(|b| label_w.get() <= b)  // None = unknown = show
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
                    Some(b) if b >= MIN_LABEL_W => format!("{}px", b.max(0.0).floor()),
                    Some(_) => "0px".to_string(),
                    None => "none".to_string(),
                }
            >
                {move || pdf_core::filename::display_name(
                    state.reader.document.title.get().as_deref(),
                    state.reader.document.path.get().as_deref(),
                )
                .unwrap_or_default()}
            </span>
        </div>
    }
}
// The toolbar document-name label. Its width is the actual space between the
// leading controls and trailing toolbar cluster; it deliberately measures DOM
// rects rather than reproducing title-bar padding or sidebar-state assumptions.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;

use pdf_core::filename::display_name;

/// Gap after the leading controls (`gap-1`).
const GAP_LEFT: f64 = 4.0;
/// Breathing room before the trailing control cluster.
const GAP_RIGHT: f64 = 12.0;
const TITLE_MIN_LABEL_W: f64 = crate::components::chrome::metrics::MIN_DOC_TITLE_WIDTH;

/// Measure the title's real slot in toolbar-row coordinates. This remains
/// correct through the sidebar close slide because it uses the live rects,
/// not the raw sidebar mode or title-bar padding model.
fn measure_available() -> Option<f64> {
    let doc = web_sys::window()?.document()?;
    let row = doc.get_element_by_id("toolbar-row")?;
    let row_rect = row.get_bounding_client_rect();
    if row_rect.width() <= 0.0 {
        return None;
    }
    let pre = doc.get_element_by_id("toolbar-left-pre")?;
    let pre_rect = pre.get_bounding_client_rect();
    let right = doc.get_element_by_id("toolbar-right")?;
    let right_rect = right.get_bounding_client_rect();

    // The pin button follows #toolbar-right inside its ml-auto parent. Using
    // the trailing group's left edge therefore reserves it automatically.
    let start = pre_rect.right() - row_rect.left() + GAP_LEFT;
    let end = right_rect.left() - row_rect.left() - GAP_RIGHT;
    Some((end - start).max(0.0))
}

#[component]
pub fn DocumentTitle(state: AppState) -> impl IntoView {
    let avail = RwSignal::new(None::<f64>);
    let observer_handle = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    let callback_handle =
        StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>);

    let remeasure = move || {
        request_animation_frame(move || {
            if let Some(w) = measure_available() {
                let prev = avail.get_untracked();
                if prev.is_none_or(|p: f64| (p - w).abs() > 0.5) {
                    avail.set(Some(w));
                }
            }
        });
    };
    let remeasure_for_ro = remeasure.clone();

    // The route/page mount order may put this effect ahead of its anchor ids.
    // Do not mark installation complete until at least one live anchor was
    // observed; subsequent reactive runs then self-heal that initial miss.
    let try_install = move || {
        if callback_handle.with_value(|c| c.is_some()) {
            return;
        }
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> =
            Closure::wrap(Box::new(move |_: Vec<ResizeObserverEntry>| {
                remeasure_for_ro();
            }) as Box<dyn FnMut(Vec<ResizeObserverEntry>)>);
        let fn_ref: &js_sys::Function = callback.as_ref().unchecked_ref();
        let Ok(observer) = web_sys::ResizeObserver::new(fn_ref) else {
            return;
        };
        let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
            return;
        };

        let mut observed = false;
        for id in ["toolbar-row", "toolbar-left-pre", "toolbar-right"] {
            if let Some(el) = doc.get_element_by_id(id) {
                observer.observe(&el);
                observed = true;
            }
        }
        // The title row changes inset at the end of the close hold, whereas
        // this aside changes width during every frame of the 300ms slide.
        if let Some(aside) = doc.query_selector("aside.sidebar-aside").ok().flatten() {
            observer.observe(&aside);
            observed = true;
        }
        if observed {
            observer_handle.set_value(Some(observer));
            callback_handle.set_value(Some(callback));
        } else {
            observer.disconnect();
        }
    };

    on_cleanup(move || {
        if let Some(observer) = observer_handle.try_get_value().flatten() {
            observer.disconnect();
        }
        observer_handle.try_set_value(None);
        callback_handle.try_set_value(None);
    });

    Effect::new(move |_| {
        try_install();
        _ = state.reader.document.status.get();
        _ = state.reader.document.num_pages.get();
        _ = state.reader.document.title.get();
        _ = state.reader.document.path.get();
        _ = state.ui.sidebar.get();
        remeasure();
    });

    let name = move || {
        display_name(
            state.reader.document.title.get().as_deref(),
            state.reader.document.path.get().as_deref(),
        )
        .unwrap_or_else(|| "No document".to_string())
    };
    let full = move || name();
    let hidden = move || avail.get().is_some_and(|w| w < TITLE_MIN_LABEL_W);

    view! {
        <span
            id="toolbar-doc-title"
            data-tauri-drag-region="true"
            class="min-w-0 shrink truncate text-sm text-ink"
            class=("hidden", hidden)
            title=full
            style:max-width=move || match avail.get() {
                Some(w) if w >= TITLE_MIN_LABEL_W => format!("{}px", w.floor()),
                Some(_) => "0px".to_string(),
                None => "none".to_string(),
            }
        >
            {name}
        </span>
    }
}
