//! The dictionary card: one word's senses, centered under the word.

use std::time::Duration;

use ai_core::gloss::{GlossBox, GlossMark};
use app_chrome::floating::dismiss::{DismissPolicy, DismissTrigger, use_dismiss};
use app_chrome::floating::types::Rect;
use app_chrome::hooks::use_timeout::use_debounce;
use app_chrome::hooks::use_viewport::viewport_size;
use app_chrome::layers::POPOVER;
use app_ui::components::primitives::floating::anchor_bubble::AnchorBubble;
use app_ui::components::primitives::hooks::use_custom_event::use_typed_event_from;
use app_ui::events::{DICT_HOVER_EVENT, DICT_LEAVE_EVENT, DICT_OPEN_EVENT};
use leptos::prelude::*;

use crate::context::ReaderContext;
use crate::pane::origin::raised_in;
use crate::services;
use crate::services::dict::EntryMirror;

use super::DictOpen;

/// How long the card lingers after the pointer leaves a word.
const LEAVE_GRACE_MS: u64 = 350;

#[component]
pub fn DictHoverHost(state: ReaderContext) -> impl IntoView {
    let visible = RwSignal::new(false);
    let pinned = RwSignal::new(false);
    let word = RwSignal::new(String::new());
    let entries: RwSignal<Vec<EntryMirror>> = RwSignal::new(Vec::new());
    let index = RwSignal::new(0usize);
    // The word's box in page space; scroll carries the card with it.
    let page_box = RwSignal::new(None::<GlossBox>);
    let scroll_top = state.reader.viewer.scroll_top;
    let anchor = Signal::derive(move || {
        page_box
            .get()
            .map(|b| Rect::new(b.x, b.y - scroll_top.get(), b.w, b.h))
    });
    let packs = services::dict::packs();

    // The card's language: the one the settings name, or the first built.
    let to = Signal::derive(move || {
        let rows = packs.get();
        let want = state.settings.with(|st| st.dict.default_lang.clone());
        services::dict::answer_pack(&rows, want.as_deref())
            .map(|pack| pack.target.clone())
            .unwrap_or_default()
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

    // Both asks hand over a box on screen; the card tracks the page.
    let show = Callback::new(
        move |(ask, ctx, screen, pin): (String, String, GlossBox, bool)| {
            retire.cancel();
            let top = scroll_top.get_untracked();
            page_box.set(Some(GlossBox {
                y: screen.y + top,
                ..screen
            }));
            word.set(ask.clone());
            entries.set(Vec::new());
            index.set(0);
            pinned.set(pin);
            visible.set(true);
            fetch_for.run((ask, ctx));
        },
    );

    use_typed_event_from::<GlossMark>(DICT_HOVER_EVENT, move |mark, origin| {
        if !raised_in(&state.reader.dom, origin.as_ref()) {
            return;
        }
        let Some(el) = origin.as_ref() else {
            return;
        };
        let r = el.get_bounding_client_rect();
        let box_ = GlossBox {
            x: r.left(),
            y: r.top(),
            w: r.width(),
            h: r.height(),
            r: 6.0,
        };
        show.run((mark.word, mark.context, box_, false));
    });

    use_typed_event_from::<DictOpen>(DICT_OPEN_EVENT, move |open, origin| {
        if !raised_in(&state.reader.dom, origin.as_ref()) {
            return;
        }
        show.run((open.word, open.context, open.anchor, true));
    });

    use_typed_event_from::<GlossMark>(DICT_LEAVE_EVENT, move |_mark, origin| {
        if !raised_in(&state.reader.dom, origin.as_ref()) {
            return;
        }
        // An asked-for card waits for a dismiss, not for the pointer.
        if pinned.get_untracked() {
            return;
        }
        retire.trigger();
    });

    use_dismiss(
        pinned.into(),
        Callback::new(move |_| {
            pinned.set(false);
            visible.set(false);
        }),
        DismissPolicy {
            escape: true,
            outside: Some(DismissTrigger::PointerDown),
            exclude_selectors: vec![".dict-card"],
            enabled: None,
            topmost_only: false,
        },
        |_| false,
    );

    // A word scrolled out of view takes its card with it.
    Effect::new(move |_| {
        if !visible.get() {
            return;
        }
        let Some(r) = anchor.get() else {
            return;
        };
        let (_, vh) = viewport_size();
        if r.bottom() < 0.0 || r.y > vh {
            pinned.set(false);
            visible.set(false);
        }
    });

    // A zoom re-cuts the boxes the card points at.
    Effect::new(move |_| {
        if state.reader.viewer.zooming().get() && visible.get_untracked() {
            pinned.set(false);
            visible.set(false);
        }
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
            <AnchorBubble
                anchor=anchor
                gap=10.0
                class=format!(
                    "dict-card {POPOVER} w-80 max-w-[calc(100vw-16px)] rounded-xl \
                     border border-line bg-surface shadow-xl"
                )
            >
                <div
                    on:mouseenter=move |_| retire.cancel()
                    on:mouseleave=move |_| {
                        if !pinned.get_untracked() {
                            retire.trigger();
                        }
                    }
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
                        let nearest = nearest_word(&entry);
                        view! {
                            <div class="px-3 py-2.5">
                                <div class="flex items-baseline gap-2">
                                    <span class="text-xs text-muted">{tags}</span>
                                    {nearest
                                        .map(|taken| {
                                            view! {
                                                <span class="text-xs text-muted">
                                                    {"nearest: "}{taken}
                                                </span>
                                            }
                                        })}
                                    {entry
                                        .via
                                        .clone()
                                        .map(|via| {
                                            view! {
                                                <span class="ml-auto text-xs text-muted">
                                                    {"via "}{via}
                                                </span>
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
            </AnchorBubble>
        </Show>
    }
}

/// The headword a near miss answered with; `None` when it is the ask.
fn nearest_word(entry: &EntryMirror) -> Option<String> {
    (entry.word_match != "exact").then(|| entry.word.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(word: &str, word_match: &str) -> EntryMirror {
        EntryMirror {
            word: word.to_string(),
            word_match: word_match.to_string(),
            ..EntryMirror::default()
        }
    }

    #[test]
    fn an_exact_answer_names_no_word_it_took() {
        let nearest = nearest_word(&entry("run", "exact"));
        assert!(nearest.is_none());
    }

    #[test]
    fn a_near_answer_names_the_word_it_took() {
        let nearest = nearest_word(&entry("runner", "prefix"));
        assert_eq!(nearest.as_deref(), Some("runner"));
        let nearest = nearest_word(&entry("ran", "fuzzy"));
        assert_eq!(nearest.as_deref(), Some("ran"));
    }
}
