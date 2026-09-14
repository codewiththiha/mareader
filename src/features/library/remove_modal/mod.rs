//! The remove sheet: what a removal costs, itemised.
//!
//! A removal here is not a dismissal. It takes the resume point, every shelf placement, the
//! cached cover and the highlights with it, and for a book the app copied it can take the
//! bytes too — so the sheet reads as a receipt of what is about to go.


mod receipt;

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use library_core::text::{human_size, plural};

use crate::components::primitives::controls::button::{Button, ButtonTone, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter};
use crate::components::primitives::form::row::Row;
use crate::services::library::{PurgeOpts, delete_shelf, purge_books};
use crate::state::AppState;

use receipt::{Receipt, deepest_first, receipt as build_receipt};

/// A context for the same reason the import sheet is one: a remove affordance lives on a grid card, on a list row and on the selection bar.
#[derive(Clone, Copy)]
pub(crate) struct RemoveSheet {
    pub open: RwSignal<bool>,
    /// One id for a card's ✕, several for a selection; empty means the sheet has nothing to ask about and closes itself.
    pub books: RwSignal<Vec<String>>,
    /// Separate from [`Self::books`] because the two are different operations with one confirmation: a shelf is taken apart and keeps every book in the library.
    pub shelves: RwSignal<Vec<String>>,
    /// Reset by every ask rather than remembered, because a cascade is a decision about ONE removal: a switch that persisted would be a preference the sheet never offered as one.
    pub cascade: RwSignal<bool>,
}

impl RemoveSheet {
    pub fn provide() -> Self {
        let sheet = Self {
            open: RwSignal::new(false),
            books: RwSignal::new(Vec::new()),
            shelves: RwSignal::new(Vec::new()),
            cascade: RwSignal::new(false),
        };
        provide_context(sheet);
        sheet
    }

    /// A card's ✕ is never a question about a shelf, so the shelf half is cleared rather than left over from the last selection.
    pub fn ask(&self, book_id: &str) {
        self.books.set(vec![book_id.to_string()]);
        self.shelves.set(Vec::new());
        self.cascade.set(false);
        self.open.set(true);
    }

    pub fn ask_many(&self, book_ids: Vec<String>, shelf_ids: Vec<String>) {
        if book_ids.is_empty() && shelf_ids.is_empty() {
            return;
        }
        self.books.set(book_ids);
        self.shelves.set(shelf_ids);
        self.cascade.set(false);
        self.open.set(true);
    }
}


#[component]
pub(crate) fn RemoveBookModal(state: AppState, sheet: RemoveSheet) -> impl IntoView {
    // On by default: copies the app made for books that are leaving the library are files nothing will ever read again.
    let delete_copy = RwSignal::new(true);

    // Done in an effect rather than in the view, because a view that writes a signal is a view that can be asked to render and mutate in the same pass.
    Effect::new(move |_| {
        if !sheet.open.get() {
            return;
        }
        let ids = sheet.books.get();
        let shelf_ids = sheet.shelves.get();
        let books_alive = !ids.is_empty()
            && state
                .library
                .books
                .with(|rows| rows.iter().any(|r| ids.iter().any(|id| id == r.id())));
        let shelves_alive = !shelf_ids.is_empty()
            && state
                .library
                .shelves
                .with(|shelves| shelves.iter().any(|s| shelf_ids.contains(&s.id)));
        if !books_alive && !shelves_alive {
            sheet.open.set(false);
        }
    });

    view! {
        <ModalShell
            open=sheet.open
            aria_label="Remove from the library"
            width="min(92vw, 420px)"
        >
                {move || {
                    let ids = sheet.books.get();
                    let shelf_ids = sheet.shelves.get();
                    // Read here rather than inside the sheet, so flipping the switch rebuilds the receipt and the sheet together: every row, the store-copy switch and the button's own wording are all answers about ONE set of books.
                    let cascade = sheet.cascade.get();
                    let info = build_receipt(state, &ids, &shelf_ids, cascade)?;
                    let cover_path = info
                        .books
                        .first()
                        .map(|b| b.path().to_string())
                        .unwrap_or_default();
                    let alt = info.heading();
                    Some(view! {
                        <ReceiptSheet
                            state=state
                            sheet=sheet
                            delete_copy=delete_copy
                            info=info
                            cover_path=cover_path
                            alt=alt
                        />
                    })
                }}
        </ModalShell>
    }
}

