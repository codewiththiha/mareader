//! The no-books prompt: the plain, centered design the app had before the
//! library — info text above, an "Open…" button in the middle.

use leptos::prelude::*;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::services::document;
use crate::state::AppState;

/// The no-books prompt: the plain, centered design the app had before the
/// library — info text above, an "Open…" button in the middle.
#[component]
pub(crate) fn EmptyState(state: AppState) -> impl IntoView {
    let has_tauri = tauri_bridge::has_tauri();
    view! {
        <div class="flex h-full w-full items-center justify-center pt-12 text-muted">
            <div class="flex max-w-md flex-col items-center gap-3 text-center">
                <p class="text-lg text-ink">"Open a document to start reading"</p>
                {has_tauri.then(|| view! {
                    // The kinds come out of the format registry rather than out
                    // of a sentence someone has to remember to update.
                    <p class="text-sm text-muted">
                        {move || format!("Or drop a {} file anywhere in the window", reader_core::format::kind_list())}
                    </p>
                })}
                <Button
                    on_click=move |_| document::open_dialog(state)
                    variant=ButtonVariant::Primary
                    title="Open a document file"
                >
                    <span>"Open…"</span>
                </Button>
            </div>
        </div>
    }
}

