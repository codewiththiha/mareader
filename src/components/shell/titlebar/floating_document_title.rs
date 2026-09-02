//! Difference-blend floating document name, parked TOP-LEFT.
//!
//! Overlap policy: the label may sit over the page canvas, but may cover at
//! most `MAX_CANVAS_OVERLAP` of the canvas width. Its budget is the blank gap
//! left of the page plus that overlap allowance (minus a safety margin); the
//! name shows in full only when its NATURAL width fits that budget, otherwise
//! it disappears entirely — it never truncates over the page. "Label Width
//! Limit" scales that budget down; "Always Show Label" opts out of the
//! auto-hide rules — except the one that matters most:
//!
//! THE RAIL ALWAYS WINS. The label is portaled to `<body>` and sits at the
//! window's top-left, which is exactly where the rail's header is, so a label
//! that ignores the rail reads as text laid over the sidebar (docked) or
//! floating above it (overlay). "Always" means the title bar and the width
//! budget stop being reasons to hide; the rail is still a reason. It follows
//! [`ShellController::rail_present`], so the label stays out of the way for the
//! whole close slide rather than reappearing on the frame the mode flips.
//!
//! Shown only when a document is open, the sidebar is off, and — unless
//! "Always Show Label" is on — the titlebar is not visible (the bar contains
//! the name) and the name fits its budget.
//!
//! Blend contract: `mix-blend-difference` blends only against what is painted
//! *inside the element's isolation group* (its nearest ancestor stacking
//! context). The shell subtree creates such contexts freely (toolbar glass
//! `backdrop-filter`, `opacity` fades, z-token wrappers, `prop:inert`
//! toggling), and whenever the group happened to exclude the pages the
//! backdrop read transparent — `white difference transparent = white` — with
//! the blend snapping back only when an unrelated animation forced the
//! compositor to re-invalidate the layer. So the label is PORTALED to
//! `<body>` and `position: fixed`: its only ancestors are body/html, its
//! isolation group is the root canvas group, and that group always contains
//! the pages' pixels — deterministic, with no ancestor able to isolate it.
//!
//! Rules that keep it working forever:
//! 1. `mix-blend-difference` must sit on the SAME element that is
//!    `position: fixed`. `fixed` creates a stacking context, so a fixed
//!    wrapper around a blended child isolates the child against a transparent
//!    backdrop (`white difference transparent = white`) — that exact shape is
//!    what made the portaled label read white in the browser.
//! 2. The blending node and its ancestors up to `<body>` must otherwise stay
//!    stacking-context-FREE: no z-index, opacity, transform, filter,
//!    backdrop-filter, isolation, contain or will-change. Above/below is
//!    solved with DOM order, never z-index.
//! 3. The fade (`opacity-0`) stays on the inner span, a DESCENDANT of the
//!    blending node: a descendant's opacity never isolates the blend.
//!    Mid-fade the text simply fades; the blend stays live.
//! 4. If it ever reads white again: DevTools → blending node → walk the
//!    ancestors to `<body>` and look for the properties in rule 2, and make
//!    sure no `position: fixed|sticky` or stacking-context ancestor sits
//!    BETWEEN the blend and `<body>` other than the blending node itself.

use leptos::html;
use leptos::portal::Portal;
use leptos::prelude::*;

use pdf_engine::types::DocStatus;
use crate::components::ai::anchor::host_id_for_mode;
use crate::components::shell::controller::ShellController;
use app_chrome::titlebar::root::TitleBarCtx;
use crate::state::AppState;
use app_chrome::hooks::dom::{by_id, VIEWER_SLOT_ID};
use app_chrome::hooks::use_window_event::use_window_event;

/// Fraction of the canvas width the label may cover.
const MAX_CANVAS_OVERLAP: f64 = 0.25;
/// Safety margin subtracted from the budget (right inset + breather).
const SAFETY: f64 = 8.0;
/// Minimum budget to even attempt showing the label.
const MIN_LABEL_W: f64 = 40.0;

