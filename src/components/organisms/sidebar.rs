//! Left sidebar with tabs (outline / search / thumbnails). OWNED BY branch C
//! (panels/sidebar).

use leptos::prelude::*;

use crate::components::atoms::icon::IconName;
use crate::components::molecules::sidebar_item::SidebarItem;
use crate::components::organisms::outline_panel::OutlinePanel;
use crate::components::organisms::search_panel::SearchPanel;
use crate::components::organisms::thumbnails_panel::ThumbnailsPanel;
use crate::core::state::{AppState, SidebarMode};

#[component]
pub fn Sidebar(state: AppState) -> impl IntoView {
    // Tab rail: re-runs when the active mode or the search hit-count changes so
    // the `active` highlight and the count badge stay in sync.
    let header = move || {
        let mode = state.sidebar.get();
        let search_total = state.search.total.get();
        view! {
            <div class="flex flex-col gap-0.5 border-b border-line p-2">
                <SidebarItem
                    icon=IconName::Outline
                    label="Outline".to_string()
                    active=(mode == SidebarMode::Outline)
                    on_click=move || {
                        state.sidebar.set(if state.sidebar.get() == SidebarMode::Outline {
                            SidebarMode::None
                        } else {
                            SidebarMode::Outline
                        });
                    }
                />
                <SidebarItem
                    icon=IconName::Search
                    label="Search".to_string()
                    active=(mode == SidebarMode::Search)
                    badge=(if search_total > 0 { search_total.to_string() } else { String::new() })
                    on_click=move || {
                        state.sidebar.set(if state.sidebar.get() == SidebarMode::Search {
                            SidebarMode::None
                        } else {
                            SidebarMode::Search
                        });
                    }
                />
                <SidebarItem
                    icon=IconName::Thumbs
                    label="Thumbs".to_string()
                    active=(mode == SidebarMode::Thumbs)
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

    // Active panel below the tab rail. `ThumbsPanel` is the branch-D stub.
    let panel = move || match state.sidebar.get() {
        SidebarMode::Outline => view! { <OutlinePanel state=state.clone() /> }.into_any(),
        SidebarMode::Search => view! { <SearchPanel state=state.clone() /> }.into_any(),
        SidebarMode::Thumbs => view! { <ThumbnailsPanel state=state.clone() /> }.into_any(),
        SidebarMode::None => ().into_any(),
    };

    view! {
        <Show when=move || state.sidebar.get() != SidebarMode::None>
            {move || view! {
                <aside class="flex w-72 shrink-0 flex-col border-r border-line bg-surface">
                    {header}
                    <div class="flex min-h-0 flex-1 flex-col">
                        {panel}
                    </div>
                </aside>
            }}
        </Show>
    }
}
