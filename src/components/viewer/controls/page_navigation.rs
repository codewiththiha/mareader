//! Previous/next + page-number input — and the stream's screenful twin.
//! Prev/Next clamp viewer.page to 1..=num_pages; the editable readout parses on
//! commit and clamps the same way. In Dual mode the buttons step whole spreads.
//!
//! The "– / –" empty branch is a safety net: this control is only mounted
//! from ReaderBottomBar on a Ready document, but the readout still has to
//! survive a close mid-keystroke.

use leptos::prelude::*;

use app_chrome::hooks::dom::page_list;
use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;
use app_chrome::tooltip::Tooltip;
use crate::state::ReaderState;
use reader_core::view::{ViewMode, last_spread_start, spread_start, spread_step_next, spread_step_prev};

/// One screenful step through the continuous text stream, with the
/// percentage standing where the page readout sits in the paged modes.
/// A page number means nothing where the stream flows, but the gesture —
/// step forward, step back, see where you are — is the same gesture, so
/// the control keeps the same seat and shape. The step keeps a sliver of
/// the outgoing screen (the keyboard's PageDown keeps the same overlap) so
/// a screen turn never loses the reading line.
#[component]
pub fn StreamPageNav(state: ReaderState) -> impl IntoView {
    view! {
        <div class="flex items-center gap-1">
            <Tooltip text="Previous screen (ArrowLeft)">
                <IconButton
                    icon=IconName::Prev
                    title="Previous screen (ArrowLeft)"
                    on_click=move || step_screen(-1.0)
                />
            </Tooltip>
            <span class="w-10 text-center text-sm tabular-nums text-muted">
                {move || format!("{}%", state.stream_percent())}
            </span>
            <Tooltip text="Next screen (ArrowRight)">
                <IconButton
                    icon=IconName::Next
                    title="Next screen (ArrowRight)"
                    on_click=move || step_screen(1.0)
                />
            </Tooltip>
        </div>
    }
}

/// Scroll the stream one screenful in `direction` (-1 up, 1 down), keeping
/// a tenth of the outgoing screen for continuity.
fn step_screen(direction: f64) {
    let Some(el) = page_list() else {
        return;
    };
    let viewport = el.client_height() as f64;
    let max = (el.scroll_height() - el.client_height()).max(0) as f64;
    let next = ((el.scroll_top() as f64) + direction * viewport * 0.9).clamp(0.0, max);
    el.set_scroll_top(next as i32);
}

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
