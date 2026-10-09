//! Red ink over a reflowable block, measured on the mounted row.

use leptos::prelude::*;

use ai_core::gloss::{GlossBox, PageAnchor, ReflowSpot};

use cefr_core::Planned;

use std::hash::Hash;
use std::sync::Arc;

use super::layer::{CefrBox, CefrMarkLayer};
use super::measure::measure;
use super::scan::with_row_scan;
use super::{Sink, WalkKey, WalkMemo, ask, marks_fingerprint, pane_settings, text_fingerprint};
use crate::components::ai::anchor::captured_mark;
use crate::components::ai::reflow_anchor::{page_of_block, spot_envelope};
use crate::components::formats::reflow::spot::range_for_span;
use crate::components::viewer::page_host::block_row_id;
use crate::services;
use crate::state::ReaderState;
use app_chrome::hooks::dom::range_rects;
use app_ui::epoch::epoch_signal;

/// Boxes one row paints, mirroring the engine's per-page cap.
const MAX_BOXES_PER_ROW: usize = 200;

/// The tracked inputs one reflow walk runs against.
#[derive(Clone, Copy)]
struct Ctx {
    fp: u64,
    zoom: u64,
    threshold: u8,
    marks: u64,
    generation: u64,
}

#[component]
pub fn BlockCefrMarks(state: ReaderState, block: usize) -> impl IntoView {
    let settings = pane_settings();
    let boxes: RwSignal<Vec<CefrBox>> = RwSignal::new(Vec::new());
    let memo: StoredValue<Option<WalkMemo>, LocalStorage> = StoredValue::new_local(None);
    let row_id = block_row_id(block);
    let mirror = services::cefr::dataset();
    let dataset_ready =
        Signal::derive(move || mirror.with(|m| m.as_ref().is_some_and(|m| m.is_ready())));
    // Everything that re-wraps the row without the reader scrolling.
    let relayout = {
        let reflow = state.document.content.reflow;
        let container = state.viewer.container_size;
        let margin = state.viewer.page_margin;
        epoch_signal(move |hasher| {
            reflow.cut_generation.get().hash(hasher);
            reflow
                .heights
                .with(|heights| (Arc::as_ptr(heights) as usize).hash(hasher));
            let geo = reflow.geometry.get();
            geo.content_width.to_bits().hash(hasher);
            container.get().0.to_bits().hash(hasher);
            margin.get().to_bits().hash(hasher);
        })
    };

    Effect::new(move |_| {
        // Everything below is TRACKED; the memo makes unchanged
        // re-runs free.
        let generation = state.cefr.generation.get();
        let settled = state.viewer.zoom.committed.get();
        let mid_zoom = state.viewer.zoom.transition.get().is_some();
        let fp = relayout.get();
        let (enabled, threshold) = settings.with(|s| (s.cefr_enabled, s.cefr_level.band()));
        if !enabled || !dataset_ready.get() || mid_zoom {
            // The memo survives a clear: re-enabling restores for free.
            clear(&boxes);
            return;
        }
        // The suppression set, folded tracked: a mark edit re-walks.
        let marks = state.gloss.marks.with(|all| {
            marks_fingerprint(all.iter().filter_map(|m| {
                crate::components::ai::reflow_anchor::read_spot(&m.context)
                    .filter(|spot| spot.block == block)
                    .map(|_| m.id.as_str())
            }))
        });
        let ctx = Ctx {
            fp,
            zoom: settled.to_bits(),
            threshold,
            marks,
            generation,
        };
        let Some(row) = state.dom.by_id(&row_id) else {
            // Not yet attached: one frame from now it is, so retry once.
            clear(&boxes);
            let (id, state, boxes) = (row_id.clone(), state, boxes);
            request_animation_frame(move || {
                // The retry can outlive the row; the row's own signal
                // ends it.
                if boxes.try_get_untracked().is_none() {
                    return;
                }
                if let Some(row) = state.dom.by_id(&id) {
                    paint(state, block, &id, &row, ctx, &boxes, memo);
                }
            });
            return;
        };
        paint(state, block, &row_id, &row, ctx, &boxes, memo);
    });

    view! {
        <CefrMarkLayer
            boxes=boxes.read_only().into()
            scale=Signal::derive(|| 1.0)
            click_explain=Signal::derive(move || settings.with(|s| s.cefr_click_explain))
        />
    }
}

