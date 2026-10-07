//! Generic hover and grab titlebar shell with render-prop slots.

use leptos::children::ViewFn;
use leptos::html;
use leptos::prelude::*;

use crate::hooks::dom::{
    TOOLBAR_CENTER_TITLE_ID, TOOLBAR_LEADING_ID, TOOLBAR_ROW_ID, TOOLBAR_TRAILING_ID, by_id,
};
use crate::hooks::use_resize_observer::observe_elements;
use crate::hooks::use_window_event::use_window_event;
use crate::hooks::{DEFAULT_HOVER_DELAY, HoverConfig, use_hover_reveal};
use crate::icon::IconName;
use crate::icon_button::IconButton;
use crate::layers::BAR;
use crate::tooltip::Tooltip;

/// Breathing room between the measured clusters and the centered slot.
const CENTER_GAP: f64 = 8.0;
/// Below this width a centered title is a stub ("R…") — hide it instead.
const MIN_CENTER_SLOT: f64 = 60.0;

/// The box the center content renders in, `(start, width)` in row
/// coordinates.
fn resolve_center_slot(
    row_width: f64,
    left: f64,
    right: f64,
    title_width: Option<f64>,
) -> (f64, f64) {
    let center = row_width * 0.5;
    // At the row center it spans center ± w/2; fits while both ends clear.
    let fits_center =
        title_width.is_some_and(|w| w <= 2.0 * (center - left) && w <= 2.0 * (right - center));
    if fits_center {
        return (0.0, row_width);
    }
    let start = left.max(0.0);
    (start, (right - start).max(0.0))
}

/// The live measurement behind [`resolve_center_slot`].
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
    Some(resolve_center_slot(
        row_rect.width(),
        left,
        right,
        title_width,
    ))
}

/// Coalesced slot re-measure, written only when the value changes.
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

/// Wire the center slot's resolved box from row, cluster and title
/// observations.
fn use_center_slot(
    row_ref: NodeRef<html::Div>,
    trailing_ref: NodeRef<html::Div>,
    center_title_ref: NodeRef<html::Span>,
) -> RwSignal<Option<(f64, f64)>> {
    let center_slot = RwSignal::new(None::<(f64, f64)>);
    Effect::new(move |_| {
        // No row yet: the build that sets this ref re-runs the effect.
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
    /// The row's node, reactive across page remounts.
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
    /// True while a docked sidebar owns the left inset.
    band_inset: Signal<bool>,
    /// The row's left padding in px: the light gutter, or the resting one.
    #[prop(into)]
    left_gutter: Signal<f64>,
    #[prop(into)] left: ViewFn,
    /// Center slot; dead center while it clears both clusters, else the
    /// stretch between them.
    #[prop(into, default = ViewFn::from(|| ()))]
    center: ViewFn,
    #[prop(into)] right: ViewFn,
    /// The row's far-edge cluster: the frameless caption buttons.
    #[prop(into, default = ViewFn::from(|| ()))]
    end: ViewFn,
    children: Children,
) -> impl IntoView {
    let held_count = RwSignal::new(0usize);
    let is_held = Signal::derive(move || held_count.get() > 0);
    let center_title_ref = NodeRef::<html::Span>::new();
    // Refs rather than ids: this body runs a tick before its DOM exists.
    let row_ref = NodeRef::<html::Div>::new();
    let trailing_ref = NodeRef::<html::Div>::new();
    // Show on enter, hide after a grace unless a hold or the pin keeps it.
    let hover = use_hover_reveal(HoverConfig {
        delay: DEFAULT_HOVER_DELAY,
        hold: Some(Signal::derive(move || is_held.get() | extra_hold.get())),
        pin: Some(pinned.into()),
    });
    let visible = hover.visible;
    provide_context(TitleBarCtx {
        visible,
        held_count,
        center_title_ref,
        row_ref,
    });

    let (enter_band, leave_band) = hover.bind();
    let (enter_bar, leave_bar) = hover.bind();
    let sidebar_open = move || band_inset.get();

    // The center slot's resolved box, kept current off live rects (see
    // `use_center_slot`).
    let center_slot = use_center_slot(row_ref, trailing_ref, center_title_ref);

    view! {
        <>
            {children()}
            // The hover band never sits over a DOCKED sidebar.
            <div
                class=format!("absolute top-0 right-0 {BAR} h-12")
                class=("left-72", sidebar_open)
                class=("left-0", move || !sidebar_open())
                data-tauri-drag-region="deep"
                on:mouseenter=move |_| enter_band()
                on:mouseleave=move |_| leave_band()
            >
                <div
                    // #toolbar-row: the centered slot's measurement anchor.
                    id=TOOLBAR_ROW_ID
                    node_ref=row_ref
                    // "deep", not "true": the row is a CONTAINER of children.
                    data-tauri-drag-region="deep"
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
                        // pointer-events-none is load-bearing: it spans the row
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
                        // #toolbar-trailing: the trailing cluster.
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
fn PinButton(pinned: RwSignal<bool>, on_pin_change: Callback<bool>) -> impl IntoView {
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
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
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
