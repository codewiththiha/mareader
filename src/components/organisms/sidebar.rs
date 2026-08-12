//! Left sidebar with tabs (outline / thumbnails). OWNED BY branch C
//! (panels/sidebar). Search moved out of the sidebar into the floating overlay
//! (Phase 2), so this rail only carries Outline + Thumbs.
//!
//! The `<aside>` is ALWAYS mounted and animates its `width` between 18rem and 0
//! (single-phase slide, no two-phase unmount). The inner content stays fixed at
//! `w-72` so it doesn't collapse mid-transition — `overflow-hidden` on the aside
//! clips it while closed. BOTH panels below the rail stay mounted for the app
//! lifetime; the inactive one is `invisible` (`visibility:hidden`) — NOT
//! `hidden` (`display:none`), whose height collapse would re-evict the
//! thumbnails virtualization window and re-render every thumb on each open.
//! Visibility keeps layout + ResizeObserver geometry alive, so canvases stay
//! engine-bound across toggles. When collapsed the content is made `inert` so
//! the clipped rail can't be tab-focused / activated.
//!
//! Gotcha: each reactive `class=("name", cond)` toggle becomes one
//! `classList.add("name")` call — a space-separated token throws a swallowed
//! SyntaxError and the class is silently never applied. Keep every conditional
//! class to a SINGLE token (hence `w-0` and `border-r-0` as separate toggles).

use leptos::prelude::*;

use crate::components::atoms::icon::IconName;
use crate::components::molecules::sidebar_item::SidebarItem;
use crate::components::organisms::outline_panel::OutlinePanel;
use crate::components::organisms::thumbnails::ThumbnailsPanel;
use crate::core::state::{AppState, SidebarMode};

/// Ask the visible panel to scroll to wherever the reader currently is.
///
/// A CustomEvent rather than a signal in `AppState`: this is a one-shot
/// gesture with no state to hold, and a counter signal would need to be read
/// by both panels and could not distinguish "asked twice" from "asked once"
/// without extra bookkeeping. Same mechanism the PDF link layer uses.
fn request_reveal_active() {
    if let Some(win) = web_sys::window() {
        let _ = win.dispatch_event(&web_sys::CustomEvent::new("pdfreader:reveal-active").unwrap());
    }
}

#[component]
pub fn Sidebar(state: AppState) -> impl IntoView {
    // Tab rail: re-runs when the active mode changes so the `active` highlight
    // stays in sync.
    let header = move || {
        let mode = state.sidebar.get();
        view! {
            <div class="flex flex-col gap-0.5 border-b border-line p-2">
                <SidebarItem
                    icon=IconName::Thumbs
                    label="Thumbs".to_string()
                    active=mode == SidebarMode::Thumbs
                    on_click=move || {
                        // Re-clicking the ACTIVE tab means "take me to where I
                        // am", not "close". Closing is what the toolbar's
                        // Toggle sidebar button is for, and a reader who has
                        // scrolled the panel away from their position has no
                        // other way to get back to it — they were hunting for
                        // it by hand.
                        if state.sidebar.get() == SidebarMode::Thumbs {
                            request_reveal_active();
                        } else {
                            state.sidebar.set(SidebarMode::Thumbs);
                        }
                    }
                />
                <SidebarItem
                    icon=IconName::Outline
                    label="Outline".to_string()
                    active=mode == SidebarMode::Outline
                    on_click=move || {
                        if state.sidebar.get() == SidebarMode::Outline {
                            request_reveal_active();
                        } else {
                            state.sidebar.set(SidebarMode::Outline);
                        }
                    }
                />
            </div>
        }
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
                // Both panels stay permanently mounted; the inactive one is
                // `invisible` (`visibility:hidden`, which keeps layout +
                // ResizeObserver geometry alive) so the thumbnails virtualization
                // window never evicts — and re-renders — on sidebar toggles.
                <div class="relative min-h-0 flex-1">
                    <div
                        class="absolute inset-0 flex flex-col"
                        class=("invisible", move || state.sidebar.get() != SidebarMode::Outline)
                    >
                        <OutlinePanel state=state.clone() />
                    </div>
                    <div
                        class="absolute inset-0 flex flex-col"
                        class=("invisible", move || state.sidebar.get() != SidebarMode::Thumbs)
                    >
                        <ThumbnailsPanel state=state.clone() />
                    </div>
                </div>
            </div>
        </aside>
    }
}
