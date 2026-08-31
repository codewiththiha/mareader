//! The persistent gloss highlighter stroke layer.
//!
//! One of these is rendered by every `.pdf-page` host, alongside the canvas
//! and the text layer. Because Leptos owns it, it is re-created whenever the
//! page mounts — which is what makes a mark survive the virtualizer's
//! unmounting, a zoom's textLayer rebuild, a view-mode flip and (through
//! localStorage) the session itself. Marks are page-space rects, so the only
//! thing that changes on zoom is the multiplication by `scale`.
//!
//! This stroke is the reader's ONLY highlight for a glossed word: the native
//! `::selection` tint is cleared when the gloss takes over, and the morphing
//! surface unmounts once it has settled back onto this box, so nothing ever
//! stacks on top of it. It is also the whole "thinking" UI — while the model
//! is working the stroke wears `gloss-mark-processing` (a quiet ring) and a
//! non-blended `gloss-mark-pulse` overlay laps a faint light round the
//! stroke's inner edge; no surface exists at all until the first chunk
//! lands.
//!
//! Interaction surface:
//! * **Click** re-opens the card (toggle-to-close lives in the controller's
//!   open effect) — or, in selection mode, toggles selection instead.
//! * **Long-press** (≥ `LONG_PRESS_MS`, drift < `LONG_PRESS_SLOP_PX`) enters
//!   multi-select mode with this mark already selected. The gesture itself is
//!   the generic [`use_long_press`] primitive; the press uses pointer capture
//!   so it survives drifting off the stroke, and the click (plus the
//!   synthetic `contextmenu` mobile fires after it) are swallowed by one-shot
//!   suppression flags.
//! * **Right-click** asks for the remove menu (`pdfreader:gloss-context`) —
//!   or toggles selection when selection mode is already active.
//!
//! Clicking travels as a `pdfreader:gloss-open` CustomEvent rather than a
//! callback prop: the popover lives at the reader-page level, far above the
//! page hosts, and threading a callback through `PageList`/`SinglePageView`/
//! `PageCanvas` would couple three view layers to the AI feature for one
//! message. Selection state is the exception: it is shared reactive state on
//! `state.reader.gloss`, threaded down like `marks` and `processing` because
//! every stroke must repaint the moment it changes.

use std::collections::HashSet;

use leptos::prelude::*;
use pdf_core::gloss::{GlossBox, GlossMark};

use crate::components::ai::gloss::selection_mode::{
    dispatch_gloss_context, toggle_selected, LONG_PRESS_MS, LONG_PRESS_SLOP_PX,
};
use crate::components::primitives::interactions::long_press::{use_long_press, LongPressOptions};

pub use crate::events::GLOSS_OPEN_EVENT;
use crate::events::dispatch_typed_event;

/// Exact-fit stroke radius. Shared with `ai::anchor::screen_box` so the
/// morphing surface settles onto EXACTLY the box the stroke occupies — one
/// geometry. No hug-padding: the stroke is the stored union rect itself.
pub const MARK_RADIUS: f64 = 3.0;

#[component]
pub fn GlossMarkLayer(
    /// 1-based page this host renders; the layer paints only its own marks.
    page: u32,
    /// Every mark of the open document.
    marks: Signal<Vec<GlossMark>>,
    /// The display scale, so the rects follow a zoom for free.
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
            class="glossLayer"
            class=("glossLayer-selecting", move || selecting.get())
            aria-hidden="false"
        >
            <For
                each=move || {
                    marks.get().into_iter().filter(|m| m.page == page).collect::<Vec<_>>()
                }
                key=|m: &GlossMark| m.id.clone()
                children=move |m: GlossMark| {
                    let rect = m.rect;
                    // Exact-fit stroke: the stored union rect itself, no padding.
                    let style = move || stroke_pos_style(rect, scale.get());
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
                                request_gloss_open(&click_mark);
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
                                dispatch_gloss_context(
                                    ev.client_x() as f64,
                                    ev.client_y() as f64,
                                    &context_id,
                                );
                            }
                        />
                        // The animated half of the processing feedback: a
                        // sibling overlay that is NOT mix-blended, so the
                        // light lapping its inner edge composites over the
                        // static canvas like the card's and toast's do —
                        // animating the blended stroke itself would re-blend
                        // the canvas every frame (the page shake, see
                        // gloss.css). Mounted only while this stroke is the
                        // one waiting on the model.
                        {move || {
                            is_processing.get().then(|| {
                                view! {
                                    <span
                                        class="gloss-mark-pulse"
                                        aria-hidden="true"
                                        style=move || stroke_pos_style(rect, scale.get())
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

/// Tell the popover to open on `mark`. Used by both the persisted stroke
/// click and the selection Info pill so every open is a self-contained
/// CustomEvent (mark in the detail) that bumps `open_req` — never a bare
/// `popover_open = true` that races against `detail` being cleared.
pub fn request_gloss_open(mark: &GlossMark) {
    dispatch_typed_event(GLOSS_OPEN_EVENT, mark);
}

/// Screen-space position of a stroke at the current display scale. Shared
/// verbatim by the stroke button and the processing pulse overlay so the
/// animated ring can never drift from the stroke it emphasises.
fn stroke_pos_style(rect: GlossBox, s: f64) -> String {
    format!(
        "left:{}px;top:{}px;width:{}px;height:{}px",
        rect.x * s,
        rect.y * s,
        rect.w * s,
        rect.h * s,
    )
}
