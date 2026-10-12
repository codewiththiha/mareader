//! The shared ink layer: boxes, the click that explains,
//! the hover that translates.

use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use ai_core::gloss::GlossMark;
use app_ui::events::dispatch_typed_event_on;
use app_ui::events::{DICT_HOVER_EVENT, DICT_LEAVE_EVENT};

use crate::components::ai::gloss::mark_layer::request_gloss_open;

/// How long the pointer rests on a word before the card is asked for.
const HOVER_DWELL_MS: u64 = 160;

/// The attribute a box carries: its index in this layer's own list.
const BOX_INDEX: &str = "data-cefr";

/// One painted word: the layer box plus the activation's payload.
#[derive(Debug, Clone, PartialEq)]
pub struct CefrBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// The word's own mark: the activation's payload, built once per walk.
    pub mark: GlossMark,
}

/// The one seam a red word's activation crosses: today the AI card.
fn activate(origin: &web_sys::EventTarget, mark: &GlossMark) {
    request_gloss_open(origin, mark);
}

/// Boxes paired with their index; a painted box carries its own.
fn indexed(boxes: Vec<CefrBox>) -> Vec<(usize, CefrBox)> {
    boxes.into_iter().enumerate().collect()
}

/// The mark the pointer is on, by the index its box carries.
fn mark_at(boxes: Signal<Vec<CefrBox>>, target: &web_sys::EventTarget) -> Option<GlossMark> {
    let element = target.clone().dyn_into::<web_sys::Element>().ok()?;
    let index = element.get_attribute(BOX_INDEX)?.parse::<usize>().ok()?;
    boxes.with_untracked(|all| all.get(index).map(|b| b.mark.clone()))
}

#[component]
pub fn CefrMarkLayer(
    /// The word boxes, already in this layer's own coordinates at scale 1.
    boxes: Signal<Vec<CefrBox>>,
    /// Multiplier from box coordinates to layer pixels; a reflow row
    /// always passes one.
    scale: Signal<f64>,
    /// Whether a red word is a control at all; off leaves it pure paint.
    click_explain: Signal<bool>,
    /// Whether hovering translates; independent of the click.
    hover_dict: Signal<bool>,
) -> impl IntoView {
    // One dwell timer for the layer: a sweep asks for nothing,
    // a rest asks once.
    let dwell = StoredValue::new_local(None::<TimeoutHandle>);
    let hovered = StoredValue::new_local(None::<GlossMark>);
    on_cleanup(move || {
        if let Some(handle) = dwell.try_get_value().flatten() {
            handle.clear();
        }
    });
    let disarm = move || {
        if let Some(handle) = dwell.try_get_value().flatten() {
            handle.clear();
        }
        let _ = dwell.try_set_value(None);
    };

    let on_over = move |ev: web_sys::MouseEvent| {
        disarm();
        if !hover_dict.get_untracked() {
            return;
        }
        let Some(origin) = ev.target() else {
            return;
        };
        let Some(mark) = mark_at(boxes, &origin) else {
            return;
        };
        let _ = hovered.try_set_value(Some(mark.clone()));
        let handle = set_timeout_with_handle(
            move || dispatch_typed_event_on(&origin, DICT_HOVER_EVENT, &mark),
            Duration::from_millis(HOVER_DWELL_MS),
        )
        .ok();
        let _ = dwell.try_set_value(handle);
    };

    let on_leave = move |ev: web_sys::MouseEvent| {
        disarm();
        let Some(mark) = hovered.try_get_value().flatten() else {
            return;
        };
        let _ = hovered.try_set_value(None);
        let Some(origin) = ev.target() else {
            return;
        };
        dispatch_typed_event_on(&origin, DICT_LEAVE_EVENT, &mark);
    };

    let on_click = move |ev: web_sys::MouseEvent| {
        if !click_explain.get_untracked() {
            return;
        }
        let Some(origin) = ev.target() else {
            return;
        };
        let Some(mark) = mark_at(boxes, &origin) else {
            return;
        };
        ev.stop_propagation();
        activate(&origin, &mark);
    };

    let on_down = move |ev: web_sys::MouseEvent| {
        // Only the click owns the selection; hover-only leaves it flowing.
        if click_explain.get_untracked() {
            ev.prevent_default();
        }
    };

    view! {
        <div
            class="cefr-layer"
            // A focusable mark under `aria-hidden` contradicts itself.
            attr:aria-hidden=move || (!click_explain.get()).then_some("true")
            on:mouseover=on_over
            on:mouseleave=on_leave
            on:click=on_click
            on:mousedown=on_down
        >
            <For
                each=move || indexed(boxes.get())
                key=|(_, b): &(usize, CefrBox)| format!("{}@{},{}", b.mark.word, b.x, b.y)
                children=move |(index, b): (usize, CefrBox)| {
                    let word = b.mark.word.clone();
                    view! {
                        <button
                            type="button"
                            class="cefr-mark"
                            // Two hundred boxes a page would flood the
                            // tab order; the pointer still reaches them.
                            tabindex="-1"
                            aria-label=format!("Explain {word}")
                            attr:data-cefr=index.to_string()
                            // A control while either feature answers;
                            // paint alone when both are off.
                            prop:disabled=move || {
                                !click_explain.get() && !hover_dict.get()
                            }
                            style=move || {
                                let s = scale.get();
                                format!(
                                    "left:{}px;top:{}px;width:{}px;height:{}px",
                                    b.x * s,
                                    b.y * s,
                                    b.w.max(1.0) * s,
                                    b.h.max(1.0) * s,
                                )
                            }
                        />
                    }
                }
            />
        </div>
    }
}
