//! The virtualized strip of reflowable pages: the PDF strip's twin,
//! with a known page size.

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
    /// The strip's axis; offsets, bars and sizing follow it.
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

    // At least one A4 page tall, so zoom-past-fit scrolls.
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
                        // The host id is the slot's; centring follows the axis.
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

/// Per-axis wrapper id, a free function for both `<For>` ends.
fn wrapper_id(axis: Axis, index: usize, page: u32) -> String {
    match axis {
        Axis::Vertical => format!("txv-{index}-wrap"),
        Axis::Horizontal => format!("txh-{page}-wrap"),
    }
}
