//! Search query input + submit. OWNED BY branch C (panels/sidebar).

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::engine;
use crate::components::atoms::button::{Button, ButtonKind};
use crate::components::atoms::icon::IconName;
use crate::core::state::AppState;
use crate::effects::search_effects::run_search;

#[allow(dead_code)] // orphaned once U5 removes the sidebar Search tab (Phase 2, parallel)
#[component]
pub fn SearchBox(state: AppState) -> impl IntoView {
    let do_search = move || spawn_local(run_search(state));
    let clear = move || {
        state.search.query.set(String::new());
        engine::clear_highlights();
        state.search.total.set(0);
        state.search.results.set(Vec::new());
        state.search.active.set(None);
    };
    view! {
        <div class="flex flex-col gap-2 border-b border-line p-3">
            <div class="flex items-center gap-1">
                <input
                    type="text"
                    placeholder="Search…"
                    class="h-9 min-w-0 flex-1 rounded-lg border border-line bg-paper px-2.5 text-sm text-ink placeholder:text-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    prop:value=move || state.search.query.get()
                    on:input=move |ev| state.search.query.set(event_target_value(&ev))
                    on:keydown=move |ev| {
                        if ev.key() == "Enter" {
                            spawn_local(run_search(state));
                        }
                    }
                />
                <Button on_click=move |_| do_search() kind=ButtonKind::Primary icon=IconName::Search title="Search".to_string() />
                <Button on_click=move |_| clear() kind=ButtonKind::Ghost icon=IconName::Close title="Clear".to_string() />
            </div>
            <div class="flex h-4 items-center text-xs text-muted">
                {move || {
                    let n = state.search.total.get();
                    if n > 0 {
                        format!("{n} result(s)")
                    } else {
                        String::new()
                    }
                }}
            </div>
        </div>
    }
}
