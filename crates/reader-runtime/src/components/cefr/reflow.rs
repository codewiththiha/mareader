//! Red ink over a reflowable block, measured on the mounted row.

use leptos::prelude::*;

use ai_core::gloss::{GlossBox, PageAnchor, ReflowSpot};
use reader_core::settings::Settings;

use cefr_core::text::{MAX_WORD_CHARS, sentence_around, tokenize};

use std::hash::Hash;
use std::sync::Arc;

use super::layer::{CefrBox, CefrMarkLayer};
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

#[component]
pub fn BlockCefrMarks(state: ReaderState, block: usize) -> impl IntoView {
    let settings =
        use_context::<RwSignal<Settings>>().expect("Settings must be provided by the pane realm");
    let boxes: RwSignal<Vec<CefrBox>> = RwSignal::new(Vec::new());
    let row_id = block_row_id(block);
    let dataset_ready = Signal::derive(move || {
        services::cefr::dataset().with(|mirror| mirror.as_ref().is_some_and(|m| m.is_ready()))
    });
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
        // Everything below is TRACKED: the walk re-runs when any of it
        // moves.
        let _ = state.cefr.generation.get();
        let settled = state.viewer.zoom.committed.get();
        let mid_zoom = state.viewer.zoom.transition.get().is_some();
        let moved = relayout.get();
        let _ = (settled, moved);
        let (enabled, threshold) = settings.with(|s| (s.cefr_enabled, s.cefr_level.band()));
        if !enabled || !dataset_ready.get() || mid_zoom {
            clear(&boxes);
            return;
        }
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
                    paint(state, block, &row, threshold, &boxes);
                }
            });
            return;
        };
        paint(state, block, &row, threshold, &boxes);
    });

    view! {
        <CefrMarkLayer
            boxes=boxes.read_only().into()
            scale=Signal::derive(|| 1.0)
            click_explain=Signal::derive(move || settings.with(|s| s.cefr_click_explain))
        />
    }
}

/// The one walk: tokens, cache pass, backend pass, then the paint.
fn paint(
    state: ReaderState,
    block: usize,
    row: &web_sys::Element,
    threshold: u8,
    boxes: &RwSignal<Vec<CefrBox>>,
) {
    if state.document.content.reflow.block_at(block).is_none() {
        clear(boxes);
        return;
    }
    // The row's rendered text is the shared space; markdown markers do
    // not survive rendering.
    let text = row.text_content().unwrap_or_default();
    if text.is_empty() {
        clear(boxes);
        return;
    }
    let tokens = tokenize(&text);
    let chars: Vec<char> = text.chars().collect();

    let (hard, misses) = {
        let mut hard: Vec<(usize, usize)> = Vec::new();
        let mut misses: Vec<String> = Vec::new();
        let _ = state.cefr.levels.try_with_value(|cache| {
            for token in &tokens {
                let word: String = chars[token.start..token.end].iter().collect();
                if word.chars().count() > MAX_WORD_CHARS {
                    continue;
                }
                let mut candidates = cefr_core::lookup_candidates(&word);
                candidates.extend(cefr_core::hyphen_parts(&word));
                for key in &candidates {
                    if cache.get(key).is_none() && !misses.iter().any(|m| m == key) {
                        misses.push(key.clone());
                    }
                }
                if cache.any_above(&candidates, threshold) && hard.len() < MAX_BOXES_PER_ROW {
                    hard.push((token.start, token.end));
                }
            }
        });
        (hard, misses)
    };
    if !misses.is_empty() {
        let asked = misses.clone();
        services::cefr::fetch_levels(misses, move |levels| {
            state.cefr.ingest(asked, levels);
        });
    }

    // Words the AI stroke already owns, by spot: the red yields to it.
    let glossed: Vec<(usize, usize)> = state.gloss.marks.with(|marks| {
        marks
            .iter()
            .filter_map(|m| crate::components::ai::reflow_anchor::read_spot(&m.context))
            .filter(|s| s.block == block)
            .map(|s| (s.start, s.end))
            .collect()
    });

    let origin = row.get_bounding_client_rect();
    let page = page_of_block(state.document.content.reflow, block).unwrap_or(1);
    let mut painted: Vec<CefrBox> = Vec::new();
    for (start, end) in hard {
        if glossed.contains(&(start, end)) {
            continue;
        }
        let word: String = chars[start..end].iter().collect();
        let context = sentence_around(&text, start, end);
        let spot = ReflowSpot::new(block, start, end);
        let mark = captured_mark(
            word,
            spot_envelope(&spot, &context),
            PageAnchor {
                page,
                // The spot, not this snapshot, is what the stroke later
                // resolves through.
                rect: GlossBox::default(),
            },
        );
        if let Some(range) = range_for_span(row, start, end) {
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
    }
    // Compared before written: an unchanged walk must not re-render.
    if boxes.get_untracked() != painted {
        boxes.set(painted);
    }
}

fn clear(boxes: &RwSignal<Vec<CefrBox>>) {
    if !boxes.get_untracked().is_empty() {
        boxes.set(Vec::new());
    }
}
