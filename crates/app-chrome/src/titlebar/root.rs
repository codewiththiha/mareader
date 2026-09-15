//! Generic hover/grab titlebar shell. `left`/`right` are render-prop slots so
//! each page composes its own controls; the shell owns the hover/pin state,
//! the hide timers, the drag/hover band, and the center slot's resolved box
//! (`resolve_center_slot`).
//!
//! It WRAPS its children so descendants (the floating doc title, the slot
//! menus' popovers) can read the shared [`TitleBarCtx`] — leptos context flows
//! down the reactive tree, so a sibling overlay would not see it.
//!
//! The shell knows nothing about the application: pin state, the traffic
//! lights, the caption cluster (`end`), sidebar insets and search holds
//! arrive as props computed by `app_title_bar.rs`.

use leptos::children::ViewFn;
use leptos::html;
use leptos::prelude::*;

use crate::layers::BAR;
use crate::hooks::{DEFAULT_HOVER_DELAY, HoverConfig, use_hover_reveal};
use crate::hooks::dom::{
    by_id, TOOLBAR_CENTER_TITLE_ID, TOOLBAR_LEADING_ID, TOOLBAR_ROW_ID, TOOLBAR_TRAILING_ID,
};
use crate::hooks::use_resize_observer::observe_elements;
use crate::hooks::use_window_event::use_window_event;
use crate::icon::IconName;
use crate::icon_button::IconButton;
use crate::tooltip::Tooltip;

/// Breathing room between the measured clusters and the centered slot.
const CENTER_GAP: f64 = 8.0;
/// Below this width a centered title is a stub ("R…") — hide it instead.
const MIN_CENTER_SLOT: f64 = 60.0;

/// The box the center content renders in, in row coordinates `(start,
/// width)`, centered inside. Two tiers: the whole row while the content fits
/// at the row's EXACT center (center ± w/2 clears both clusters), else the
/// free stretch between the clusters, where the content centers and truncates.
///
/// Pure geometry so the tiers stay unit-testable; [`measure_center_slot`]
/// feeds it live DOM. The cluster edges already reserve whatever sits on
/// either side, so there is no platform branch here.
fn resolve_center_slot(
    row_width: f64,
    left: f64,
    right: f64,
    title_width: Option<f64>,
) -> (f64, f64) {
    let center = row_width * 0.5;
    // At the row center the content spans center ± w/2; it fits while both
    // ends clear the clusters.
    let fits_center = title_width.is_some_and(|w| {
        w <= 2.0 * (center - left) && w <= 2.0 * (right - center)
    });
    if fits_center {
        return (0.0, row_width);
    }
    let start = left.max(0.0);
    (start, (right - start).max(0.0))
}

/// The live measurement behind [`resolve_center_slot`]: the row rect, both
/// cluster edges (row coordinates, breathing gap applied) and the title's
/// natural width — `scroll_width` on the truncate span reports the full
/// content even while clipped. `None` while any anchor is absent.
fn measure_center_slot() -> Option<(f64, f64)> {
    let row = by_id(TOOLBAR_ROW_ID)?;
    let row_rect = row.get_bounding_client_rect();
    if row_rect.width() <= 0.0 {
        return None;
    }
    let leading = by_id(TOOLBAR_LEADING_ID)?;
    let trailing = by_id(TOOLBAR_TRAILING_ID)?;
    let left = leading.get_bounding_client_rect().right() - row_rect.left() + CENTER_GAP;
    let right = trailing.get_bounding_client_rect().left() - row_rect.left() - CENTER_GAP;
    let title_width = by_id(TOOLBAR_CENTER_TITLE_ID).map(|title| title.scroll_width() as f64);
    Some(resolve_center_slot(row_rect.width(), left, right, title_width))
}

/// Coalesced slot re-measure (write-if-changed, so a sidebar slide costs
/// one style update per frame, not a notify storm).
fn schedule_slot_measure(center_slot: RwSignal<Option<(f64, f64)>>) {
    request_animation_frame(move || {
        let next = measure_center_slot();
        let changed = match (center_slot.try_get_untracked().flatten(), next) {
            (Some((ps, pw)), Some((ns, nw))) => (ps - ns).abs() > 0.5 || (pw - nw).abs() > 0.5,
            (None, None) => false,
            _ => true,
        };
        if changed {
            let _ = center_slot.try_set(next);
        }
    });
}

