//! Left sidebar with tabs (outline / thumbnails). OWNED BY branch C
//! (panels/sidebar). Search moved out of the sidebar into the floating overlay
//! (Phase 2), so this rail only carries Outline + Thumbs.
//!
//! The `<aside>` is ALWAYS mounted and animates its `width` between 18rem and 0
//! (single-phase slide, no two-phase unmount). The inner content stays fixed at
//! `w-72` so it doesn't collapse mid-transition — `overflow-hidden` on the aside
//! clips it while closed. The PANEL below the rail is what mounts/unmounts on
//! mode change, so `ThumbnailsPanel`'s `on_cleanup` (canvas unregistration)
//! still fires when the sidebar collapses. When collapsed the content is made
//! `inert` so the clipped rail can't be tab-focused / activated.
//!
//! Gotcha: each reactive `class=("name", cond)` toggle becomes one
//! `classList.add("name")` call — a space-separated token throws a swallowed
//! SyntaxError and the class is silently never applied. Keep every conditional
//! class to a SINGLE token (hence `w-0` and `border-r-0` as separate toggles).

use leptos::prelude::*;

use crate::components::atoms::icon::IconName;
use crate::components::molecules::sidebar_item::SidebarItem;
use crate::components::organisms::outline_panel::OutlinePanel;
use crate::components::organisms::thumbnails_panel::ThumbnailsPanel;
use crate::core::state::{AppState, SidebarMode};

#[component]
pub fn Sidebar(state: AppState) -> impl IntoView {
    // Tab rail: re-runs when the active mode changes so the `active` highlight
    // stays in sync.
    let header = move || {
        let mode = state.sidebar.get();
        view! {
            <div class="flex flex-col gap-0.5 border-b border-line p-2">
                <SidebarItem
                    icon=IconName::Outline
                    label="Outline".to_string()
                    active=mode == SidebarMode::Outline
                    on_click=move || {
                        state.sidebar.set(if state.sidebar.get() == SidebarMode::Outline {
                            SidebarMode::None
                        } else {
                            SidebarMode::Outline
                        });
                    }
                />
                <SidebarItem
                    icon=IconName::Thumbs
                    label="Thumbs".to_string()
                    active=mode == SidebarMode::Thumbs
                    on_click=move || {
                        state.sidebar.set(if state.sidebar.get() == SidebarMode::Thumbs {
                            SidebarMode::None
                        } else {
                            SidebarMode::Thumbs
                        });
                    }
                />
            </div>
        }
    };

    // Active panel below the tab rail. Kept inside the `state.sidebar.get()`
    // match so the panel (de)mounts with the mode — ThumbnailsPanel's
    // `on_cleanup` unregisters its canvases when the sidebar leaves Thumbs.
    let panel = move || match state.sidebar.get() {
        SidebarMode::Outline => view! { <OutlinePanel state=state.clone() /> }.into_any(),
        SidebarMode::Thumbs => view! { <ThumbnailsPanel state=state.clone() /> }.into_any(),
        SidebarMode::None => ().into_any(),
    };

    view! {
        <aside
            class="flex shrink-0 flex-col overflow-hidden border-r border-line bg-surface transition-[width] duration-300 ease-in-out"
            class=("w-72", move || state.sidebar.get() != SidebarMode::None)
            class=("w-0", move || state.sidebar.get() == SidebarMode::None)
            class=("border-r-0", move || state.sidebar.get() == SidebarMode::None)
        >
            <div
                class="flex h-full w-72 min-h-0 flex-col"
                prop:inert=move || state.sidebar.get() == SidebarMode::None
            >
                {header}
                <div class="flex min-h-0 flex-1 flex-col">
                    {panel}
                </div>
            </div>
        </aside>
    }
}
