//! The dictionary card: one word's senses, centered under the word.

use std::time::Duration;

use ai_core::gloss::{GlossMark, PageAnchor, ReflowSpot};
use app_chrome::floating::dismiss::{DismissPolicy, DismissTrigger, use_dismiss};
use app_chrome::floating::types::Rect;
use app_chrome::hooks::use_timeout::use_debounce;
use app_chrome::layers::POPOVER;
use app_ui::components::primitives::floating::anchor_bubble::AnchorBubble;
use app_ui::components::primitives::hooks::use_custom_event::use_typed_event_from;
use app_ui::events::{DICT_HOVER_EVENT, DICT_LEAVE_EVENT, DICT_OPEN_EVENT, GLOSS_OPEN_EVENT};
use leptos::prelude::*;

use crate::components::ai::anchor::{
    anchor_resolver, no_invalidation, reflow_invalidation, watch_page_anchor,
};
use crate::components::ai::reflow_anchor::{self, parse_spot};
use crate::context::ReaderContext;
use crate::pane::origin::raised_in;
use crate::services;
use crate::services::dict::EntryMirror;

use super::DictOpen;

/// How long the card lingers after the pointer leaves a word.
const LEAVE_GRACE_MS: u64 = 350;

/// One word asked for: what to look up, and where the word sits.
struct Ask {
    word: String,
    /// The sentence around the word, which the role tagger reads.
    context: String,
    /// Where the word is, in the space a gloss mark keeps.
    anchor: PageAnchor,
    /// The reflow spot the anchor resolves through, when there is one.
    spot: Option<ReflowSpot>,
}

#[component]
pub fn DictHoverHost(state: ReaderContext) -> impl IntoView {
    let visible = RwSignal::new(false);
    let pinned = RwSignal::new(false);
    let word = RwSignal::new(String::new());
    let entries: RwSignal<Vec<EntryMirror>> = RwSignal::new(Vec::new());
    let index = RwSignal::new(0usize);
    // The word the card is on; None leaves the watch nothing to follow.
    let placed: RwSignal<Option<Ask>> = RwSignal::new(None);
    let scroll_top = state.reader.viewer.scroll_top;
    // The menu's own glue: an anchor resolved on every scroll, zoom
    // and re-cut.
    let spot = Signal::derive(move || placed.with(|ask| ask.as_ref().and_then(|ask| ask.spot)));
    let invalidate = if state.reader.reflowable_now() {
        reflow_invalidation(state.reader)
    } else {
        no_invalidation()
    };
    let watch = watch_page_anchor(
        Signal::derive(move || placed.with(|ask| ask.as_ref().map(|ask| ask.anchor))),
        anchor_resolver(state.reader, spot),
        state.reader.viewer.zoom.display.into(),
        scroll_top.into(),
        state.reader.viewer.page.into(),
        invalidate,
    );
    let anchor = Signal::derive(move || watch.screen.get().map(|b| Rect::new(b.x, b.y, b.w, b.h)));
    let packs = services::dict::packs();

    // The card's language: the one the settings name, or the first built.
    let to = Signal::derive(move || {
        let rows = packs.get();
        let want = state.settings.with(|st| st.dict.default_lang.clone());
        services::dict::answer_pack(&rows, want.as_deref())
            .map(|pack| pack.target.clone())
            .unwrap_or_default()
    });

    // The card is gone: no word left to follow, nothing to retire.
    let hide = Callback::new(move |_| {
        placed.set(None);
        pinned.set(false);
        visible.set(false);
    });

    // The pointer left; retire unless something else claims the card.
    let retire = use_debounce(Duration::from_millis(LEAVE_GRACE_MS), move || hide.run(()));
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

    let show = Callback::new(move |(ask, pin): (Ask, bool)| {
        retire.cancel();
        let (asked, context) = (ask.word.clone(), ask.context.clone());
        placed.set(Some(ask));
        word.set(asked.clone());
        entries.set(Vec::new());
        index.set(0);
        pinned.set(pin);
        visible.set(true);
        fetch_for.run((asked, context));
    });

    use_typed_event_from::<GlossMark>(DICT_HOVER_EVENT, move |mark, origin| {
        if !raised_in(&state.reader.dom, origin.as_ref()) {
            return;
        }
        // The sentence, not the envelope a reflowable mark keeps it in.
        let context = reflow_anchor::explain_context(&mark);
        let ask = Ask {
            word: mark.word.clone(),
            context,
            anchor: mark.anchor,
            spot: parse_spot(state.reader.gloss.spots, &mark.context),
        };
        show.run((ask, false));
    });

    use_typed_event_from::<DictOpen>(DICT_OPEN_EVENT, move |open, origin| {
        if !raised_in(&state.reader.dom, origin.as_ref()) {
            return;
        }
        let Some(anchor) = open.anchor else {
            return;
        };
        let ask = Ask {
            word: open.word,
            context: open.context,
            anchor,
            spot: open.spot,
        };
        show.run((ask, true));
    });

    // The AI card owns the word once it opens; this one goes at once.
    use_typed_event_from::<GlossMark>(GLOSS_OPEN_EVENT, move |_mark, origin| {
        if !raised_in(&state.reader.dom, origin.as_ref()) {
            return;
        }
        retire.cancel();
        hide.run(());
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
        hide,
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
        if watch.exited.get() && visible.get_untracked() {
            hide.run(());
        }
    });

    // A zoom re-cuts the boxes the card points at.
    Effect::new(move |_| {
        if state.reader.viewer.zooming().get() && visible.get_untracked() {
            hide.run(());
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
