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

use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_engine::types::DocStatus;
use crate::core::library::RecentBook;
use crate::core::open_flow;
use crate::core::state::AppState;

#[component]
pub fn LibraryView(state: AppState) -> impl IntoView {
    let status = state.doc.status;
    let error = state.doc.error;

    // The reactive list (most-recent first). Small enough (<= RECENT_CAP) that
    // re-deriving the whole vec on any library write is cheap.
    let books = move || state.library.get();

    let is_idle = move || status.get() == DocStatus::Idle;
    let is_opening = move || status.get() == DocStatus::Opening;
    let is_error = move || status.get() == DocStatus::Error;

    let has_tauri = pdf_engine::bridge::has_tauri();
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
                            on_click=move |_| open_flow::close_document(state)
                            kind=ButtonKind::Ghost
                            label="Cancel".to_string()
                            title="Cancel and return to the library".to_string()
                        />
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
                                        on_click=move |_| open_flow::open_dialog(open_state)
                                        kind=ButtonKind::Primary
                                        icon=IconName::Open
                                        label="Open PDF".to_string()
                                        title="Open a PDF file".to_string()
                                    />
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

/// The no-books prompt: the plain, centered design the app had before the
/// library — info text above, an "Open…" button in the middle.
#[component]
fn EmptyState(state: AppState) -> impl IntoView {
    let has_tauri = pdf_engine::bridge::has_tauri();
    view! {
        <div class="flex h-full w-full items-center justify-center pt-12 text-muted">
            <div class="flex max-w-md flex-col items-center gap-3 text-center">
                <p class="text-lg text-ink">"Open a PDF to start reading"</p>
                {has_tauri.then(|| view! {
                    <p class="text-sm text-muted">"Or drop a PDF anywhere in the window"</p>
                })}
                <Button
                    on_click=move |_| open_flow::open_dialog(state)
                    kind=ButtonKind::Primary
                    label="Open…".to_string()
                    title="Open a PDF file".to_string()
                />
            </div>
        </div>
    }
}

/// One book on the shelf.
#[component]
fn BookCard(state: AppState, book: RecentBook) -> impl IntoView {
    // Owned copies so each closure below captures its own value (the card
    // renders many closures that outlive this function's frame).
    let path = book.path.clone();
    let title = book.title.clone().unwrap_or_else(|| {
        pdf_core::filename::file_stem_from_path(&path).unwrap_or_else(|| path.clone())
    });
    let page = book.page;
    let num = book.num_pages;

    let page_line = if num > 0 {
        format!("Page {page} of {num}")
    } else {
        format!("Page {page}")
    };
    let progress = if num > 0 {
        Some((page.min(num) as f64 / num as f64 * 100.0).clamp(0.0, 100.0))
    } else {
        None
    };

    // Aspect ratio (width / height) for the cover box, so a landscape plate
    // stays landscape on the shelf. Clamped so a pathological page can't break
    // the grid; falls back to 3:4 portrait.
    let cover_path = path.clone();
    let aspect = move || {
        state
            .covers
            .get()
            .get(&cover_path)
            .map(|c| {
                if c.width > 0.0 && c.height > 0.0 {
                    (c.width / c.height).clamp(0.55, 1.8)
                } else {
                    0.75
                }
            })
            .unwrap_or(0.75)
    };

    let click_path = path.clone();
    let open = move |_| open_flow::open_path(state, click_path.clone());

    let key_path = path.clone();
    let key_state = state;
    let on_key = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" {
            open_flow::open_path(key_state, key_path.clone());
        }
    };

    let remove_path = path.clone();
    let remove = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        state.library.update(|books| {
            crate::core::library::remove(books, &remove_path);
        });
        state.covers.update(|covers| {
            covers.remove(&remove_path);
        });
        crate::util::storage::save_library(&state.library.get_untracked());
        crate::util::storage::save_covers(&state.covers.get_untracked());
    };

    let alt_path = path.clone();
    let alt_title = title.clone();
    let cover_title = title.clone();
    let meta_title = title.clone();
    let card_title = title.clone();
    let progress_str = progress.map(|p| format!("{p:.1}%"));

    view! {
        <div
            class="book group"
            role="button"
            tabindex="0"
            on:click=open
            on:keydown=on_key
        >
            <div class="book-cover" style:aspect-ratio=move || format!("{:.5} / {:.5}", aspect(), 1.0)>
                // Fore-edge: stacked page sheets peeking past the right side.
                <div class="book-pages"></div>
                {move || match state.covers.get().get(&alt_path).cloned() {
                    Some(c) => view! {
                        <img class="book-cover-img" src=c.data_url alt=alt_title.clone() loading="lazy" />
                    }
                        .into_any(),
                    None => view! {
                        <div class="book-cover-fallback">
                            <span>{cover_title.clone()}</span>
                        </div>
                    }
                        .into_any(),
                }}
            </div>

            <div class="book-meta">
                <span class="book-title" title=meta_title.clone()>{card_title.clone()}</span>
                <span class="book-page">{page_line.clone()}</span>
                {progress_str.map(|p| view! {
                    <span class="book-progress">
                        <span class="book-progress-fill" style:width=p></span>
                    </span>
                })}
            </div>

            <button
                class="book-remove"
                type="button"
                title="Remove from library"
                aria-label="Remove from library"
                on:click=remove
            >
                <Icon name=IconName::Close size=12 />
            </button>
        </div>
    }
}
