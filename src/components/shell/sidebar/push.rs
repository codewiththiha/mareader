//! The docked rail's mount point: a flex sibling of `<main>`, so the page
//! gives up the rail's width. Self-gating — the page mounts it unconditionally
//! inside the reader's flex row and it renders nothing while the shell
//! controller says the layout is overlay.
//!
//! `contents` drops this wrapper out of the box tree so the `<aside>` is the
//! flex item itself.

use leptos::children::ChildrenFn;
use leptos::prelude::*;

use crate::components::shell::controller::ShellController;

#[component]
pub fn PushRail(shell: ShellController, children: ChildrenFn) -> impl IntoView {
    view! {
        <Show when=move || !shell.is_overlay().get()>
            <div class="contents">{children()}</div>
        </Show>
    }
}
