//! Axis-generic virtualized page strip, shared by the two scrolling layouts.
//!
//! This is the unified replacement for `page_list` (vertical) and the inline
//! loop that used to live inside `HorizontalView`: one component renders the
//! mounted page window along either axis, absolutely positioning each page at
//! the virtualizer's `item_top`, and reporting the rendered main-axis size
//! back into the virtualizer's size model.
//!
//! A zoom resizes this strip for real: the viewer engine rescales the
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
use pdf_core::layout::{Axis, TOOLBAR_H};
use virtual_list_leptos::{VirtualItem, VirtualItemState, Virtualizer};

use crate::components::document::PageCanvas;
use crate::components::document::page_canvas::component::GlossOverlayProps;
use crate::state::{ReaderState, TextureSignal};

#[component]
pub fn PageStrip(
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
    let items = v.items();
    let total_size = v.total_size();

    // Horizontal-only: the strip is at least as tall as the tallest page at
    // the live scale, so a zoom past fit-height yields real vertical scroll
    // range as the zoom happens, not only once it lands.
    let strip_h = Memo::new(move |_| {
        let scale = state.viewer.zoom.display.get();
        let tallest = state
            .document
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

    // Report a rendered page's main-axis extent back into the virtualizer.
    // Vertical uses the measured height (+ gap); horizontal uses the measured
    // width (+ the two horizontal margins, which are part of the main span).
    // BOTH axes refuse to report while a zoom transaction is in flight: the
    // rendered size belongs to the committed geometry, and a mid-tween page
    // stretching to the visual scale would feed the virtualizer a size from
    // a geometry model that does not exist yet. (Only the vertical axis used
    // to be guarded — the asymmetry let the horizontal strip's window model
    // drift during the exact frames it needed to stay still.)
    let on_geometry = match axis {
        Axis::Vertical => {
            let handle = handle.clone();
            Callback::new(move |(page, _w, height): (u32, f64, f64)| {
                if state.viewer.zooming_now() {
                    return;
                }
                let index = page.saturating_sub(1) as usize;
                state.document.metrics.css_heights.update(|heights| {
                    while heights.len() <= index {
                        heights.push(0.0);
                    }
                    heights[index] = height;
                });
                let gap = state.viewer.page_gap.get_untracked();
                handle.with_value(|v| v.report_size(index, height + gap));
            })
        }
        Axis::Horizontal => {
            let handle = handle.clone();
            Callback::new(move |(_page, w, _h): (u32, f64, f64)| {
                if state.viewer.zooming_now() {
                    return;
                }
                if w > 0.0 {
                    let m = state.viewer.page_margin.get_untracked();
                    handle
                        .with_value(|v| v.report_size(_page.saturating_sub(1) as usize, w + 2.0 * m));
                }
            })
        }
    };

    view! {
        <div id=scroller_id node_ref=list_ref class=scroller_class tabindex="0">
            {match axis {
                Axis::Vertical => {
                    let handle = handle.clone();
                    let each_items = items.clone();
                    view! {
                        <div
                            class="relative"
                            style=move || format!("margin-top:{TOOLBAR_H}px")
                        >
                            <div aria-hidden="true" style:height=move || format!("{}px", total_size.get())></div>
                            <For
                                each=move || each_items.get()
                                key=|item: &VirtualItem| item.index
                                children=move |item: VirtualItem| {
                                    let index = item.index;
                                    let page = (index + 1) as u32;
                                    let top = handle.with_value(|v| v.item_top(index));
                                    let dormant = dormant_signal(items.clone(), index);
                                    let style = move || format!(
                                        "position:absolute;top:{}px;left:0;right:0;display:flex;padding-inline:{}px",
                                        top.get(), state.viewer.page_margin.get()
                                    );
                                    view! {
                                        <div id=wrapper_id(Axis::Vertical, index, page) style=style>
                                            <PageCanvas
                                                page=page
                                                scale=page_scale
                                                render_scale=state.viewer.zoom.committed
                                                zoom_animating=state.viewer.zooming()
                                                dormant=dormant
                                                gesture_owns=gesture_owns
                                                texture=texture
                                                canvas_id=format!("cont-{index}-cv")
                                                host_id=format!("cont-{index}-pg")
                                                render_text=true
                                                on_geometry=on_geometry
                                                gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
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
                    let handle = handle.clone();
                    let each_items = items.clone();
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
                                    let dormant = dormant_signal(items.clone(), index);
                                    // top:0 — the strip owns the full window height and
                                    // the auto-hiding title bar overlays it, like Spread.
                                    let style = move || format!(
                                        "position:absolute;top:0;left:{}px;height:100%;display:flex;padding-inline:{}px",
                                        left.get(), state.viewer.page_margin.get()
                                    );
                                    view! {
                                        <div id=wrapper_id(Axis::Horizontal, index, page) style=style>
                                            <PageCanvas
                                                page=page
                                                scale=page_scale
                                                render_scale=state.viewer.zoom.committed
                                                zoom_animating=state.viewer.zooming()
                                                dormant=dormant
                                                gesture_owns=gesture_owns
                                                texture=texture
                                                canvas_id=format!("hp-{page}-cv")
                                                host_id=format!("hp-{page}-pg")
                                                render_text=true
                                                on_geometry=on_geometry
                                                gloss_overlay=GlossOverlayProps::from_gloss(state.gloss)
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

/// Per-axis wrapper id, kept as a free function so both ends of the strip's
/// `<For>` can name it without capturing anything by move.
fn wrapper_id(axis: Axis, index: usize, page: u32) -> String {
    match axis {
        Axis::Vertical => format!("cont-{index}-wrap"),
        Axis::Horizontal => format!("hp-{page}-wrap"),
    }
}

/// Whether one mounted item is currently a RETAINED ZOMBIE — freshly evicted
/// from the window and bridged by the virtualizer's retention grace. A
/// zombie page keeps its DOM and its last bitmap; it must not start new
/// expensive work (a crisp re-render) for the few frames it has left.
fn dormant_signal(items: Signal<Vec<VirtualItem>, LocalStorage>, index: usize) -> Signal<bool, LocalStorage> {
    Signal::derive_local(move || {
        items
            .get()
            .iter()
            .any(|item| item.index == index && item.state == VirtualItemState::Zombie)
    })
}
