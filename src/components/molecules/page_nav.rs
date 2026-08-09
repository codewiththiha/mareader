//! Previous/next + page-number input. OWNED BY branch B (viewer/chrome).
//! Prev/Next clamp viewer.page to 1..=num_pages; the editable readout parses on
//! commit and clamps the same way. Shows "– / –" when no document is open.

use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};
use crate::components::atoms::tooltip::Tooltip;
use crate::core::state::AppState;

#[component]
pub fn PageNav(state: AppState) -> impl IntoView {
    let num_pages = state.doc.num_pages;
    let page = state.viewer.page;

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

    let base_btn = "inline-flex h-9 w-9 items-center justify-center rounded-lg border text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent border-line bg-surface text-ink hover:bg-line";

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
        n == 0 || p >= n
    };

    view! {
        <div class="flex items-center gap-1">
            <Tooltip text="Previous page (ArrowLeft)".to_string()>
                <button
                    type="button"
                    title="Previous page (ArrowLeft)"
                    disabled=prev_disabled
                    on:click=move |_| {
                        let p = prev_state.viewer.page.get();
                        if p > 1 {
                            prev_state.viewer.page.set(p - 1);
                        }
                    }
                    class=move || {
                        if prev_disabled() {
                            format!("{base_btn} opacity-50")
                        } else {
                            base_btn.to_string()
                        }
                    }
                >
                    <Icon name=IconName::Prev size=16 />
                </button>
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
                            let max = commit_state.doc.num_pages.get().max(1);
                            commit_state.viewer.page.set(n.clamp(1, max));
                        }
                        // Invalid input: snap the readout back to the current page.
                        Err(_) => {
                            let cur = commit_state.viewer.page.get();
                            if commit_state.doc.num_pages.get() == 0 {
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

            <Tooltip text="Next page (ArrowRight)".to_string()>
                <button
                    type="button"
                    title="Next page (ArrowRight)"
                    disabled=next_disabled
                    on:click=move |_| {
                        let p = next_state.viewer.page.get();
                        let n = next_state.doc.num_pages.get();
                        if n > 0 && p < n {
                            next_state.viewer.page.set(p + 1);
                        }
                    }
                    class=move || {
                        if next_disabled() {
                            format!("{base_btn} opacity-50")
                        } else {
                            base_btn.to_string()
                        }
                    }
                >
                    <Icon name=IconName::Next size=16 />
                </button>
            </Tooltip>
        </div>
    }
}
