//! Red ink over one PDF page, stored in page space: a zoom never re-measures.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use ai_core::gloss::{GlossBox, PageAnchor};

use cefr_core::Planned;

use super::layer::{CefrBox, CefrMarkLayer};
use super::measure::measure;
use super::{Sink, WalkKey, WalkMemo, ask, marks_fingerprint, pane_settings, text_fingerprint};
use crate::components::ai::anchor::captured_mark;
use crate::components::ai::reflow_anchor::union_box;
use crate::components::formats::reflow::spot::range_for_span;
use crate::services;
use crate::state::ReaderState;
use app_chrome::hooks::dom::range_rects;
use app_state::dom_contract::TEXT_LAYER_CLASS;

/// Boxes one page paints, mirroring the engine's per-page cap.
const MAX_BOXES_PER_PAGE: usize = 200;
/// Animation-frame retries for a host that attaches one flush late.
const HOST_TRIES: u32 = 10;
/// Layers painted OVER the page's text, whose mutations are this layer's own.
const OVERLAY_SELECTOR: &str = ".cefr-layer, .gloss-layer";

/// The observer and its callback, kept alive until teardown drops them.
type ObserverSlot = Option<(
    web_sys::MutationObserver,
    Closure<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>,
)>;

/// One page's text layer, read once: its text, its spans, their char ranges.
struct LayerText {
    text: String,
    elements: Vec<web_sys::Element>,
    ranges: Vec<(usize, usize)>,
}

/// The page host's text layer, assembled in DOM order.
fn read_layer(host: &web_sys::Element) -> Option<LayerText> {
    let layer = host
        .query_selector(&format!(".{TEXT_LAYER_CLASS}"))
        .ok()
        .flatten()?;
    let list = layer.query_selector_all("span").ok()?;
    let count = list.length() as usize;
    let mut read = LayerText {
        text: String::new(),
        elements: Vec::with_capacity(count),
        ranges: Vec::with_capacity(count),
    };
    for index in 0..list.length() {
        let Some(node) = list.get(index) else {
            continue;
        };
        let Ok(element) = node.dyn_into::<web_sys::Element>() else {
            continue;
        };
        let piece = element.text_content().unwrap_or_default();
        let start = read.text.chars().count();
        read.ranges.push((start, start + piece.chars().count()));
        read.text.push_str(&piece);
        read.elements.push(element);
    }
    (!read.elements.is_empty()).then_some(read)
}

/// Whether a mutation is the page's own text, not a layer painted over it.
fn moves_the_text(record: &web_sys::MutationRecord) -> bool {
    let Some(target) = record.target() else {
        return true;
    };
    let Some(element) = target.dyn_ref::<web_sys::Element>() else {
        return true;
    };
    element.closest(OVERLAY_SELECTOR).ok().flatten().is_none()
}

/// Watch `host_id`'s subtree; every text-layer swap wakes `wake`.
fn install_observer(
    state: ReaderState,
    host_id: String,
    wake: Trigger,
    installed: StoredValue<bool, LocalStorage>,
    held: StoredValue<ObserverSlot, LocalStorage>,
    tries: StoredValue<u32, LocalStorage>,
) {
    if installed.try_get_value() != Some(false) {
        // Installed, or the owner is gone (the counter reads None then).
        return;
    }
    let Some(host) = state.dom.by_id(&host_id) else {
        let spent = tries.try_get_value().unwrap_or(HOST_TRIES);
        if spent >= HOST_TRIES {
            return;
        }
        tries.set_value(spent + 1);
        request_animation_frame(move || {
            install_observer(state, host_id, wake, installed, held, tries);
        });
        return;
    };
    let callback = Closure::<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>::new(
        move |records: js_sys::Array, _obs: web_sys::MutationObserver| {
            // This layer's own paint mutates the subtree it observes.
            let relevant = (0..records.length()).any(|index| {
                records
                    .get(index)
                    .dyn_into::<web_sys::MutationRecord>()
                    .is_ok_and(|record| moves_the_text(&record))
            });
            if relevant {
                wake.notify();
            }
        },
    );
    let Ok(observer) = web_sys::MutationObserver::new(callback.as_ref().unchecked_ref()) else {
        return;
    };
    let init = web_sys::MutationObserverInit::new();
    init.set_child_list(true);
    init.set_subtree(true);
    let _ = observer.observe_with_options(&host, &init);
    installed.try_set_value(true);
    held.try_update_value(|slot| *slot = Some((observer, callback)));
}

