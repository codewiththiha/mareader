//! Red ink over one PDF page, stored in page space: a zoom never re-measures.

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;

use ai_core::gloss::{GlossBox, PageAnchor};
use reader_core::settings::Settings;

use cefr_core::text::{MAX_WORD_CHARS, sentence_around, tokenize};

use super::layer::{CefrBox, CefrMarkLayer};
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

/// The observer and its callback, kept alive until teardown drops them.
type ObserverSlot = Option<(
    web_sys::MutationObserver,
    Closure<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>,
)>;

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
        move |_records: js_sys::Array, _obs: web_sys::MutationObserver| {
            wake.notify();
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
    let settings =
        use_context::<RwSignal<Settings>>().expect("Settings must be provided by the pane realm");
    let boxes: RwSignal<Vec<CefrBox>> = RwSignal::new(Vec::new());
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

    // The dataset's arrival retires every "word unknown" answer from
    // before it.
    let was_ready = StoredValue::new_local(false);
    {
        Effect::new(move |_| {
            // The re-derive clock: cache fills, threshold edits, dataset swaps
            // and text-layer batches all raise it.
            wake.track();
            let _ = state.cefr.generation.get();
            let (enabled, threshold) = settings.with(|s| (s.cefr_enabled, s.cefr_level.band()));
            let ready = dataset_ready.get();
            if ready && !was_ready.get_value() {
                was_ready.set_value(true);
                state.cefr.invalidate();
                return;
            }
            if !enabled || !ready {
                clear(&boxes);
                return;
            }
            let Some(host) = state.dom.by_id(&host_id) else {
                clear(&boxes);
                return;
            };
            let Some(layer) = host
                .query_selector(&format!(".{TEXT_LAYER_CLASS}"))
                .ok()
                .flatten()
            else {
                // Not rendered yet: the observer wakes this when it lands.
                return;
            };
            // Divide by the glyphs' live scale; a tween never re-walks.
            let display = scale.get_untracked();
            if display <= 0.0 {
                return;
            }
            walk(state, page, &host, &layer, display, threshold, &boxes);
        });
    }

    view! {
        <CefrMarkLayer
            boxes=boxes.read_only().into()
            scale=scale.into()
            click_explain=Signal::derive(move || settings.with(|s| s.cefr_click_explain))
        />
    }
}

/// The one walk: spans to text, tokens, cache pass, backend pass, paint.
fn walk(
    state: ReaderState,
    page: u32,
    host: &web_sys::Element,
    layer: &web_sys::Element,
    display: f64,
    threshold: u8,
    boxes: &RwSignal<Vec<CefrBox>>,
) {
    let span_els: Vec<web_sys::Element> = {
        let list = match layer.query_selector_all("span") {
            Ok(list) => list,
            Err(_) => return,
        };
        (0..list.length())
            .filter_map(|i| list.get(i))
            .filter_map(|node| node.dyn_into::<web_sys::Element>().ok())
            .collect()
    };
    if span_els.is_empty() {
        return;
    }
    // The page's text in DOM order, with each span's char range in it.
    let mut text = String::new();
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(span_els.len());
    for el in &span_els {
        let piece = el.text_content().unwrap_or_default();
        let start = text.chars().count();
        text.push_str(&piece);
        spans.push((start, text.chars().count()));
    }

    let tokens = tokenize(&text);
    let chars: Vec<char> = text.chars().collect();

    // Cache pass: what is already decidable, and what the backend owes.
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
                if cache.any_above(&candidates, threshold) && hard.len() < MAX_BOXES_PER_PAGE {
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

    // Words the AI stroke owns: the red yields to the accent.
    let glossed: Vec<GlossBox> = state.gloss.marks.with(|marks| {
        marks
            .iter()
            .filter(|m| {
                m.page == page
                    && crate::components::ai::reflow_anchor::read_spot(&m.context).is_none()
            })
            .map(|m| m.anchor.rect)
            .collect()
    });

    // Paint pass: only tokens the cache answers and no gloss mark covers.
    let host_rect = host.get_bounding_client_rect();
    let inverse = 1.0 / display;
    let mut painted: Vec<CefrBox> = Vec::new();
    'token: for (start, end) in hard {
        let word: String = chars[start..end].iter().collect();
        let context = sentence_around(&text, start, end);
        // A token may cross spans; the union is its one mark.
        let mut fragments: Vec<GlossBox> = Vec::new();
        for (i, (s, e)) in spans.iter().enumerate() {
            if *e <= start || *s >= end {
                continue;
            }
            let local_start = start.max(*s) - *s;
            let local_end = (*e).min(end) - *s;
            let Some(range) = range_for_span(&span_els[i], local_start, local_end) else {
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
                continue 'token;
            }
        }
        let mark = captured_mark(word, context, PageAnchor { page, rect: union });
        for local in fragments {
            if painted.len() >= MAX_BOXES_PER_PAGE {
                break 'token;
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
