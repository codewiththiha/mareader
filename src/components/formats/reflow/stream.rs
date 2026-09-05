//! The continuous text stream: vertical reading without pages.
//!
//! The paginated modes cut a reflowable document into A4 pages, and that is
//! the honest shape for a "book" — but vertical reading is not paging. The
//! stream therefore virtualizes the BLOCKS themselves: every paragraph,
//! heading, code fence and list chunk is one virtual item, mounted in a
//! window, laid out edge to edge with no cuts, no gaps and no boxes between
//! them. The page cut still exists (the paginated modes and the page
//! bookkeeping hang off it), but in this mode it is pure bookkeeping — the
//! reader scrolls text, not pages.
//!
//! The window is the page: the scroller paints the paper colour and the
//! blocks flow over it in a centered reading column, narrowed by the page
//! margin and positioned by the column-alignment setting. Nothing floats:
//! no card, no shadow, no gap — the document reads as one sheet.
//!
//! What the stream deliberately reuses rather than reinvents:
//!
//! * the SCROLLER ID (`page-list`) — the overlay scrollbar, the container
//!   observer, auto-scroll and the keyboard column all address it by name;
//! * `viewer.awaiting_anchor` — the stream anchors a fresh mount on the
//!   resume point (the saved fraction when the last session streamed, else
//!   the first block of the saved page) before the page bookkeeping may
//!   listen to it;
//! * `viewer.page` — the stream keeps it naming the page cut the dominant
//!   block belongs to, which is what progress persistence and the paged
//!   modes resume through. The chrome that would show it a page (the
//!   indicator, the bottom bar) shows a percentage instead while the
//!   stream is live.
//!
//! And what it does NOT reuse: the vertical PAGE virtualizer, which stays
//! unbound in this mode. Its scroll→page and page→scroll arms stand down
//! for the stream (see `effects::reader::navigation_sync`), because both
//! would speak page-cut geometry to a scroller that holds blocks.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use leptos::html;
use leptos::prelude::*;
use reader_core::view::RENDER_BUDGET;
use virtual_list::Viewport;
use virtual_list_leptos::{
    use_virtualizer, Align, ScrollMode, VirtualItem, Virtualizer, VirtualizerOptions,
};
use wasm_bindgen::JsCast;

use app_chrome::hooks::dom::PAGE_LIST_ID;
use app_chrome::hooks::use_resize_observer::observe_content_size;
use reflow_core::pager::first_block_of_page;

use crate::components::formats::block_render::BlockView;
use crate::components::formats::reflow::BlockSearchHits;
use super::block_render;
use crate::components::viewer::controls::overlay_scrollbar::OverlayScrollbar;
use crate::components::viewer::controls::progress_strip::ProgressStrip;
use crate::components::viewer::page_host::block_row_id;
use crate::components::viewer::texture_surface::{texture_class, zoom_style};
use super::page::content_style;
use crate::state::reader::TypographySignal;
use crate::state::ReaderState;

/// How many frames the mount anchor re-asserts the resume position before it
/// trusts the layout. More than the page strip's budget, and for a reason that
/// belongs to the aim: see [`anchor_stream`].
const ANCHOR_SETTLE_FRAMES: u32 = 5;

/// Air under the last block, so the end of a document is a resting point
/// rather than a hard wall at the screen's edge.
const STREAM_TAIL_PADDING: f64 = 96.0;

/// The height a block is assumed to have before anything better is known
/// (the estimate store already holds a real number in practice; this is
/// the floor for a block whose estimate never landed).
const FALLBACK_BLOCK_H: f64 = 24.0;

