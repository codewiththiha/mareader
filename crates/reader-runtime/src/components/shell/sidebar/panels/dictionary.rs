//! The Dictionary panel: search over the built packs, or between
//! two shores.

use std::time::Duration;

use app_chrome::hooks::use_timeout::use_debounce;
use app_chrome::icon::{Icon, IconName};
use app_ui::components::primitives::floating::menu_popover::MenuPopover;
use app_ui::components::primitives::menu::menu_item::MenuItem;
use leptos::html;
use leptos::prelude::*;

use crate::services::dict::{self, EntryMirror, Pair};

/// How long a keystroke waits before the packs are asked.
const SEARCH_DEBOUNCE_MS: u64 = 180;

/// One shore's picker.
#[component]
fn ShoreSelect(
    /// The languages to offer; `None` is the word's own.
    #[prop(into)]
    options: Signal<Vec<Option<String>>>,
    /// The shore that stands.
    #[prop(into)]
    current: Signal<Option<String>>,
    on_pick: Callback<Option<String>>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <button
                type="button"
                on:click=move |_| open.set(!open.get())
                class="flex max-w-[122px] items-center gap-1 rounded-md border border-line px-1.5 py-0.5 text-xs text-ink hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
                <span class="truncate">{move || shore_label(&current.get())}</span>
                <Icon name=IconName::ChevronDown size=11 class="shrink-0 text-muted" />
            </button>
            <MenuPopover
                open=open
                anchor=root_ref
                width=180u32
                class="p-1".to_string()
                hold_titlebar=false
            >
                {move || {
                    options
                        .get()
                        .into_iter()
                        .map(|value| {
                            let picked = value.clone();
                            let click = value.clone();
                            let is_current = Signal::derive(move || current.get() == picked);
                            view! {
                                <MenuItem
                                    label=shore_label(&value)
                                    selected=is_current
                                    check=true
                                    on_click=move || {
                                        on_pick.run(click.clone());
                                        open.set(false);
                                    }
                                />
                            }
                        })
                        .collect_view()
                }}
            </MenuPopover>
        </div>
    }
}

