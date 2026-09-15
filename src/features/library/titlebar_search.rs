//! The library's search bar, in the title bar's centre slot.
//!
//! The reader's floating search is an overlay over a document; this is a
//! filter over a shelf, so it borrows the look only: an always-present pill
//! that narrows the grid on the spot.

use std::time::Duration;

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use library_core::query::{self, Suggestion, SUGGEST_LIMIT};
use library_core::text::plural;

use crate::components::primitives::floating::menu_popover::MenuPopover;
use crate::events::FOCUS_LIBRARY_SEARCH_EVENT;
use crate::features::library::search_suggest::SearchSuggestions;
use crate::services::library::reveal_book;
use crate::state::AppState;

/// The quiet gap between a keystroke and the suggestion scan: long enough
/// for a fast typist's letters to land, short enough that the panel still
/// feels like it answers the typing.
const SUGGEST_DEBOUNCE_MS: u64 = 90;

fn placeholder(state: AppState) -> Signal<String> {
    Signal::derive(move || {
        let books = state
            .library
            .books
            .with(|rows| library_core::book::book_rows(rows).count());
        match books {
            0 => "Search the library".to_string(),
            n => format!("Search {}", plural(n, "book", "books")),
        }
    })
}

#[component]
pub(crate) fn TitlebarSearch(state: AppState) -> impl IntoView {
    let hint = placeholder(state);
    let has_query = Signal::derive(move || state.library.query.with(|q| !q.is_empty()));
    let input_ref: NodeRef<html::Input> = NodeRef::new();
    let anchor: NodeRef<html::Div> = NodeRef::new();

    // `open` is the popover's own signal (its dismissal writes it too), so a
    // closed panel renders and computes nothing.
    let open = RwSignal::new(false);
    let active = RwSignal::new(0usize);
    let suggestions: RwSignal<Vec<Suggestion>> = RwSignal::new(Vec::new());
    let panel_width = RwSignal::new(320u32);

    let show = move || {
        let q = state.library.query.get_untracked();
        let rows = if query::is_active(&q) {
            state
                .library
                .books
                .with_untracked(|rows| query::suggest(rows, &q, SUGGEST_LIMIT))
        } else {
            Vec::new()
        };
        let opening = !rows.is_empty() && !open.get_untracked();
        if opening
            && let Some(node) = anchor.get_untracked()
        {
            let wide = node.get_bounding_client_rect().width();
            if wide > 0.0 {
                panel_width.set(wide as u32);
            }
        }
        suggestions.set(rows);
        active.set(0);
        open.set(!suggestions.with_untracked(|s| s.is_empty()));
    };

    // The suggest pass is debounced, the query write is not: the shelf
    // filters per keystroke, while suggestions scan every row in the library.
    // One pending timer, replaced by each keystroke, so only the last lands.
    let pending_show: RwSignal<Option<TimeoutHandle>> = RwSignal::new(None);
    let queue_show = move || {
        if let Some(handle) = pending_show.get_untracked() {
            handle.clear();
        }
        let handle =
            set_timeout_with_handle(show, Duration::from_millis(SUGGEST_DEBOUNCE_MS)).ok();
        pending_show.set(handle);
    };
    on_cleanup(move || {
        if let Some(handle) = pending_show.get_untracked() {
            handle.clear();
        }
    });

    let pick = move |id: String| {
        reveal_book(state, &id);
        open.set(false);
    };

    // The shortcut layer dispatches and forgets: it has no business knowing
    // the library's bar owns an input node.
    let focus_handle =
        window_event_listener(
            leptos::ev::Custom::new(FOCUS_LIBRARY_SEARCH_EVENT),
            move |_: web_sys::CustomEvent| {
                if let Some(node) = input_ref.get_untracked() {
                    _ = node.focus();
                    node.select();
                }
            },
        );
    on_cleanup(move || focus_handle.remove());

    view! {
        <div node_ref=anchor class="relative w-full max-w-xl">
            <div
                class="pointer-events-auto flex w-full items-center gap-2 rounded-full \
                       border border-line bg-surface/70 px-3 py-1.5 backdrop-blur \
                       focus-within:border-accent"
            >
                <Icon name=IconName::Search size=15 class="shrink-0 text-muted" />
                <input
                    node_ref=input_ref
                    type="text"
                    role="combobox"
                    aria-label="Search the library"
                    aria-expanded=move || open.get()
                    aria-controls="lib-suggest-list"
                    aria-autocomplete="list"
                    aria-activedescendant=move || {
                        if open.get() {
                            format!("lib-sug-{}", active.get())
                        } else {
                            String::new()
                        }
                    }
                    autocomplete="off"
                    spellcheck="false"
                    placeholder=move || hint.get()
                    prop:value=move || state.library.query.get()
                    on:input=move |ev| {
                        state.library.query.set(event_target_value(&ev));
                        queue_show();
                    }
                    on:focus=move |_| show()
                    on:blur=move |_| open.set(false)
                    on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                        match ev.key().as_str() {
                            "ArrowDown" => {
                                ev.prevent_default();
                                if open.get_untracked() {
                                    let len = suggestions.with_untracked(|s| s.len());
                                    if len > 0 {
                                        active.update(|a| *a = (*a + 1).min(len - 1));
                                    }
                                } else {
                                    show();
                                }
                            }
                            "ArrowUp" => {
                                ev.prevent_default();
                                if open.get_untracked() {
                                    if active.get_untracked() == 0 {
                                        open.set(false);
                                    } else {
                                        active.update(|a| *a -= 1);
                                    }
                                }
                            }
                            "Enter" => {
                                if open.get_untracked() {
                                    let chosen = suggestions.with_untracked(|s| {
                                        s.get(active.get_untracked().min(s.len().saturating_sub(1)))
                                            .map(|row| row.book.id.clone())
                                    });
                                    if let Some(id) = chosen {
                                        ev.prevent_default();
                                        pick(id);
                                    }
                                }
                            }
                            "Escape" => {
                                if open.get_untracked() {
                                    ev.stop_propagation();
                                    open.set(false);
                                } else if has_query.get_untracked() {
                                    ev.stop_propagation();
                                    state.library.query.set(String::new());
                                    suggestions.set(Vec::new());
                                }
                            }
                            _ => {}
                        }
                    }
                    class="w-full min-w-0 bg-transparent text-sm text-ink placeholder:text-muted \
                           focus:outline-none"
                />
                {move || {
                    has_query.get().then(|| {
                        view! {
                            <button
                                type="button"
                                aria-label="Clear the search"
                                title="Clear the search"
                                on:click=move |_| {
                                    state.library.query.set(String::new());
                                    suggestions.set(Vec::new());
                                    open.set(false);
                                }
                                class="flex h-5 w-5 shrink-0 items-center justify-center rounded-full \
                                       text-muted transition-colors hover:bg-line hover:text-ink \
                                       focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                            >
                                <Icon name=IconName::Close size=12 />
                            </button>
                        }
                    })
                }}
            </div>
            <MenuPopover
                open=open
                anchor=anchor
                width=Signal::derive(move || panel_width.get())
                coordinate_space="toolbar-row"
                class="pointer-events-auto p-1.5"
            >
                <SearchSuggestions
                    state=state
                    suggestions=suggestions.read_only()
                    active=active
                    pick=Callback::new(pick)
                />
            </MenuPopover>
        </div>
    }
}