/// The inputs one PDF page walk runs against (all zoom-independent).
#[derive(Clone, Copy)]
struct Ctx {
    threshold: u8,
    generation: u64,
    marks: u64,
}

#[component]
pub fn PdfCefrLayer(
    state: ReaderState,
    /// The page this host renders.
    page: u32,
    /// The page host's id: where the text layer lives.
    #[prop(into)]
    host_id: String,
    /// The display scale, from the page host.
    scale: ReadSignal<f64>,
) -> impl IntoView {
    let settings = pane_settings();
    let boxes: RwSignal<Vec<CefrBox>> = RwSignal::new(Vec::new());
    let memo: StoredValue<Option<WalkMemo>, LocalStorage> = StoredValue::new_local(None);
    let wake = Trigger::new();
    // The phase alone flips re-derive; the mirror handle is bound once.
    let mirror = services::cefr::dataset();
    let dataset_ready =
        Signal::derive(move || mirror.with(|m| m.as_ref().is_some_and(|m| m.is_ready())));

    // The text layer lands after the raster: observed, not polled, in a
    // component-scope slot.
    let installed = StoredValue::new_local(false);
    let held: StoredValue<ObserverSlot, LocalStorage> = StoredValue::new_local(None);
    on_cleanup(move || {
        if let Some((observer, _callback)) = held.try_update_value(|v| v.take()).flatten() {
            observer.disconnect();
        }
    });
    install_observer(
        state,
        host_id.clone(),
        wake,
        installed,
        held,
        StoredValue::new_local(0u32),
    );

    Effect::new(move |_| {
        // The re-derive clock: cache fills, threshold edits, dataset
        // swaps and text-layer batches all raise it.
        wake.track();
        let generation = state.cefr.generation.get();
        let (enabled, threshold) = settings.with(|s| (s.cefr_enabled, s.cefr_level.band()));
        if !enabled || !dataset_ready.get() {
            clear(&boxes);
            return;
        }
        // The suppression set, folded tracked: a mark edit re-walks.
        let marks = state.gloss.marks.with(|all| {
            marks_fingerprint(
                all.iter()
                    .filter(|m| {
                        m.page == page
                            && crate::components::ai::reflow_anchor::read_spot(&m.context).is_none()
                    })
                    .map(|m| m.id.as_str()),
            )
        });
        // The observer wakes this when a missing layer lands.
        let Some(host) = state.dom.by_id(&host_id) else {
            clear(&boxes);
            return;
        };
        let ctx = Ctx {
            threshold,
            generation,
            marks,
        };
        walk(state, page, &host, scale, ctx, &boxes, memo);
    });

    view! {
        <CefrMarkLayer
            boxes=boxes.read_only().into()
            scale=scale.into()
            click_explain=Signal::derive(move || settings.with(|s| s.cefr_click_explain))
            hover_dict=Signal::derive(move || settings.with(|s| s.dict.hover))
        />
    }
}

