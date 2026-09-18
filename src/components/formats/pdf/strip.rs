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
//! The strip is also where the virtualizer's TIERS become visible. A strip
//! running an adaptive policy mounts more pages than it rasterises, and each
//! mounted item says which of the three it is owed: [`VirtualItemState::Active`]
//! gets a page host and a full raster, [`VirtualItemState::Preview`] the same
//! host at a fraction of the resolution and with no text layer, and
//! [`VirtualItemState::Blank`] a placeholder box with no canvas in it at all
//! ([`PdfPagePlaceholder`]). A retained zombie keeps the host and the bitmap it
//! already has, and starts no new work. That is what makes a wider mount window
//! affordable: the pages a fling flies past are boxes, not rasters.
//!
//! The same frame is published to the engine's render scheduler
//! ([`use_motion_bridge`]), so the queue in front of pdf.js is ordered by where
//! the reader is going rather than by the order the pages were mounted in.
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
use super::placeholder::PdfPagePlaceholder;
use crate::components::viewer::page_host::{canvas_id_for_axis, host_id_for_axis};
use crate::features::reader::motion::use_motion_bridge;
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
    // Both derived ONCE for the strip, not per page and not per tier crossing:
    // each call mints a new memo node, and a page that crosses tiers a hundred
    // times in a long scroll would leave a hundred of them behind on the owner
    // that outlives it.
    let zoom_animating = state.viewer.zooming();
    // The fling gate's input: while the scroller is still moving, unpainted
    // pages stay on their thumbnail underlay and rasterise once the strip
    // settles (see the page host's SCROLL-FLING GATE). The prop wraps it in
    // the Option the page-mode hosts default to.
    let settled: Signal<bool> = v.settled().into();
    let items = v.items();
    let total_size = v.total_size();
    // Publish this strip's motion to the engine's scheduler: the phase, the
    // direction, the predicted destination and the two tier windows, once per
    // frame that changes any of them. The strip owns the scroller, so it is the
    // only place that knows which virtualizer is live.
    use_motion_bridge(&v);

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
        <div id=scroller_id node_ref=list_ref class=scroller_class tabindex="0">
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
                                    let size = handle.with_value(|v| v.item_size(index));
                                    let (dormant, preview, content) = tiers(&handle, index);
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
                                            {move || {
                                                if content.get() {
                                                    view! {
                                                        <PdfPageCanvas
                                                            page=page
                                                            scale=page_scale
                                                            render_scale=state.viewer.zoom.committed
                                                            zoom_animating=zoom_animating
                                                            dormant=dormant
                                                            preview=preview
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
                                                    }
                                                        .into_any()
                                                } else {
                                                    view! {
                                                        <PdfPagePlaceholder
                                                            state=state
                                                            axis=Axis::Vertical
                                                            index=index
                                                            size=size
                                                            texture=texture
                                                            class="mx-auto"
                                                        />
                                                    }
                                                        .into_any()
                                                }
                                            }}
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
                                    let size = handle.with_value(|v| v.item_size(index));
                                    let (dormant, preview, content) = tiers(&handle, index);
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
                                            {move || {
                                                if content.get() {
                                                    view! {
                                                        <PdfPageCanvas
                                                            page=page
                                                            scale=page_scale
                                                            render_scale=state.viewer.zoom.committed
                                                            zoom_animating=zoom_animating
                                                            dormant=dormant
                                                            preview=preview
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
                                                    }
                                                        .into_any()
                                                } else {
                                                    view! {
                                                        <PdfPagePlaceholder
                                                            state=state
                                                            axis=Axis::Horizontal
                                                            index=index
                                                            size=size
                                                            texture=texture
                                                            class="my-auto"
                                                        />
                                                    }
                                                        .into_any()
                                                }
                                            }}
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

/// The three booleans one mounted child reads off its item's tier.
///
/// The tier is a signal rather than the [`VirtualItem`] snapshot the `<For>`
/// child was handed, because a child does not re-run for a key it already holds
/// — a scroll that promotes a page has to reach the view through a reactive
/// read. All three are projections of it, so they cannot disagree about which
/// tier a page is in: a zombie keeps its bitmap and starts no new work, a
/// preview is owed a cheap raster and no text layer, and a page that is neither
/// is owed the full thing.
///
/// `content` is the coarse one, and it is the only one the view SWITCHES on —
/// deliberately. It stays true across every tier that paints, so a page moving
/// between the full tier, the preview ring and retention keeps its host and
/// re-renders through the props; only the crossing into or out of the
/// placeholder tier rebuilds. Switching on the tier itself would tear the host
/// down on every promotion: unregister the canvas, drop the bitmap it just
/// made, and start the crisp render from nothing — a flash where a scroll
/// should have been a sharpening.
fn tiers(
    handle: &StoredValue<Virtualizer, LocalStorage>,
    index: usize,
) -> (
    Signal<bool, LocalStorage>,
    Signal<bool, LocalStorage>,
    Signal<bool, LocalStorage>,
) {
    let tier = handle.with_value(|v| v.item_state(index));
    let dormant = Signal::derive_local(move || tier.get() == VirtualItemState::Zombie);
    let preview = Signal::derive_local(move || tier.get() == VirtualItemState::Preview);
    let content = Signal::derive_local(move || tier.get().paints());
    (dormant, preview, content)
}
