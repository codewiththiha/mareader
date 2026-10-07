//! The docked rail's mount point: a flex sibling of `<main>`.

use leptos::children::ChildrenFn;
use leptos::prelude::*;

use app_ui::components::shell::controller::ShellController;

#[component]
pub fn PushRail(shell: ShellController, children: ChildrenFn) -> impl IntoView {
    view! {
        <Show when=move || !shell.is_overlay().get()>
            <div class="contents">{children()}</div>
        </Show>
    }
}