/// The walk's cheap half: text, cache pass, plan; a frame measures.
fn walk(
    state: ReaderState,
    page: u32,
    host: &web_sys::Element,
    scale: ReadSignal<f64>,
    ctx: Ctx,
    boxes: &RwSignal<Vec<CefrBox>>,
    memo: StoredValue<Option<WalkMemo>, LocalStorage>,
) {
    let Some(layer) = read_layer(host) else {
        return;
    };
    let key = WalkKey {
        fp: 0,
        zoom: 0,
        text: text_fingerprint(&layer.text),
        generation: ctx.generation,
        marks: ctx.marks,
        threshold: ctx.threshold,
    };
    if let Some(found) = memo.try_get_value().flatten()
        && found.key == key
    {
        // Page-space memo boxes stay exact across a zoom rebuild.
        if found.measured && boxes.get_untracked() != found.boxes {
            boxes.set(found.boxes.clone());
        }
        return;
    }

    let plan = state
        .cefr
        .levels
        .try_with_value(|cache| {
            cefr_core::plan(&layer.text, cache, ctx.threshold, MAX_BOXES_PER_PAGE)
        })
        .unwrap_or_default();
    ask(state, plan.misses);

    let words = cefr_core::planned(&layer.text, &plan.hard);
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
    let task_host = host.clone();
    let sink = Sink {
        key,
        boxes: *boxes,
        memo,
    };
    measure(move || run_plan(state, page, task_host, scale, words, sink));
}

/// The scheduled half: measure in page space, minus the AI's words.
fn run_plan(
    state: ReaderState,
    page: u32,
    host: web_sys::Element,
    scale: ReadSignal<f64>,
    plan: Vec<Planned>,
    sink: Sink,
) {
    if !host.is_connected() {
        return;
    }
    if let Some(found) = sink.memo.try_get_value().flatten()
        && (found.key != sink.key || found.measured)
    {
        return; // a newer walk owns the memo
    }
    // The layer may have swapped since the plan; identical text only.
    let Some(layer) = read_layer(&host) else {
        return;
    };
    if text_fingerprint(&layer.text) != sink.key.text {
        return;
    }
    // Divide by the glyphs' live scale, read at measure time.
    let display = scale.get_untracked();
    if display <= 0.0 {
        return;
    }
    // Words the AI stroke owns: the red yields to the accent.
    let glossed: Vec<GlossBox> = state.gloss.marks.with_untracked(|all| {
        all.iter()
            .filter(|m| {
                m.page == page
                    && crate::components::ai::reflow_anchor::read_spot(&m.context).is_none()
            })
            .map(|m| m.anchor.rect)
            .collect()
    });

    let host_rect = host.get_bounding_client_rect();
    let inverse = 1.0 / display;
    let mut painted: Vec<CefrBox> = Vec::new();
    'word: for word in &plan {
        let mut fragments: Vec<GlossBox> = Vec::new();
        for (i, (s, e)) in layer.ranges.iter().enumerate() {
            if *e <= word.start || *s >= word.end {
                continue;
            }
            let local_start = word.start.max(*s) - *s;
            let local_end = (*e).min(word.end) - *s;
            let Some(range) = range_for_span(&layer.elements[i], local_start, local_end) else {
                continue;
            };
            let Some(fragment) = union_box(&range_rects(&range)) else {
                continue;
            };
            let local = GlossBox {
                x: (fragment.x - host_rect.left()) * inverse,
                y: (fragment.y - host_rect.top()) * inverse,
                w: fragment.w * inverse,
                h: fragment.h * inverse,
                r: 0.0,
            };
            if local.w <= 0.5 || local.h <= 0.5 {
                continue;
            }
            fragments.push(local);
        }
        let Some(union) = union_box(
            &fragments
                .iter()
                .map(|b| (b.x, b.y, b.x + b.w, b.y + b.h))
                .collect::<Vec<_>>(),
        ) else {
            continue;
        };
        for g in &glossed {
            let (overlap_x, overlap_y) = (
                union.x < g.x + g.w + 0.5 && g.x < union.x + union.w + 0.5,
                union.y < g.y + g.h + 0.5 && g.y < union.y + union.h + 0.5,
            );
            if overlap_x && overlap_y {
                continue 'word;
            }
        }
        let mark = captured_mark(
            word.word.clone(),
            word.context.clone(),
            PageAnchor { page, rect: union },
        );
        for local in fragments {
            if painted.len() >= MAX_BOXES_PER_PAGE {
                break 'word;
            }
            painted.push(CefrBox {
                x: local.x,
                y: local.y,
                w: local.w,
                h: local.h,
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
