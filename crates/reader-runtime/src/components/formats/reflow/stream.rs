//! The stream: vertical reading with one virtual item per block; the page
//! cut is bookkeeping.

use std::hash::Hash;
use std::sync::Arc;

use leptos::html;
use leptos::prelude::*;
use virtual_list::{Budget, Viewport};
use virtual_list_leptos::{
    Align, ScrollMode, VirtualItem, VirtualItemState, Virtualizer, VirtualizerOptions,
    use_virtualizer,
};
use wasm_bindgen::JsCast;

use app_chrome::hooks::dom::PAGE_LIST_ID;
use app_chrome::hooks::use_resize_observer::{observe_content_size_with, use_resize_observer};
use reflow_core::pager::first_block_of_page;

use super::block_render;
use super::page::content_style;
use crate::components::formats::block_render::BlockView;
use crate::components::formats::reflow::BlockSearchHits;
use crate::components::viewer::controls::overlay_scrollbar::OverlayScrollbar;
use crate::components::viewer::controls::progress_strip::ProgressStrip;
use crate::components::viewer::page_host::block_row_id;
use crate::components::viewer::texture_surface::{texture_class, zoom_style};
use crate::state::ReaderState;
use crate::state::TypographySignal;
use app_ui::epoch::epoch_signal;

/// Frames the mount anchor re-asserts the resume position before it
/// trusts the layout.
const ANCHOR_SETTLE_FRAMES: u32 = 5;

/// Air under the last block: the end of a document is a resting point.
const STREAM_TAIL_PADDING: f64 = 96.0;

/// A block's assumed height before anything better is known.
const FALLBACK_BLOCK_H: f64 = 24.0;

/// The mount budget, wider than the render band; rows outside stay
/// empty.
const STREAM_MOUNT_BUDGET: Budget = Budget::screenfuls(2.0, 128);

