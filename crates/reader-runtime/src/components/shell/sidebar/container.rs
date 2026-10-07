//! The rail's container: the `<aside>` and its four slots.

use leptos::children::ViewFn;
use leptos::prelude::*;

use app_state::state::SidebarMode;

/// Ask the visible panel to scroll to the reader's place.
pub(crate) fn request_reveal_active() {
    app_ui::events::dispatch_event(app_ui::events::REVEAL_ACTIVE_EVENT);
}

#[component]
pub fn SidebarShell(
    mode: RwSignal<SidebarMode>,
    /// The floating layout: the wrapper's fade is open/close.
    #[prop(into)]
    overlay: Signal<bool>,
    // `no_slide` freezes the width tween, read tracked.
    #[prop(into)] no_slide: Signal<bool>,
    #[prop(into)] header: ViewFn,
    #[prop(optional, into)] info_row: Option<ViewFn>,
    #[prop(into)] panels: ViewFn,
    #[prop(optional, into)] footer: Option<ViewFn>,
) -> impl IntoView {
    view! {
        <aside
            class="sidebar-aside flex h-full shrink-0 flex-col overflow-hidden border-r border-line bg-surface transition-[width] duration-300 ease-in-out"
            class=("w-72", move || {
                overlay.get() || mode.get() != SidebarMode::None
            })
            class=("w-0", move || !overlay.get() && mode.get() == SidebarMode::None)
            class=("border-r-0", move || !overlay.get() && mode.get() == SidebarMode::None)
            class=("no-slide", move || no_slide.get())
        >
            <div
                class="flex h-full w-72 min-h-0 flex-col"
                prop:inert=move || mode.get() == SidebarMode::None
            >
                {header.run()}
                {info_row.map(|row| row.run())}
                <div class="relative min-h-0 flex-1">
                    {panels.run()}
                </div>
                {footer.map(|row| row.run())}
            </div>
        </aside>
    }
}
