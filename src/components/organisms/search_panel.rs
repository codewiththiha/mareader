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

fn result_key(result: &SearchResult) -> String {
    format!("{}-{}", result.page, result.text)
}

#[component]
pub fn SearchPanel(state: AppState) -> impl IntoView {
    view! {
        <div class="flex min-h-0 flex-1 flex-col overflow-hidden">
            <SearchBox state=state.clone() />
            <div class="min-h-0 flex-1 overflow-y-auto">
                {move || {
                    if state.search.results.get().is_empty() {
                        view! {
                            <div class="p-4 text-sm text-muted">No results</div>
                        }
                        .into_any()
                    } else {
                        view! {
                            <For
                                each=move || state.search.results.get()
                                key=result_key
                                children=move |result: SearchResult| {
                                    let page = result.page;
                                    let text = result.text.clone();
                                    let snippet_text = snippet(&result.text);
                                    let is_active = move || {
                                        let active = state.search.active.get();
                                        let results = state.search.results.get();
                                        active
                                            .and_then(|i| results.get(i))
                                            .map_or(false, |r| r.page == page && r.text == text)
                                    };
                                    view! {
                                        <button
                                            type="button"
                                            on:click=move |_| jump_to_result(state, page)
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
                            />
                        }
                        .into_any()
                    }
                }}
            </div>
        </div>
    }
}