#[component]
pub fn ReflowStreamLayout(
    state: ReaderState,
    #[prop(into)] progress_visible: Signal<bool>,
) -> impl IntoView {
    let typography =
        use_context::<TypographySignal>().expect("TypographySignal must be provided by app bootstrap");
    let texture_class = texture_class(state);
    let tx_zoom = zoom_style(state);
    observe_content_size(PAGE_LIST_ID, state.viewer.container_size);
    // The stream takes the mount anchor's flag exactly like a page strip:
    // raised here for a remount, and by the open flow for a document that
    // arrives over a mounted reader. The anchor below consumes it.
    state.viewer.awaiting_anchor.set(true);

    // The stream's size model: one virtual item per BLOCK, sized by the
    // measured (or estimated) scale-1 height times the live display scale.
    // The count and the heights are tracked, so a re-parse or a re-measure
    // rebuilds the layout in the same flush; the epoch exists for the
    // heights changing IN PLACE (a measurement that lands without moving
    // the cut still moves the stream's geometry).
    let block_count = Signal::derive(move || {
        state.document.content.reflow.blocks.track();
        state.document.content.reflow.heights.with(|h| h.len())
    });
    let estimate = move |index: usize| {
        state
            .document
            .content
            .reflow
            .heights
            .with_untracked(|h| h.get(index).copied().unwrap_or(FALLBACK_BLOCK_H))
            * state.viewer.zoom.visual_scale()
    };
    let epoch = Signal::derive(move || {
        let mut hasher = DefaultHasher::new();
        state
            .document
            .content
            .reflow
            .blocks
            .with(|blocks| (Arc::as_ptr(blocks) as usize).hash(&mut hasher));
        let heights = state.document.content.reflow.heights.get();
        (Arc::as_ptr(&heights) as usize).hash(&mut hasher);
        heights.len().hash(&mut hasher);
        hasher.finish()
    });
    let initial_vh = {
        let (_, height) = state.viewer.container_size.get_untracked();
        if height > 1.0 { height } else { 800.0 }
    };
    let v = use_virtualizer(
        VirtualizerOptions::list(block_count, estimate)
            .budget(RENDER_BUDGET)
            .padding(0.0, STREAM_TAIL_PADDING)
            .initial(Viewport::main_only(initial_vh), 0.0)
            .epoch(epoch),
    );

    // Publish the handle: search reveal and the bottom bar's scrubber aim
    // the stream through `state.document.content.reflow.stream` rather than a second wiring.
    state.document.content.reflow.stream.set_value(Some(v.clone()));
    {
        on_cleanup(move || state.document.content.reflow.stream.set_value(None));
    }

    let list_ref: NodeRef<html::Div> = NodeRef::new();
    // Bind the container FIRST (the anchor effect below must find it bound
    // on its first run, exactly like the page strip's shell).
    {
        let v = v.clone();
        Effect::new(move |_| {
            let Some(div) = list_ref.get() else {
                return;
            };
            v.bind_container(div.clone().unchecked_into());
        });
    }

    // THE mount anchor — the one place the stream takes its position from
    // the resume bookkeeping. Same shape as the page strip's: bound
    // container, instant jump, re-asserted until the DOM agrees, then the
    // flag lowers and the page bookkeeping may listen again.
    {
        let v = v.clone();
        Effect::new(move |_| {
            if !state.viewer.awaiting_anchor.get() {
                return;
            }
            if list_ref.get().is_none() || !v.is_bound() {
                return;
            }
            anchor_stream(state, &v);
        });
    }

    // The stream mirrors its scroll offset into `viewer.scroll_top`, the
    // same contract the vertical page strip honours — the progress math,
    // the fraction save and the indicator percentage all read it there.
    {
        let scroll_top = state.viewer.scroll_top;
        let offset = v.scroll_offset();
        Effect::new(move |_| scroll_top.set(offset.get()));
    }

    // The extent rides a plain signal for the same reason: the chrome that
    // reads it (the progress strip's fraction, the percentage indicator)
    // builds `Send` closures, which the virtualizer's thread-local signals
    // can never enter.
    {
        let mirror = state.document.content.reflow.stream_total;
        let total = v.total_size();
        Effect::new(move |_| mirror.set(total.get()));
    }

    // Zoom: the stream owns its relayout. The page strips are unbound in
    // this mode, so the engine's rescale reaches nothing the reader can
    // see; this effect follows the display scale instead, rescaling the
    // block heights through the same anchored factor the engine uses for
    // pages, so the point under the viewport centre holds still.
    {
        let v = v.clone();
        let applied = StoredValue::new_local(state.viewer.zoom.display.get_untracked());
        Effect::new(move |_| {
            let scale = state.viewer.zoom.display.get();
            let heights = state.document.content.reflow.heights.get();
            let prev = applied.get_value();
            if (scale - prev).abs() <= 1e-4 {
                return;
            }
            applied.set_value(scale);
            let factor = (scale / prev).max(0.01);
            v.rescale(factor, move |i| heights.get(i).copied().unwrap_or(0.0) * scale);
        });
    }

    // A zoom transaction freezes the stream's scroll echo and measurements
    // for the same reasons the zoom coordinator freezes the page strips
    // (see `crate::zoom::coordinator`): a rescale's scroll write echoes one
    // frame late, and adopting that echo mid-tween pins the next anchored
    // rescale from a stale offset. The coordinator owns the page strips and
    // cannot reach this one, so the stream freezes itself for exactly the
    // transaction's duration.
    {
        let v = v.clone();
        Effect::new(move |_| {
            if state.viewer.zooming().get() {
                v.suspend_scroll_feedback();
                v.suspend_measurements();
            } else {
                v.resume_scroll_feedback();
                v.resume_measurements();
            }
        });
    }

    // The rendered truth: whatever the window actually mounted, measured
    // and reported back. The model above is measured at the PAGE column
    // width; when the reading column is narrower (a small window, a fat
    // margin), the real blocks run taller, and this pass is what keeps the
    // stream's geometry honest without a second offscreen column. It runs
    // one frame after any input that can move a rendered height — window
    // churn, typography, margin, container, zoom commits — and reports
    // nothing while a zoom transaction is open (a mid-tween height belongs
    // to a geometry that is already being replaced).
    let column_ref: NodeRef<html::Div> = NodeRef::new();
    {
        let v = v.clone();
        let items = v.items();
        Effect::new(move |_| {
            let mounted = items.get();
            let _typography = typography.get();
            let _ = state.viewer.page_margin.get();
            let (cw, ch) = state.viewer.container_size.get();
            let _scale = state.viewer.zoom.display.get();
            let _epoch = state.document.content.reflow.remeasure.get();
            let _ = (mounted.len(), cw, ch);
            let column = column_ref;
            let v = v.clone();
            request_animation_frame(move || {
                if state.viewer.zooming_now() {
                    return;
                }
                let Some(col) = column.get() else {
                    return;
                };
                let children = col.children();
                let count = mounted.len().min(children.length() as usize);
                for slot in 0..count {
                    let (Some(child), Some(item)) = (children.item(slot as u32), mounted.get(slot))
                    else {
                        continue;
                    };
                    let Ok(el) = child.dyn_into::<web_sys::HtmlElement>() else {
                        continue;
                    };
                    let height = el.offset_height() as f64;
                    if height > 0.0 {
                        v.report_size(item.index, height);
                    }
                }
            });
        });
    }

    // Scroll → page bookkeeping: the dominant block names the page cut it
    // belongs to, which is what progress persistence resumes through. The
    // page→scroll arm is standing down for this mode, so this write can
    // never bounce back as a scroll command.
    {
        let v = v.clone();
        Effect::new(move |_| {
            if state.viewer.awaiting_anchor.get() {
                return;
            }
            let block = v.dominant().get();
            let page = state
                .document
                .content
                .reflow
                .block_page
                .get()
                .get(block)
                .map_or(1, |p| p + 1)
                .clamp(1, state.document.num_pages.get().max(1));
            if page != state.viewer.page.get_untracked() {
                state.viewer.page.set(page);
            }
        });
    }

    let items = v.items();
    let total_size = v.total_size();
    let handle = StoredValue::new_local(v.clone());
    let scale = state.viewer.zoom.display.read_only();
    let margin = state.viewer.page_margin.read_only();
    // The reading column: as wide as a page's content at the live scale,
    // never wider than the viewport minus the page margin, and positioned
    // by the alignment setting (left / center / right).
    let column_class = move || {
        format!(
            "tx-stream-col {}",
            typography.get().column_align.container_class()
        )
    };
    let column_style = move || {
        let s = scale.get();
        let geo = reflow_core::geometry::geometry(typography.get().book_layout);
        let m = margin.get().round();
        // The width is the reading column alone; the page margin is an
        // INSET (--tx-col-inset, consumed by the .tx-align-* classes),
        // so Left/Right honour the margin exactly like Center and a zero
        // margin still reaches the true window edge. The min() engages
        // only as a narrow-window clamp.
        format!(
            "width: min({}px, calc(100% - {}px));--tx-col-inset:{}px;",
            (geo.content_width * s).round(),
            (m * 2.0).round(),
            m
        )
    };
    let progress = move || {
        let st = state.viewer.scroll_top.get();
        let (_, ch) = state.viewer.container_size.get();
        let total = state.document.content.reflow.stream_total.get();
        if total > ch && total > 0.0 {
            (st / (total - ch)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };

    view! {
        <div class="relative h-full w-full">
            <div
                id=PAGE_LIST_ID
                node_ref=list_ref
                data-reader-host=crate::dom_contract::HOST_REFLOW
                class=move || {
                    let base =
                        "tx-stream scrollbar-none h-full w-full overflow-y-auto outline-none";
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
                <div class="relative w-full" style:height=move || format!("{}px", total_size.get())>
                    <div node_ref=column_ref class=column_class style=column_style>
                        <For
                            each=move || {
                                let doc_id = state.document.content.reflow.document_id();
                                items
                                    .get()
                                    .into_iter()
                                    .map(|item| (doc_id, item))
                                    .collect::<Vec<(usize, VirtualItem)>>()
                            }
                            key=|(doc_id, item): &(usize, VirtualItem)| (*doc_id, item.index)
                            children=move |(_, item): (usize, VirtualItem)| {
                                let index = item.index;
                                let top = handle.with_value(|v| v.item_top(index));
                                let block = state.document.content.reflow.block_at(index);
                                view! {
                                    <div
                                        class="tx-content"
                                        lang="en"
                                        // The row IS the block here: one text
                                        // node tree, one virtual item, and so
                                        // the row carries the block's handles
                                        // itself rather than leaving them to the
                                        // `BlockView` inside it — the id the gloss
                                        // projection resolves a mark's block by,
                                        // the index the selection tracker walks up
                                        // to, and the page for that tracker's page
                                        // range (which in this mode is
                                        // bookkeeping, since nothing here is
                                        // paginated).
                                        id=block_row_id(index)
                                        data-block-index=index
                                        data-host-page=move || {
                                            state
                                                .document
                                                .content
                                                .reflow
                                                .block_page
                                                .with(|map| map.get(index).map_or(1, |p| p + 1))
                                        }
                                        style=move || format!(
                                            "{}position:absolute;top:{}px;left:0;right:0;",
                                            content_style(scale.get()),
                                            top.get().round(),
                                        )
                                    >
                                        {match block {
                                            Some(block) => {
                                                view! {
                                                    <BlockView state=state block=block render=block_render(state) />
                                                    <BlockSearchHits state=state block=index />
                                                }
                                                    .into_any()
                                            }
                                            // A re-measure can briefly hold a
                                            // window from the outgoing layout;
                                            // an out-of-range index renders
                                            // nothing rather than panicking.
                                            None => ().into_any(),
                                        }}
                                    </div>
                                }
                            }
                        />
                    </div>
                </div>
            </div>
            // ONE stroke layer for the whole reading surface, not one per
            // block: the stream's blocks are virtualized individually and are
            // not pages, so a per-page layer would have nothing to attach to
            // and a per-block layer would drop every mark whose block scrolled
            // out of the window. It is positioned against the scroller, so its
            // strokes live in the reader's box and are clipped by it. A mark
            // whose text is not mounted hides until it is — the same semantic
            // the PDF's virtualized pages already have.
            <crate::components::formats::reflow::ReflowGlossLayer
                state=state
                host_id=PAGE_LIST_ID
            />
            <OverlayScrollbar scroller_id=PAGE_LIST_ID horizontal=false />
            <Show when=move || progress_visible.get()>
                <ProgressStrip fraction=Signal::derive(progress) />
            </Show>
        </div>
    }
}

/// Put the stream on its resume position: the saved fraction when the last
/// session streamed (it beats the page — it is the same position, kept at
/// full precision), else the first block of the saved page's cut.
///
/// The re-assert loop is the page strip's ([`crate::components::viewer::shells::anchor_settle`]);
/// what the stream adds is the aim. It gets more frames than a page strip
/// because it is aiming at a fraction of a total extent that is still growing
/// while the blocks report their measured heights, so the offset the first
/// write lands on is not yet the offset that fraction will mean.
fn anchor_stream(state: ReaderState, v: &Virtualizer) {
    // The aim needs its own handle: the loop borrows the one it settles, and
    // the closure that aims it has to own what it runs on every frame.
    let aim = v.clone();
    crate::components::viewer::shells::anchor_settle::settle(
        state,
        v,
        ANCHOR_SETTLE_FRAMES,
        move || {
            aim.remeasure_viewport();
            if let Some(fraction) = state.document.content.reflow.resume_fraction.get_untracked() {
                // Consume the fraction: a later remount (a mode flip and back)
                // anchors on the page — the fraction described a layout the
                // reader has since left.
                state.document.content.reflow.resume_fraction.set(None);
                let total = aim.total_size().get_untracked();
                let viewport = aim.viewport().get_untracked().main;
                let extent = (total - viewport).max(0.0);
                aim.scroll_to_offset(fraction * extent, ScrollMode::Instant);
            } else {
                let page = state.viewer.page.get_untracked();
                let block = state
                    .document
                    .content
                    .reflow
                    .cuts
                    .with_untracked(|cuts| first_block_of_page(cuts, page));
                aim.scroll_to_index(block, Align::Start, ScrollMode::Instant);
            }
        },
    );
}

