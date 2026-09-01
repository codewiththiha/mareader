//! One book on the shelf: cover, spine, title, resume hint, and the
//! hover-only remove button. Feature-local on purpose — it understands
//! `RecentBook`, cover persistence and the open flow.

use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};
use crate::services::document;
use crate::state::library::RecentBook;
use crate::state::reader::DEFAULT_PAGE_ASPECT;
use crate::state::AppState;

/// One book on the shelf.
#[component]
pub(crate) fn BookCard(state: AppState, book: RecentBook) -> impl IntoView {
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
        state.library.covers.with(|covers| {
            covers
                .get(&cover_path)
                .map(|c| {
                    if c.width > 0.0 && c.height > 0.0 {
                        (c.width / c.height).clamp(0.55, 1.8)
                    } else {
                        DEFAULT_PAGE_ASPECT
                    }
                })
                .unwrap_or(DEFAULT_PAGE_ASPECT)
        })
    };

    let click_path = path.clone();
    let open = move |_| document::open_path(state, click_path.clone());

    let key_path = path.clone();
    let key_state = state;
    let on_key = move |ev: leptos::ev::KeyboardEvent| {
        if ev.key() == "Enter" {
            document::open_path(key_state, key_path.clone());
        }
    };

    let remove_path = path.clone();
    let remove = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        state.library.books.update(|books| {
            crate::state::library::remove(books, &remove_path);
        });
        state.library.covers.update(|covers| {
            covers.remove(&remove_path);
        });
        if let Err(e) = state
            .library
            .books
            .with_untracked(|books| crate::storage::save_library(books))
        {
            e.report();
        }
        if let Err(e) = state
            .library
            .covers
            .with_untracked(crate::storage::save_covers)
        {
            e.report();
        }
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
                {move || match state
                    .library
                    .covers
                    .with(|covers| covers.get(&alt_path).cloned())
                {
                    Some(c) => view! {
                        <img class="book-cover-img" src=c.data_url.clone() alt=alt_title.clone() loading="lazy" />
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
