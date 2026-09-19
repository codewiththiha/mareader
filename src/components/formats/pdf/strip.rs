//! Axis-generic virtualized page strip, shared by the two scrolling layouts.
//!
//! This is the unified replacement for the vertical page list and the inline
//! loop the horizontal layout used to carry itself: one component renders the
//! mounted page window along either axis, absolutely positioning each page at
//! the virtualizer's `item_top`, and reporting the rendered main-axis size
//! back into the virtualizer's size model.
//!
//! A zoom resizes this strip for real: the zoom actuator rescales the
//! virtualizer's items frame by frame and holds the document point under the
//! viewport centre still, while the page hosts below stretch the bitmap they
//! already hold to the new size. Nothing here animates a transform over
//! frozen geometry — a CSS `scale()` would scale the page gaps along with the
//! pages, and the layout deliberately does not.
//!
//! The strip is pure presentation. It owns no scroll policy, no wheel
//! translation, no container binding — those live in [`ScrollShell`], which
//! creates the scroller element this strip draws into. The page-host ids keep
//! their per-axis prefixes (`cont-` / `hp-`) because the engine's selection
//! and the AI gloss layer parse them back into page numbers.
//!
//! Every offset this strip writes is snapped to the device-pixel grid (see
//! [`pdf_core::pixel_grid`]), because the sizes the page
//! hosts write are: a wrapper positioned half a device pixel off its page's
//! painted edge is exactly the compositor seam that snapping exists to close.
//!
//! Cross-axis centering is the same rule in every layout: the page host is
//! centred with an AUTO margin (`mx-auto` here, `my-auto` in the horizontal
//! strip, `m-auto` in [`PageShell`]) rather than flex `justify-content` /
//! `align-items`. An auto margin centres the page when it fits and degrades to
//! start-alignment when it overflows, so a zoomed page that is wider (or
//! taller) than the viewport scrolls to BOTH its edges — the near edge is
//! never clipped. Flex centering instead overflows symmetrically, which makes
//! the near edge unreachable: the whole point of the margin-auto degrade.

use leptos::html;
use leptos::prelude::*;
use reader_core::view::Axis;
use virtual_list_leptos::{VirtualItem, VirtualItemState, Virtualizer};

