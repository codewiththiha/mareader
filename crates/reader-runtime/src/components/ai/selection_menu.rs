//! The selection's menu: copy, explain, look up — centered under the text.

use ai_core::gloss::{GlossMark, MAX_GLOSS_CHARS, is_glossable};
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::ai::anchor::{
    FormatAnchorBridge, ReflowAnchorBridge, anchor_resolver, capture_selection_mark, captured_mark,
    no_invalidation, reflow_invalidation, watch_page_anchor,
};
use crate::components::ai::gloss::mark_layer::request_gloss_open;
use crate::components::ai::reflow_anchor::spot_envelope;
use crate::components::dict::DictOpen;
use crate::pane::origin::raised_in;
use app_chrome::icon::{Icon, IconName};
use app_chrome::layers::AI_SELECTION;
use app_ui::components::primitives::floating::anchor_bubble::AnchorBubble;
use app_ui::components::primitives::hooks::use_custom_event::use_typed_event_from;
use app_ui::components::primitives::overlay::toast::{ToastData, ToastTone};
use app_ui::components::primitives::overlay::toast_host::{ToastHost, use_toast_slot};
use app_ui::events::{DICT_OPEN_EVENT, dispatch_typed_event_on};
use ui_geom::floating::Rect;

/// The clipboard write, as the browser's promise settles it.
async fn write_clipboard(text: &str) -> bool {
    let Some(win) = web_sys::window() else {
        return false;
    };
    wasm_bindgen_futures::JsFuture::from(win.navigator().clipboard().write_text(text))
        .await
        .is_ok()
}