/// The name a shore is offered by; `None` is the word's own language.
fn shore_label(value: &Option<String>) -> String {
    match value {
        Some(code) => dict_core::name(code),
        None => "Detect".to_string(),
    }
}

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

    // The pair, when asked for: the card's language stands in
    // for a target.
    let pair = Signal::derive(move || {
        s.with(|st| {
            st.dict.pair.then(|| Pair {
                from: st.dict.from.clone(),
                to: st.dict.to.clone().or_else(|| st.dict.default_lang.clone()),
            })
        })
    });

    // The seats: the settings' own list, else the card's language's pack.
    let selected = Signal::derive(move || {
        let (langs, default_lang) =
            s.with(|st| (st.dict.langs.clone(), st.dict.default_lang.clone()));
        let rows = packs.get();
        if !langs.is_empty() {
            return dict::named_packs(&rows, &langs);
        }
        dict::answer_pack(&rows, default_lang.as_deref())
            .map(|pack| vec![pack.id.clone()])
            .unwrap_or_else(|| dict::built_packs(&rows))
    });

    let fire = move || {
        let q = query.get_untracked().trim().to_string();
        if q.is_empty() {
            results.set(Vec::new());
            asked.set(false);
            return;
        }
        // The word speaks last: an ask is only settled once it is read.
        let settled = pair
            .get_untracked()
            .and_then(|ask| dict::resolve(&packs.get_untracked(), &ask, &q));
        let from = settled.as_ref().map(|ask| ask.from.clone());
        let to = settled.as_ref().map(|ask| ask.to.clone());
        // A pair names the packs it rides; the chips name them without one.
        let filter = if from.is_some() {
            None
        } else {
            Some(selected.get_untracked())
        };
        dict::search(q, filter, from, to, move |entries| {
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

    // A seat or a shore changed: the packs are asked again.
    Effect::new(move |_| {
        selected.get();
        pair.get();
        debounce.trigger();
    });

    // A chip takes or gives a seat; the table never sits
    // empty.
    let toggle_seat = move |id: String| {
        let mut seats = selected.get_untracked();
        if seats.contains(&id) {
            seats.retain(|seat| seat != &id);
            if seats.is_empty() {
                seats = dict::built_packs(&packs.get_untracked());
            }
        } else {
            seats.push(id);
        }
        s.update(move |st| st.dict.langs = seats);
        debounce.trigger();
    };

    // The shores a pair may name: every built pack's two.
    let langs = Signal::derive(move || dict::languages(&packs.get()));
    // What the pair settled on, for the row that reads it out.
    let settled = Signal::derive(move || {
        let ask = pair.get()?;
        dict::resolve(&packs.get(), &ask, &query.get())
    });

    // The word's own language, when it is not the one being asked.
    let otherwise = Signal::derive(move || {
        let ask = pair.get()?;
        let rows = packs.get();
        let got = dict::resolve(&rows, &ask, &query.get())?;
        let looks = dict_core::detect(&query.get(), &langs.get())
            .unwrap_or_else(|| dict_core::HUB.to_string());
        (looks != got.from).then_some(looks)
    });

    // The pair's own note: what the word said, and the way between.
    let note = Signal::derive(move || {
        let ask = pair.get()?;
        let got = settled.get()?;
        let mut parts = Vec::new();
        // Only a shore left to Detect has anything to report.
        if ask.from.is_none()
            && let Some(lang) = got.detected
        {
            parts.push(format!("Detected {}", dict_core::name(&lang)));
        }
        if got.route == dict::Route::Bridged {
            parts.push(format!("via {}", dict_core::name(dict_core::HUB)));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    });

    // One shore's chips, for an ask that names no pair.
    let chips = move || {
        packs
            .get()
            .iter()
            .filter(|pack| pack.built)
            .map(|pack| {
                let id = pack.id.clone();
                let label = pack.label.clone();
                let seat_id = pack.id.clone();
                let active = Signal::derive(move || selected.get().contains(&id));
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
    };

    let pick_from = Callback::new(move |lang: Option<String>| {
        s.update(move |st| st.dict.from = lang);
        debounce.trigger();
    });
    let pick_to = Callback::new(move |lang: Option<String>| {
        s.update(move |st| st.dict.to = lang);
        debounce.trigger();
    });
    let toggle_pair = move |_| {
        s.update(|st| st.dict.pair = !st.dict.pair);
        debounce.trigger();
    };

    // A pair offers every shore but the one the ask stands on.
    let from_options = Signal::derive(move || {
        let mut rows = vec![None];
        rows.extend(langs.get().into_iter().map(Some));
        rows
    });
    let to_options = Signal::derive(move || {
        let taken = settled.get().map(|got| got.from).unwrap_or_default();
        langs
            .get()
            .into_iter()
            .filter(|lang| *lang != taken)
            .map(Some)
            .collect::<Vec<Option<String>>>()
    });
    let from_current = Signal::derive(move || s.with(|st| st.dict.from.clone()));
    let to_current = Signal::derive(move || {
        settled
            .get()
            .map(|got| got.to)
            .or_else(|| s.with(|st| st.dict.to.clone()))
    });

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
                <div class="flex flex-wrap items-center gap-1 pt-2">
                    {move || {
                        if pair.get().is_some() {
                            view! {
                                <ShoreSelect
                                    options=from_options
                                    current=from_current
                                    on_pick=pick_from
                                />
                                <span class="text-xs text-muted" aria-hidden="true">"→"</span>
                                <ShoreSelect options=to_options current=to_current on_pick=pick_to />
                            }
                                .into_any()
                        } else {
                            view! { {chips} }.into_any()
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
                {move || {
                    note.get()
                        .map(|text| {
                            view! { <p class="pt-1 text-xs text-muted">{text}</p> }
                        })
                }}
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
                            <div class="px-1 pt-3">
                                <p class="text-xs text-muted">
                                    "Nothing found. Try fewer letters, or another shore."
                                </p>
                                {move || {
                                    otherwise
                                        .get_untracked()
                                        .map(|lang| {
                                            let label = format!("Search as {}", dict_core::name(&lang));
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
                    let ask = pair.get_untracked();
                    let wanted = settled.get_untracked().map(|got| got.to);
                    let rows_packs = packs.get_untracked();
                    view! {
                        <div class="flex flex-col gap-2 pt-1">
                            {rows
                                .into_iter()
                                .map(|entry| {
                                    // A pair's answer is the side it named.
                                    let pack = rows_packs
                                        .iter()
                                        .find(|pack| pack.id == entry.pack);
                                    let (head, tail) = match (&wanted, pack) {
                                        (Some(to), Some(pack)) if pack.target == *to => {
                                            (entry.definition.clone(), entry.word.clone())
                                        }
                                        (Some(to), Some(pack)) if pack.source == *to => {
                                            (entry.word.clone(), entry.definition.clone())
                                        }
                                        _ => (entry.word.clone(), entry.definition.clone()),
                                    };
                                    let tags = entry.tags.join(" · ");
                                    let fuzzy = entry.word_match == "fuzzy";
                                    view! {
                                        <div class="rounded-lg border border-line px-3 py-2">
                                            <div class="flex items-baseline gap-2">
                                                <span class="min-w-0 truncate text-sm font-medium text-ink">
                                                    {head}
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
                                            <div class="pt-0.5 text-xs text-muted">{tail}</div>
                                            {entry
                                                .romanization
                                                .clone()
                                                .map(|rom| {
                                                    view! { <div class="text-xs text-muted">{rom}</div> }
                                                })}
                                            {entry
                                                .sense
                                                .clone()
                                                .map(|sense| {
                                                    view! {
                                                        <div class="text-xs text-muted italic">{sense}</div>
                                                    }
                                                })}
                                            {ask
                                                .is_none()
                                                .then(|| {
                                                    entry
                                                        .via
                                                        .clone()
                                                        .map(|via| {
                                                            view! {
                                                                <div class="pt-0.5 text-xs text-muted">
                                                                    {"via "}{via}
                                                                </div>
                                                            }
                                                        })
                                                })
                                                .flatten()}
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
