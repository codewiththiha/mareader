//! Keyboard keycap atom, used to document shortcuts in menus/tooltips.
//! Consumed by the MoreMenu keyboard-shortcuts panel (U7).

use leptos::prelude::*;

#[component]
pub fn Kbd(children: Children) -> impl IntoView {
    view! {
        <kbd class="inline-flex h-5 min-w-5 items-center justify-center rounded border border-line bg-surface px-1.5 text-[11px] font-medium text-muted">
            {children()}
        </kbd>
    }
}