#[component]
pub fn ReflowStreamLayout(
    state: ReaderState,
    #[prop(into)] progress_visible: Signal<bool>,
) -> impl IntoView {
    let typography = use_context::<TypographySignal>()
        .expect("TypographySignal must be provided by app bootstrap");
    let texture_class = texture_class(state);
    let tx_zoom = zoom_style(state);
    // The container observation dies with this layout: an observer outliving
    // its scroller retains it.
    let dom = state.dom;
    let stop_observing =
        observe_content_size_with(move || dom.by_id(PAGE_LIST_ID), state.viewer.container_size);
    on_cleanup(stop_observing);
    // The mount anchor's flag, raised like a page strip's; the anchor
    // below consumes it.
    state.viewer.awaiting_anchor.set(true);

    // One virtual item per block, sized by its measured height times the
    // display scale.
    let block_count = Signal::derive(move || {
        state.document.content.reflow.blocks.track();
        state.document.content.reflow.heights.with(|h| h.len())
    });
    // The epoch follows the estimate count: echoes are already in the model,
    // re-estimates are not.
    let epoch = epoch_signal(move |hasher| {
        state
            .document
            .content
            .reflow
            .blocks
            .with(|blocks| (Arc::as_ptr(blocks) as usize).hash(hasher));
        state
            .document
            .content
            .reflow
            .estimate_generation
            .get()
            .hash(hasher);
    });
    let estimate = move |index: usize| {
        // Runs in flush/rebuild paths that can outlive the close.
        let Some(height) = state
            .document
            .content
            .reflow
            .heights
            .try_with_untracked(|h| h.get(index).copied().unwrap_or(FALLBACK_BLOCK_H))
        else {
            return FALLBACK_BLOCK_H;
        };
        let Some(scale) = state.viewer.zoom.display.try_get_untracked() else {
            return FALLBACK_BLOCK_H;
        };
        height * scale
    };
    let initial_vh = {
        let (_, height) = state.viewer.container_size.get_untracked();
        if height > 1.0 { height } else { 800.0 }
    };
    // Start at the resume position: the first window mounts where the anchor
    // aims.
    let initial_offset = {
        let scale = state.viewer.zoom.visual_scale();
        let total = state
            .document
            .content
            .reflow
            .heights
            .with_untracked(|heights| heights.iter().sum::<f64>())
            * scale
            + STREAM_TAIL_PADDING;
        match state
            .document
            .content
            .reflow
            .resume_fraction
            .get_untracked()
        {
            Some(fraction) if total > initial_vh => {
                (fraction * (total - initial_vh)).clamp(0.0, total)
            }
            _ => {
                let page = state.viewer.page.get_untracked().max(1);
                let block = state
                    .document
                    .content
                    .reflow
                    .cuts
                    .with_untracked(|cuts| first_block_of_page(cuts, page));
                state
                    .document
                    .content
                    .reflow
                    .heights
                    .with_untracked(|heights| heights.iter().take(block).sum::<f64>())
                    * scale
            }
        }
    };
    let v = use_virtualizer(
        VirtualizerOptions::stream(block_count, estimate)
            .budget(STREAM_MOUNT_BUDGET)
            .padding(0.0, STREAM_TAIL_PADDING)
            .initial(Viewport::main_only(initial_vh), initial_offset)
            .epoch(epoch),
    );
    // The virtualizer joins the diagnostics registry for its lifetime.
    crate::diagnostics::track_virtualizer(&v);
    // The PANE owns the instance (its dispose disposes it); the cleanup
    // pairs.
    let stream_pane = use_context::<crate::pane::handle::PaneHandle>()
        .expect("the document pane provides its handle");
    stream_pane.track_virtualizer(&v);
    let tracked = StoredValue::new_local(v.clone());
    on_cleanup(move || {
        tracked.with_value(crate::diagnostics::untrack_virtualizer);
        stream_pane.untrack_virtualizer(&tracked.get_value());
    });

    // Publish the handle: search reveal and the scrubber aim the stream
    // through the state.
    state
        .document
        .content
        .reflow
        .stream
        .set_value(Some(v.clone()));
    {
        on_cleanup(move || state.document.content.reflow.stream.set_value(None));
    }

    let list_ref: NodeRef<html::Div> = NodeRef::new();
    // Bind the container FIRST: the anchor effect must find it bound.
    {
        let v = v.clone();
        Effect::new(move |_| {
            let Some(div) = list_ref.get() else {
                return;
            };
            v.bind_container(div.clone().unchecked_into());
        });
    }

    // THE mount anchor: the stream's one position from resume bookkeeping.
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

    // Mirror the scroll offset into `viewer.scroll_top`, the strip contract.
    {
        let scroll_top = state.viewer.scroll_top;
        let offset = v.scroll_offset();
        Effect::new(move |_| scroll_top.set(offset.get()));
    }

    // The extent rides a plain signal: the chrome builds `Send` closures.
    {
        let mirror = state.document.content.reflow.stream_total;
        let total = v.total_size();
        Effect::new(move |_| mirror.set(total.get()));
    }

    // Zoom: the stream owns its relayout, rescaling heights about the
    // viewport centre.
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
            v.rescale(factor, move |i| {
                heights.get(i).copied().unwrap_or(0.0) * scale
            });
        });
    }

    // A zoom transaction freezes the scroll echo and measurements; the
    // coordinator cannot reach this one.
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

    // The rendered truth: what the window mounted, reported to the model and
    // to the store.
    let column_ref: NodeRef<html::Div> = NodeRef::new();
    // A settle cues the measure pass, which stands down while the scroller
    // moves.
    let measure_now = ArcTrigger::new();
    {
        let v = v.clone();
        let cue = measure_now.clone();
        v.on_scroll_idle(move || cue.notify());
    }
    {
        let v = v.clone();
        let items = v.items();
        let zooming = state.viewer.zooming();
        Effect::new(move |_| {
            // A mid-tween report stands down; completion must wake it.
            let _ = zooming.get();
            // Re-run on a scroll settle: the pass skips while moving.
            measure_now.track();
            let mounted = items.get();
            let _typography = typography.get();
            let _ = state.viewer.page_margin.get();
            let _ = state.viewer.column_width_pct.get();
            let (cw, ch) = state.viewer.container_size.get();
            let _scale = state.viewer.zoom.display.get();
            let _ = (mounted.len(), cw, ch);
            let column = column_ref;
            let v = v.clone();
            request_animation_frame(move || {
                // A frame later the close can land; a dead item signal ends it.
                if items.try_get_untracked().is_none() {
                    return;
                }
                if state.viewer.try_zooming_now() != Some(false) {
                    return;
                }
                // The measurement fling gate: while the scroller moves the
                // pass would force a full layout.
                if !v.settled_now() {
                    return;
                }
                let Some(col) = column.get() else {
                    return;
                };
                let scale = state.viewer.zoom.visual_scale();
                // The rows belong to the reflow session as it stands now.
                let session = state.pane.reflow_session();
                let children = col.children();
                let mut batch: Vec<(usize, f64)> = Vec::new();
                for slot in 0..children.length() {
                    let Some(child) = children.item(slot) else {
                        continue;
                    };
                    // Every mounted row is measured, blanks included; a blank
                    // reports the layout's own height.
                    let Ok(el) = child.dyn_into::<web_sys::HtmlElement>() else {
                        continue;
                    };
                    let Some(index) = el
                        .get_attribute("data-block-index")
                        .and_then(|value| value.parse::<usize>().ok())
                    else {
                        continue;
                    };
                    let height = el.offset_height() as f64;
                    if height > 0.0 {
                        v.report_size_now(index, height);
                        if scale > 0.0 {
                            batch.push((index, height / scale));
                        }
                    }
                }
                if let Some(session) = session {
                    state.measure.ingest(session, scale, &batch);
                }
            });
        });
    }

    // Scroll to page: the dominant block names the page cut.
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
    let column_pct = state.viewer.column_width_pct.read_only();
    // The reading column: a page's content width at scale, capped by the
    // viewport.
    let column_class = move || {
        format!(
            "tx-stream-col {}",
            typography.get().column_align.container_class()
        )
    };
    let column_style = move || {
        let s = scale.get();
        let pct = column_pct.get();
        let geo =
            reflow_core::geometry::geometry(typography.get().book_layout).with_column_pct(pct);
        let m = margin.get().round();
        // The margin is an INSET, so Left/Right honour it; min() only clamps a
        // narrow window.
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
        reader_core::view::scroll_fraction(st, total, ch)
    };

    view! {
        <div class="relative h-full w-full">
            <div
                id=PAGE_LIST_ID
                node_ref=list_ref
                data-reader-host=app_state::dom_contract::HOST_REFLOW
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
                                // THE ROW'S OWN OBSERVER: reports
                                // land before paint, so rows never
                                // overlap.
                                let row_ref: NodeRef<html::Div> = NodeRef::new();
                                {
                                    let v_row = handle.get_value();
                                    let row_el = row_ref;
                                    use_resize_observer(row_ref, move |_| {
                                        // Mid-transaction: the tween rewrites
                                        // every row's size; the settled pass
                                        // measures them again.
                                        if state.viewer.try_zooming_now() != Some(false) {
                                            return;
                                        }
                                        let Some(el) = row_el.get() else {
                                            return;
                                        };
                                        let height = el.offset_height() as f64;
                                        if height <= 0.0 {
                                            return;
                                        }
                                        v_row.report_size_now(index, height);
                                        // The scale-1 truth feeds the shared
                                        // store; one ingest per row.
                                        let scale = state.viewer.zoom.visual_scale();
                                        if scale > 0.0
                                            && let Some(session) = state.pane.reflow_session()
                                        {
                                            state.measure.ingest(
                                                session,
                                                scale,
                                                &[(index, height / scale)],
                                            );
                                        }
                                    });
                                }
                                // The row's render state as a signal: a
                                // `For` child never re-runs, so band
                                let row_state = handle.with_value(|v| v.item_state(index));
                                // The virtualizer's own height: what a
                                // blank placeholder sizes itself to, so
                                // layout never moves.
                                let blank_height = handle.with_value(|v| v.item_size(index));
                                let block = state.document.content.reflow.block_at(index);
                                view! {
                                    <div
                                        class="tx-content"
                                        lang="en"
                                        // The row is the block: one text tree,
                                        // one virtual item; its handles ride
                                        // it.
                                        node_ref=row_ref
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
                                                    // In band: type, hits.
                                                    // Outside: an empty box
                                                    // sized by the model.
                                                    {move || {
                                                        if row_state.get() == VirtualItemState::Blank {
                                                            view! {
                                                                <div
                                                                    class="tx-blank"
                                                                    aria-hidden="true"
                                                                    style=move || format!(
                                                                        "height:{}px",
                                                                        blank_height.get().round()
                                                                    )
                                                                />
                                                            }
                                                                .into_any()
                                                        } else {
                                                            view! {
                                                                <BlockView state=state block=block.clone() render=block_render(state) />
                                                                <BlockSearchHits state=state block=index />
                                                            }
                                                                .into_any()
                                                        }
                                                    }}
                                                }
                                                    .into_any()
                                            }
                                            // A stale index renders nothing.
                                            None => ().into_any(),
                                        }}
                                    </div>
                                }
                            }
                        />
                    </div>
                </div>
            </div>
            // ONE stroke layer for the whole surface; marks whose block
            // scrolled out stay hidden.
            <crate::components::formats::reflow::ReflowGlossLayer
                state=state
                host_id=PAGE_LIST_ID
            />
            <OverlayScrollbar dom=state.dom scroller_id=PAGE_LIST_ID horizontal=false />
            <Show when=move || progress_visible.get()>
                <ProgressStrip fraction=Signal::derive(progress) />
            </Show>
        </div>
    }
}

/// Aim the stream at its resume position: the saved fraction, else the
/// saved page's block.
fn anchor_stream(state: ReaderState, v: &Virtualizer) {
    // The aim needs its own handle: the loop borrows the one it settles.
    let aim = v.clone();
    crate::components::viewer::shells::anchor_settle::settle(
        state,
        v,
        ANCHOR_SETTLE_FRAMES,
        move || {
            aim.remeasure_viewport();
            if let Some(fraction) = state
                .document
                .content
                .reflow
                .resume_fraction
                .get_untracked()
            {
                // Consume the fraction: a later remount anchors on the page.
                state.document.content.reflow.resume_fraction.set(None);
                let total = aim.total_size().get_untracked();
                let viewport = aim.viewport().get_untracked().main;
                aim.scroll_to_offset(
                    reader_core::view::fraction_offset(fraction, total, viewport),
                    ScrollMode::Instant,
                );
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
