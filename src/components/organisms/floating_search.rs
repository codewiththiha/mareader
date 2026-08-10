//! Floating search overlay (Chrome/VS-Code style). Phase 2 of the redesign:
//! search is a transient task, so it lives in a floating bar over the viewer
//! rather than in the docked sidebar. Mounted by the coordinator inside
//! `main#viewer-slot` (which is `relative`); the bar positions itself at the
//! slot's top-right, just below the toolbar.

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::engine;
use crate::components::atoms::icon::{Icon, IconName};
use crate::core::state::AppState;
use crate::effects::search_effects::{jump_to_result, run_search, search_navigate};

use super::search_panel::ResultList;

/// Compact icon-button class shared by the bar's raw `<button>`s.
const ICON_BTN: &str = "inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";

/// Small up/down chevron used by the prev/next result buttons.
#[component]
fn Chevron(up: bool) -> impl IntoView {
    let paths = if up {
        "<path d='m18 15-6-6-6 6'/>"
    } else {
        "<path d='m6 9 6 6 6-6'/>"
    };
    view! {
        <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            inner_html=paths
        />
    }
}

#[component]
pub fn FloatingSearch(state: AppState) -> impl IntoView {
    let last_query = RwSignal::new(String::new());
    let show_results = RwSignal::new(false);
    // Monotonic id for the newest submitted search; guards against out-of-order
    // completions so only the latest submit lands its jump / marker.
    let search_gen = RwSignal::new(0u64);
    let input_ref: NodeRef<html::Input> = NodeRef::new();
    let container_ref: NodeRef<html::Div> = NodeRef::new();

    // Autofocus the query input whenever the overlay becomes visible. Deferred
    // to a microtask so the input node exists by the time it runs (the Show
    // mounts the bar in the same flush the visible flag flips).
    Effect::new(move |_| {
        if state.search.visible.get() {
            queue_microtask(move || {
                if let Some(node) = input_ref.get() {
                    let _ = node.focus();
                }
            });
        }
    });

    // Outside-click dismiss: while visible, any pointerdown landing outside the
    // bar's DOM node closes it. The listener is re-registered on each
    // visible-flip and removed on cleanup. Hiding the bar also closes an open
    // results dropdown so reopening starts from a clean slate.
    Effect::new(move |_| {
        if state.search.visible.get() {
            let handle = window_event_listener(
                leptos::ev::pointerdown,
                move |ev: leptos::ev::PointerEvent| {
                    let target: web_sys::Node = event_target(&ev);
                    let contains = container_ref
                        .get()
                        .as_ref()
                        .map_or(false, |c| c.contains(Some(&target)));
                    if !contains {
                        state.search.visible.set(false);
                    }
                },
            );
            on_cleanup(move || handle.remove());
        } else {
            show_results.set(false);
        }
    });

    // Run the query and land on the first result. `run_search` only marks
    // active = Some(0) without navigating; the explicit jump keeps the viewport
    // in sync with the highlighted first match and makes the first "next"
    // advance instead of skipping it.
    //
    // Empty submissions clear the stale results/highlights instead of reusing
    // the previous query's state. `search_gen` + the query-unchanged check keep
    // a superseded search (an older query resolving late, or the user typing
    // mid-flight) from applying its jump or its `last_query` marker.
    let submit = move || {
        let q = state.search.query.get();
        if q.trim().is_empty() {
            engine::clear_highlights();
            state.search.total.set(0);
            state.search.results.set(Vec::new());
            state.search.active.set(None);
            last_query.set(q);
            return;
        }
        search_gen.update(|g| *g += 1);
        let gen = search_gen.get();
        spawn_local(async move {
            run_search(state).await;
            // Only the newest submit applies; skip if a newer search was
            // submitted or the input changed while this one was in flight.
            if search_gen.get() == gen && state.search.query.get() == q {
                // Mark as searched only when this query actually produced
                // results, so a failed/empty search can be retried with Enter.
                if !state.search.results.get().is_empty() {
                    last_query.set(q);
                }
                let first_page = state.search.results.get().first().map(|r| r.page);
                if let Some(p) = first_page {
                    jump_to_result(state, p);
                }
            }
        });
    };

    // Enter = submit on a fresh/empty query, else next result; Shift+Enter =
    // previous; Escape = close. stopPropagation on Escape keeps the global
    // shortcut from ALSO closing the sidebar in the same keystroke (the bar is
    // the first dismiss step).
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let key = ev.key();
        if key == "Enter" {
            if ev.shift_key() {
                search_navigate(state, -1);
            } else if state.search.query.get().trim().is_empty()
                || last_query.get() != state.search.query.get()
            {
                submit();
            } else {
                search_navigate(state, 1);
            }
        } else if key == "Escape" {
            ev.stop_propagation();
            state.search.visible.set(false);
        }
    };

    view! {
        <Show when=move || state.search.visible.get()>
            <div
                node_ref=container_ref
                class="floating-search-enter absolute right-4 top-2 z-40 w-[min(560px,90vw)] rounded-xl border border-line bg-surface/90 shadow-xl backdrop-blur-md"
            >
                <div class="flex items-center gap-1.5 p-1.5">
                    <button
                        type="button"
                        title="Search (Enter)"
                        on:click=move |_| submit()
                        class="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-transparent text-muted transition-colors hover:bg-line hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    >
                        <Icon name=IconName::Search size=16 />
                    </button>
                    <input
                        node_ref=input_ref
                        type="text"
                        placeholder="Search in document…"
                        class="h-9 min-w-0 flex-1 rounded-lg border border-line bg-paper px-2.5 text-sm text-ink placeholder:text-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        prop:value=move || state.search.query.get()
                        on:input=move |ev| state.search.query.set(event_target_value(&ev))
                        on:keydown=on_keydown
                    />
                    <span class="whitespace-nowrap px-1 text-xs text-muted tabular-nums">
                        {move || match state.search.active.get() {
                            Some(i) => format!("{}/{}", i + 1, state.search.total.get()),
                            None => String::new(),
                        }}
                    </span>
                    <button
                        type="button"
                        title="Previous result (Shift+Enter)"
                        on:click=move |_| search_navigate(state, -1)
                        class=ICON_BTN
                    >
                        <Chevron up=true />
                    </button>
                    <button
                        type="button"
                        title="Next result (Enter)"
                        on:click=move |_| search_navigate(state, 1)
                        class=ICON_BTN
                    >
                        <Chevron up=false />
                    </button>
                    <button
                        type="button"
                        title="Close (Esc)"
                        on:click=move |_| state.search.visible.set(false)
                        class=ICON_BTN
                    >
                        <Icon name=IconName::Close size=16 />
                    </button>
                    <button
                        type="button"
                        title="Toggle results list"
                        on:click=move |_| show_results.update(|v| *v = !*v)
                        class=ICON_BTN
                    >
                        <svg
                            width="14"
                            height="14"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            aria-hidden="true"
                            inner_html=move || {
                                if show_results.get() {
                                    "<path d='m18 15-6-6-6 6'/>"
                                } else {
                                    "<path d='m6 9 6 6 6-6'/>"
                                }
                            }
                        />
                    </button>
                </div>

                <Show when=move || show_results.get()>
                    <div
                        class="absolute right-0 top-full z-50 mt-1 w-full overflow-hidden rounded-xl border border-line bg-surface shadow-xl"
                        on:click=move |_| show_results.set(false)
                    >
                        <div class="max-h-72 overflow-y-auto">
                            <ResultList state=state />
                        </div>
                    </div>
                </Show>
            </div>
        </Show>
    }
}
