//! The Dictionary panel: search over the built packs, or between
//! two shores.

use std::time::Duration;

use app_chrome::hooks::use_timeout::use_debounce;
use app_ui::components::primitives::menu::select_menu::SelectMenu;
use leptos::html;
use leptos::prelude::*;

use crate::services::dict;

mod ask;
mod rows;

use ask::{Ask, SEARCH_DEBOUNCE_MS};
use rows::AnswerRow;

#[component]
pub(crate) fn SidebarDictionary(
    state: crate::context::ReaderContext,
    #[prop(into)] shown: Signal<bool>,
    #[prop(into)] outro: Signal<bool>,
    #[prop(into)] intro: Signal<bool>,
) -> impl IntoView {
    let s = state.settings;
    let packs = dict::packs();
    // The window's ask, not a pane's: a tab away leaves it standing.
    let held = Ask::window();
    let (query, results, asked) = (held.query, held.results, held.asked);
    let input_ref: NodeRef<html::Input> = NodeRef::new();

    let pair = ask::pair(s);
    let seat = ask::seat(s, packs);
    // The shores a pair may name: every built pack's two.
    let langs = Signal::derive(move || dict::languages(&packs.get()));
    let settled = ask::settled(query, pair, packs);
    let otherwise = ask::otherwise(query, pair, packs, langs);
    // One dropdown per choice the ask makes, and the rows they offer.
    let from_options = ask::from_options(langs);
    let to_options = ask::to_options(langs, settled);
    let pack_options = ask::pack_options(packs);
    let from_current = ask::from_current(settled, s);
    let to_current = ask::to_current(settled, s);

    let fire = held.firer(packs, pair, seat);
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

    // A seat or a shore changed: the packs are asked again.
    Effect::new(move |_| {
        if !shown.get() {
            return;
        }
        seat.get();
        pair.get();
        debounce.trigger();
    });

    let pick_from = Callback::new(move |lang: Option<String>| {
        s.update(move |st| st.dict.from = lang);
        debounce.trigger();
    });
    let pick_to = Callback::new(move |lang: Option<String>| {
        s.update(move |st| st.dict.to = lang);
        debounce.trigger();
    });
    let pick_pack = Callback::new(move |id: Option<String>| {
        s.update(move |st| st.dict.langs = id.into_iter().collect());
        debounce.trigger();
    });
    let toggle_pair = move |_| {
        s.update(|st| st.dict.pair = !st.dict.pair);
        debounce.trigger();
    };

    view! {
        <div
            class="sidebar-panel absolute inset-0 flex flex-col"
            class=("is-away", move || !shown.get())
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
                    <div class="flex flex-wrap items-center gap-1 pt-2">
                        {move || {
                            if pair.get().is_some() {
                                view! {
                                    <SelectMenu
                                        options=from_options
                                        current=from_current
                                        on_pick=pick_from
                                        alive=shown
                                    />
                                    <span class="text-xs text-muted" aria-hidden="true">"→"</span>
                                    <SelectMenu
                                        options=to_options
                                        current=to_current
                                        on_pick=pick_to
                                        alive=shown
                                    />
                                }
                                    .into_any()
                            } else {
                                view! {
                                    <SelectMenu
                                        options=pack_options
                                        current=seat
                                        on_pick=pick_pack
                                        alive=shown
                                    />
                                }
                                    .into_any()
                            }
                        }}
                        <button
                            type="button"
                            prop:disabled=move || langs.get().len() < 2
                            class=move || {
                                if pair.get().is_some() {
                                    "ml-auto rounded-full border border-accent bg-accent-soft px-2 py-0.5 text-xs text-accent disabled:opacity-45"
                                } else {
                                    "ml-auto rounded-full border border-line px-2 py-0.5 text-xs text-muted hover:text-ink disabled:opacity-45"
                                }
                            }
                            attr:aria-pressed=move || pair.get().is_some().to_string()
                            title="Name the language you type in and the one you want"
                            on:click=toggle_pair
                        >
                            "Pair"
                        </button>
                    </div>
                </div>
                <div class="min-h-0 flex-1 overflow-y-auto px-3 pb-3">
                    {move || {
                        let rows = results.get();
                        if !asked.get() {
                            return view! { <span class="hidden"></span> }.into_any();
                        }
                        if rows.is_empty() {
                            return view! {
                                <div class="px-1 pt-3">
                                    <p class="text-xs text-muted">"Nothing found."</p>
                                    {move || {
                                        otherwise
                                            .get_untracked()
                                            .map(|lang| {
                                                let label = format!(
                                                    "Search as {}",
                                                    dict_core::lang_label(&lang),
                                                );
                                                view! {
                                                    <button
                                                        type="button"
                                                        class="mt-1 rounded-full border border-line px-2 py-0.5 text-xs text-muted hover:text-ink"
                                                        on:click=move |_| {
                                                            let lang = lang.clone();
                                                            s.update(move |st| st.dict.from = Some(lang));
                                                            debounce.trigger();
                                                        }
                                                    >
                                                        {label}
                                                    </button>
                                                }
                                            })
                                    }}
                                </div>
                            }
                                .into_any();
                        }
                        let wanted = settled.get_untracked().map(|got| got.to);
                        let rows_packs = packs.get_untracked();
                        view! {
                            <div class="flex flex-col gap-2 pt-1">
                                {rows
                                    .into_iter()
                                    .map(|entry| {
                                        view! {
                                            <AnswerRow
                                                entry=entry
                                                packs=rows_packs.clone()
                                                wanted=wanted.clone()
                                            />
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