#[component]
pub fn FloatingDocumentTitle(state: AppState) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    let shell = use_context::<ShellController>()
        .expect("the page provides the shell controller");
    // The blending node is the positioned <div> (see the view's CRITICAL
    // note); scroll_width() there is the natural text width, as before.
    let label_ref: NodeRef<html::Div> = NodeRef::new();
    // Allowed total width in px, or None = hide.
    let budget = RwSignal::new(None::<f64>);
    // Natural (unclipped) width of the label; Infinity until first measured.
    let label_w = RwSignal::new(f64::INFINITY);

    let measure = move || {
        request_animation_frame(move || {
            // Mid-zoom relayout: geometry is moving; the effect re-runs when
            // the zoom transition ends, so skipping here loses nothing.
            if state.reader.viewer.zooming_now() { return; }

            // THE page under the eyes, by id — never an arbitrary mounted
            // page. The id format is the anchor module's; duplicating it here
            // once drifted from the single-page host convention.
            let page = state.reader.viewer.page.get_untracked().max(1);
            let mode = state.reader.viewer.mode.get_untracked();
            // A missing host is the ordinary virtualization gap (the page
            // under the eyes is between mounts), so this stays a silent
            // `by_id` — but the viewer slot itself is chrome.
            let Some(doc_el) = by_id(&host_id_for_mode(page, mode)) else { return };
            let Some(viewer) = by_id(VIEWER_SLOT_ID) else { return };

            let pr = doc_el.get_bounding_client_rect();
            let vr = viewer.get_bounding_client_rect();
            let canvas_w = pr.width();
            if canvas_w <= 0.0 { return; } // not laid out yet: keep last budget

            let gap = (pr.left() - vr.left()).max(0.0);
            // Overlap allowance only when there is a real blank margin. When
            // the page spans the viewer (fit-width, zoomed-in), the label
            // would sit on the page and must disappear entirely instead of
            // covering up to 25% of it.
            let overlap = if gap > 1.0 {
                MAX_CANVAS_OVERLAP * canvas_w
            } else {
                0.0
            };
            let new_budget = gap + overlap - SAFETY;

            // Only write on a real change — avoids class/style closure churn
            // every rAF during the sidebar slide. The rAF can outlive this
            // component (closing the document unmounts it while a frame is in
            // flight), so try-accessors make a stale frame a silent no-op.
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
    //
    // The zoom transition is tracked so the effect re-runs when a gesture SETTLES
    // (the rAF below skips while the flag is up): a zoom-in that fills the
    // viewer with the page must collapse the budget and hide the label, a
    // zoom-out must bring it back. Without this the label would sit over the
    // page indefinitely after zooming, because zooming does not move
    // `page`/`container_size` (the anchored page stays dominant).
    Effect::new(move |_| {
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
        use pdf_core::settings::FloatingLabelStyle::*;
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
        // The rail owns the top-left corner in either mode: docked, its
        // identity row already shows the name; floating, it is painted right
        // under this label.
        let rail_off = !shell.rail_present().get();
        // Persist means auto-hide does not: the title bar and the width budget
        // stop being reasons to disappear. The rail is not a budget.
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
        //
        // CRITICAL: `mix-blend-difference` lives on the SAME node as
        // `position: fixed`. `fixed` creates a stacking context, so a fixed
        // *wrapper* around a blended child would isolate the child (its only
        // backdrop would be the transparent wrapper -> white). On the fixed
        // node itself, the backdrop is the portal/body group = the whole app.
        // opacity-0 (not `hidden`) keeps the inner span measurable.
        <Portal>
            <div
                node_ref=label_ref
                class="pointer-events-none fixed left-3 top-3 block truncate text-sm font-medium \
                       text-white mix-blend-difference"
                style:max-width=move || {
                    // Persist must beat the width clamp too. When the page
                    // fills the viewer the gap is ~0, so the budget falls
                    // under MIN_LABEL_W and this returned "0px" — the label
                    // was truncated to nothing no matter what `shown` said.
                    // The rail case never reaches here: `shown` is already off
                    // and the fade hides the text.
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
