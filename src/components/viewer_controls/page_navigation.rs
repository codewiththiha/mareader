//! Previous/next + page-number input.
//! Prev/Next clamp viewer.page to 1..=num_pages; the editable readout parses on
//! commit and clamps the same way. In Dual mode the buttons step whole spreads.
//!
//! The "– / –" empty branch is a safety net: this control is only mounted
//! from ReaderBottomBar on a Ready document, but the readout still has to
//! survive a close mid-keystroke.

use leptos::prelude::*;

use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;
use app_chrome::tooltip::Tooltip;
use crate::state::ReaderState;
use pdf_core::layout::{ViewMode, last_spread_start, spread_start, spread_step_next, spread_step_prev};

#[component]
pub fn PageNavigation(state: ReaderState) -> impl IntoView {
    let num_pages = state.document.num_pages;
    let page = state.viewer.page;
    let mode = state.viewer.mode;

    // Editable readout. A local signal holds the text so typing never fights the
    // reactive `page` signal; the effect resyncs it when page/numpages change.
    let text = RwSignal::new("–".to_string());
    Effect::new(move || {
        let n = num_pages.get();
        let p = page.get();
        if n == 0 {
            text.set("–".to_string());
        } else {
            text.set(p.to_string());
        }
    });

    let prev_state = state;
    let next_state = state;
    let commit_state = state;

    // Named closures (avoids the view! macro terminating a `move ||` body at a
    // top-level `||` when comparing with `>=`).
    let prev_disabled = move || {
        let n = num_pages.get();
        let p = page.get();
        n == 0 || p == 1
    };
    let next_disabled = move || {
        let n = num_pages.get();
        let p = page.get();
        if n == 0 {
            return true;
        }
        if mode.get() == ViewMode::Spread {
            spread_start(p) >= last_spread_start(n)
        } else {
            p >= n
        }
    };

    view! {
        <div class="flex items-center gap-1">
            <Tooltip text="Previous page (ArrowLeft)">
                <IconButton
                    icon=IconName::Prev
                    title="Previous page (ArrowLeft)"
                    disabled=Signal::derive(prev_disabled)
                    on_click=move || {
                        if prev_state.viewer.mode.get() == ViewMode::Spread {
                            let next = spread_step_prev(prev_state.viewer.page.get());
                            prev_state.viewer.page.set(next);
                        } else {
                            let p = prev_state.viewer.page.get();
                            if p > 1 {
                                prev_state.viewer.page.set(p - 1);
                            }
                        }
                    }
                />
            </Tooltip>

            <input
                type="text"
                inputmode="numeric"
                title="Page number"
                class="h-9 w-12 rounded-lg border border-line bg-surface px-1 text-center text-sm text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                prop:value=move || text.get()
                on:input=move |ev| text.set(event_target_value(&ev))
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    match v.trim().parse::<u32>() {
                        Ok(n) => {
                            let max = commit_state.document.num_pages.get().max(1);
                            let clamped = n.clamp(1, max);
                            if commit_state.viewer.mode.get() == ViewMode::Spread {
                                // Snap typed input onto the spread that contains it.
                                commit_state.viewer.page.set(spread_start(clamped));
                            } else {
                                commit_state.viewer.page.set(clamped);
                            }
                        }
                        // Invalid input: snap the readout back to the current page.
                        Err(_) => {
                            let cur = commit_state.viewer.page.get();
                            if commit_state.document.num_pages.get() == 0 {
                                text.set("–".to_string());
                            } else {
                                text.set(cur.to_string());
                            }
                        }
                    }
                }
            />
            <span class="text-sm text-muted">/</span>
            <span class="w-8 text-sm text-muted">
                {move || {
                    if num_pages.get() > 0 {
                        num_pages.get().to_string()
                    } else {
                        "–".to_string()
                    }
                }}
            </span>

            <Tooltip text="Next page (ArrowRight)">
                <IconButton
                    icon=IconName::Next
                    title="Next page (ArrowRight)"
                    disabled=Signal::derive(next_disabled)
                    on_click=move || {
                        let n = next_state.document.num_pages.get();
                        if next_state.viewer.mode.get() == ViewMode::Spread {
                            let next = spread_step_next(n, next_state.viewer.page.get());
                            next_state.viewer.page.set(next);
                        } else {
                            let p = next_state.viewer.page.get();
                            if n > 0 && p < n {
                                next_state.viewer.page.set(p + 1);
                            }
                        }
                    }
                />
            </Tooltip>
        </div>
    }
}
