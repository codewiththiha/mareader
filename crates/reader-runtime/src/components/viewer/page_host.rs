//! The reader's one page interface: a layout says which page, this
//! picks the drawing.

use leptos::html;
use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use reader_core::view::{Axis, ViewMode};
use reflow_core::geometry::SpineSide;

#[cfg(feature = "pdf")]
use crate::components::formats::pdf::{GlossOverlayProps, PdfPageCanvas, PdfPageStrip};
#[cfg(feature = "reflow")]
use crate::components::formats::reflow::{ReflowPage, ReflowPageStrip, ReflowStreamLayout};
#[cfg(feature = "pdf")]
use crate::components::viewer::shells::scroll_shell::ScrollShell;
use crate::effects::reader::reflow_measure::RowBox as ReflowRowBox;
use crate::state::ReaderState;

/// Where a page sits in the current layout, as a slot, not a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSlot {
    Single,
    SpreadLeft,
    SpreadRight,
}

impl PageSlot {
    /// The mode this slot belongs to, for the id scheme and the fit maths.
    pub fn mode(self) -> ViewMode {
        match self {
            PageSlot::Single => ViewMode::Single,
            PageSlot::SpreadLeft | PageSlot::SpreadRight => ViewMode::Spread,
        }
    }

    /// Which side of the spine this slot's page reads as, with a book
    /// layout.
    pub fn spine(self) -> SpineSide {
        match self {
            PageSlot::Single => SpineSide::Auto,
            PageSlot::SpreadLeft => SpineSide::Left,
            PageSlot::SpreadRight => SpineSide::Right,
        }
    }
}

/// The host element id of `page` in `mode`: the one page identity.
pub fn host_id_for_mode(mode: ViewMode, page: u32) -> String {
    match mode {
        ViewMode::Single => format!("sp-{page}-pg"),
        ViewMode::Spread => format!("dp-{page}-pg"),
        ViewMode::ScrollHorizontal => format!("hp-{page}-pg"),
        // The vertical strip indexes its window from 0, and so do its ids.
        ViewMode::ScrollVertical => format!("cont-{}-pg", page.saturating_sub(1)),
    }
}

/// The id of the row rendering `block`, in any mode.
pub fn block_row_id(block: usize) -> String {
    format!("tx-block-{block}")
}

/// What a mounted row was showing when its box was read.
///
/// A placeholder row's box is the layout's own estimate; measuring it would
/// let the estimate confirm itself and recut the document under the reader.
pub fn row_box(el: &web_sys::Element) -> ReflowRowBox {
    if el
        .get_attribute(app_state::dom_contract::PLACEHOLDER_ATTR)
        .is_some()
    {
        ReflowRowBox::Placeholder
    } else {
        ReflowRowBox::Content
    }
}

/// The canvas id of `page` in `mode`: the host id plus a suffix.
#[cfg(feature = "pdf")]
pub(crate) fn canvas_id_for_mode(mode: ViewMode, page: u32) -> String {
    host_id_for_mode(mode, page).replacen("-pg", "-cv", 1)
}

/// The host id of a strip page, by axis.
pub(crate) fn host_id_for_axis(axis: Axis, page: u32) -> String {
    host_id_for_mode(
        match axis {
            Axis::Vertical => ViewMode::ScrollVertical,
            Axis::Horizontal => ViewMode::ScrollHorizontal,
        },
        page,
    )
}

#[cfg(feature = "pdf")]
pub(crate) fn canvas_id_for_axis(axis: Axis, page: u32) -> String {
    canvas_id_for_mode(
        match axis {
            Axis::Vertical => ViewMode::ScrollVertical,
            Axis::Horizontal => ViewMode::ScrollHorizontal,
        },
        page,
    )
}

