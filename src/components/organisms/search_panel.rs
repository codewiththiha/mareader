//! Search results list. OWNED BY branch C (panels/sidebar).
//!
//! `SearchPanel` is orphaned (U5 moved search into the floating overlay, which
//! composes `ResultList` directly) but is kept as the panel form of the UI.
//! The `allow` is module-scoped rather than on the item because leptos 0.8
//! expands a component into a generated `…Props` struct, and an attribute on
//! the fn does not reach that struct's fields.
#![allow(dead_code)]

use leptos::prelude::*;

use crate::components::molecules::search_box::SearchBox;
use crate::core::search::SearchMatch;
use crate::core::state::AppState;
use crate::effects::search_effects::activate_match;

fn snippet(text: &str) -> String {
    if text.chars().count() > 80 {
        let s: String = text.chars().take(80).collect();
        format!("{s}…")
    } else {
        text.to_string()
    }
}

/// Stable `For` key for a match. The list index is unique on its own (matches
/// are a flat, ordered list), and including page + ordinal keeps the key
/// meaningful when the list is rebuilt for the same query.
pub fn result_key(m: &SearchMatch, index: usize) -> String {
    format!("{}-{}-{}", m.page, m.index, index)
}

/// One result row: page badge + snippet, with the current match highlighted.
#[component]
pub fn ResultRow(state: AppState, result: SearchMatch, index: usize) -> impl IntoView {
    let page = result.page;
    let snippet_text = snippet(&result.text);
    // Compare by list index: `active` indexes `matches`, so this stays exact
    // even when a page holds several identical snippets.
    let is_active = move || state.search.active.get() == Some(index);
    // Selecting a row scrolls that MATCH into view (not its page top) and moves
    // the current-match marker onto it.
    let on_click = move |_| activate_match(state, index);
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

/// Scrollable list of matches (or a "No results" empty state). Shared by the
/// sidebar SearchPanel and the floating-search results dropdown so the list
/// markup lives in exactly one place.
#[component]
pub fn ResultList(state: AppState) -> impl IntoView {
    view! {
        {move || {
            if state.search.matches.get().is_empty() {
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
                                .matches
                                .get()
                                .into_iter()
                                .enumerate()
                                .collect::<Vec<_>>()
                        }
                        key=move |(index, m): &(usize, SearchMatch)| result_key(m, *index)
                        children=move |(index, m): (usize, SearchMatch)| {
                            view! { <ResultRow state=state result=m index=index /> }
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
