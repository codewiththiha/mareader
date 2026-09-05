//! The virtualized strip of reflowable pages — the counterpart of
//! [`PdfPageStrip`](crate::components::formats::pdf::PdfPageStrip) for a document
//! made of type.
//!
//! The shape is the same (mounted window, absolutely-positioned hosts at the
//! virtualizer's offsets, device-pixel snapping), and one difference carries all
//! the others: a text page's size is KNOWN — every page is A4 — so this strip
//! never reports geometry back. There is no `on_geometry`, no render scale to
//! reconcile, no bitmap to stretch: the host IS the content, sized from the live
//! display scale, and the virtualizer's size model is seeded exact by the open
//! flow.
//!
//! Both axes are honest here, exactly as in the PDF strip, and the page host picks
//! this component by axis without knowing what it lays out. Vertical reading of a
//! reflowable document is NOT this strip — it is the continuous block stream (see
//! [`ReflowStreamLayout`](super::ReflowStreamLayout)); this is what the horizontal
//! scroll mode and any future paged-with-gaps mode walk. Like its PDF twin, the
//! strip is pure presentation: scroll policy and container binding live in
//! [`ScrollShell`](crate::components::viewer::shells::scroll_shell::ScrollShell).

use leptos::html;
use leptos::prelude::*;
use reader_core::view::Axis;
use virtual_list_leptos::{VirtualItem, Virtualizer};

use pdf_core::pixel_grid::snap_px;
use reflow_core::geometry::PAGE_HEIGHT;

use super::page::ReflowPage;
use crate::components::viewer::page_host::host_id_for_axis;
use crate::components::viewer::texture_surface::{texture_class, zoom_style};
use crate::state::ReaderState;

#[component]
pub fn ReflowPageStrip(
    state: ReaderState,
    virtualizer: Virtualizer,
    /// The strip's axis. The offsets, the scroll bars and the sizing rule all
    /// follow it — the same three differences the PDF strip has.
    #[prop(default = Axis::Vertical)]
    axis: Axis,
    /// The scroller element this strip lays out into (owned by the shell).
    scroller_id: &'static str,
    list_ref: NodeRef<html::Div>,
) -> impl IntoView {
    let v = virtualizer;
    let handle = StoredValue::new_local(v.clone());
    let items = v.items();
    let total_size = v.total_size();
    let page_scale = state.viewer.zoom.display.read_only();

    // The strip is at least as tall as one A4 page at the live scale, so a zoom
    // past fit-height yields real vertical scroll range as the zoom happens (the
    // same rule as the PDF strip).
    let strip_h = Memo::new(move |_| PAGE_HEIGHT * state.viewer.zoom.display.get());
    let vertical = axis == Axis::Vertical;
    let texture_class = texture_class(state);
    let tx_zoom = zoom_style(state);

    view! {
        <div
            id=scroller_id
            node_ref=list_ref
            class=move || {
                let base = match axis {
                    Axis::Vertical => {
                        "tx-strip scrollbar-none h-full w-full overflow-y-auto outline-none"
                    }
                    Axis::Horizontal => {
                        "tx-strip scrollbar-none h-full w-full overflow-x-auto overflow-y-auto outline-none"
                    }
                };
                let tex = texture_class.get();
                if tex.is_empty() {
                    base.to_string()
                } else {
                    format!("{base} {tex}")
                }
            }
            style=move || tx_zoom.get()
            tabindex="0"
        >
            <div
                class="relative"
                style=move || {
                    if vertical {
                        format!("width:100%;height:{}px", total_size.get())
                    } else {
                        format!(
                            "width:{}px;height:max(100%, {}px)",
                            total_size.get(),
                            strip_h.get().ceil()
                        )
                    }
                }
            >
                <For
                    each=move || items.get()
                    key=|item: &VirtualItem| item.index
                    children=move |item: VirtualItem| {
                        let index = item.index;
                        let page = (index + 1) as u32;
                        let offset = handle.with_value(|v| v.item_top(index));
                        let margin = state.viewer.page_margin;
                        let style = move || {
                            let snapped = snap_px(offset.get());
                            if vertical {
                                format!(
                                    "position:absolute;top:{}px;left:0;right:0;display:flex;padding-inline:{}px",
                                    snapped,
                                    margin.get(),
                                )
                            } else {
                                format!(
                                    "position:absolute;top:0;left:{}px;height:100%;display:flex;padding-inline:{}px",
                                    snapped,
                                    margin.get(),
                                )
                            }
                        };
                        // The host id is the slot's, shared with the PDF strip, so
                        // the chrome that finds a page by id never asks who painted
                        // it. Centring follows the axis: `mx-auto` on a page that
                        // scrolls vertically, `my-auto` on one that scrolls past it.
                        view! {
                            <div id=wrapper_id(axis, index, page) style=style>
                                <ReflowPage
                                    page=page
                                    state=state
                                    scale=page_scale
                                    host_id=host_id_for_axis(axis, page)
                                    class=if vertical { "mx-auto" } else { "my-auto" }
                                />
                            </div>
                        }
                    }
                />
            </div>
        </div>
    }
}

/// Per-axis wrapper id, kept as a free function so both ends of the strip's
/// `<For>` can name it without capturing anything by move.
fn wrapper_id(axis: Axis, index: usize, page: u32) -> String {
    match axis {
        Axis::Vertical => format!("txv-{index}-wrap"),
        Axis::Horizontal => format!("txh-{page}-wrap"),
    }
}