/// The walk's cheap half: scan, cache pass, plan; a frame measures.
fn paint(
    state: ReaderState,
    block: usize,
    row_id: &str,
    row: &web_sys::Element,
    ctx: Ctx,
    boxes: &RwSignal<Vec<CefrBox>>,
    memo: StoredValue<Option<WalkMemo>, LocalStorage>,
) {
    if state.document.content.reflow.block_at(block).is_none() {
        clear(boxes);
        return;
    }
    let page = page_of_block(state.document.content.reflow, block).unwrap_or(1);
    with_row_scan(row_id, ctx.fp, row, |scan| {
        let key = WalkKey {
            fp: ctx.fp,
            zoom: ctx.zoom,
            text: text_fingerprint(&scan.text),
            generation: ctx.generation,
            marks: ctx.marks,
            threshold: ctx.threshold,
        };
        if let Some(found) = memo.try_get_value().flatten()
            && found.key == key
        {
            // Measured: restore (a gate clear undid it). Pending: the
            // scheduled frame owns the answer.
            if found.measured && boxes.get_untracked() != found.boxes {
                boxes.set(found.boxes.clone());
            }
            return;
        }
        let plan = state
            .cefr
            .levels
            .try_with_value(|cache| {
                cefr_core::plan_tokens(
                    &scan.text,
                    &scan.tokens,
                    cache,
                    ctx.threshold,
                    MAX_BOXES_PER_ROW,
                )
            })
            .unwrap_or_default();
        ask(state, plan.misses);
        // Words the AI stroke already owns, by spot: the red yields to it.
        let glossed: Vec<(usize, usize)> = state.gloss.marks.with_untracked(|all| {
            all.iter()
                .filter_map(|m| crate::components::ai::reflow_anchor::read_spot(&m.context))
                .filter(|s| s.block == block)
                .map(|s| (s.start, s.end))
                .collect()
        });
        let kept: Vec<_> = plan
            .hard
            .into_iter()
            .filter(|span| !glossed.contains(&(span.start, span.end)))
            .collect();
        let words = cefr_core::planned(&scan.text, &kept);
        memo.try_update_value(|slot| {
            *slot = Some(WalkMemo {
                key,
                measured: words.is_empty(),
                boxes: Vec::new(),
            });
        });
        if words.is_empty() {
            clear(boxes);
            return;
        }
        let task_row = row.clone();
        let sink = Sink {
            key,
            boxes: *boxes,
            memo,
        };
        measure(move || run_plan(block, page, task_row, words, sink));
    });
}

/// The scheduled half: measure the plan against the live row.
fn run_plan(block: usize, page: u32, row: web_sys::Element, plan: Vec<Planned>, sink: Sink) {
    // A detached row has no geometry; its signal is gone anyway.
    if !row.is_connected() {
        return;
    }
    if let Some(found) = sink.memo.try_get_value().flatten()
        && (found.key != sink.key || found.measured)
    {
        return; // a newer walk owns the memo
    }
    let origin = row.get_bounding_client_rect();
    let mut painted: Vec<CefrBox> = Vec::new();
    for word in &plan {
        if painted.len() >= MAX_BOXES_PER_ROW {
            break;
        }
        let Some(range) = range_for_span(&row, word.start, word.end) else {
            continue;
        };
        let spot = ReflowSpot::new(block, word.start, word.end);
        let envelope = spot_envelope(&spot, &word.context);
        let mark = captured_mark(
            word.word.clone(),
            envelope,
            PageAnchor {
                page,
                // The spot, not this snapshot, is what the stroke later
                // resolves through.
                rect: GlossBox::default(),
            },
        );
        for (left, top, right, bottom) in range_rects(&range) {
            let (width, height) = (right - left, bottom - top);
            // A zero-sized fragment at a line-box edge is not ink.
            if width <= 0.0 || height <= 0.0 || painted.len() >= MAX_BOXES_PER_ROW {
                continue;
            }
            painted.push(CefrBox {
                x: left - origin.left(),
                y: top - origin.top(),
                w: width.max(1.0),
                h: height.max(1.0),
                mark: mark.clone(),
            });
        }
    }
    super::publish(&sink, painted);
}

fn clear(boxes: &RwSignal<Vec<CefrBox>>) {
    if !boxes.get_untracked().is_empty() {
        boxes.set(Vec::new());
    }
}
