//! Search results panel. OWNED BY branch C (panels/sidebar).

use leptos::prelude::*;

use crate::components::molecules::search_box::SearchBox;
use crate::core::search::SearchResult;
use crate::core::state::AppState;
use crate::effects::search_effects::jump_to_result;

fn snippet(text: &str) -> String {
    if text.chars().count() > 80 {
        let s: String = text.chars().take(80).collect();
        format!("{s}…")
    } else {
        text.to_string()
    }
}

/// Stable `For` key for a search result. `index` disambiguates results that
/// share a page + text (identical snippets on one page), which would otherwise
/// collide as duplicate keys.
pub fn result_key(result: &SearchResult, index: usize) -> String {
    format!("{}-{}-{}", result.page, result.text, index)
}

/// One search-result row: page badge + truncated snippet, with the active
/// result highlighted. Shared by the sidebar SearchPanel and the floating
/// search results dropdown (U4).
#[component]
pub fn ResultRow(state: AppState, result: SearchResult, index: usize) -> impl IntoView {
    let page = result.page;
    let snippet_text = snippet(&result.text);
    // Compare by index, not page+text: `active` is an index into `results`, so
    // index comparison is exact and stays correct when a page holds two
    // identical snippets.
    let is_active = move || state.search.active.get() == Some(index);
    let on_click = move |_| {
        // Jump AND set `active` to this index so the "current" marker follows
        // the viewport (fixes the stale-highlight bug from U2's cross-file
        // review: previously a click only jumped, so the marker stayed put).
        state.search.active.set(Some(index));
        jump_to_result(state, page);
    };
    view! {
        <button
            type="button"
            on:click=on_click
            class=move || {
                let base = "flex w-full flex-col gap-0.5 border-l-2 px-3 py-2 text-left text-sm transition-colors";
                if is_active() {
                    format!("{base} border-accent bg-accent-soft text-ink")
                } else {
                    format!("{base} border-transparent text-muted hover:bg-line hover:text-ink")
                }
            }
        >
            <span class="inline-block w-fit rounded bg-line px-1.5 py-0.5 text-[10px] font-medium text-muted">
                {format!("p. {}", page)}
            </span>
            <span class="block truncate">{snippet_text}</span>
        </button>
    }
}

/// Scrollable list of results (or a "No results" empty state). Shared by the
/// sidebar SearchPanel and the floating-search results dropdown so the list
/// markup lives in exactly one place.
#[component]
pub fn ResultList(state: AppState) -> impl IntoView {
    view! {
        {move || {
            if state.search.results.get().is_empty() {
                view! {
                    <div class="p-4 text-sm text-muted">No results</div>
                }
                .into_any()
            } else {
                view! {
                    <For
                        each=move || {
                            state
                                .search
                                .results
                                .get()
                                .into_iter()
                                .enumerate()
                                .collect::<Vec<_>>()
                        }
                        key=move |(index, result): &(usize, SearchResult)| result_key(result, *index)
                        children=move |(index, result): (usize, SearchResult)| {
                            view! { <ResultRow state=state result=result index=index /> }
                        }
                    />
                }
                .into_any()
            }
        }}
    }
}

#[allow(dead_code)] // orphaned once U5 removes the sidebar Search tab (Phase 2, parallel)
#[component]
pub fn SearchPanel(state: AppState) -> impl IntoView {
    view! {
        <div class="flex min-h-0 flex-1 flex-col overflow-hidden">
            <SearchBox state=state.clone() />
            <div class="min-h-0 flex-1 overflow-y-auto">
                <ResultList state=state.clone() />
            </div>
        </div>
    }
}