#[component]
pub fn UniversalPageHost(
    /// 1-based page number to draw.
    page: u32,
    state: ReaderState,
    /// Which half of the layout this page is (`slot` is taken).
    page_slot: PageSlot,
    /// Extra classes, passed through to whichever component mounts (the
    /// cross-axis centring `mx-auto` both formats understand).
    #[prop(default = String::new(), into)]
    class: String,
) -> impl IntoView {
    // Hosts live at the display scale; the raster follows
    // `render_scale`.
    #[cfg(any(feature = "pdf", feature = "reflow"))]
    let page_scale = state.viewer.zoom.display.read_only();
    #[cfg(feature = "pdf")]
    let texture = use_context::<crate::state::TextureSignal>()
        .expect("TextureSignal is provided by the pane realm, from its own look");
    #[cfg(any(feature = "pdf", feature = "reflow"))]
    let host_id = host_id_for_mode(page_slot.mode(), page);
    #[cfg(not(any(feature = "pdf", feature = "reflow")))]
    let _ = (page, page_slot, class);

    view! {
        {move || {
            if state.reflowable() {
                #[cfg(feature = "reflow")]
                {
                    view! {
                        <ReflowPage
                            page=page
                            state=state
                            scale=page_scale
                            host_id=host_id.clone()
                            spine=page_slot.spine()
                            class=class.clone()
                        />
                    }
                    .into_any()

                }
                #[cfg(not(feature = "reflow"))]
                {
                    ().into_any()
                }
            } else {
                #[cfg(feature = "pdf")]
                {
                    view! {
                        <PdfPageCanvas
                            page=page
                            scale=page_scale
                            render_scale=state.viewer.zoom.committed
                            zoom_animating=state.viewer.zooming()
                            gesture_owns=state.viewer.gesture_owns()
                            texture=texture
                            canvas_id=canvas_id_for_mode(page_slot.mode(), page)
                            host_id=host_id.clone()
                            render_text=true
                            on_rendered=crate::zoom::target::page_rendered(state)
                            on_sized=crate::zoom::target::page_sized_cb(state)
                            gloss_overlay=GlossOverlayProps::from_gloss(state)
                            class=class.clone()
                        />
                    }
                    .into_any()

                }
                #[cfg(not(feature = "pdf"))]
                {
                    ().into_any()
                }
            }
        }}
    }
}

/// The virtualized page strip, in either format.
#[component]
pub fn UniversalStripHost(
    state: ReaderState,
    virtualizer: Virtualizer,
    axis: Axis,
    /// The scroller element this strip lays out into (owned by the shell).
    scroller_id: &'static str,
    list_ref: NodeRef<html::Div>,
) -> impl IntoView {
    #[cfg(any(feature = "pdf", feature = "reflow"))]
    let v = StoredValue::new_local(virtualizer);
    #[cfg(not(any(feature = "pdf", feature = "reflow")))]
    let _ = (virtualizer, axis, scroller_id, list_ref);
    view! {
        {move || {
            #[cfg(any(feature = "pdf", feature = "reflow"))]
            let virtualizer = v.get_value();
            if state.reflowable() {
                #[cfg(feature = "reflow")]
                {
                    view! {
                        <ReflowPageStrip
                            state=state
                            virtualizer=virtualizer
                            axis=axis
                            scroller_id=scroller_id
                            list_ref=list_ref
                        />
                    }
                    .into_any()

                }
                #[cfg(not(feature = "reflow"))]
                {
                    ().into_any()
                }
            } else {
                #[cfg(feature = "pdf")]
                {
                    view! {
                        <PdfPageStrip
                            state=state
                            virtualizer=virtualizer
                            axis=axis
                            scroller_id=scroller_id
                            list_ref=list_ref
                        />
                    }
                    .into_any()

                }
                #[cfg(not(feature = "pdf"))]
                {
                    ().into_any()
                }
            }
        }}
    }
}

/// Continuous reading: a block column or a page strip.
#[component]
pub fn UniversalStreamHost(
    state: ReaderState,
    /// The page virtualizer, used for the PDF strip and parked by the layout.
    virtualizer: Virtualizer,
    #[prop(into)] progress_visible: Signal<bool>,
) -> impl IntoView {
    #[cfg(feature = "pdf")]
    let strip = StoredValue::new_local(virtualizer);
    #[cfg(not(feature = "pdf"))]
    let _ = virtualizer;
    #[cfg(any(feature = "pdf", feature = "reflow"))]
    let progress = progress_visible;
    #[cfg(not(any(feature = "pdf", feature = "reflow")))]
    let _ = progress_visible;
    view! {
        {move || {
            if state.reflowable() {
                #[cfg(feature = "reflow")]
                {
                    view! { <ReflowStreamLayout state=state progress_visible=progress /> }.into_any()

                }
                #[cfg(not(feature = "reflow"))]
                {
                    ().into_any()
                }
            } else {
                #[cfg(feature = "pdf")]
                {
                    view! {
                        <ScrollShell
                            state=state
                            virtualizer=strip.get_value()
                            axis=Axis::Vertical
                            progress_visible=progress
                        />
                    }
                    .into_any()

                }
                #[cfg(not(feature = "pdf"))]
                {
                    ().into_any()
                }
            }
        }}
    }
}