/// A floating icon bar under the reader's text selection.
#[component]
pub fn SelectionMenu(state: crate::context::ReaderContext) -> impl IntoView {
    let detail = state.reader.ai_selection.detail;
    let popover_open = state.reader.ai_selection.popover_open;

    // The menu follows the selection through whichever format owns it.
    let spot = Signal::derive(move || state.reader.ai_selection.detail.get().and_then(|d| d.spot));
    let resolve = anchor_resolver(state.reader, spot);
    // A re-cut relocates a selection with nothing scrolling.
    let invalidate = if state.reader.reflowable_now() {
        reflow_invalidation(state.reader)
    } else {
        no_invalidation()
    };
    let watch = watch_page_anchor(
        Signal::derive(move || state.reader.ai_selection.anchor.get()),
        resolve,
        state.reader.viewer.zoom.display.into(),
        state.reader.viewer.scroll_top.into(),
        state.reader.viewer.page.into(),
        invalidate,
    );

    // Once the origin leaves the viewport, the menu is gone for good.
    Effect::new(move |_| {
        if watch.exited.get() && detail.get().is_some() {
            detail.set(None);
            state.reader.ai_selection.anchor.set(None);
        }
    });

    let anchor = Signal::derive(move || watch.screen.get().map(|b| Rect::new(b.x, b.y, b.w, b.h)));

    // The asked-for card owns the space under the selection until it changes.
    let yielded = RwSignal::new(false);
    Effect::new(move |_| {
        detail.get();
        yielded.set(false);
    });
    use_typed_event_from::<DictOpen>(DICT_OPEN_EVENT, move |_open, origin| {
        if raised_in(&state.reader.dom, origin.as_ref()) {
            yielded.set(true);
        }
    });

    // Any selection earns the menu; the AI's own gate answers on the click.
    let visible = Signal::derive(move || {
        detail.get().is_some_and(|s| !s.text.trim().is_empty())
            && !popover_open.get()
            && !yielded.get()
            && !watch.exited.get()
            && watch.screen.get().is_some()
    });

    // One toast slot for the copy's receipt and the AI's rule.
    let toast: RwSignal<Option<ToastData>> = RwSignal::new(None);
    use_toast_slot(
        toast.into(),
        move |id| toast.with_untracked(|t| t.as_ref().is_some_and(|t| t.id == id)),
        move |id| {
            toast.update(|t| {
                if t.as_ref().is_some_and(|t| t.id == id) {
                    *t = None;
                }
            });
        },
    );
    let say = move |message: &str| {
        let seed = app_state::state::Toast::new(message);
        toast.set(Some(ToastData::new(seed.id, seed.message, ToastTone::Info)));
    };

    let copy = move |_| {
        let Some(sel) = detail.get_untracked() else {
            return;
        };
        let text = sel.text.clone();
        spawn_local(async move {
            let note = if write_clipboard(&text).await {
                "Copied."
            } else {
                "The clipboard refused the copy."
            };
            say(note);
        });
    };

    let explain = move |_| {
        let Some(sel) = detail.get_untracked() else {
            return;
        };
        if !is_glossable(&sel.text) {
            say(&format!(
                "AI explains one word up to {MAX_GLOSS_CHARS} characters."
            ));
            return;
        }
        // Prefer the captured anchor; else a live DOM capture.
        let captured = state.reader.ai_selection.anchor.get_untracked();
        let reflow = sel.is_reflow();
        let mark: Option<GlossMark> = captured
            .map(|pa| {
                // One word passes the gate, so trimming gives the token.
                let word = sel.text.trim();
                // The context is an envelope: spot plus sentence.
                let context = match sel.spot {
                    Some(spot) if reflow => spot_envelope(&spot, &sel.context),
                    _ => sel.context.trim().to_string(),
                };
                captured_mark(word, context, pa)
            })
            .or_else(|| {
                if reflow {
                    // No usable anchor: walk the live range here.
                    ReflowAnchorBridge {
                        state: state.reader,
                        spot: None,
                        mode: state.reader.viewer.mode.get_untracked(),
                    }
                    .capture(state.reader.viewer.zoom.visual_scale())
                    .and_then(|pa| {
                        crate::components::ai::reflow_anchor::capture_selection(state.reader)
                            .map(|(spot, _)| (spot, pa))
                    })
                    .map(|(spot, pa)| {
                        captured_mark(sel.text.trim(), spot_envelope(&spot, &sel.context), pa)
                    })
                } else {
                    capture_selection_mark(
                        state.reader.viewer.zoom.visual_scale(),
                        sel.text.clone(),
                        sel.context.clone(),
                    )
                }
            });
        let root = state.reader.dom.root();
        if let (Some(m), Some(root)) = (mark, root) {
            // Self-contained open: the mark rides the request.
            request_gloss_open(&root, &m);
        } else {
            // Don't leave a stale open flag if capture failed.
            popover_open.set(false);
        }
    };

    let menu_ref: NodeRef<html::Div> = NodeRef::new();
    let lookup = move |_| {
        let Some(sel) = detail.get_untracked() else {
            return;
        };
        let Some(anchor) = watch.screen.get_untracked() else {
            return;
        };
        let Some(origin) = menu_ref.get() else {
            return;
        };
        let open = DictOpen {
            word: sel.text.trim().to_string(),
            context: sel.context.clone(),
            anchor,
        };
        dispatch_typed_event_on(&origin, DICT_OPEN_EVENT, &open);
    };

    // A live selection's right-click is this menu's, not the OS's.
    Effect::new(move |_| {
        let handle = window_event_listener_untyped("contextmenu", move |ev: web_sys::Event| {
            if detail.with_untracked(Option::is_some) {
                ev.prevent_default();
            }
        });
        on_cleanup(move || handle.remove());
    });

    view! {
        <Show when=move || visible.get()>
            <AnchorBubble anchor=anchor gap=10.0 class=format!("selection-menu {AI_SELECTION}")>
                <div
                    node_ref=menu_ref
                    role="toolbar"
                    aria-label="Selection actions"
                    class="flex items-center gap-0.5"
                >
                    <IconAction title="Copy selection" icon=IconName::Copy on_click=Callback::new(copy) />
                    <IconAction
                        title="Explain with AI"
                        icon=IconName::Sparkles
                        on_click=Callback::new(explain)
                    />
                    <IconAction
                        title="Look up in the dictionary"
                        icon=IconName::Book
                        on_click=Callback::new(lookup)
                    />
                </div>
            </AnchorBubble>
        </Show>
        <ToastHost toasts=Signal::derive(move || toast.get()) />
    }
}

/// One icon-only button of the selection menu.
#[component]
fn IconAction(title: &'static str, icon: IconName, on_click: Callback<()>) -> impl IntoView {
    view! {
        <button
            type="button"
            title=title
            attr:aria-label=title
            on:click=move |_| on_click.run(())
            // The default would kill the selection behind the menu.
            on:mousedown=move |ev| ev.prevent_default()
            class="flex size-8 items-center justify-center rounded-lg text-muted \
                   hover:bg-line/50 hover:text-ink focus:outline-none \
                   focus-visible:ring-2 focus-visible:ring-accent"
        >
            <Icon name=icon size=15 />
        </button>
    }
}
