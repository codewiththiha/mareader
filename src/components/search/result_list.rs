//! Search results list: one row per match plus the "No results" empty
//! state. Shared by the floating-search results dropdown (and any future
//! search surface) so the list markup lives in exactly one place.

use std::sync::Arc;

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use crate::effects::reader::search::activate_match;
use crate::state::ReaderState;
use pdf_core::search::SearchMatch;

/// Page + snippet for one list row. Built when `matches` changes, not
/// when the active index ticks.
#[derive(Clone, Debug, PartialEq)]
struct ResultRowView {
    index: usize,
    page: u32,
    ordinal: u32,
    snippet: String,
}

fn snippet(text: &str) -> String {
    if text.chars().count() > 80 {
        let s: String = text.chars().take(80).collect();
        format!("{s}…")
    } else {
        text.to_string()
    }
}

fn display_rows(matches: &[SearchMatch]) -> Arc<Vec<ResultRowView>> {
    Arc::new(
        matches
            .iter()
            .enumerate()
            .map(|(index, m)| ResultRowView {
                index,
                page: m.page,
                ordinal: m.index,
                snippet: snippet(m.text.as_ref()),
            })
            .collect(),
    )
}

fn result_key(row: &ResultRowView) -> String {
    format!("{}-{}-{}", row.page, row.ordinal, row.index)
}

/// One result row: page badge + snippet, with the current match highlighted.
#[component]
fn ResultRow(
    state: ReaderState,
    virtualizer: StoredValue<Virtualizer, LocalStorage>,
    row: ResultRowView,
) -> impl IntoView {
    let index = row.index;
    let page = row.page;
    let snippet_text = row.snippet;
    // Compare by list index: `active` indexes `matches`, so this stays exact
    // even when a page holds several identical snippets.
    let is_active = move || state.search.active.get() == Some(index);
    // Selecting a row scrolls that MATCH into view (not its page top) and moves
    // the current-match marker onto it.
    let on_click = move |_| virtualizer.with_value(|v| activate_match(state, v, index));
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
pub fn ResultList(
    state: ReaderState,
    virtualizer: StoredValue<Virtualizer, LocalStorage>,
) -> impl IntoView {
    // Only rebuild when the match set changes — not when the highlight moves.
    let rows = Memo::new(move |_| state.search.matches.with(|m| display_rows(m)));

    view! {
        {move || {
            let rows = rows.get();
            if rows.is_empty() {
                view! {
                    <div class="p-4 text-sm text-muted">No results</div>
                }
                .into_any()
            } else {
                view! {
                    <For
                        each=move || rows.as_ref().clone()
                        key=|row: &ResultRowView| result_key(row)
                        children=move |row: ResultRowView| {
                            view! { <ResultRow state=state virtualizer=virtualizer row=row /> }
                        }
                    />
                }
                .into_any()
            }
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_core::search::SearchMatch;

    fn m(page: u32, index: u32, text: &str) -> SearchMatch {
        SearchMatch {
            page,
            index,
            text: text.into(),
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
        }
    }

    #[test]
    fn display_rows_project_page_ordinal_and_truncated_snippet() {
        let long = "a".repeat(100);
        let rows = display_rows(&[m(3, 0, "short"), m(3, 1, &long)]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].index, 0);
        assert_eq!(rows[0].page, 3);
        assert_eq!(rows[0].ordinal, 0);
        assert_eq!(rows[0].snippet, "short");
        assert_eq!(rows[1].ordinal, 1);
        assert_eq!(rows[1].snippet.chars().count(), 81); // 80 + ellipsis
        assert!(rows[1].snippet.ends_with('…'));
        assert_ne!(result_key(&rows[0]), result_key(&rows[1]));
    }

    #[test]
    fn display_rows_empty_is_empty() {
        assert!(display_rows(&[]).is_empty());
    }
}
