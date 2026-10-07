//! Floating document name, difference-blended, top-left.

use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;

use crate::components::ai::anchor::host_id_for_mode;
use app_chrome::hooks::use_window_event::use_window_event;
use app_chrome::titlebar::root::TitleBarCtx;
use app_ui::components::shell::controller::ShellController;
use reader_core::document::DocStatus;

/// Fraction of the canvas width the label may cover.
const MAX_CANVAS_OVERLAP: f64 = 0.25;
/// Safety margin subtracted from the budget (right inset + breather).
const SAFETY: f64 = 8.0;
/// Minimum budget to even attempt showing the label.
const MIN_LABEL_W: f64 = 40.0;

#[component]
pub fn FloatingDocumentTitle(state: crate::context::ReaderContext) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    let shell = use_context::<ShellController>().expect("the page provides the shell controller");
    // The blending node is the positioned <div>; its scroll_width is the
    // natural text width.
    let label_ref: NodeRef<html::Div> = NodeRef::new();
    // Allowed total width in px, or None = hide.
    let budget = RwSignal::new(None::<f64>);
    // Natural (unclipped) width of the label; Infinity until first measured.
    let label_w = RwSignal::new(f64::INFINITY);

    let measure = move || {
        request_animation_frame(move || {
            // Mid-zoom: the effect re-runs when the transition ends.
            if state.reader.viewer.try_zooming_now() != Some(false) {
                return;
            }

            // THE page under the eyes, by id — never an arbitrary mounted page.
            let Some(page) = state.reader.viewer.page.try_get_untracked() else {
                return;
            };
            let Some(mode) = state.reader.viewer.mode.try_get_untracked() else {
                return;
            };
            let page = page.max(1);
            // A missing host is the ordinary virtualization gap: a silent miss.
            let dom = state.reader.dom;
            let Some(doc_el) = dom.by_id(&host_id_for_mode(mode, page)) else {
                return;
            };
            let Some(viewer) = dom.root() else {
                return;
            };

            let pr = doc_el.get_bounding_client_rect();
            let vr = viewer.get_bounding_client_rect();
            let canvas_w = pr.width();
            if canvas_w <= 0.0 {
                return;
            } // not laid out yet: keep last budget

            let gap = (pr.left() - vr.left()).max(0.0);
            // Overlap allowance only when there is a real blank margin.
            let overlap = if gap > 1.0 {
                MAX_CANVAS_OVERLAP * canvas_w
            } else {
                0.0
            };
            let new_budget = gap + overlap - SAFETY;

            // Only write on a real change; stale frames are a silent no-op.
            if budget
                .try_get_untracked()
                .flatten()
                .is_none_or(|b| (b - new_budget).abs() > 0.5)
            {
                let _ = budget.try_set(Some(new_budget));
            }
            if let Some(span) = label_ref.get() {
                let w = span.scroll_width() as f64;
                let prev = label_w.try_get_untracked();
                if w > 0.0 && prev.is_none_or(|p| (p - w).abs() > 0.5) {
                    let _ = label_w.try_set(w);
                }
            }
        });
    };

    // Re-measure whenever geometry or identity can change, and on resize.
    Effect::new(move |_| {
        // The pane's own box: a pane resized in the workspace re-measures
        // too.
        _ = state.reader.dom.bounds();
        _ = state.reader.viewer.container_size.get();
        _ = state.reader.viewer.page.get();
        _ = state.reader.viewer.mode.get();
        _ = state.reader.viewer.zoom.transition.get();
        _ = state.reader.viewer.zoom.display.get();
        _ = state.reader.document.title.get();
        _ = state.reader.document.path.get();
        _ = state.reader.document.outline.get();
        _ = state.settings.with(|s| {
            (
                s.layout.floating_label,
                s.layout.floating_label_style,
                s.layout.floating_label_persist,
                s.layout.floating_label_max_pct,
            )
        });
        measure();
        use_window_event("resize", move |_| measure());
    });

    let enabled = move || state.settings.with(|st| st.layout.floating_label);
    let label = move || {
        use reader_core::settings::FloatingLabelStyle::*;
        let st = state.settings.with(|s| s.layout.floating_label_style);
        let r = &state.reader;
        match st {
            FileName => r.document.display_name(),
            Chapter => {
                let page = r.viewer.page.get();
                r.document
                    .outline
                    .with(|o| o.iter().rfind(|n| n.page <= page).map(|n| n.title.clone()))
                    .unwrap_or_else(|| r.document.display_name())
            }
        }
    };
    let shown = move || {
        if !enabled() || state.reader.document.status.get() != DocStatus::Ready {
            return false;
        }
        // The label is portaled to the body, so the cover cannot mask it.
        if !state.reader.viewer.first_paint.get() {
            return false;
        }
        // The rail owns the top-left corner in either mode.
        let rail_off = !shell.rail_present().get();
        // Persist means auto-hide does not; the rail is not a budget.
        if state.settings.with(|st| st.layout.floating_label_persist) {
            return rail_off;
        }
        let max_pct = state.settings.with(|st| st.layout.floating_label_max_pct);
        rail_off
            && ctx.map(|c| !c.visible.get()).unwrap_or(true)
            // None = unknown = show
            && budget.get().is_none_or(|b| label_w.get() <= b * max_pct / 100.0)
    };

    view! {
        // Portal to <body>: root canvas group = pages + label, always.
        <Portal>
            <div
                node_ref=label_ref
                class="pointer-events-none fixed block truncate text-sm font-medium \
                       text-white mix-blend-difference"
                // The top-left corner of THIS pane's box, a plain inset.
                style:left=move || format!("calc(0.75rem + {}px)", state.reader.dom.bounds().x)
                style:top=move || format!("calc(0.75rem + {}px)", state.reader.dom.bounds().y)
                style:max-width=move || {
                    // Persist must beat the width clamp too.
                    if state.settings.with(|st| st.layout.floating_label_persist) {
                        return "min(70vw, 560px)".to_string();
                    }
                    let max_pct = state.settings.with(|st| st.layout.floating_label_max_pct);
                    match budget.get() {
                        Some(b) if b >= MIN_LABEL_W => {
                            format!("{}px", (b * max_pct / 100.0).max(0.0).floor())
                        }
                        Some(_) => "0px".to_string(),
                        None => "none".to_string(),
                    }
                }
            >
                <span class="block transition-opacity duration-200" class=("opacity-0", move || !shown())>
                    {label}
                </span>
            </div>
        </Portal>
    }
}
