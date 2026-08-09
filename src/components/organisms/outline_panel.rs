//! Document outline (TOC) panel. OWNED BY branch C (panels/sidebar).

use leptos::prelude::*;

use crate::core::document::OutlineNode;
use crate::core::state::{AppState, SidebarMode};

fn outline_key(node: &OutlineNode) -> String {
    format!("{}-{}-{}", node.page, node.depth, node.title)
}

#[component]
pub fn OutlinePanel(state: AppState) -> impl IntoView {
    view! {
        <div class="flex min-h-0 flex-1 flex-col overflow-y-auto">
            {move || {
                if state.doc.outline.get().is_empty() {
                    view! {
                        <div class="flex flex-1 items-center justify-center p-4 text-sm text-muted">No outline</div>
                    }
                    .into_any()
                } else {
                    view! {
                        <For
                            each=move || state.doc.outline.get()
                            key=outline_key
                            children=move |node: OutlineNode| {
                                let page = node.page;
                                let depth = node.depth;
                                let title = node.title.clone();
                                view! {
                                    <button
                                        type="button"
                                        class="block w-full truncate border-l-2 border-transparent px-3 py-1 text-left text-sm text-muted hover:bg-line hover:text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                                        style:padding-left=move || format!("{}px", 8 + depth * 14)
                                        on:click=move |_| {
                                            state.viewer.page.set(page);
                                            state.sidebar.set(SidebarMode::None);
                                        }
                                    >
                                        {title}
                                    </button>
                                }
                            }
                        />
                    }
                    .into_any()
                }
            }}
        </div>
    }
}
