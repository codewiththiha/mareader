//! Shared text input: one contract for value/input/keydown/aria so the
//! preset editor, the search query and future settings fields stop
//! duplicating the same `prop:value` + `on:input` + focus ring markup.

use leptos::ev::KeyboardEvent;
use leptos::prelude::*;

/// A controlled text input.
#[component]
pub fn TextInput(
    value: RwSignal<String>,
    on_input: Callback<String>,
    #[prop(into, optional)] placeholder: Option<String>,
    #[prop(into, optional)] aria_label: Option<String>,
    #[prop(optional)] list: Option<&'static str>,
    #[prop(optional, into)] class: Option<String>,
    #[prop(optional)] on_keydown: Option<Callback<KeyboardEvent>>,
    #[prop(default = false)] autofocus: bool,
    #[prop(default = false)] disabled: bool,
) -> impl IntoView {
    let class = class.unwrap_or_else(|| {
        "w-full rounded border border-line bg-paper px-2 py-1 text-xs text-ink focus:border-accent focus:outline-none"
            .to_string()
    });

    view! {
        <input
            type="text"
            placeholder=placeholder
            aria-label=aria_label
            list=list
            autofocus=autofocus
            disabled=disabled
            prop:value=move || value.get()
            on:input=move |ev| on_input.run(event_target_value(&ev))
            on:keydown=move |ev| {
                if let Some(cb) = on_keydown {
                    cb.run(ev);
                }
            }
            class=class
        />
    }
}
