//! Axis-generic virtualized page strip, shared by the two scrolling layouts.

use leptos::html;
use leptos::prelude::*;
use reader_core::view::Axis;
use virtual_list_leptos::{VirtualItem, VirtualItemState, Virtualizer};

use super::canvas::{GlossOverlayProps, PdfPageCanvas};
use crate::components::viewer::page_host::{canvas_id_for_axis, host_id_for_axis};
use crate::state::{ReaderState, TextureSignal};
use pdf_core::pixel_grid::{one_device_px, snap_px};

#[component]
pub fn PdfPageStrip(
    state: ReaderState,
    virtualizer: Virtualizer,
    axis: Axis,
    /// The scroller element this strip lays out into (owned by ScrollShell).
    scroller_id: &'static str,
    list_ref: NodeRef<html::Div>,
) -> impl IntoView {
    let texture =
        use_context::<TextureSignal>().expect("TextureSignal is provided by the pane realm");

    let v = virtualizer;
    let handle = StoredValue::new_local(v.clone());
    // The live VISUAL scale: hosts stretch what they hold. Rasterisation
    // follows `committed`.
    let page_scale = state.viewer.zoom.display.read_only();
    let gesture_owns = state.viewer.gesture_owns();
    // Fling gate input: while the scroller moves, offscreen pages stay blank.
    let settled: Signal<bool> = v.settled().into();
    let items = v.items();
    let total_size = v.total_size();

    // Horizontal-only: min-height is the tallest page, so a zoom past
    // fit-height yields vertical range immediately.
    let strip_h = Memo::new(move |_| {
        let scale = state.viewer.zoom.display.get();
        let tallest = state
            .document
            .content
            .metrics
            .intrinsic
            .with(|pages| pages.iter().map(|p| p.height).fold(0.0, f64::max));
        tallest * scale
    });

    let scroller_class = match axis {
        Axis::Vertical => "scrollbar-none h-full w-full overflow-y-auto outline-none",
        Axis::Horizontal => {
            "scrollbar-none h-full w-full overflow-x-auto overflow-y-auto outline-none"
        }
    };

    // A report can outlive the strip: the pane's generation, then
    // `report_alive`, keeps it honest.
    let report_alive = StoredValue::new_local(true);
    on_cleanup({
        move || {
            let _ = report_alive.try_set_value(false);
        }
    });
    // This pane's generation: another pane's open never moves it.
    let report_epoch = state.pane.generation();
    let on_geometry = match axis {
        Axis::Vertical => {
            Callback::new(move |(page, _w, height): (u32, f64, f64)| {
                if report_alive.try_get_value() != Some(true) {
                    return;
                }
                // The unit purges a beat before this scope: probe it
                // before any `update`.
                if state
                    .document
                    .content
                    .metrics
                    .css_heights
                    .try_with_untracked(|heights| heights.len())
                    .is_none()
                {
                    return;
                }
                let Some(gap) = state.viewer.page_gap.try_get_untracked() else {
                    return;
                };
                if !state.pane.owns_generation(report_epoch) {
                    return;
                }
                if state.viewer.try_zooming_now() != Some(false) {
                    return;
                }
                let index = page.saturating_sub(1) as usize;
                state
                    .document
                    .content
                    .metrics
                    .css_heights
                    .update(|heights| {
                        while heights.len() <= index {
                            heights.push(0.0);
                        }
                        heights[index] = height;
                    });
                handle.with_value(|v| v.report_size(index, height + gap));
                // First-paint gate lifts here: a report means the
                // reader's page has pixels (see `crate::pane`).
                if page == state.viewer.page.get_untracked()
                    && !state.viewer.first_paint.get_untracked()
                {
                    state.viewer.first_paint.set(true);
                }
            })
        }
        Axis::Horizontal => {
            Callback::new(move |(page, w, _h): (u32, f64, f64)| {
                if report_alive.try_get_value() != Some(true) {
                    return;
                }
                // Same reader-state probe as the vertical arm.
                let Some(m) = state.viewer.page_margin.try_get_untracked() else {
                    return;
                };
                if !state.pane.owns_generation(report_epoch) {
                    return;
                }
                if state.viewer.try_zooming_now() != Some(false) {
                    return;
                }
                if w > 0.0 {
                    handle.with_value(|v| {
                        v.report_size(page.saturating_sub(1) as usize, w + 2.0 * m)
                    });
                    // Same gate as the vertical arm, same reason.
                    if page == state.viewer.page.get_untracked()
                        && !state.viewer.first_paint.get_untracked()
                    {
                        state.viewer.first_paint.set(true);
                    }
                }
            })
        }
    };

    view! {
        <div id=scroller_id node_ref=list_ref class=scroller_class tabindex="0" data-page-strip="">
            {match axis {
                Axis::Vertical => {
                    let each_items = items;
                    view! {
                        <div class="relative">
                            <div aria-hidden="true" data-strip-extent="vertical" style:height=move || format!("{}px", total_size.get())></div>
                            <For
                                each=move || each_items.get()
                                key=|item: &VirtualItem| item.index
                                children=move |item: VirtualItem| {
                                    let index = item.index;
                                    let page = (index + 1) as u32;
                                    let top = handle.with_value(|v| v.item_top(index));
                                    let dormant = dormant_signal(items, index);
                                    let in_view = in_view_signal(items, index);
                                    let rank = handle.with_value(|c| rank_signal(items, c, index));
                                    // Snapped like the sizes: no-gap mode
                                    // overlaps by one device pixel.
                                    let style = move || {
                                        let overlap = if index > 0
                                            && state.viewer.page_gap.get() <= 1e-9
                                        {
                                            one_device_px()
                                        } else {
                                            0.0
                                        };
                                        format!(
                                            "position:absolute;top:{}px;left:0;right:0;display:flex;padding-inline:{}px",
                                            snap_px(top.get()) - overlap,
                                            state.viewer.page_margin.get(),
                                        )
                                    };
                                    view! {
                                        <div id=wrapper_id(Axis::Vertical, index, page) style=style>
                                            <PdfPageCanvas
                                                page=page
                                                scale=page_scale
                                                render_scale=state.viewer.zoom.committed
                                                zoom_animating=state.viewer.zooming()
                                                dormant=dormant
                                                settled=settled
                                                in_view=in_view
                                                rank=rank
                                                gesture_owns=gesture_owns
                                                texture=texture
                                                canvas_id=canvas_id_for_axis(axis, page)
                                                host_id=host_id_for_axis(axis, page)
                                                render_text=true
                                                on_geometry=on_geometry
                                                on_rendered=crate::zoom::target::page_rendered(state)
                                                on_sized=crate::zoom::target::page_sized_cb(state)
                        gloss_overlay=GlossOverlayProps::from_gloss(state)
                                                class="mx-auto"
                                            />
                                        </div>
                                    }
                                }
                            />
                        </div>
                    }.into_any()
                }
                Axis::Horizontal => {
                    let each_items = items;
                    view! {
                        <div
                            class="relative"
                            data-strip-extent="horizontal"
                            style=move || {
                                format!(
                                    "width:{}px;height:max(100%, {}px)",
                                    total_size.get(),
                                    strip_h.get().ceil()
                                )
                            }
                        >
                            <For
                                each=move || each_items.get()
                                key=|item: &VirtualItem| item.index
                                children=move |item: VirtualItem| {
                                    let index = item.index;
                                    let page = (index + 1) as u32;
                                    let left = handle.with_value(|v| v.item_top(index));
                                    let dormant = dormant_signal(items, index);
                                    let in_view = in_view_signal(items, index);
                                    let rank = handle.with_value(|c| rank_signal(items, c, index));
                                    // The auto-hiding title bar overlays
                                    // this strip, so it owns full height.
                                    let style = move || format!(
                                        "position:absolute;top:0;left:{}px;height:100%;display:flex;padding-inline:{}px",
                                        snap_px(left.get()), state.viewer.page_margin.get()
                                    );
                                    view! {
                                        <div id=wrapper_id(Axis::Horizontal, index, page) style=style>
                                            <PdfPageCanvas
                                                page=page
                                                scale=page_scale
                                                render_scale=state.viewer.zoom.committed
                                                zoom_animating=state.viewer.zooming()
                                                dormant=dormant
                                                settled=settled
                                                in_view=in_view
                                                rank=rank
                                                gesture_owns=gesture_owns
                                                texture=texture
                                                canvas_id=canvas_id_for_axis(axis, page)
                                                host_id=host_id_for_axis(axis, page)
                                                render_text=true
                                                on_geometry=on_geometry
                                                on_rendered=crate::zoom::target::page_rendered(state)
                                                on_sized=crate::zoom::target::page_sized_cb(state)
                        gloss_overlay=GlossOverlayProps::from_gloss(state)
                                                class="my-auto"
                                            />
                                        </div>
                                    }
                                }
                            />
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

/// A free function so both ends of the `<For>` can name it without moving.
fn wrapper_id(axis: Axis, index: usize, page: u32) -> String {
    match axis {
        Axis::Vertical => format!("cont-{index}-wrap"),
        Axis::Horizontal => format!("hp-{page}-wrap"),
    }
}

/// A zombie holds its DOM and last bitmap: no new expensive work for it.
fn dormant_signal(
    items: Signal<Vec<VirtualItem>, LocalStorage>,
    index: usize,
) -> Signal<bool, LocalStorage> {
    Signal::derive_local(move || {
        items
            .get()
            .iter()
            .any(|item| item.index == index && item.state == VirtualItemState::Zombie)
    })
}

/// The band's class first, then distance from the landing index.
fn rank_signal(
    items: Signal<Vec<VirtualItem>, LocalStorage>,
    virt: &Virtualizer,
    index: usize,
) -> Signal<u32, LocalStorage> {
    /// Wide enough that distance can never carry a page into the next class.
    const CLASS: u32 = 1 << 16;
    let v = virt.clone();
    Signal::derive_local(move || {
        // Reading `items` is what re-derives this when the band moves.
        let _ = items.get();
        let distance = (index as i64 - v.landing_index() as i64)
            .unsigned_abs()
            .min((CLASS - 1) as u64) as u32;
        u32::from(v.fill_priority(index).rank())
            .saturating_mul(CLASS)
            .saturating_add(distance)
    })
}

/// The virtualizer's own band answers, so no local estimate can disagree.
fn in_view_signal(
    items: Signal<Vec<VirtualItem>, LocalStorage>,
    index: usize,
) -> Signal<bool, LocalStorage> {
    Signal::derive_local(move || {
        items
            .get()
            .iter()
            .any(|item| item.index == index && item.state == VirtualItemState::Active)
    })
}
