//! The Dictionary panel: quick and fuzzy search over the built packs.

use std::time::Duration;

use app_chrome::hooks::use_timeout::use_debounce;
use leptos::html;
use leptos::prelude::*;

use crate::services::dict::{self, EntryMirror};

/// How long a keystroke waits before the packs are asked.
const SEARCH_DEBOUNCE_MS: u64 = 180;

#[component]
pub(crate) fn SidebarDictionary(
    state: crate::context::ReaderContext,
    #[prop(into)] shown: Signal<bool>,
    #[prop(into)] outro: Signal<bool>,
    #[prop(into)] intro: Signal<bool>,
) -> impl IntoView {
    let s = state.settings;
    let packs = dict::packs();
    let query = RwSignal::new(String::new());
    let results: RwSignal<Vec<EntryMirror>> = RwSignal::new(Vec::new());
    let asked = RwSignal::new(false);
    let input_ref: NodeRef<html::Input> = NodeRef::new();

    // The seats: the settings' selection over what is built.
    // No selection means every built pack.
    let selected = Signal::derive(move || {
        let langs = s.with(|st| st.dict.langs.clone());
        packs
            .get()
            .into_iter()
            .filter(|pack| {
                pack.built
                    && (langs.is_empty()
                        || langs.contains(&pack.id)
                        || langs.contains(&format!("{}-{}", pack.source, pack.target)))
            })
            .map(|pack| pack.id)
            .collect()
    });

    let fire = move || {
        let q = query.get_untracked().trim().to_string();
        if q.is_empty() {
            results.set(Vec::new());
            asked.set(false);
            return;
        }
        let filter = selected.get_untracked();
        // The backend reads an empty filter as every built pack too.
        let filter = Some(filter);
        dict::search(q, filter, move |entries| {
            results.set(entries);
            asked.set(true);
        });
    };

    let debounce = use_debounce(Duration::from_millis(SEARCH_DEBOUNCE_MS), fire);
    on_cleanup(move || debounce.cancel());

    // Reveal takes the caret straight to the ask.
    Effect::new(move |_| {
        if shown.get() {
            queue_microtask(move || {
                if let Some(node) = input_ref.get() {
                    _ = node.focus();
                }
            });
        }
    });

    // A seat taken or given re-asks the packs.
    Effect::new(move |_| {
        selected.get();
        debounce.trigger();
    });

    // A chip takes or gives a seat; the table never sits
    // empty.
    let toggle_seat = move |id: String| {
        let included = selected.with_untracked(|sel| sel.contains(&id));
        s.update(move |st| {
            let mut langs = st.dict.langs.clone();
            if langs.is_empty() {
                langs = packs
                    .get()
                    .iter()
                    .filter(|pack| pack.built)
                    .map(|pack| pack.id.clone())
                    .collect();
            }
            if included {
                langs.retain(|seat| seat != &id);
                if langs.is_empty() {
                    langs = packs
                        .get()
                        .iter()
                        .filter(|pack| pack.built)
                        .map(|pack| pack.id.clone())
                        .collect();
                }
            } else if !langs.contains(&id) {
                langs.push(id);
            }
            st.dict.langs = langs;
        });
        debounce.trigger();
    };

    view! {
        <div
            class="sidebar-panel absolute inset-0 flex flex-col"
            class=("invisible", move || !shown.get())
            class=("is-outro", move || outro.get())
            class=("is-intro", move || intro.get())
        >
        <div class="flex h-full flex-col">
            <div class="shrink-0 px-3 pb-2">
                <input
                    node_ref=input_ref
                    type="search"
                    placeholder="Search any word…"
                    class="h-9 w-full rounded-lg border border-line bg-paper px-2.5 text-sm text-ink placeholder:text-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    attr:aria-label="Search the dictionary"
                    prop:value=move || query.get()
                    on:input=move |ev: leptos::ev::Event| {
                        query.set(event_target_value(&ev));
                        debounce.trigger();
                    }
                />
                <div class="flex flex-wrap gap-1 pt-2">
                    {move || {
                        packs
                            .get()
                            .iter()
                            .filter(|pack| pack.built)
                            .map(|pack| {
                                let id = pack.id.clone();
                                let label = pack.label.clone();
                                let active = Signal::derive(move || {
                                    selected.get().contains(&id)
                                });
                                let seat_id = pack.id.clone();
                                view! {
                                    <button
                                        type="button"
                                        class=move || {
                                            if active.get() {
                                                "rounded-full border border-accent bg-accent-soft px-2 py-0.5 text-xs text-accent"
                                            } else {
                                                "rounded-full border border-line px-2 py-0.5 text-xs text-muted hover:text-ink"
                                            }
                                        }
                                        attr:aria-pressed=move || active.get().to_string()
                                        on:click=move |_| toggle_seat(seat_id.clone())
                                    >
                                        {label}
                                    </button>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </div>
            <div class="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
                {move || {
                    let rows = results.get();
                    if !asked.get() {
                        return view! {
                            <p class="px-1 pt-3 text-xs text-muted">
                                "Type a word in any language. Misspellings still find it."
                            </p>
                        }
                        .into_any();
                    }
                    if rows.is_empty() {
                        return view! {
                            <p class="px-1 pt-3 text-xs text-muted">
                                "Nothing found. Try fewer letters, or another pack."
                            </p>
                        }
                        .into_any();
                    }
                    view! {
                        <div class="flex flex-col gap-2 pt-1">
                            {rows
                                .into_iter()
                                .map(|entry| {
                                    let tags = entry.tags.join(" · ");
                                    let fuzzy = entry.word_match == "fuzzy";
                                    view! {
                                        <div class="rounded-lg border border-line px-3 py-2">
                                            <div class="flex items-baseline gap-2">
                                                <span class="min-w-0 truncate text-sm font-medium text-ink">
                                                    {entry.word.clone()}
                                                </span>
                                                <span class="shrink-0 text-xs text-muted">{tags}</span>
                                                {fuzzy
                                                    .then(|| {
                                                        view! {
                                                            <span class="ml-auto shrink-0 rounded-full bg-line/60 px-1.5 text-xs text-muted">
                                                                "≈"
                                                            </span>
                                                        }
                                                    })}
                                            </div>
                                            <div class="pt-0.5 text-sm text-ink">
                                                {entry.definition.clone()}
                                            </div>
                                            {entry
                                                .romanization
                                                .clone()
                                                .map(|rom| {
                                                    view! {
                                                        <div class="text-xs text-muted">{rom}</div>
                                                    }
                                                })}
                                            {entry
                                                .sense
                                                .clone()
                                                .map(|sense| {
                                                    view! {
                                                        <div class="text-xs text-muted italic">{sense}</div>
                                                    }
                                                })}
                                            {entry
                                                .via
                                                .clone()
                                                .map(|via| {
                                                    view! {
                                                        <div class="pt-0.5 text-xs text-muted">
                                                            {"via "}{via}
                                                        </div>
                                                    }
                                                })}
                                        </div>
                                    }
                                })
                                .collect_view()}
                        </div>
                    }
                    .into_any()
                }}
            </div>
        </div>
        </div>
    }
}
