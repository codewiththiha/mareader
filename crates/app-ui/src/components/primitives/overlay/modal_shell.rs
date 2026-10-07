//! The modal sheet's chrome: backdrop, panel, lane registration, Escape.

use leptos::children::ChildrenFn;
use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app_chrome::floating::dismiss::use_modal_escape;

use super::lanes::{OverlayPolicy, use_overlay_lane};

const FOCUSABLE: &str = "a[href],button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[contenteditable=\"true\"],[tabindex]:not([tabindex=\"-1\"])";

fn active_element() -> Option<web_sys::HtmlElement> {
    web_sys::window()?
        .document()?
        .active_element()?
        .dyn_into()
        .ok()
}

fn contains(dialog: &web_sys::HtmlElement, element: &web_sys::HtmlElement) -> bool {
    dialog
        .unchecked_ref::<web_sys::Node>()
        .contains(Some(element.unchecked_ref::<web_sys::Node>()))
}

fn same_node(left: &web_sys::HtmlElement, right: &web_sys::HtmlElement) -> bool {
    left.unchecked_ref::<web_sys::Node>()
        .is_same_node(Some(right.unchecked_ref::<web_sys::Node>()))
}

fn focusable_elements(dialog: &web_sys::HtmlElement) -> Vec<web_sys::HtmlElement> {
    let Ok(nodes) = dialog.query_selector_all(FOCUSABLE) else {
        return Vec::new();
    };
    (0..nodes.length())
        .filter_map(|index| nodes.get(index))
        .filter_map(|node| node.dyn_into::<web_sys::HtmlElement>().ok())
        .collect()
}

fn focus_dialog(dialog: &web_sys::HtmlElement) {
    // Native `autofocus` runs while the children mount.
    if active_element().is_some_and(|active| contains(dialog, &active)) {
        return;
    }
    if let Some(first) = focusable_elements(dialog).first() {
        let _ = first.focus();
    } else {
        let _ = dialog.focus();
    }
}

fn trap_tab(dialog: &web_sys::HtmlElement, event: &web_sys::KeyboardEvent) {
    if event.key() != "Tab" {
        return;
    }
    let focusable = focusable_elements(dialog);
    let Some((first, last)) = focusable.first().zip(focusable.last()) else {
        event.prevent_default();
        let _ = dialog.focus();
        return;
    };
    let active = active_element();
    let outside = active
        .as_ref()
        .is_none_or(|active| !contains(dialog, active));
    let wraps_backward = event.shift_key()
        && (outside
            || active
                .as_ref()
                .is_some_and(|active| same_node(active, first)));
    let wraps_forward = !event.shift_key()
        && (outside
            || active
                .as_ref()
                .is_some_and(|active| same_node(active, last)));
    if wraps_backward || wraps_forward {
        event.prevent_default();
        let target = if wraps_backward { last } else { first };
        let _ = target.focus();
    }
}

#[component]
pub fn ModalShell(
    /// Whether the sheet is up: the lane and Escape rules both read it.
    open: RwSignal<bool>,
    /// What the dialog calls itself to a screen reader.
    aria_label: &'static str,
    /// The panel's width as a CSS value.
    width: &'static str,
    /// The panel's height, for a sheet that sizes itself.
    #[prop(optional)]
    height: Option<&'static str>,
    /// `ChildrenFn`, not `Children`: `Show`'s closure must be an `Fn`.
    children: ChildrenFn,
) -> impl IntoView {
    let dialog_ref = NodeRef::<html::Div>::new();
    let previous_focus = StoredValue::new_local(None::<web_sys::HtmlElement>);
    let was_open = StoredValue::new_local(false);

    // One modal at a time; a menu replaces it rather than stacking.
    use_overlay_lane(open, OverlayPolicy::MODAL);
    // A popover inside the sheet owns the press; one layer peels at a time.
    use_modal_escape(open);

    Effect::new(move |_| {
        let is_open = open.get();
        let was = was_open.get_value();
        if is_open && !was {
            previous_focus.set_value(active_element());
            request_animation_frame(move || {
                // The frame can outlive the owner: a disposed `open` is gone.
                if open.try_get_untracked().is_none() {
                    return;
                }
                if let Some(dialog) = dialog_ref.get() {
                    focus_dialog(dialog.unchecked_ref());
                }
            });
        } else if !is_open && was {
            if let Some(previous) = previous_focus.get_value() {
                let _ = previous.focus();
            }
            previous_focus.set_value(None);
        }
        was_open.set_value(is_open);
    });
    on_cleanup(move || {
        if let Some(previous) = previous_focus.get_value() {
            let _ = previous.focus();
        }
    });

    view! {
        <Show when=move || open.get()>
            <div
                class="fixed inset-0 z-[var(--z-popover)] flex items-center justify-center bg-black/45 p-4"
                on:click=move |_| open.set(false)
            >
                <div
                    node_ref=dialog_ref
                    class="flex max-h-[86vh] w-full flex-col overflow-hidden rounded-2xl border border-line bg-surface shadow-2xl"
                    style=format!(
                        "width:{width}{}",
                        height.map_or(String::new(), |h| format!(";height:{h}"))
                    )
                    // The panel is not the backdrop: a click inside the sheet
                    // is the sheet's.
                    on:click=move |ev| ev.stop_propagation()
                    on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                        if let Some(dialog) = dialog_ref.get() {
                            trap_tab(dialog.unchecked_ref(), &ev);
                        }
                    }
                    role="dialog"
                    aria-modal="true"
                    aria-label=aria_label
                    tabindex="-1"
                >
                    {children()}
                </div>
            </div>
        </Show>
    }
}
