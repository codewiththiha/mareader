//! Search hits, painted over the row that renders their block.

use leptos::prelude::*;

use app_chrome::hooks::dom::range_rects;

use std::hash::Hash;
use std::sync::Arc;

use super::spot::range_for_span;
use crate::components::cefr::measure::measure;
use crate::components::cefr::scan::with_row_scan;
use crate::components::viewer::page_host::block_row_id;
use crate::pane::dom::PaneDom;
use crate::state::ReaderState;
use app_ui::epoch::epoch_signal;

/// Boxes one row will paint, mirroring the engine's per-page cap.
const MAX_BOXES_PER_ROW: usize = 200;

/// One painted box, in its row's own CSS px.
#[derive(Debug, Clone, Copy, PartialEq)]
struct HitBox {
    /// Which occurrence of the query in this block the box covers.
    occurrence: u32,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

#[component]
pub fn BlockSearchHits(
    state: ReaderState,
    /// The block whose row this layer covers, found by its row id.
    block: usize,
) -> impl IntoView {
    let boxes: RwSignal<Vec<HitBox>> = RwSignal::new(Vec::new());
    // Built outside the effect: a per-run fingerprint would be a new node
    // per frame.
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
    let row_id = block_row_id(block);

    Effect::new(move |_| {
        // Everything below is TRACKED: the walk re-runs when any of it moves.
        let query = state.search.query.get();
        // The COMMITTED scale: per-frame walks would cost the tween its
        // frames.
        let settled = state.viewer.zoom.committed.get();
        let mid_zoom = state.viewer.zoom.transition.get().is_some();
        // Everything that re-wraps the row without the reader scrolling.
        let fp = relayout.get();
        let _ = settled;

        let needle = query.trim();
        if needle.is_empty() || mid_zoom {
            clear_if_painted(boxes);
            return;
        }
        let dom = state.dom;
        if dom.by_id(&row_id).is_none() {
            // Not yet attached: one frame from now it is, so retry once.
            clear_if_painted(boxes);
            let (id, needle) = (row_id.clone(), needle.to_string());
            request_animation_frame(move || {
                // The retry can outlive the row; the row's own signal ends it.
                if boxes.try_get_untracked().is_none() {
                    return;
                }
                paint_row(dom, &id, &needle, fp, boxes);
            });
            return;
        }
        paint_row(dom, &row_id, needle, fp, boxes);
    });

    // The occurrence in THIS block the reader stepped to, if any.
    let active_here = Signal::derive(move || {
        let index = state.search.active.get()?;
        let hit = state
            .search
            .matches
            .with(|matches| matches.get(index).and_then(|found| found.block_hit))?;
        (hit.block as usize == block).then_some(hit.occurrence)
    });

    view! {
        <div class="tx-hits" aria-hidden="true">
            {move || {
                boxes
                    .get()
                    .into_iter()
                    .map(|hit| {
                        let occurrence = hit.occurrence;
                        view! {
                            <div
                                class="highlight"
                                class=(
                                    "is-active",
                                    move || active_here.get() == Some(occurrence)
                                )
                                data-match=occurrence.to_string()
                                style=format!(
                                    "left:{}px;top:{}px;width:{}px;height:{}px",
                                    hit.left,
                                    hit.top,
                                    hit.width,
                                    hit.height
                                )
                            />
                        }
                    })
                    .collect::<Vec<_>>()
            }}
        </div>
    }
}

/// Plan the row's occurrences over the shared scan; a frame measures.
fn paint_row(dom: PaneDom, row_id: &str, needle: &str, fp: u64, boxes: RwSignal<Vec<HitBox>>) {
    // A row that is not mounted has no text to cover.
    let Some(row) = dom.by_id(row_id) else {
        clear_if_painted(boxes);
        return;
    };
    let plan = with_row_scan(row_id, fp, &row, |scan| {
        let lower = scan.text.to_lowercase();
        reader_core::search::occurrence_spans(&scan.text, &lower, needle)
            .into_iter()
            .take(MAX_BOXES_PER_ROW)
            .collect::<Vec<_>>()
    });
    if plan.is_empty() {
        clear_if_painted(boxes);
        return;
    }
    let task_row = row.clone();
    measure(move || {
        if !task_row.is_connected() || boxes.try_get_untracked().is_none() {
            return;
        }
        let origin = task_row.get_bounding_client_rect();
        let mut painted: Vec<HitBox> = Vec::new();
        for (occurrence, (start, end)) in plan.into_iter().enumerate() {
            // The cap is on BOXES, which is what the engine caps.
            if painted.len() >= MAX_BOXES_PER_ROW {
                break;
            }
            let Some(range) = range_for_span(&task_row, start, end) else {
                continue;
            };
            for (left, top, right, bottom) in range_rects(&range) {
                if painted.len() >= MAX_BOXES_PER_ROW {
                    break;
                }
                let (width, height) = (right - left, bottom - top);
                // A zero-sized fragment at a line-box edge is not a highlight.
                if width <= 0.0 || height <= 0.0 {
                    continue;
                }
                painted.push(HitBox {
                    occurrence: occurrence as u32,
                    left: left - origin.left(),
                    top: top - origin.top(),
                    // A hairline match still gets a visible box.
                    width: width.max(1.0),
                    height: height.max(1.0),
                });
            }
        }
        // Compared before written: an unchanged walk must not re-render.
        if boxes.get_untracked() != painted {
            boxes.set(painted);
        }
    });
}

/// Drop the painted boxes, but only if there are any.
fn clear_if_painted(boxes: RwSignal<Vec<HitBox>>) {
    if !boxes.get_untracked().is_empty() {
        boxes.set(Vec::new());
    }
}
