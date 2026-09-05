//! The reader's one page interface. A layout says *which* page and *where* it
//! sits; this module decides what drawing it means.
//!
//! Before this existed, every layout that could show a document carried its own
//! fork — `if format.is_text() { <TextPage/> } else { <PageCanvas/> }` — and a
//! fork is never one line for long. The spread grew a spine side, the single
//! layout grew a gloss overlay, the shell grew a virtualizer parked in local
//! storage, and all of it had to be copied in the next place that could show a
//! page; adding a format meant finding them all with a grep. Here the fork is
//! once, on a [`PageSlot`], and the layouts are dumb pipes again.
//!
//! Three hosts, one per shape the reader can take:
//!
//! * [`UniversalPageHost`] — a single page or one half of a spread. Both formats
//!   get the same page, scale, host id and `class`, and the same texture from
//!   context; the PDF's extras (canvas id, text layer, geometry callback, gloss
//!   overlay) are constructed here from the slot rather than passed in by a
//!   layout, and the spine side goes only to the pipeline that paints padding —
//!   a raster's gutter is the spread's gap, not its page's style.
//! * [`UniversalStripHost`] — the virtualized strip, both axes, either format.
//! * [`UniversalStreamHost`] — continuous reading, where the two formats genuinely
//!   disagree about the SURFACE: a reflowable document reads as one column of
//!   blocks, a PDF as a strip of pages. This is the only place that difference is
//!   allowed to live, which is why `ScrollVerticalLayout` mounts this instead of
//!   choosing between them.
//!
//! Nothing here owns layout, scroll policy or animation: the shell does
//! (`shells::scroll_shell`), and the reactive primitives live in `reader-core`'s
//! `view` module. What this module DOES own is the DOM identity of a page —
//! [`host_id_for_mode`] is the single definition of the `sp-`/`dp-`/`hp-`/`cont-`
//! ids that the floating chapter label, a selection anchor and the engine's page
//! registration all address pages by, format included. Two formats answering to
//! one id per slot is the point: chrome that finds a page never has to know what
//! it is made of.

use leptos::html;
use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use reader_core::view::{Axis, ViewMode};
use reflow_core::geometry::SpineSide;

use crate::components::formats::pdf::{GlossOverlayProps, PdfPageCanvas, PdfPageStrip};
use crate::components::formats::reflow::{ReflowPage, ReflowPageStrip, ReflowStreamLayout};
use crate::components::viewer::shells::scroll_shell::ScrollShell;
use crate::state::ReaderState;

/// Where a page sits in the current layout — the only thing a layout has to say
/// about itself that its pages cannot derive.
///
/// A slot, not a `ViewMode`: `SpreadLeft`/`SpreadRight` carry information the mode
/// does not (which half of the pair this host is), and the mode would let a layout
/// ask for a page in a mode it is not in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSlot {
    /// The single-page view's one page.
    Single,
    /// The spread's left-hand page.
    SpreadLeft,
    /// The spread's right-hand page.
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

    /// Which side of the book spine this slot's page reads as, when a book layout
    /// is on. A spread's pages are FIXED to their sides — the spine sits between
    /// the two hosts, so a page's own parity is irrelevant there; every other
    /// slot alternates with parity like a bound book.
    pub fn spine(self) -> SpineSide {
        match self {
            PageSlot::Single => SpineSide::Auto,
            PageSlot::SpreadLeft => SpineSide::Left,
            PageSlot::SpreadRight => SpineSide::Right,
        }
    }
}

/// The host element id of `page` in `mode` — THE definition of the reader's page
/// identity in the DOM, shared by both formats (see the module note).
pub fn host_id_for_mode(mode: ViewMode, page: u32) -> String {
    match mode {
        ViewMode::Single => format!("sp-{page}-pg"),
        ViewMode::Spread => format!("dp-{page}-pg"),
        ViewMode::ScrollHorizontal => format!("hp-{page}-pg"),
        // The vertical strip indexes its window from 0, and so do its ids.
        ViewMode::ScrollVertical => format!("cont-{}-pg", page.saturating_sub(1)),
    }
}

