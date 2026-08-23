//! Keyboard-shortcut reference row: label on the left, keycaps on the right.

use leptos::prelude::*;

use super::kbd::Kbd;

#[component]
pub fn ShortcutRow(label: &'static str, keys: Vec<&'static str>) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between gap-2 px-1 py-0.5">
            <span class="text-xs text-muted">{label}</span>
            <span class="flex gap-0.5">
                {keys.into_iter().map(|k| view! { <Kbd>{k}</Kbd> }).collect_view()}
            </span>
        </div>
    }
}
