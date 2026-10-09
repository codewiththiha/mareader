//! The hover card: one red word's senses, beside the pointer.

use std::time::Duration;

use ai_core::gloss::GlossMark;
use app_chrome::hooks::use_timeout::use_debounce;
use app_chrome::layers::POPOVER;
use app_ui::components::primitives::hooks::use_custom_event::use_typed_event_from;
use app_ui::events::{DICT_HOVER_EVENT, DICT_LEAVE_EVENT};
use leptos::prelude::*;

use crate::context::ReaderContext;
use crate::pane::origin::raised_in;
use crate::services;
use crate::services::dict::EntryMirror;

/// How long the card lingers after the pointer leaves a word.
const LEAVE_GRACE_MS: u64 = 350;

#[component]
pub fn DictHoverHost(state: ReaderContext) -> impl IntoView {
    let visible = RwSignal::new(false);
    let word = RwSignal::new(String::new());
    let context = RwSignal::new(String::new());
    let entries: RwSignal<Vec<EntryMirror>> = RwSignal::new(Vec::new());
    let index = RwSignal::new(0usize);
    let x = RwSignal::new(0f64);
    let y = RwSignal::new(0f64);
    // The pack the chooser stands on.
    let to = RwSignal::new(String::new());
    let packs = services::dict::packs();

    // The chooser defaults to the first built pack.
    Effect::new(move |_| {
        let rows = packs.get();
        if to.get_untracked().is_empty()
            && let Some(pack) = rows.iter().find(|pack| pack.built)
        {
            to.set(pack.target.clone());
        }
    });

    // The pointer left; retire unless something else claims the card.
    let retire = use_debounce(Duration::from_millis(LEAVE_GRACE_MS), move || {
        visible.set(false)
    });
    on_cleanup(move || retire.cancel());

    // Senses now, POS-ranked answers when the tagger lands.
    let fetch_for = Callback::new(move |(ask, ctx): (String, String)| {
        let target = to.get_untracked();
        let live = word;
        let asked = ask.clone();
        services::dict::lookup(ask.clone(), "en".to_string(), target.clone(), None, {
            let asked = asked.clone();
            move |found| {
                if live.get_untracked() == asked {
                    entries.set(found);
                    index.set(0);
                }
            }
        });
        services::cefr::fetch_pos(ask.clone(), ctx, {
            let target = target.clone();
            move |answer| {
                let Some(answer) = answer else {
                    return;
                };
                if live.get_untracked() != asked {
                    return;
                }
                services::dict::lookup(ask, "en".to_string(), target, Some(answer.pos), {
                    let asked = asked.clone();
                    move |found| {
                        if live.get_untracked() == asked {
                            entries.set(found);
                            index.set(0);
                        }
                    }
                });
            }
        });
    });

    use_typed_event_from::<GlossMark>(DICT_HOVER_EVENT, move |mark, origin| {
        if !raised_in(&state.reader.dom, origin.as_ref()) {
            return;
        }
        retire.cancel();
        if let Some(el) = origin.as_ref() {
            let rect = el.get_bounding_client_rect();
            x.set(rect.left().max(8.0));
            y.set((rect.bottom() + 8.0).max(8.0));
        }
        word.set(mark.word.clone());
        context.set(mark.context.clone());
        entries.set(Vec::new());
        index.set(0);
        visible.set(true);
        fetch_for.run((mark.word.clone(), mark.context.clone()));
    });

    use_typed_event_from::<GlossMark>(DICT_LEAVE_EVENT, move |_mark, origin| {
        if !raised_in(&state.reader.dom, origin.as_ref()) {
            return;
        }
        retire.trigger();
    });

    // One entry rides at a time; the arrows and a swipe trade it.
    let step = move |by: i32| {
        let len = entries.with_untracked(Vec::len);
        if len == 0 {
            return;
        }
        index.update(|i| {
            let len = len as i32;
            *i = ((*i as i32 + by).rem_euclid(len)) as usize;
        });
    };
    let swipe_x = StoredValue::new_local(0f64);

    view! {
        <Show when=move || visible.get()>
            <div
                class=move || format!("fixed {POPOVER} w-80 max-w-[calc(100vw-16px)] rounded-xl border border-line bg-surface shadow-xl")
                style=move || format!("left:{}px;top:{}px", x.get(), y.get())
                on:mouseenter=move |_| retire.cancel()
                on:mouseleave=move |_| retire.trigger()
                on:pointerdown=move |ev: web_sys::PointerEvent| {
                    swipe_x.set_value(ev.client_x() as f64);
                }
                on:pointerup=move |ev: web_sys::PointerEvent| {
                    let dx = ev.client_x() as f64 - swipe_x.get_value();
                    if dx > 40.0 {
                        step(-1);
                    } else if dx < -40.0 {
                        step(1);
                    }
                }
            >
                <div class="flex items-center gap-2 border-b border-line px-3 py-2">
                    <span class="min-w-0 flex-1 truncate text-sm font-medium text-ink">
                        {move || word.get()}
                    </span>
                    // The language chooser lives in the title area.
                    <select
                        class="shrink-0 rounded-md border border-line bg-paper px-1.5 py-0.5 text-xs text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        attr:aria-label="Translate into"
                        prop:value=move || to.get()
                        on:change=move |ev: leptos::ev::Event| {
                            to.set(event_target_value(&ev));
                            fetch_for.run((word.get_untracked(), context.get_untracked()));
                        }
                    >
                        {move || {
                            packs
                                .get()
                                .iter()
                                .filter(|pack| pack.built)
                                .map(|pack| {
                                    view! {
                                        <option value=pack.target.clone()>{pack.label.clone()}</option>
                                    }
                                })
                                .collect_view()
                        }}
                    </select>
                </div>
                {move || {
                    let rows = entries.get();
                    if rows.is_empty() {
                        return view! {
                            <div class="px-3 py-2.5 text-xs text-muted">
                                "No senses in this pack."
                            </div>
                        }
                        .into_any();
                    }
                    let i = index.get().min(rows.len() - 1);
                    let entry = rows[i].clone();
                    let tags = entry.tags.join(" · ");
                    view! {
                        <div class="px-3 py-2.5">
                            <div class="flex items-baseline gap-2">
                                <span class="text-xs text-muted">{tags}</span>
                                {entry
                                    .via
                                    .clone()
                                    .map(|via| {
                                        view! {
                                            <span class="ml-auto text-xs text-muted">{"via "}{via}</span>
                                        }
                                    })}
                            </div>
                            <div class="pt-1 text-sm text-ink">{entry.definition.clone()}</div>
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
                                    view! { <div class="text-xs text-muted italic">{sense}</div> }
                                })}
                        </div>
                    }
                    .into_any()
                }}
                <div class="flex items-center justify-between border-t border-line px-2 py-1.5">
                    <button
                        type="button"
                        class="rounded-md px-2 py-1 text-xs text-muted hover:bg-line/40 hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        attr:aria-label="Previous sense"
                        on:click=move |_| step(-1)
                    >
                        "‹"
                    </button>
                    <span class="text-xs text-muted tabular-nums">
                        {move || {
                            let len = entries.with(Vec::len);
                            if len == 0 {
                                "0/0".to_string()
                            } else {
                                format!("{}/{}", index.get().min(len - 1) + 1, len)
                            }
                        }}
                    </span>
                    <button
                        type="button"
                        class="rounded-md px-2 py-1 text-xs text-muted hover:bg-line/40 hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        attr:aria-label="Next sense"
                        on:click=move |_| step(1)
                    >
                        "›"
                    </button>
                </div>
            </div>
        </Show>
    }
}
