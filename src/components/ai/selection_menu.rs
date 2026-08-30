use leptos::prelude::*;
use pdf_core::gloss::{GlossMark, is_glossable, is_hintable};

use crate::components::ai::anchor::{MENU_EXIT_FRAC, capture_selection_mark, watch_page_anchor};
use crate::components::ai::gloss::mark_layer::request_gloss_open;
use crate::components::primitives::icon::{Icon, IconName};
use crate::state::AppState;

/// A small floating pill that appears near the user's text selection.
/// Contains the "Info" button that opens the AI popover.
///
/// Position is re-derived from a page-space anchor on every scroll/zoom/mode
/// change, so the pill travels with the word and disappears once the origin
/// fully leaves the viewport.
///
/// Length gate: word lookup is for words and short phrases. Past
/// `pdf_core::gloss::MAX_GLOSS_CHARS` the pill stays visible but MUTED
/// (disabled, explaining tooltip) up to the hint band's edge, and vanishes
/// beyond it — a disabled affordance reads as a rule, where a silently
/// vanishing menu reads as a bug.
///
/// The Info click does **not** flip `popover_open` and hope `detail` survives:
/// it builds a self-contained [`GlossMark`] at click time and dispatches the
/// same `pdfreader:gloss-open` event the persisted stroke uses. The popover's
/// listener bumps `open_req` and sets `pending_mark`, so the open effect is
/// guaranteed to run with a mark in hand — no race against the exit-watch
/// clearing `detail`, and no stale-`true` suppression across documents.
///
/// The root carries `data-ai-popover`: the engine's selection tracker
/// treats mousedowns inside that attribute as AI-UI interaction and does
/// NOT clear the selection detail — otherwise the button would swallow
/// its own click (the press collapses the selection before click fires).
#[component]
pub fn SelectionMenu(state: AppState) -> impl IntoView {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    let watch = watch_page_anchor(
        Signal::derive(move || state.reader.ai_selection.anchor.get()),
        state.reader.viewer.zoom.display.into(),
        state.reader.viewer.mode.into(),
        state.reader.viewer.scroll_top.into(),
        state.reader.viewer.page.into(),
        MENU_EXIT_FRAC,
    );

    // Once the selection's origin leaves the viewport, the menu is gone for
    // good (same as before: the next selection replaces it).
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

    // Selection past the word-lookup cap: the pill renders muted inside the
    // hint band and not at all beyond it (see `visible` below).
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
                class=format!("ai-selection-menu-enter {}", crate::components::primitives::floating::types::z::AI_SELECTION)
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
                    // Preventing the mousedown default keeps the document
                    // selection (and focus) alive, so the highlight stays
                    // visible behind the card this button opens — and the
                    // button can never be unmounted by its own press.
                    on:mousedown=move |ev| ev.prevent_default()
                    on:click=move |_| {
                        // Disabled buttons don't fire; belt and braces.
                        if too_long.get_untracked() {
                            return;
                        }
                        let Some(sel) = detail.get_untracked() else {
                            return;
                        };
                        let scale = state.reader.viewer.zoom.display.get_untracked();
                        // Prefer the page-space anchor captured with the
                        // selection; fall back to a live DOM capture.
                        let mark = state
                            .reader
                            .ai_selection
                            .anchor
                            .get_untracked()
                            .map(|pa| GlossMark {
                                id: format!("g{}-{}", pa.page, js_sys::Date::now() as u64),
                                page: pa.page,
                                // Only a single word passes the `is_glossable`
                                // gate this click is behind, so trimming the
                                // edges yields the canonical token — the card
                                // header and the persisted mark never carry a
                                // stray surrounding space.
                                word: sel.text.trim().to_string(),
                                context: sel.context.trim().to_string(),
                                rect: pa.rect,
                            })
                            .or_else(|| {
                                capture_selection_mark(scale, sel.text.clone(), sel.context.clone())
                            });
                        if let Some(m) = mark {
                            // Self-contained open: bumps open_req with the
                            // mark in hand. Never races detail being cleared.
                            request_gloss_open(&m);
                        } else {
                            // Don't leave a stale open flag if capture failed.
                            popover_open.set(false);
                        }
                    }
                    class="flex min-h-11 items-center gap-1.5 rounded-full border border-line \
                           bg-surface px-5 text-sm font-medium tracking-wide text-ink \
                           shadow-[var(--gloss-shadow-menu)] \
                           transition-[transform,background-color,opacity] duration-150 ease-out \
                           active:scale-[0.96] \
                           disabled:cursor-not-allowed disabled:opacity-45 disabled:active:scale-100 \
                           focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                >
                    <Icon name=IconName::More size=13 />
                    <span>"Info"</span>
                </button>
            </div>
        </Show>
    }
}
