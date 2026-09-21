//! The Active section: the workspace's current reading sessions, shown only
//! once there is more than one pane to tell apart. It derives entirely from
//! the host's pane registry — one payload, no second ownership map — so a
//! book that is also filed in the library appears here without the tree
//! duplicating it.

use leptos::prelude::*;
use serde_json::json;

use crate::runtime::workspace::{PaneInfo, WorkspaceBridge};

#[component]
pub fn WorkspaceActive(bridge: WorkspaceBridge) -> impl IntoView {
    view! {
        <Show when=move || { bridge.panes.get().len() >= 2 }>
            <section class="workspace-section">
                <h2 class="workspace-section-title">Active</h2>
                <For
                    each=move || bridge.panes.get()
                    key=|pane: &PaneInfo| pane.pane_id.clone()
                    children=move |pane: PaneInfo| {
                        let label = pane.title.clone().unwrap_or_else(|| pane.book_id.clone());
                        let close_label = format!("Close {label}");
                        let data_id = pane.pane_id.clone();
                        let focus_id = pane.pane_id.clone();
                        let close_id = pane.pane_id.clone();
                        let format = pane.format;
                        view! {
                            <div class="workspace-active-row">
                                <button
                                    class="workspace-book"
                                    data-pane-id=data_id
                                    on:click=move |_| crate::runtime::emit(json!({"type":"pane-focus","paneId":focus_id}))
                                >
                                    <span class="format-badge">{format.to_uppercase()}</span>
                                    <span class="truncate">{label}</span>
                                </button>
                                <button
                                    class="workspace-active-close"
                                    aria-label=close_label
                                    on:click=move |_| crate::runtime::emit(json!({"type":"pane-close","paneId":close_id}))
                                >"×"</button>
                            </div>
                        }
                    }
                />
            </section>
        </Show>
    }
}
