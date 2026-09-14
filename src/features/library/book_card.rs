//! One book on the shelf: a cover in a frame, a title, and the two things a
//! library needs that a recents list never did — a drag handle, and a way back
//! when the address the book points at dies.
//!
//! The cover sits in a frame (`.book-cover-wrap` in `styles/components/library/grid.css`) rather
//! than carrying its own shadow, spine gradient and fore-edge. Three reasons, and
//! the first is the one that decided it: the reading-progress bar belongs at the
//! bottom of the ART, and a bar under the title is a bar the eye has to leave the
//! cover to find. A frame that clips gives the bar a box to be flush with, and
//! gives the lift, the ring and the selection outline one element to be painted on
//! instead of four. The second is that the skeuomorphic spine and fore-edge were
//! two pseudo-elements spent on a decoration, and third is that a shelf of
//! mixed page sizes reads calmer as one row of frames than as one row of volumes.
//!
//! The prop is the row the `For` keyed this card on, and a keyed row is not
//! re-created when the row's CONTENT changes — a startup measurement marking the
//! book missing, a relink moving its address, a conflict sheet's rename, a fold
//! merging a twin into it. So the facts that can move are read back out of the
//! state by id (the rule the folder card and the list's tree rows follow), and
//! the prop supplies the identity: which book this card is.
//!
//! The card's outer element is the shelf's one item shell
//! (`crate::features::library::shelf_item`): the press contract, the drop
//! registration and the state classes are its, and what is left here is the
//! card's own content and the two classes that are facts about the BOOK — the
//! reveal's light and the missing grey.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use library_core::book::Book;

use crate::features::library::cover_thumb::CoverThumb;
use crate::features::library::entry::{EntryDescriptor, EntryShell};
use crate::features::library::gestures::book_policy;
use crate::features::library::facts::book_facts;
use crate::features::library::remove_modal::RemoveSheet;
use crate::features::library::selection::SelectionCheck;
use crate::features::library::shelf_item::SeamVocab;
use crate::services::library::relink_dialog;
use crate::state::AppState;
use crate::state::reader::DEFAULT_PAGE_ASPECT;

/// One book on the shelf.
///
/// `crop` is the view's cover treatment, passed in rather than read here so the
/// whole grid switches on one signal instead of every card subscribing to the
/// view separately.
#[component]
pub(crate) fn BookCard(state: AppState, book: Book, crop: Signal<bool>) -> impl IntoView {
    // The card keeps its own ✕: the menu is what a right-click asks and the
    // sheet is what a removal costs, and the second is reached from the first
    // as well as from the button.
    let remove_sheet = use_context::<RemoveSheet>().expect("the library page provides the sheet");


    // The prop supplies the identity; everything that can move while the card
    // is mounted is read back by it. `None` is the beat between a removal and
    // the list catching up — the card paints its blanks and is gone next tick.
    let id = book.id.clone();
    let facts = book_facts(state, &id);

    // Aspect ratio (width / height) for the cover box, so a landscape plate stays
    // landscape on the shelf. Clamped so a pathological page cannot break the
    // grid; falls back to 3:4 portrait.
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

    // The card's one own class beyond the reveal's light, which the shell
    // paints: the grey a book wears whose address stopped resolving.
    let missing_class = Signal::derive(move || {
        facts.with(|f| f.as_ref().is_some_and(|x| x.missing))
    });
    // The id the cover's check mark reads through: the mark asks the library
    // for the membership itself, the way the shell's selected class does.
    let check_id = id.clone();

    // The press contract's three surface answers (see
    // `crate::features::library::gestures`). Opening names the ROW, not its
    // address: the library can hold two rows of one file, and the address
    // cannot say which of them the reader clicked. The menu's missing flag is
    // read when the menu is ASKED rather than carried from the mount: the row
    // it describes is exactly the one a background measurement can change
    // between the two.
    // No shelf of its own: a card is drawn by the open level, which is the
    // container the session resolves a nameless lift to.
    let entry = EntryDescriptor {
        id: id.clone(),
        vocab: SeamVocab::GridCard,
        base_class: "book-card",
        policy: book_policy(state, &id, facts, None),
    };

    // A removal asks first. The card does not know what a removal costs — the
    // resume point, the placements, the highlights, the app's own copy — and
    // the sheet that does is one context away.
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
                            // A4 portrait, so a shelf of mixed scans and exports
                            // reads as one row of identical frames.
                            "210 / 297".to_string()
                        } else {
                            format!("{:.5} / 1", aspect())
                        }
                    }
                >
                    // The set membership, printed on the cover while the shelf is
                    // choosing: an outline alone asks the reader to remember which
                    // cards they have already tapped.
                    <SelectionCheck state=state id=check_id />
                    <CoverThumb
                        state=state
                        path=Signal::derive(move || {
                            facts.with(|f| f.as_ref().map(|x| x.path.clone()).unwrap_or_default())
                        })
                        alt=Signal::derive(move || {
                            facts.with(|f| f.as_ref().map(|x| x.title.clone()).unwrap_or_default())
                        })
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
                        facts.with(|f| f.as_ref().is_some_and(|x| x.missing))
                            .then(|| {
                                view! {
                                    <span class="book-missing-badge" title="This file is not where the library left it">
                                        <Icon name=IconName::Close size=11 />
                                    </span>
                                }
                            })
                    }}
                </div>
                // Reading progress, flush with the bottom of the art rather than
                // under the title: it is a fact about the cover the reader is
                // looking at, and the frame is what makes it flush with anything.
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
                    title=move || {
                        facts.with(|f| f.as_ref().map(|x| x.title.clone()).unwrap_or_default())
                    }
                >
                    {move || {
                        facts.with(|f| f.as_ref().map(|x| x.title.clone()).unwrap_or_default())
                    }}
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
                facts.with(|f| f.as_ref().is_some_and(|x| x.missing))
                    .then(|| {
                        let at = relink_id.clone();
                        view! {
                            <button
                                class="book-relink"
                                type="button"
                                title="Find this book again"
                                aria-label="Find this book again"
                                on:click=move |ev: leptos::ev::MouseEvent| {
                                    ev.stop_propagation();
                                    relink_dialog(state, at.clone());
                                }
                            >
                                <Icon name=IconName::Open size=11 />
                            </button>
                        }
                    })
            }}
            <button
                class="book-remove"
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
