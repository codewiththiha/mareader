//! Full-screen feedback shown while a file is being dragged over the window.
//!
//! Purely visual (`pointer-events: none`), so the window-level drop handlers
//! keep receiving the drag. Themed entirely through the design tokens
//! (`--color-accent`, `--color-paper`, `--color-surface`, `--color-ink`,
//! `--color-muted`), so it follows the active base mode and tint with no extra
//! wiring.

use leptos::prelude::*;
use pdf_viewer::components::atoms::icon::{Icon, IconName};

#[component]
pub fn DragOverlay() -> impl IntoView {
    view! {
        <div class="drag-overlay" role="presentation" aria-hidden="true">
            <div class="drag-dropzone">
                <div class="drag-dropzone-icon">
                    <Icon name=IconName::Drop size=40 />
                </div>
                <p class="drag-dropzone-title">"Drop to open"</p>
                <p class="drag-dropzone-sub">"Release your PDF to start reading"</p>
            </div>
        </div>
    }
}
