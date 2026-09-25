//! One book on the shelf: a cover in a frame, a title, a drag handle, and a
//! way back when the address the book points at dies.
//!
//! The cover sits in a frame (`.book-cover-wrap` in
//! `styles/components/library/grid.css`) rather than carrying its own shadow,
//! spine gradient and fore-edge.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;

use crate::features::library::cover_thumb::CoverThumb;
use crate::features::library::entry::{EntryDescriptor, EntryShell};
use crate::features::library::facts::book_facts;
use crate::features::library::gestures::book_policy;
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::selection::SelectionCheck;
use crate::features::library::shelf_item::SeamVocab;
use crate::services::ask_relink;
use runtime_contract::covers::DEFAULT_PAGE_ASPECT;

#[component]
pub(crate) fn BookCard(
    state: crate::context::LibraryContext,
    book: Book,
    crop: Signal<bool>,
) -> impl IntoView {
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");

    let id = book.id.clone();
    let facts = book_facts(state, &id);

    // Clamped so a pathological aspect cannot break the grid; falls back to
    // 3:4 portrait.
    let aspect = move || {
        let Some(f) = facts.get() else {
            return DEFAULT_PAGE_ASPECT;
        };
        state.library.covers.with(|covers| {
            covers
                .get(&f.path)
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

    let missing_class =
        Signal::derive(move || facts.with(|f| f.as_ref().is_some_and(|x| x.missing)));
    let book_title = Signal::derive(move || {
        facts.with(|f| f.as_ref().map(|x| x.title.clone()).unwrap_or_default())
    });
    let check_id = id.clone();

    // Opening names the row, not its address: the library can hold two rows
    // of one file, and the address cannot say which was clicked.
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::GridCard,
        base_class: "book-card",
        policy: book_policy(state, &id, facts, None),
    };

    let remove_id = id.clone();
    let remove = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        remove_sheet.ask(&remove_id);
    };
    let relink_id = id;

    view! {
        <EntryShell
            state=state
            entry=entry
            extra_classes=vec![("book-missing".to_string(), missing_class)]
        >
            <div class="book-cover-wrap">
                <div
                    class="book-cover"
                    class=("book-cover-crop", move || crop.get())
                    style:aspect-ratio=move || {
                        if crop.get() {
                            "210 / 297".to_string()
                        } else {
                            format!("{:.5} / 1", aspect())
                        }
                    }
                >
                    <SelectionCheck state=state id=check_id />
                    <CoverThumb
                        state=state
                        path=Signal::derive(move || {
                            facts.with(|f| f.as_ref().map(|x| x.path.clone()).unwrap_or_default())
                        })
                        alt=book_title
                        img_class="book-cover-img"
                        fallback=Callback::new(move |_| {
                            let Some(f) = facts.get() else {
                                return ().into_any();
                            };
                            let title = f.title.clone();
                            view! {
                                <div class="book-cover-fallback">
                                    <span>{title}</span>
                                </div>
                            }
                                .into_any()
                        })
                    />
                    {move || {
                        missing_class
                            .get()
                            .then(|| {
                                view! {
                                    <span class="book-missing-badge" title="This file is not where the library left it">
                                        <Icon name=IconName::Close size=11 />
                                    </span>
                                }
                            })
                    }}
                </div>
                {move || {
                    let p = facts.get().and_then(|f| f.progress)?;
                    let width = format!("{:.0}%", p * 100.0);
                    let now = format!("{:.0}", (p * 100.0).round());
                    Some(view! {
                        <div
                            class="book-progress-track"
                            role="progressbar"
                            aria-label="Reading progress"
                            aria-valuemin="0"
                            aria-valuemax="100"
                            aria-valuenow=now
                        >
                            <div class="book-progress-fill" style:width=width></div>
                        </div>
                    })
                }}
            </div>

            <div class="book-info">
                <span
                    class="book-title"
                    title=move || book_title.get()
                >
                    {move || book_title.get()}
                </span>
                {move || {
                    let Some(f) = facts.get() else {
                        return ().into_any();
                    };
                    match f.author {
                        Some(author) => {
                            let author_title = author.clone();
                            view! { <span class="book-author" title=author_title>{author}</span> }
                                .into_any()
                        }
                        None => {
                            let hint = f.path.clone();
                            view! {
                                <span class="book-page" title=hint>{f.page_line.clone()}</span>
                            }
                                .into_any()
                        }
                    }
                }}
            </div>

            {move || {
                missing_class.get().then(|| {
                    let at = relink_id.clone();
                    view! {
                        <button
                            class="icon-ghost book-relink"
                            type="button"
                            title="Find this book again"
                            aria-label="Find this book again"
                            on:click=move |ev: leptos::ev::MouseEvent| {
                                ev.stop_propagation();
                                // One function owns which door an open
                                // takes (picker from the reader, sheet
                                // from the library), so the card and the
                                // menu cannot differ.
                                ask_relink(state, at.clone());
                            }
                        >
                            <Icon name=IconName::Open size=11 />
                        </button>
                    }
                })
            }}
            <button
                class="icon-ghost book-remove"
                type="button"
                title="Remove from library"
                aria-label="Remove from library"
                on:click=remove
            >
                <Icon name=IconName::Close size=12 />
            </button>
        </EntryShell>
    }
}
