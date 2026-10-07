//! The persistent gloss stroke layer: one per page host.

use std::collections::HashSet;

use ai_core::gloss::{GlossBox, GlossMark};
use leptos::prelude::*;

use crate::components::ai::gloss::selection_mode::{
    LONG_PRESS_MS, LONG_PRESS_SLOP_PX, dispatch_gloss_context, toggle_selected,
};
use app_ui::components::primitives::interactions::long_press::{LongPressOptions, use_long_press};

pub use app_ui::events::GLOSS_OPEN_EVENT;
use app_ui::events::dispatch_typed_event_on;

/// Exact-fit stroke radius, shared with the PDF screen box.
pub const MARK_RADIUS: f64 = 3.0;

#[component]
pub fn GlossMarkLayer(
    /// The page this host renders; `None` for a surface-wide layer.
    #[prop(default = None)]
    page: Option<u32>,
    /// Every mark of the open document.
    marks: Signal<Vec<GlossMark>>,
    /// Where a mark sits now, in this layer's coordinates.
    resolve: Callback<(GlossMark, f64), Option<GlossBox>>,
    /// What makes the layer re-derive.
    #[prop(into)]
    refresh: Signal<u64>,
    /// The display scale, handed to `resolve`.
    scale: ReadSignal<f64>,
    /// Id of the mark currently waiting on the model, if any.
    processing: Signal<Option<String>>,
    /// Whether gloss multi-select mode is active (long-press initiated).
    selecting: RwSignal<bool>,
    /// Ids of the currently selected marks; strokes paint the selected tint.
    selected: RwSignal<HashSet<String>>,
) -> impl IntoView {
    view! {
        <div
            class="gloss-layer"
            class=("gloss-layer-selecting", move || selecting.get())
            aria-hidden="false"
        >
            <For
                each=move || {
                    let all = marks.get();
                    match page {
                        Some(page) => all.into_iter().filter(|m| m.page == page).collect::<Vec<_>>(),
                        // A viewport-level layer cannot
                        // pre-filter by page.
                        None => all,
                    }
                }
                key=|m: &GlossMark| m.id.clone()
                children=move |m: GlossMark| {
                    // Exact-fit stroke: the resolved box,
                    // re-derived as inputs move.
                    let placed = {
                        let mark = m.clone();
                        Signal::derive(move || {
                            let _ = refresh.get();
                            resolve.run((mark.clone(), scale.get()))
                        })
                    };
                    // One memo, shared by the stroke and its pulse overlay.
                    let style = Signal::derive(move || {
                        placed
                            .get()
                            .map(stroke_pos_style)
                            .unwrap_or_else(|| "display:none".to_string())
                    });
                    let is_processing = {
                        let id = m.id.clone();
                        Signal::derive(move || {
                            processing.get().as_deref() == Some(id.as_str())
                        })
                    };
                    let is_selected = {
                        let id = m.id.clone();
                        move || selected.with(|s| s.contains(&id))
                    };

                    // ── Long-press gesture (generic primitive) ─────────────
                    let on_select = {
                        let id = m.id.clone();
                        Callback::new(move |_: ()| {
                            selecting.set(true);
                            selected.update(|s| {
                                s.insert(id.clone());
                            });
                        })
                    };
                    let lp = use_long_press(LongPressOptions {
                        press_ms: LONG_PRESS_MS,
                        slop_px: LONG_PRESS_SLOP_PX,
                        capture_pointer: true,
                        enabled: Signal::derive(move || !selecting.get_untracked()),
                        on_press: on_select,
                    });

                    let aria_id = m.id.clone();
                    let click_mark = m.clone();
                    let context_id = m.id.clone();

                    view! {
                        <button
                            type="button"
                            class="gloss-mark"
                            class=("gloss-mark-processing", move || is_processing.get())
                            class=("gloss-mark-selected", is_selected)
                            class=("gloss-mark-pressing", move || lp.pressing.get())
                            title=m.word.clone()
                            aria-label=format!("Explain {}", m.word)
                            aria-pressed=move || {
                                selecting.get().then(|| {
                                    if selected.with(|s| s.contains(&aria_id)) { "true" } else { "false" }
                                })
                            }
                            style=style
                            // Keep the document selection (and the page's own
                            // press handling) out of a stroke interaction.
                            on:mousedown=move |ev| ev.prevent_default()
                            on:pointerdown=move |ev| {
                                // Only the primary button starts a gesture —
                                // right-click owns the context menu.
                                if ev.button() != 0 {
                                    return;
                                }
                                (lp.on_pointerdown)(&ev);
                            }
                            on:pointermove=move |ev| (lp.on_pointermove)(&ev)
                            on:pointerup=move |ev| (lp.on_pointerup)(&ev)
                            on:pointercancel=move |ev| (lp.on_pointercancel)(&ev)
                            on:click=move |ev| {
                                ev.stop_propagation();
                                if (lp.swallow_click)() {
                                    return; // this press became a long-press
                                }
                                if selecting.get_untracked() {
                                    toggle_selected(selected, &click_mark.id);
                                    return;
                                }
                                if let Some(origin) = ev.target() {
                                    request_gloss_open(&origin, &click_mark);
                                }
                            }
                            on:contextmenu=move |ev| {
                                ev.prevent_default();
                                ev.stop_propagation();
                                if (lp.swallow_context)() {
                                    return; // synthetic, after a long-press
                                }
                                if selecting.get_untracked() {
                                    toggle_selected(selected, &context_id);
                                    return;
                                }
                                let Some(origin) = ev.target() else {
                                    return;
                                };
                                dispatch_gloss_context(
                                    &origin,
                                    ev.client_x() as f64,
                                    ev.client_y() as f64,
                                    &context_id,
                                );
                            }
                        />
                        // The animated half of the
                        // feedback: a non-blended overlay.
                        {move || {
                            is_processing.get().then(|| {
                                view! {
                                    <span
                                        class="gloss-mark-pulse"
                                        aria-hidden="true"
                                        style=style
                                    />
                                }
                            })
                        }}
                    }
                }
            />
        </div>
    }
}

/// Tell the popover to open on `mark`, as a bubbling CustomEvent.
pub fn request_gloss_open(origin: &web_sys::EventTarget, mark: &GlossMark) {
    dispatch_typed_event_on(origin, GLOSS_OPEN_EVENT, mark);
}

/// Position of a stroke inside its layer, at the live scale.
fn stroke_pos_style(box_: GlossBox) -> String {
    format!(
        "left:{}px;top:{}px;width:{}px;height:{}px",
        box_.x, box_.y, box_.w, box_.h,
    )
}