/// Split out so the body can take the receipt by value: the outer view answers "is there still anything to talk about?" on every run, and this one is built once per answer.
#[component]
fn ReceiptSheet(
    state: AppState,
    sheet: RemoveSheet,
    delete_copy: RwSignal<bool>,
    info: Receipt,
    cover_path: String,
    alt: String,
) -> impl IntoView {
    let cascade = info.cascade;
    let purge_ids = info.book_ids.clone();
    let delete_ids = info.shelf_ids.clone();
    let inside_books = info.inside_books;
    let inside_shelves = info.inside_shelves;
    let offers_cascade = inside_books > 0 || inside_shelves > 0;
    // A `view!` body is a builder, not a place to compute: an attribute and a child that need the same string each need their own copy.
    let heading = info.heading();
    let tooltip = heading.clone();
    let subtitle = info.subtitle();
    let many = info.many();
    let marks = info.marks;
    let covers = info.covers;
    let placements = info.placements.clone();
    let placements_line = placements.join(", ");
    let has_placements = !placements.is_empty();
    let watched = info.watched;
    let stored_count = info.stored_count;
    let stored_bytes = info.stored_bytes;
    let page_line = match info.books.first() {
        Some(book) if !many => library_core::text::page_line(book.page, book.num_pages),
        _ => String::new(),
    };
    let started = !many
        && info
            .books
            .first()
            .is_some_and(|b| b.page > 1 || b.fraction.is_some());
    let marks_line = plural(marks, "mark", "marks");
    // Hoisted out of the view: an `if` in attribute position is an expression the macro has to guess the end of. "Cover art" rather than "cached cover(s)": the cache is the app's business, the picture is the reader's.
    let covers_label = "Cover art";
    let covers_line = plural(covers, "image", "images");
    let books_line = plural(info.books.len(), "book", "books");
    let copy_label = match stored_count {
        1 => format!("Delete the app's own copy ({})", human_size(stored_bytes)),
        n => format!("Delete the app's {n} copies ({})", human_size(stored_bytes)),
    };
    let copy_note_on = match stored_count {
        1 => format!(
            "The copy the app made ({}) goes with the book. The file it was copied from is never touched.",
            human_size(stored_bytes)
        ),
        n => format!(
            "The {n} copies the app made ({}) go with the books. The files they were copied from are never touched.",
            human_size(stored_bytes)
        ),
    };
    // A cascade is already counted in both numbers — the books inside are in `books` and the shelves inside are in `shelves` — except where a shelf holds no books at all and only empty folders.
    let remove_label = match (info.books.len(), info.shelves.len()) {
        (1, 0) => "Remove".to_string(),
        (0, 1) if cascade && inside_shelves > 0 => {
            "Remove the shelf and the ones inside".to_string()
        }
        (0, 1) => "Remove the shelf".to_string(),
        (0, n) if cascade && inside_shelves > 0 => {
            format!("Remove {n} shelves and the ones inside")
        }
        (0, n) => format!("Remove {n} shelves"),
        (b, 0) => format!("Remove {b} books"),
        (b, s) => format!("Remove {b} books and {s} shelves"),
    };
    let show_cover = !many && !info.books.is_empty();
    // Without the cascade the row says what SURVIVES it, and with the cascade on it says what GOES, because that is now the honest answer and the same words would mean the opposite thing.
    let shelf_rows: Vec<(String, String)> = info
        .shelves
        .iter()
        .map(|s| {
            let mut detail = match (s.books, cascade) {
                (0, _) => "empty".to_string(),
                (n, true) => format!("{} go with it", plural(n, "book", "books")),
                (n, false) => plural(n, "book", "books"),
            };
            if s.lifted > 0 {
                detail.push_str(&format!(
                    " · {} move up",
                    plural(s.lifted, "shelf", "shelves")
                ));
            }
            (s.name.clone(), detail)
        })
        .collect();
    let has_shelves = !shelf_rows.is_empty();
    let shelf_watched = info.shelves.iter().any(|s| s.watched);
    let cascade_note = if cascade {
        match (inside_books, inside_shelves) {
            (0, shelves) => format!(
                "{} inside are taken apart with it, instead of moving up a level.",
                plural(shelves, "shelf", "shelves")
            ),
            (books, 0) => format!(
                "{} inside go with the shelf, and are itemised above.",
                plural(books, "book", "books")
            ),
            (books, shelves) => format!(
                "{} inside go with the shelf and {} inside are taken apart too.",
                plural(books, "book", "books"),
                plural(shelves, "shelf", "shelves")
            ),
        }
    } else {
        "Off: the books inside stay in the library and the shelves inside move up a level."
            .to_string()
    };

    view! {
        <>
            <header class="flex shrink-0 items-start gap-3 px-4 pb-3 pt-4">
                {show_cover.then(|| {
                    view! {
                        <span class="remove-cover">
                            {move || {
                                state
                                    .library
                                    .covers
                                    .with(|covers| covers.get(&cover_path).cloned())
                                    .map(|cover| {
                                        view! {
                                            <img
                                                class="remove-cover-img"
                                                src=cover.data_url.clone()
                                                alt=alt.clone()
                                            />
                                        }
                                    })
                            }}
                        </span>
                    }
                })}
                <span class="min-w-0 flex-1">
                    <span class="block truncate text-sm font-semibold text-ink" title=tooltip>
                        {heading}
                    </span>
                    <span class="mt-0.5 block text-xs text-muted">{subtitle}</span>
                </span>
                <IconButton
                    icon=IconName::Close
                    title="Close"
                    class="rounded-full bg-line/60 hover:bg-line".to_string()
                    on_click=move || sheet.open.set(false)
                />
            </header>

            <SheetBody>
                <div class="divide-y divide-line rounded-xl border border-line">
                    {many.then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Library
                                label="Books"
                                value=books_line.clone()
                            />
                        }
                    })}
                    {started.then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Library
                                label="Reading position"
                                value=page_line.clone()
                            />
                        }
                    })}
                    {(marks > 0).then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Type
                                label="Highlights"
                                value=marks_line.clone()
                            />
                        }
                    })}
                    {(covers > 0).then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Thumbs
                                label=covers_label
                                value=covers_line.clone()
                            />
                        }
                    })}
                    {has_placements.then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Outline
                                label="Filed on"
                                value=placements_line.clone()
                            />
                        }
                    })}
                </div>

                {has_shelves.then(|| {
                    view! {
                        <div class="mt-3 divide-y divide-line rounded-xl border border-line">
                            {shelf_rows
                                .iter()
                                .map(|(name, detail)| {
                                    view! {
                                        <ReceiptRow
                                            icon=IconName::Outline
                                            label="Shelf taken apart"
                                            value=format!("{name} — {detail}")
                                        />
                                    }
                                })
                                .collect_view()}
                        </div>
                    }
                })}

                {offers_cascade.then(|| {
                    view! {
                        <div class="mt-3 rounded-xl border border-line">
                            <Row label="Remove everything inside">
                                <Switch
                                    checked=Signal::derive(move || sheet.cascade.get())
                                    on_change=Callback::new(move |on| sheet.cascade.set(on))
                                    title="Take the books and the shelves inside with this shelf"
                                        .to_string()
                                />
                            </Row>
                            <p class="px-4 pb-3 text-xs text-muted">{cascade_note.clone()}</p>
                        </div>
                    }
                })}

                {shelf_watched.then(|| {
                    view! {
                        <p class="mt-3 text-xs text-muted">
                            "A shelf here came from a watched folder. Removing takes it off the
                             list; the folder keeps watching, and the shelf returns when the
                             folder gets new books."
                        </p>
                    }
                })}

                {(stored_count > 0).then(|| {
                    view! {
                        <div class="mt-3 rounded-xl border border-line">
                            <Row label="Delete the copied files">
                                <Switch
                                    checked=Signal::derive(move || delete_copy.get())
                                    on_change=Callback::new(move |on| delete_copy.set(on))
                                    title=copy_label.clone()
                                />
                            </Row>
                            <p class="px-4 pb-3 text-xs text-muted">
                                {move || {
                                    if delete_copy.get() {
                                        copy_note_on.clone()
                                    } else {
                                        "The copies stay in the app's store, with nothing left to read them."
                                            .to_string()
                                    }
                                }}
                            </p>
                        </div>
                    }
                })}

                {watched.then(|| {
                    view! {
                        <p class="mt-3 text-xs text-muted">
                            "Some of these came from a watched folder. Removing keeps them out of
                             future auto-imports — the folder's Add menu can give them back."
                        </p>
                    }
                })}
            </SheetBody>

            <SheetFooter>
                <Button
                    on_click=move |_| sheet.open.set(false)
                    variant=ButtonVariant::Ghost
                    title="Keep these books"
                >
                    <span>"Cancel"</span>
                </Button>
                <Button
                    on_click=move |_| {
                        sheet.open.set(false);
                        if !purge_ids.is_empty() {
                            purge_books(
                                state,
                                &purge_ids,
                                PurgeOpts {
                                    delete_store_copy: delete_copy.get_untracked(),
                                },
                            );
                        }
                        // Shelves after the books, deepest first: a purge sweeps every shelf's member list, and a shelf dissolved first would be swept by nobody.
                        let shelves_now = state.library.shelves.get_untracked();
                        for shelf_id in deepest_first(&shelves_now, &delete_ids) {
                            delete_shelf(state, &shelf_id);
                        }
                    }
                    variant=ButtonVariant::Toolbar
                    tone=ButtonTone::Danger
                    title="Remove these books and everything the library holds about them"
                >
                    <Icon name=IconName::Close size=16 />
                    <span>{remove_label}</span>
                </Button>
            </SheetFooter>
        </>
    }
}

#[component]
fn ReceiptRow(icon: IconName, label: &'static str, value: String) -> impl IntoView {
    let tooltip = value.clone();
    view! {
        <div class="flex items-center gap-2.5 px-3.5 py-2.5">
            <Icon name=icon size=14 class="shrink-0 text-muted" />
            <span class="shrink-0 text-xs text-ink">{label}</span>
            <span class="ml-auto min-w-0 truncate text-xs tabular-nums text-muted" title=tooltip>
                {value}
            </span>
        </div>
    }
}

