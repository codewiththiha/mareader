//! The shared ink layer: boxes, the click that explains,
//! the hover that translates.

use leptos::prelude::*;

use ai_core::gloss::GlossMark;
use app_ui::events::{DICT_HOVER_EVENT, DICT_LEAVE_EVENT};
use app_ui::events::dispatch_typed_event_on;

use crate::components::ai::gloss::mark_layer::request_gloss_open;

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

/// A red word under the pointer: the hover card's own seam.
fn hover_in(origin: &web_sys::EventTarget, mark: &GlossMark) {
    dispatch_typed_event_on(origin, DICT_HOVER_EVENT, mark);
}

/// The pointer left the word; the hover card may retire.
fn hover_out(origin: &web_sys::EventTarget, mark: &GlossMark) {
    dispatch_typed_event_on(origin, DICT_LEAVE_EVENT, mark);
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
    view! {
        <div
            class="cefr-layer"
            // A focusable mark under `aria-hidden` contradicts itself.
            attr:aria-hidden=move || (!click_explain.get()).then_some("true")
        >
            <For
                each=move || boxes.get()
                key=|b: &CefrBox| format!("{}@{},{}", b.mark.word, b.x, b.y)
                children=move |b: CefrBox| {
                    // The gloss stroke's gesture shape, mirrored.
                    let mark = b.mark.clone();
                    let word = mark.word.clone();
                    let hover_mark = mark.clone();
                    let out_mark = mark.clone();
                    let explain = move |ev: web_sys::MouseEvent| {
                        ev.stop_propagation();
                        if let Some(origin) = ev.target() {
                            activate(&origin, &mark);
                        }
                    };
                    view! {
                        <button
                            type="button"
                            class="cefr-mark"
                            // Two hundred boxes a page would flood the
                            // tab order; the pointer still reaches them.
                            tabindex="-1"
                            aria-label=format!("Explain {word}")
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
                            on:mousedown=move |ev: web_sys::MouseEvent| {
                                // Only the click owns the selection;
                                // hover-only leaves it flowing.
                                if click_explain.get_untracked() {
                                    ev.prevent_default();
                                }
                            }
                            on:click=move |ev: web_sys::MouseEvent| {
                                if click_explain.get_untracked() {
                                    explain(ev);
                                }
                            }
                            on:mouseenter=move |ev: web_sys::MouseEvent| {
                                if hover_dict.get_untracked()
                                    && let Some(origin) = ev.target()
                                {
                                    hover_in(&origin, &hover_mark);
                                }
                            }
                            on:mouseleave=move |ev: web_sys::MouseEvent| {
                                if hover_dict.get_untracked()
                                    && let Some(origin) = ev.target()
                                {
                                    hover_out(&origin, &out_mark);
                                }
                            }
                        />
                    }
                }
            />
        </div>
    }
}
