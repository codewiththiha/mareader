//! The remove sheet: placement, copy and cover go; marks stay unless
//! the switch drops them.

mod receipt;

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use library_core::text::{human_size, plural};

use crate::services::{ReadingData, remove_entries};
use app_ui::components::primitives::controls::button::{Button, ButtonTone, ButtonVariant};
use app_ui::components::primitives::controls::switch::Switch;
use app_ui::components::primitives::form::row::Row;
use app_ui::components::primitives::overlay::modal_shell::ModalShell;
use app_ui::components::primitives::overlay::sheet::{SheetBody, SheetFooter};

use receipt::{Receipt, receipt as build_receipt};

/// A context like the import sheet's: cards, rows and the bar can remove.
#[derive(Clone, Copy)]
pub struct RemoveSheet {
    pub open: RwSignal<bool>,
    /// One id from a card, several from a selection; empty closes the sheet.
    pub books: RwSignal<Vec<String>>,
    /// Not [`Self::books`]: taking a shelf apart keeps every book.
    pub shelves: RwSignal<Vec<String>>,
    /// Reset each ask: a persisted switch would be a preference never offered.
    pub cascade: RwSignal<bool>,
    /// Off and reset by every ask: a removal must not quietly destroy marks.
    pub delete_data: RwSignal<bool>,
}

impl RemoveSheet {
    pub fn provide() -> Self {
        let sheet = Self {
            open: RwSignal::new(false),
            books: RwSignal::new(Vec::new()),
            shelves: RwSignal::new(Vec::new()),
            cascade: RwSignal::new(false),
            delete_data: RwSignal::new(false),
        };
        provide_context(sheet);
        sheet
    }

    /// A card's ✕ is no shelf question: the shelf half clears.
    pub fn ask(&self, book_id: &str) {
        self.books.set(vec![book_id.to_string()]);
        self.shelves.set(Vec::new());
        self.cascade.set(false);
        self.delete_data.set(false);
        self.open.set(true);
    }

    pub fn ask_many(&self, book_ids: Vec<String>, shelf_ids: Vec<String>) {
        if book_ids.is_empty() && shelf_ids.is_empty() {
            return;
        }
        self.books.set(book_ids);
        self.shelves.set(shelf_ids);
        self.cascade.set(false);
        self.delete_data.set(false);
        self.open.set(true);
    }
}

#[component]
pub(crate) fn RemoveBookModal(
    state: crate::context::LibraryContext,
    sheet: RemoveSheet,
) -> impl IntoView {
    // An effect, not the view: a write during render is the trap.
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
                    // Read here, so a cascade flip rebuilds the sheet whole.
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
                            info=info
                            cover_path=cover_path
                            alt=alt
                        />
                    })
                }}
        </ModalShell>
    }
}

/// Split out so the body takes the receipt by value, once per answer.
#[component]
fn ReceiptSheet(
    state: crate::context::LibraryContext,
    sheet: RemoveSheet,
    info: Receipt,
    cover_path: String,
    alt: String,
) -> impl IntoView {
    let cascade = info.cascade;
    let delete_data = sheet.delete_data;
    let purge_ids = info.book_ids.clone();
    let delete_ids = info.shelf_ids.clone();
    let inside_books = info.inside_books;
    let inside_shelves = info.inside_shelves;
    let offers_cascade = inside_books > 0 || inside_shelves > 0;
    // A `view!` body builds, it does not compute.
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
    let copy_label = if stored_count == 1 {
        "The app's own copy"
    } else {
        "The app's own copies"
    };
    let copy_line = human_size(stored_bytes);
    let offers_data = info.offers_data();
    let data_note = Signal::derive(move || {
        if delete_data.get() {
            "They go with the book: importing the file again lands it as a new book.".to_string()
        } else {
            "Kept: the marks, the place and any name you gave a book come back if the file is imported again."
                .to_string()
        }
    });
    // Hoisted out of the view; "Cover art" is the reader's word for the cache.
    let covers_label = "Cover art";
    let covers_line = plural(covers, "image", "images");
    let books_line = plural(info.books.len(), "book", "books");
    // The cascade counts in both numbers except for empty folders.
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
    // Without the cascade the row says what survives; with it, what goes.
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
                    {(stored_count > 0).then(|| {
                        view! {
                            <ReceiptRow
                                icon=IconName::Copy
                                label=copy_label
                                value=copy_line.clone()
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

                {offers_data.then(|| {
                    view! {
                        <div class="mt-3 rounded-xl border border-line">
                            <Row label="Delete the highlights and reading position">
                                <Switch
                                    checked=Signal::derive(move || delete_data.get())
                                    on_change=Callback::new(move |on| delete_data.set(on))
                                    title="Take what the reader wrote in these books with them"
                                        .to_string()
                                />
                            </Row>
                            <p class="px-4 pb-3 text-xs text-muted">{data_note}</p>
                        </div>
                    }
                })}

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
                            "A shelf here came from a watched folder. Removing it asks what the
                             books it reads in place become: copies of your own, or books the
                             folder makes again on its next import."
                        </p>
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
                        let data = if delete_data.get_untracked() {
                            ReadingData::Delete
                        } else {
                            ReadingData::Keep
                        };
                        // The shelf half is the menus' take-apart too.
                        remove_entries(state, purge_ids.clone(), delete_ids.clone(), data);
                    }
                    variant=ButtonVariant::Toolbar
                    tone=ButtonTone::Danger
                    title="Take these books and shelves out of the library"
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
