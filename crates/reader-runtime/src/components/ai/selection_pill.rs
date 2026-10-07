use ai_core::gloss::{GlossMark, is_glossable, is_hintable};
use leptos::prelude::*;

use crate::components::ai::anchor::{
    FormatAnchorBridge, ReflowAnchorBridge, anchor_resolver, capture_selection_mark, captured_mark,
    no_invalidation, reflow_invalidation, watch_page_anchor,
};
use crate::components::ai::gloss::mark_layer::request_gloss_open;
use crate::components::ai::reflow_anchor::spot_envelope;
use app_chrome::icon::{Icon, IconName};

/// A floating pill near the reader's text selection, with the
/// Explain button.
#[component]
pub fn SelectionPill(state: crate::context::ReaderContext) -> impl IntoView {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    // The pill follows the selection through whichever format owns it.
    let spot = Signal::derive(move || state.reader.ai_selection.detail.get().and_then(|d| d.spot));
    let resolve = anchor_resolver(state.reader, spot);
    // A re-cut relocates a selection with nothing scrolling.
    let invalidate = if state.reader.reflowable_now() {
        reflow_invalidation(state.reader)
    } else {
        no_invalidation()
    };
    let watch = watch_page_anchor(
        Signal::derive(move || state.reader.ai_selection.anchor.get()),
        resolve,
        state.reader.viewer.zoom.display.into(),
        state.reader.viewer.scroll_top.into(),
        state.reader.viewer.page.into(),
        invalidate,
    );

    // Once the origin leaves the viewport, the menu is gone for good.
    Effect::new(move |_| {
        if watch.exited.get() && detail.get().is_some() {
            detail.set(None);
            state.reader.ai_selection.anchor.set(None);
        }
    });

    // Live position: re-derived from the page host, so it travels with scroll.
    let style = Signal::derive(move || {
        let Some(b) = watch.screen.get() else {
            return String::new();
        };
        let left = b.x + b.w / 2.0;
        let top = b.y + b.h + 8.0;
        format!(
            "position:fixed; left:{left}px; top:{top}px; \
             transform:translateX(-50%);"
        )
    });

    // Past the word-lookup cap the pill renders muted, then not at all.
    let too_long = Signal::derive(move || detail.get().is_some_and(|s| !is_glossable(&s.text)));

    let visible = Signal::derive(move || {
        detail.get().is_some_and(|s| is_hintable(&s.text))
            && !popover_open.get()
            && !watch.exited.get()
            && watch.screen.get().is_some()
    });

    view! {
        <Show when=move || visible.get()>
            <div
                data-ai-popover=""
                style=move || style.get()
                class=format!("ai-pill-enter {}", app_chrome::layers::AI_SELECTION)
            >
                <button
                    type="button"
                    disabled=move || too_long.get()
                    title=move || {
                        if too_long.get() {
                            "Selection too long for a word lookup"
                        } else {
                            "Explain with AI"
                        }
                    }
                    aria-label="Explain selected text with AI"
                    // Preventing the default keeps the
                    // selection alive behind the card.
                    on:mousedown=move |ev| ev.prevent_default()
                    on:click=move |_| {
                        // Disabled buttons don't fire; belt and braces.
                        if too_long.get_untracked() {
                            return;
                        }
                        let Some(sel) = detail.get_untracked() else {
                            return;
                        };
                        // Prefer the captured anchor; else
                        // a live DOM capture.
                        let captured = state.reader.ai_selection.anchor.get_untracked();
                        let reflow = sel.is_reflow();
                        let mark: Option<GlossMark> = captured
                            .map(|pa| {
                                // One word passes the gate, so
                                // trimming gives the canonical token.
                                let word = sel.text.trim();
                                // The context is an envelope: spot
                                // plus sentence.
                                let context = match sel.spot {
                                    Some(spot) if reflow => spot_envelope(&spot, &sel.context),
                                    _ => sel.context.trim().to_string(),
                                };
                                captured_mark(word, context, pa)
                            })
                            .or_else(|| {
                                if reflow {
                                    // No usable anchor: walk the live
                                    // range and build the envelope here.
                                    ReflowAnchorBridge {
                                        state: state.reader,
                                        spot: None,
                                        mode: state.reader.viewer.mode.get_untracked(),
                                    }
                                    .capture(state.reader.viewer.zoom.visual_scale())
                                    .and_then(|pa| {
                                        crate::components::ai::reflow_anchor::capture_selection(
                                            state.reader,
                                        )
                                        .map(|(spot, _)| (spot, pa))
                                    })
                                    .map(|(spot, pa)| {
                                        captured_mark(
                                            sel.text.trim(),
                                            spot_envelope(&spot, &sel.context),
                                            pa,
                                        )
                                    })
                                } else {
                                    capture_selection_mark(
                                        state.reader.viewer.zoom.visual_scale(),
                                        sel.text.clone(),
                                        sel.context.clone(),
                                    )
                                }
                            });
                        let root = state.reader.dom.root();
                        if let (Some(m), Some(root)) = (mark, root) {
                            // Self-contained open: the mark
                            // rides the request.
                            request_gloss_open(&root, &m);
                        } else {
                            // Don't leave a stale open flag if capture failed.
                            popover_open.set(false);
                        }
                    }
                    class="flex min-h-11 items-center gap-1.5 rounded-full border border-line \
                           bg-surface px-5 text-sm font-medium tracking-wide text-ink \
                           shadow-[var(--gloss-shadow-float)] \
                           transition-[transform,background-color,opacity] duration-150 ease-out \
                           active:scale-[0.96] \
                           disabled:cursor-not-allowed disabled:opacity-45 disabled:active:scale-100 \
                           focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                >
                    <Icon name=IconName::More size=13 />
                    <span>"Explain"</span>
                </button>
            </div>
        </Show>
    }
}