/// The element id of the row that renders `block` of a reflowable document, in
/// any mode.
///
/// A block is the same element wherever it is mounted — a page's slot, the
/// continuous stream's row — so its id carries no mode, unlike a host's. The
/// gloss projection looks a mark's block up by this id rather than by a
/// formatted `[data-block-index='n']` selector: it does so once per mark per
/// refresh, the stream layer refreshes on every scroll frame, and an id lookup
/// allocates nothing and searches nothing. The attribute stays: the engine's
/// selection tracker walks up to it with `closest`, which an id cannot answer.
pub fn block_row_id(block: usize) -> String {
    format!("tx-block-{block}")
}

/// The canvas element id of `page` in `mode`: the host id with the canvas
/// suffix. Kept next to [`host_id_for_mode`] because the pair must never drift —
/// the engine registers a canvas against its host.
pub(crate) fn canvas_id_for_mode(mode: ViewMode, page: u32) -> String {
    host_id_for_mode(mode, page).replacen("-pg", "-cv", 1)
}

/// The host id of a strip page, by axis — the same scheme as
/// [`host_id_for_mode`], since an axis is a scroll mode with its paging.
pub(crate) fn host_id_for_axis(axis: Axis, page: u32) -> String {
    host_id_for_mode(
        match axis {
            Axis::Vertical => ViewMode::ScrollVertical,
            Axis::Horizontal => ViewMode::ScrollHorizontal,
        },
        page,
    )
}

/// The canvas id of a strip page, by axis.
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
    /// Which half of the layout this page is. Named `page_slot` rather than
    /// `slot` because `slot=` is a pseudo-attribute of the `view!` macro.
    page_slot: PageSlot,
    /// Extra classes, passed through to whichever component mounts (the
    /// cross-axis centring `mx-auto` both formats understand).
    #[prop(default = String::new(), into)]
    class: String,
) -> impl IntoView {
    // Hosts live at the live display scale; the crisp raster follows
    // `render_scale`, which only moves when a zoom lands. Both are read here so
    // neither layout has to know that a page of type needs one and a page of
    // pixels needs both.
    let page_scale = state.viewer.zoom.display.read_only();
    let texture = use_context::<crate::state::TextureSignal>()
        .expect("TextureSignal must be provided by app bootstrap");
    let host_id = host_id_for_mode(page_slot.mode(), page);

    view! {
        {move || {
            if state.reflowable() {
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
            } else {
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
                        gloss_overlay=GlossOverlayProps::from_gloss(state)
                        class=class.clone()
                    />
                }
                .into_any()
            }
        }}
    }
}

/// The virtualized page strip, in either format.
///
/// The shell's closures must stay `Send`, and the `Rc`-backed `Virtualizer` is
/// not, so the format branch below parks the handle it is given in local storage
/// and resolves it at render time — the same move the shell makes one level up,
/// and for the same reason: the parking belongs with the component that owns the
/// capture, which here is this one.
#[component]
pub fn UniversalStripHost(
    state: ReaderState,
    virtualizer: Virtualizer,
    axis: Axis,
    /// The scroller element this strip lays out into (owned by the shell).
    scroller_id: &'static str,
    list_ref: NodeRef<html::Div>,
) -> impl IntoView {
    let v = StoredValue::new_local(virtualizer);
    view! {
        {move || {
            let virtualizer = v.get_value();
            if state.reflowable() {
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
            } else {
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
        }}
    }
}

/// Continuous reading: one column of blocks for a reflowable document, the
/// page strip for a PDF. See the module note for why this is a host and not a
/// detail of `ScrollVerticalLayout`.
#[component]
pub fn UniversalStreamHost(
    state: ReaderState,
    /// The page virtualizer, used for the PDF strip and parked by the layout.
    virtualizer: Virtualizer,
    #[prop(into)] progress_visible: Signal<bool>,
) -> impl IntoView {
    let strip = StoredValue::new_local(virtualizer);
    let progress = progress_visible;
    view! {
        {move || {
            if state.reflowable() {
                view! { <ReflowStreamLayout state=state progress_visible=progress /> }.into_any()
            } else {
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
        }}
    }
}