/// Wires the center slot's resolved box: a signal kept current by observing
/// the row, both clusters and the title, a window-resize re-measure, and an
/// immediate first pass. See [`resolve_center_slot`] for the box itself.
///
/// The shell's own anchors are read through NodeRefs, and the effect WAITS
/// for the row's ref before installing: a route swap runs the incoming page's
/// bodies a whole tick before it exchanges the DOM, so the first pass after a
/// remount runs while the document still shows the OUTGOING page. Ids would
/// resolve to that page's nodes, and the one-install observer would latch
/// onto nodes one tick from death — the new page's slot never re-measured
/// again. The refs are set when this shell's own elements build, which wakes
/// the effect for the real install; the page-owned leading cluster is looked
/// up by id on that re-run, once the swap has completed.
fn use_center_slot(
    row_ref: NodeRef<html::Div>,
    trailing_ref: NodeRef<html::Div>,
    center_title_ref: NodeRef<html::Span>,
) -> RwSignal<Option<(f64, f64)>> {
    let center_slot = RwSignal::new(None::<(f64, f64)>);
    Effect::new(move |_| {
        // No row yet, nothing to measure against: the build that sets this
        // ref re-runs the effect for the install.
        let Some(row) = row_ref.get() else {
            return;
        };
        let mut els: Vec<web_sys::Element> = vec![row.into()];
        if let Some(trailing) = trailing_ref.get() {
            els.push(trailing.into());
        }
        if let Some(leading) = by_id(TOOLBAR_LEADING_ID) {
            els.push(leading);
        }
        if let Some(title) = center_title_ref.get() {
            els.push(title.into());
        }
        observe_elements(els, move |_| schedule_slot_measure(center_slot));
    });
    use_window_event("resize", move |_| schedule_slot_measure(center_slot));
    schedule_slot_measure(center_slot);
    center_slot
}

/// Shared chrome state, provided to descendants (the floating doc title and
/// the slot menus' popovers).
#[derive(Clone, Copy)]
pub struct TitleBarCtx {
    /// Effective bar visibility = pinned OR hovered.
    pub visible: Signal<bool>,
    /// Active holds count from open popovers in the titlebar.
    pub held_count: RwSignal<usize>,
    /// The resolved center-title node, reactive across conditional remounts.
    pub center_title_ref: NodeRef<html::Span>,
    /// The row's node, reactive across page remounts. Descendants that
    /// measure against the row (the traffic lights' live header height) read
    /// it instead of the row's id, which names the outgoing page's row for
    /// the tick a route swap runs page bodies (see `use_center_slot`).
    pub row_ref: NodeRef<html::Div>,
}

#[component]
pub fn TitleBar(
    /// Pinned state: while on, the bar never auto-hides.
    pinned: RwSignal<bool>,
    /// Called with the new value whenever the pin toggles (persistence).
    on_pin_change: Callback<bool>,
    /// Extra hold from outside the bar (e.g. the open floating search).
    extra_hold: Signal<bool>,
    /// True while a docked sidebar owns the left inset, so the hover band
    /// starts at `left-72` (the rail's `w-72`). A floating rail paints above
    /// the band instead, and the band keeps the full window width.
    band_inset: Signal<bool>,
    /// The row's left padding in px — the 88px traffic-light gutter while
    /// the bar owes the lights one, the resting padding once the corner
    /// belongs to something else. Computed by the shell controller's
    /// `titlebar_left_gutter`, which owns the rule.
    #[prop(into)] left_gutter: Signal<f64>,
    #[prop(into)] left: ViewFn,
    /// Center slot (e.g. the document title). Dead center while the
    /// content's natural width clears both clusters, else the free stretch
    /// between them (centered, truncated) — so the caption cluster, pin and
    /// traffic-light gutter are reserved on every platform. Defaults to
    /// empty.
    #[prop(into, default = ViewFn::from(|| ()))]
    center: ViewFn,
    #[prop(into)] right: ViewFn,
    /// The row's far-edge cluster — the frameless caption buttons on
    /// Windows/Linux, rendered after the right cluster and flush to the
    /// window's right edge. Empty wherever the OS draws its own controls, so
    /// the shell stays platform-agnostic; `app_title_bar.rs` decides what
    /// runs here.
    #[prop(into, default = ViewFn::from(|| ()))]
    end: ViewFn,
    children: Children,
) -> impl IntoView {
    let held_count = RwSignal::new(0usize);
    let is_held = Signal::derive(move || held_count.get() > 0);
    let center_title_ref = NodeRef::<html::Span>::new();
    // Refs rather than ids: a route swap runs this body a tick before its
    // DOM exists, and the observers must latch onto THIS bar's nodes (see
    // `use_center_slot`).
    let row_ref = NodeRef::<html::Div>::new();
    let trailing_ref = NodeRef::<html::Div>::new();
    // Show on enter, hide after a grace period unless a hold (an open
    // popover, the floating search) or the pin keeps the bar up. The shared
    // reveal owns the timer and the recheck; the shell owns the hold
    // definition. Non-short-circuiting `|`: the effect must track BOTH holds,
    // or a release of the untracked one never settles.
    let hover = use_hover_reveal(HoverConfig {
        delay: DEFAULT_HOVER_DELAY,
        hold: Some(Signal::derive(move || is_held.get() | extra_hold.get())),
        pin: Some(pinned.into()),
    });
    let visible = hover.visible;
    provide_context(TitleBarCtx { visible, held_count, center_title_ref, row_ref });

    let (enter_band, leave_band) = hover.bind();
    let (enter_bar, leave_bar) = hover.bind();
    let sidebar_open = move || band_inset.get();

    // The center slot's resolved box, kept current off live rects (see
    // `use_center_slot`).
    let center_slot = use_center_slot(row_ref, trailing_ref, center_title_ref);

    view! {
        <>
            {children()}
            // The hover band never sits over a DOCKED sidebar (`left-72`
            // while one is open); a floating rail paints above it, so the
            // band stays full width under either.
            <div
                class=format!("absolute top-0 right-0 {BAR} h-12")
                class=("left-72", sidebar_open)
                class=("left-0", move || !sidebar_open())
                data-tauri-drag-region="true"
                on:mouseenter=move |_| enter_band()
                on:mouseleave=move |_| leave_band()
            >
                <div
                    // #toolbar-row: the centered slot's measurement anchor
                    // (see `measure_center_slot`); the page's
                    // #toolbar-leading and this shell's #toolbar-trailing
                    // complete it. The ref carries the same node to the
                    // observers, which must not resolve it by id on a
                    // remount's first tick.
                    id=TOOLBAR_ROW_ID
                    node_ref=row_ref
                    data-tauri-drag-region="true"
                    prop:inert=move || !visible.get()
                    on:mouseenter=move |_| enter_bar()
                    on:mouseleave=move |_| leave_bar()
                    class="toolbar-glass relative flex h-full items-center gap-2 pr-2 transition-opacity duration-200"
                    // The px value is the shell controller's gutter rule.
                    style:padding-left=move || format!("{}px", left_gutter.get())
                    class=("opacity-0", move || !visible.get())
                    class=("pointer-events-none", move || !visible.get())
                >
                    {left.run()}
                    <div
                        // pointer-events-none is load-bearing: in the
                        // exact-center tier this overlay spans the whole row
                        // above the in-flow controls, and left interactive it
                        // would swallow their clicks. The title span
                        // re-enables events for its drag region + tooltip; the
                        // row behind stays the drag region for the rest.
                        class="absolute inset-y-0 flex items-center justify-center overflow-hidden pointer-events-none"
                        style=move || {
                            match center_slot.get() {
                                Some((start, width)) => format!("left:{start:.1}px;width:{width:.1}px"),
                                // First frame, not yet measured: prefer exact
                                // center, over the whole row.
                                None => "left:0;right:0".to_string(),
                            }
                        }
                    >
                        // Below the floor the slot stays an inert,
                        // click-transparent overlay.
                        <Show when=move || center_slot.get().is_none_or(|(_, w)| w >= MIN_CENTER_SLOT)>
                            {center.run()}
                        </Show>
                    </div>
                    <div
                        // #toolbar-trailing: the trailing cluster (right slot
                        // + pin) and the centered slot's right anchor.
                        // Everything right of this edge sits outside the slot.
                        id=TOOLBAR_TRAILING_ID
                        node_ref=trailing_ref
                        class="ml-auto flex shrink-0 items-center gap-1"
                    >
                        {right.run()}
                        <PinButton pinned=pinned on_pin_change=on_pin_change />
                    </div>
                    {end.run()}
                </div>
            </div>
        </>
    }
}

