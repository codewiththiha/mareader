//! Bottom icon-only rail: Thumbs / Outline panel toggles. Active state is a
//! rounded filled chip, exactly like the reference's bookmark button.

use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};
use crate::state::SidebarMode;

/// One rail toggle.
#[component]
fn RailToggle(
    icon: IconName,
    title: &'static str,
    active: Signal<bool>,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    view! {
        <button
            type="button"
            title=title
            aria-label=title
            aria-pressed=move || active.get().to_string()
            on:click=move |_| on_click()
            class="inline-flex h-9 w-14 items-center justify-center rounded-lg transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            // Repo rule: each conditional class carries ONE token. A
            // space-separated value here throws a swallowed SyntaxError and
            // the highlight silently never applies.
            class=("bg-line", move || active.get())
            class=("text-ink", move || active.get())
            class=("text-muted", move || !active.get())
            class=("hover:text-ink", move || !active.get())
        >
            <Icon name=icon size=16 />
        </button>
    }
}

#[component]
pub(crate) fn PanelSwitcher(
    mode: RwSignal<SidebarMode>,
    thumbs_active: Signal<bool>,
    outline_active: Signal<bool>,
    on_reveal: fn(),
) -> impl IntoView {
    view! {
        <div class="flex shrink-0 items-center justify-around gap-1 border-t border-line p-1.5">
            <RailToggle
                icon=IconName::Thumbs
                title="Thumbnails"
                active=thumbs_active
                on_click=move || {
                    // Re-clicking the ACTIVE tab means "take me to where
                    // I am", not "close".
                    if mode.get() == SidebarMode::Thumbs {
                        on_reveal();
                    } else {
                        mode.set(SidebarMode::Thumbs);
                    }
                }
            />
            <RailToggle
                icon=IconName::Outline
                title="Outline"
                active=outline_active
                on_click=move || {
                    if mode.get() == SidebarMode::Outline {
                        on_reveal();
                    } else {
                        mode.set(SidebarMode::Outline);
                    }
                }
            />
        </div>
    }
}
