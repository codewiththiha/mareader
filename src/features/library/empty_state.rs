//! The no-books prompt: the plain, centered design the app had before the
//! library — info text above, an "Open…" button in the middle.

use leptos::prelude::*;

use crate::components::primitives::button::{Button, ButtonVariant};
use crate::services::document;
use crate::state::AppState;

/// The no-books prompt: the plain, centered design the app had before the
/// library — info text above, an "Open…" button in the middle.
#[component]
pub(crate) fn EmptyState(state: AppState) -> impl IntoView {
    let has_tauri = pdf_engine::has_tauri();
    view! {
        <div class="flex h-full w-full items-center justify-center pt-12 text-muted">
            <div class="flex max-w-md flex-col items-center gap-3 text-center">
                <p class="text-lg text-ink">"Open a PDF to start reading"</p>
                {has_tauri.then(|| view! {
                    <p class="text-sm text-muted">"Or drop a PDF anywhere in the window"</p>
                })}
                <Button
                    on_click=move |_| document::open_dialog(state)
                    variant=ButtonVariant::Primary
                    title="Open a PDF file"
                >
                    <span>"Open…"</span>
                </Button>
            </div>
        </div>
    }
}

