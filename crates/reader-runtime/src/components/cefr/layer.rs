//! The shared ink layer: either format's boxes and the AI click.

use leptos::prelude::*;

use ai_core::gloss::GlossMark;

use crate::components::ai::gloss::mark_layer::request_gloss_open;

/// One painted word: the layer box plus the click's AI payload.
#[derive(Debug, Clone, PartialEq)]
pub struct CefrBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// The word's own mark: the click's payload, built once per walk.
    pub mark: GlossMark,
}

#[component]
pub fn CefrMarkLayer(
    /// The word boxes, already in this layer's own coordinates at scale 1.
    boxes: Signal<Vec<CefrBox>>,
    /// Multiplier from box coordinates to layer pixels; a reflow row
    /// always passes one.
    scale: Signal<f64>,
    /// Whether a click opens the AI card; hover stays visual either way.
    click_explain: Signal<bool>,
) -> impl IntoView {
    view! {
        <div class="cefr-layer" aria-hidden="true">
            <For
                each=move || boxes.get()
                key=|b: &CefrBox| {
                    format!("{}@{},{}", b.mark.word, b.x, b.y)
                }
                children=move |b: CefrBox| {
                    // The gloss stroke's gesture shape, mirrored.
                    let mark = b.mark.clone();
                    let word = mark.word.clone();
                    let explain = move |ev: web_sys::MouseEvent| {
                        ev.stop_propagation();
                        if !click_explain.get_untracked() {
                            return;
                        }
                        if let Some(origin) = ev.target() {
                            request_gloss_open(&origin, &mark);
                        }
                    };
                    view! {
                        <button
                            type="button"
                            class="cefr-mark"
                            // Generated ink: reachable by pointer, never by
                            // tab, or a page floods the tab order.
                            tabindex="-1"
                            title=word.clone()
                            aria-label=format!("Explain {word}")
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
                                ev.prevent_default();
                            }
                            on:click=explain
                        />
                    }
                }
            />
        </div>
    }
}