use super::canvas::{GlossOverlayProps, PdfPageCanvas};
use crate::components::viewer::page_host::{canvas_id_for_axis, host_id_for_axis};
use pdf_core::pixel_grid::{one_device_px, snap_px};
use crate::state::{ReaderState, TextureSignal};

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
        use_context::<TextureSignal>().expect("TextureSignal must be provided by app bootstrap");

    let v = virtualizer;
    let handle = StoredValue::new_local(v.clone());
    // The live VISUAL scale. Hosts size themselves to it and CSS-stretch
    // whatever bitmap they already hold, so a zoom resizes the page every
    // frame without kicking off a render; the crisp rasterisation follows
    // `render_scale` (`committed`), which moves only when the transaction
    // lands.
    let page_scale = state.viewer.zoom.display.read_only();
    let gesture_owns = state.viewer.gesture_owns();
    // A settled transition retries canceled cold paints, but no longer blocks
    // ordinary scrolling. The JS scheduler alone distinguishes tracking/fling.
    let settled: Signal<bool> = v.settled().into();
    let dominant = v.dominant();
    let items = reading_window(v.clone(), state);
    let total_size = v.total_size();

    // Horizontal-only: the strip is at least as tall as the tallest page at
    // the live scale, so a zoom past fit-height yields real vertical scroll
    // range as the zoom happens, not only once it lands.
    let strip_h = Memo::new(move |_| {
        let scale = state.viewer.zoom.display.get();
        let tallest = state
            .document
            .content.metrics
            .intrinsic
            .with(|pages| pages.iter().map(|p| p.height).fold(0.0, f64::max));
        tallest * scale
    });

    let scroller_axis = match axis { Axis::Vertical => "vertical", Axis::Horizontal => "horizontal" };
    let scroller_class = match axis {
        Axis::Vertical => "scrollbar-none h-full w-full overflow-y-auto outline-none",
        Axis::Horizontal => {
            "scrollbar-none h-full w-full overflow-x-auto overflow-y-auto outline-none"
        }
    };

    // Report a rendered page's main-axis extent back into the virtualizer.
    // Vertical uses the measured height (+ gap); horizontal uses the measured
    // width (+ the two horizontal margins, which are part of the main span).
    // The sizes arriving here are already snapped to the device-pixel grid by
    // the page host, so the offsets the virtualizer derives from them are
    // grid-aligned too, and every page's wrapper sits exactly on the edge the
    // page above it painted.
    // BOTH axes refuse to report while a zoom transaction is in flight: the
    // rendered size belongs to the committed geometry, and a mid-tween page
    // stretching to the visual scale would feed the virtualizer a size from
    // a geometry model that does not exist yet. (Only the vertical axis used
    // to be guarded — the asymmetry let the horizontal strip's window model
    // drift during the exact frames it needed to stay still.)
    let on_geometry = match axis {
        Axis::Vertical => {
            Callback::new(move |(page, _w, height): (u32, f64, f64)| {
                if state.viewer.zooming_now() {
                    return;
                }
                let index = page.saturating_sub(1) as usize;
                state.document.content.metrics.css_heights.update(|heights| {
                    while heights.len() <= index {
                        heights.push(0.0);
                    }
                    heights[index] = height;
                });
                let gap = state.viewer.page_gap.get_untracked();
                handle.with_value(|v| v.report_size(index, height + gap));
                // The first-paint gate lifts HERE: a geometry report only
                // arrives when a page render completes, and the fresh open's
                // window mounts around the resume page — so the first report
                // means the reader's page has pixels. Until then the loader
                // cover owns the slot (see `crate::features::reader::page`).
                if page == state.viewer.page.get_untracked()
                    && !state.viewer.first_paint.get_untracked()
                {
                    state.viewer.first_paint.set(true);
                }
            })
        }
        Axis::Horizontal => {
            Callback::new(move |(page, w, _h): (u32, f64, f64)| {
                if state.viewer.zooming_now() {
                    return;
                }
                if w > 0.0 {
                    let m = state.viewer.page_margin.get_untracked();
                    handle
                        .with_value(|v| v.report_size(page.saturating_sub(1) as usize, w + 2.0 * m));
                    // Same gate as the vertical arm, same reason: the report
                    // is a completed render, and the window is the resume
                    // page's.
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
        <div id=scroller_id node_ref=list_ref class=scroller_class tabindex="0"
            data-raster-axis=scroller_axis
            data-raster-anchor=move || dominant.get() + 1
        >
            {match axis {
                Axis::Vertical => {
                    let each_items = items;
                    view! {
                        <div class="relative">
                            <div aria-hidden="true" style:height=move || format!("{}px", total_size.get())></div>
                            <For
                                each=move || each_items.get()
                                key=|item: &VirtualItem| item.index
                                children=move |item: VirtualItem| {
                                    let index = item.index;
                                    let page = (index + 1) as u32;
                                    let top = handle.with_value(|v| v.item_top(index));
                                    let dormant = dormant_signal(items, index);
                                    // Offsets are snapped for the same reason
                                    // sizes are: the wrapper's top is a running
                                    // sum of page extents at the live scale, so
                                    // it lands mid-device-pixel at most zoom
                                    // levels and the joint between two pages
                                    // rounds into a hairline of backdrop.
                                    //
                                    // In no-gap mode snapping alone still leaves
                                    // the two rects merely TOUCHING; pull every
                                    // page after the first up by one device
                                    // pixel so they always overlap instead. The
                                    // host is opaque and pages composite
                                    // source-over against each other, so the
                                    // overlap is invisible — but a gap can no
                                    // longer open up.
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
                                            <Show
                                                when=move || !dormant.get()
                                                fallback=move || view! {
                                                    <div class="pdf-page m-auto" aria-hidden="true"
                                                        style=move || placeholder_style(state, index)></div>
                                                }
                                            >
                                            <PdfPageCanvas
                                                page=page
                                                initial_size=intrinsic_size(state, index)
                                                scale=page_scale
                                                render_scale=state.viewer.zoom.committed
                                                zoom_animating=state.viewer.zooming()
                                                settled=settled
                                                gesture_owns=gesture_owns
                                                texture=texture
                                                canvas_id=canvas_id_for_axis(axis, page)
                                                host_id=host_id_for_axis(axis, page)
                                                render_text=true
                                                on_geometry=on_geometry
                                                gloss_overlay=GlossOverlayProps::from_gloss(state)
                                                class="mx-auto"
                                            />
                                            </Show>
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
                                    // top:0 — the strip owns the full window height and
                                    // the auto-hiding title bar overlays it, like Spread.
                                    // The main-axis offset is snapped to the device-pixel
                                    // grid, same as the vertical strip's `top`: an
                                    // unsnapped left edge rounds against the gutter behind
                                    // it and paints a hairline down the side of the page.
                                    let style = move || format!(
                                        "position:absolute;top:0;left:{}px;height:100%;display:flex;padding-inline:{}px",
                                        snap_px(left.get()), state.viewer.page_margin.get()
                                    );
                                    view! {
                                        <div id=wrapper_id(Axis::Horizontal, index, page) style=style>
                                            <Show
                                                when=move || !dormant.get()
                                                fallback=move || view! {
                                                    <div class="pdf-page m-auto" aria-hidden="true"
                                                        style=move || placeholder_style(state, index)></div>
                                                }
                                            >
                                            <PdfPageCanvas
                                                page=page
                                                initial_size=intrinsic_size(state, index)
                                                scale=page_scale
                                                render_scale=state.viewer.zoom.committed
                                                zoom_animating=state.viewer.zooming()
                                                settled=settled
                                                gesture_owns=gesture_owns
                                                texture=texture
                                                canvas_id=canvas_id_for_axis(axis, page)
                                                host_id=host_id_for_axis(axis, page)
                                                render_text=true
                                                on_geometry=on_geometry
                                                gloss_overlay=GlossOverlayProps::from_gloss(state)
                                                class="my-auto"
                                            />
                                            </Show>
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

/// Per-axis wrapper id, kept as a free function so both ends of the strip's
/// `<For>` can name it without capturing anything by move.
fn wrapper_id(axis: Axis, index: usize, page: u32) -> String {
    match axis {
        Axis::Vertical => format!("cont-{index}-wrap"),
        Axis::Horizontal => format!("hp-{page}-wrap"),
    }
}

/// Keep a small PAGE-based warm window as well as the virtualizer's pixel
/// band. A screen-only overscan cannot contain the next page when a zoomed
/// current page is several screens tall. Extra hosts still use the SAME
/// geometry model; no second layout or scroll anchoring path is introduced.
const READ_AHEAD_PAGES: usize = 2;

fn reading_window(v: Virtualizer, state: ReaderState) -> Signal<Vec<VirtualItem>, LocalStorage> {
    let source = v.items();
    let dominant = v.dominant();
    let total = v.total_size();
    Signal::derive_local(move || {
        let mut items = source.get();
        let count = state.document.num_pages.get() as usize;
        if count == 0 {
            return items;
        }
        let center = dominant.get().min(count - 1);
        let last = center.saturating_add(READ_AHEAD_PAGES).min(count - 1);
        for index in center.saturating_sub(READ_AHEAD_PAGES)..=last {
            if let Some(item) = items.iter_mut().find(|item| item.index == index) {
                // A previous fringe/zombie still belongs to the warm window:
                // preserve its keyed canvas instead of discarding good pixels.
                item.state = VirtualItemState::Active;
            } else {
                let start = v.offset_of(index);
                let end = if index + 1 < count { v.offset_of(index + 1) } else { total.get() };
                items.push(VirtualItem {
                    index, start, size: (end - start).max(0.0),
                    cross_start: 0.0, cross_size: 0.0, row: index,
                    state: VirtualItemState::Active,
                });
            }
        }
        items.sort_by_key(|item| item.index);
        items
    })
}

/// Outside the pixel band AND warm page window, keep geometry only.
fn dormant_signal(items: Signal<Vec<VirtualItem>, LocalStorage>, index: usize) -> Signal<bool, LocalStorage> {
    Signal::derive_local(move || {
        !items.get().iter().any(|item| item.index == index && item.state == VirtualItemState::Active)
    })
}

fn intrinsic_size(state: ReaderState, index: usize) -> (f64, f64) {
    let size = state.document.content.metrics.intrinsic.with(|sizes| sizes.get(index).cloned())
        .or_else(|| state.document.content.metrics.page1_size.get());
    size.map_or((0.0, 0.0), |size| (size.width, size.height))
}

fn placeholder_style(state: ReaderState, index: usize) -> String {
    let scale = state.viewer.zoom.display.get();
    let (width, height) = intrinsic_size(state, index);
    format!("width:{}px;height:{}px", snap_px(width * scale), snap_px(height * scale))
}
