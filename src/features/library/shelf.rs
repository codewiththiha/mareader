//! The library shelf — the app's empty state.
//!
//! Shows every recently opened book (most-recent first) as a closed volume:
//! a page-1 cover with a spine and fore-edge, the title, and a "page X of Y"
//! resume hint. Clicking a book reopens it at the saved page; a hover-only
//! remove button dismisses it from the shelf. When the shelf is empty it
//! degrades to the plain open-a-PDF prompt.
//!
//! Rendered as the reader-view fallback whenever no document is open
//! (`doc.status != Ready`), so it is where the reader lands on launch, after
//! closing a book, and on a failed open.

use leptos::prelude::*;

use crate::components::primitives::button::{Button, ButtonVariant};
use app_chrome::icon::{Icon, IconName};
use pdf_engine::types::DocStatus;
use crate::services::document;
use crate::state::AppState;
use super::book_card::BookCard;
use super::empty_state::EmptyState;



#[component]
pub fn LibraryShelf(state: AppState) -> impl IntoView {
    let status = state.reader.document.status;
    let error = state.reader.document.error;

    // The reactive list (most-recent first). Small enough (<= RECENT_CAP) that
    // re-deriving the whole vec on any library write is cheap.
    let books = move || state.library.books.get();

    let is_idle = move || status.get() == DocStatus::Idle;
    let is_opening = move || status.get() == DocStatus::Opening;
    let is_error = move || status.get() == DocStatus::Error;

    let has_tauri = tauri_bridge::has_tauri();
    let open_state = state;

    view! {
        <div class="flex h-full w-full flex-col">
            // Opening: centered spinner.
            <Show when=is_opening fallback=|| ()>
                <div class="flex h-full w-full items-center justify-center pt-12 text-muted">
                    <div class="flex flex-col items-center gap-4">
                        <div class="flex items-center gap-3">
                            <div class="h-6 w-6 animate-spin rounded-full border-2 border-line border-t-accent"></div>
                            <p class="text-lg">"Opening…"</p>
                        </div>
                        <Button
                            on_click=move |_| document::close_document(state)
                            variant=ButtonVariant::Ghost
                            title="Cancel and return to the library"
                        >
                            <span>"Cancel"</span>
                        </Button>
                    </div>
                </div>
            </Show>
            // Error: centered message.
            <Show when=is_error fallback=|| ()>
                <div class="flex h-full w-full items-center justify-center pt-12 text-center text-muted">
                    <p class="text-lg">
                        {move || error.get().unwrap_or_else(|| "Could not open this PDF".to_string())}
                    </p>
                </div>
            </Show>
            // Idle: the shelf when there are books, else the plain prompt.
            <Show when=is_idle fallback=|| ()>
                <Show
                    when=move || !books().is_empty()
                    fallback=move || view! { <EmptyState state=state /> }
                >
                    <div class="min-h-0 flex-1 overflow-y-auto pt-12">
                        <div class="mx-auto w-full max-w-5xl px-6 py-8">
                            <header class="mb-8 flex items-end justify-between gap-4">
                                <div class="min-w-0">
                                    <h1 class="text-xl font-semibold text-ink">"Your library"</h1>
                                    <p class="mt-1 text-sm text-muted">
                                        {move || {
                                            let n = books().len();
                                            if n == 1 {
                                                "1 book · continue where you left off".to_string()
                                            } else {
                                                format!("{n} books · continue where you left off")
                                            }
                                        }}
                                    </p>
                                </div>
                                <div class="flex shrink-0 items-center gap-2">
                                    <Show when=move || has_tauri fallback=|| ()>
                                        <span class="hidden text-xs text-muted sm:block">
                                            "…or drop a PDF anywhere"
                                        </span>
                                    </Show>
                                    <Button
                                        on_click=move |_| document::open_dialog(open_state)
                                        variant=ButtonVariant::Primary
                                        title="Open a PDF file"
                                    >
                                        <Icon name=IconName::Open size=18 />
                                        <span>"Open PDF"</span>
                                    </Button>
                                </div>
                            </header>
                            <div class="library-grid">
                                <For each=books key=|b| b.path.clone() let:book>
                                    <BookCard state=state book=book />
                                </For>
                            </div>
                        </div>
                    </div>
                </Show>
            </Show>
        </div>
    }
}

