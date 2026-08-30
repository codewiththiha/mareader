//! Floating search overlay (Chrome/VS-Code style). Phase 2 of the redesign:
//! search is a transient task, so it lives in a floating bar over the viewer
//! rather than in the docked sidebar. Mounted by the coordinator inside
//! `main#viewer-slot` (which is `relative`); the bar positions itself at the
//! slot's top-right, just below the toolbar.

use std::time::Duration;

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use virtual_list_leptos::Virtualizer;

use super::result_list::ResultList;
use crate::components::primitives::floating::dismiss::{
    DismissPolicy, DismissTrigger, use_dismiss,
};
use crate::components::primitives::floating::types::z::{BAR, POPOVER};
use crate::components::primitives::hooks::use_timeout::use_debounce;
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::icon_button::IconButton;
use crate::effects::reader::search::{
    activate_match, clear_search, dismiss_search, run_search, search_navigate,
};
use crate::state::ReaderState;

const SEARCH_DEBOUNCE_MS: u64 = 180;

#[component]
pub fn FloatingSearch(
    state: ReaderState,
    virtualizer: StoredValue<Virtualizer, LocalStorage>,
) -> impl IntoView {
    let (last_query, set_last_query) = signal(String::new());
    let (show_results, set_show_results) = signal(false);
    let (search_gen, set_search_gen) = signal(0u64);
    let input_ref: NodeRef<html::Input> = NodeRef::new();
    let container_ref: NodeRef<html::Div> = NodeRef::new();

    Effect::new(move |_| {
        if state.search.visible.get() {
            queue_microtask(move || {
                if let Some(node) = input_ref.get() {
                    _ = node.focus();
                }
            });
        }
    });

    use_dismiss(
        state.search.visible.into(),
        Callback::new(move |_| dismiss_search(state)),
        DismissPolicy {
            escape: false,
            outside: Some(DismissTrigger::PointerDown),
            exclude_selectors: vec!["[data-search-chrome='true']"],
            enabled: None,
            topmost_only: false,
        },
        move |node| {
            container_ref
                .get()
                .as_ref()
                .is_some_and(|c| c.contains(Some(node)))
        },
    );
    Effect::new(move |_| {
        if !state.search.visible.get() {
            set_show_results.set(false);
        }
    });

    let fire_search = move |then: Option<Box<dyn Fn()>>| {
        let q = state.search.query.get_untracked();
        if q.trim().is_empty() {
            clear_search(state);
            set_last_query.set(String::new());
            return;
        }
        set_search_gen.update(|g| *g += 1);
        let started = search_gen.get_untracked();
        spawn_local(async move {
            run_search(state).await;
            if search_gen.get_untracked() != started || state.search.query.get_untracked() != q {
                return;
            }
            set_last_query.set(q);
            if let Some(f) = then {
                f();
            }
        });
    };

    let debounce = use_debounce(Duration::from_millis(SEARCH_DEBOUNCE_MS), {
        let fire = fire_search.clone();
        move || fire(None)
    });
    let schedule = move || debounce.trigger();
    on_cleanup(move || debounce.cancel());

    let on_input = move |ev: leptos::ev::Event| {
        state.search.query.set(event_target_value(&ev));
        schedule();
    };

    let commit = {
        let fire = fire_search.clone();
        move |dir: i32| {
            let q = state.search.query.get_untracked();
            let fresh = last_query.get_untracked() != q
                || state.search.matches.with_untracked(Vec::is_empty);
            if !fresh {
                virtualizer.with_value(|v| search_navigate(state, v, dir));
                return;
            }
            debounce.cancel();
            fire(Some(Box::new(move || {
                let len = state.search.matches.with_untracked(Vec::len);
                if len > 0 {
                    virtualizer.with_value(|v| {
                        activate_match(state, v, if dir < 0 { len - 1 } else { 0 })
                    });
                }
            })));
        }
    };

    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        let key = ev.key();
        if key == "Enter" {
            ev.prevent_default();
            commit(if ev.shift_key() { -1 } else { 1 });
        } else if key == "Escape" {
            ev.stop_propagation();
            dismiss_search(state);
        }
    };

    view! {
        <Show when=move || state.search.visible.get()>
            <div
                node_ref=container_ref
                data-search-chrome="true"
                class=format!(
                    "absolute right-4 top-14 {BAR} w-[min(560px,90vw)] \
                     rounded-xl border border-line bg-surface/90 shadow-xl backdrop-blur-md"
                )
            >
                <div class="flex items-center gap-1.5 p-1.5">
                    <IconButton
                        icon=IconName::Search
                        size=16
                        title="Search (Enter)"
                        class="text-muted hover:text-ink"
                        on_click=move || commit(1)
                    />
                    <input
                        node_ref=input_ref
                        type="text"
                        placeholder="Search in document…"
                        class="h-9 min-w-0 flex-1 rounded-lg border border-line bg-paper px-2.5 text-sm text-ink placeholder:text-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        prop:value=move || state.search.query.get()
                        on:input=on_input
                        on:keydown=on_keydown
                    />
                    <span class="whitespace-nowrap px-1 text-xs text-muted tabular-nums">
                        {move || {
                            let total = state.search.total.get();
                            match (state.search.active.get(), total) {
                                (_, 0) if state.search.query.get().trim().is_empty() => String::new(),
                                (_, 0) => "0/0".to_string(),
                                (Some(i), n) => format!("{}/{}", i + 1, n),
                                (None, n) => format!("{n}"),
                            }
                        }}
                    </span>
                    <IconButton
                        icon=IconName::ChevronUp
                        size=14
                        title="Previous match (Shift+Enter)"
                        on_click=move || commit(-1)
                    />
                    <IconButton
                        icon=IconName::ChevronDown
                        size=14
                        title="Next match (Enter)"
                        on_click=move || commit(1)
                    />
                    <IconButton
                        icon=IconName::Close
                        size=16
                        title="Close (Esc)"
                        on_click=move || dismiss_search(state)
                    />
                    <IconButton
                        title="Toggle results list"
                        on_click=move || set_show_results.update(|v| *v = !*v)
                    >
                        {move || {
                            let name = if show_results.get() {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            };
                            view! { <Icon name=name size=14 /> }
                        }}
                    </IconButton>
                </div>

                <Show when=move || show_results.get()>
                    <div
                        class=format!(
                            "absolute right-0 top-full {POPOVER} mt-1 w-full overflow-hidden \
                             rounded-xl border border-line bg-surface shadow-xl"
                        )
                        on:click=move |_| set_show_results.set(false)
                    >
                        <div class="max-h-72 overflow-y-auto">
                            <ResultList state=state virtualizer=virtualizer />
                        </div>
                    </div>
                </Show>
            </div>
        </Show>
    }
}