/// Pin toggle. The new value is reported to the caller, which owns
/// persistence.
#[component]
fn PinButton(
    pinned: RwSignal<bool>,
    on_pin_change: Callback<bool>,
) -> impl IntoView {
    view! {
        <Tooltip text="Pin titlebar open">
            <IconButton
                icon=IconName::Pin
                pressed=pinned.into()
                on_click=move || {
                    let next = !pinned.get();
                    pinned.set(next);
                    on_pin_change.run(next);
                }
            />
        </Tooltip>
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_center_slot;

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "expected {expected}, got {actual}");
    }

    #[test]
    fn a_title_clearing_both_clusters_at_center_takes_the_whole_row() {
        let (start, width) = resolve_center_slot(1200.0, 100.0, 1100.0, Some(400.0));
        close(start, 0.0);
        close(width, 1200.0);
    }

    #[test]
    fn asymmetric_clusters_do_not_shift_the_center_while_the_title_fits() {
        // The usual real case: a small leading group, a big trailing one.
        let (start, width) = resolve_center_slot(1100.0, 120.0, 900.0, Some(400.0));
        close(start, 0.0);
        close(width, 1100.0);
    }

    #[test]
    fn a_title_hitting_a_cluster_at_center_falls_back_to_the_free_stretch() {
        let (start, width) = resolve_center_slot(1100.0, 120.0, 900.0, Some(701.0));
        close(start, 120.0);
        close(width, 780.0);
    }

    #[test]
    fn no_center_content_yields_the_empty_free_stretch() {
        let (start, width) = resolve_center_slot(1100.0, 120.0, 900.0, None);
        close(start, 120.0);
        close(width, 780.0);
    }

    #[test]
    fn degenerate_edges_clamp_to_a_valid_box() {
        // Clusters past the middle of each other.
        let (start, width) = resolve_center_slot(500.0, 400.0, 100.0, Some(100.0));
        close(start, 400.0);
        close(width, 0.0);
        // Clusters partly off the row.
        let (start, width) = resolve_center_slot(500.0, -50.0, 100.0, Some(1000.0));
        close(start, 0.0);
        close(width, 100.0);
    }
}
