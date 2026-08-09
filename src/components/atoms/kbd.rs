//! Keyboard keycap atom, used to document shortcuts in menus/tooltips.
//!
//! Not yet referenced by the UI (toolbar tooltips are native `title` hints).
//! Reserved for shortcut documentation; the allow stays until a caller exists.

use leptos::prelude::*;

#[allow(dead_code)]
#[component]
pub fn Kbd(children: Children) -> impl IntoView {
    view! {
        <kbd class="inline-flex h-5 min-w-5 items-center justify-center rounded border border-line bg-surface px-1.5 text-[11px] font-medium text-muted">
            {children()}
        </kbd>
    }
}
