//! The rename sheet: one field, and the name the shelf shows is the answer.
//!
//! A rename here is a DISPLAY name and nothing else — the row keeps its id, its
//! address, its resume point and every shelf it is filed on, and the file on
//! disk keeps the name it has. That is what makes the sheet safe to answer
//! without a receipt: unlike a removal, nothing is about to go, and unlike the
//! crumb's inline field it works for a row as well as a shelf, from the same
//! right-click that carries every other act a card has.
//!
//! The name a reader types is LOCKED (`library_core::book::Book::title_locked`):
//! the load-time sweep drops a title shaped like a filename because that shape
//! is download debris a document supplied, and a name a person chose at this
//! sheet is not debris whatever it looks like. The sweep keeps healing the
//! titles documents supply; it just no longer reaches past the reader.
//!
//! No collision question is asked, and that is the level rule rather than an
//! omission: names are how the shelf reads, ids are how the library counts, and
//! two rows of one name on one level are two books the reader named that way —
//! the counter a collision mints answers an ARRIVAL, and a rename is not an
//! arrival. A blank name is refused the way the crumb's field refuses it: the
//! button sleeps and Enter does nothing, because a row with no name is a row
//! the reader cannot tell from its neighbour.

use leptos::ev::KeyboardEvent;
use leptos::prelude::*;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::form::text_input::TextInput;
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::rename_shelf;
use crate::state::AppState;

/// What the sheet is about: the row or the shelf the reader pointed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenameTarget {
    /// A row — a book or a link. The id is all a rename needs: the name is
    /// re-read at the ask, and the write goes through
    /// `LibraryState::rename_row`, which knows which half of a row a name is.
    Row(String),
    /// A shelf, renamed through the same service the crumb's inline field
    /// commits with (`crate::services::library::rename_shelf`).
    Shelf(String),
}

/// The sheet's handles, provided by the library page: whether it is open, what
/// it is about, and the name being typed.
///
/// A context for the same reason the removal sheet is one — the ask comes from
/// the right-click's menu, which is a child of the content and no child of the
/// page's modal stack, and threading signals between the two would put the
/// sheet's plumbing in every component between.
#[derive(Clone, Copy)]
pub(crate) struct RenameSheet {
    pub open: RwSignal<bool>,
    pub target: RwSignal<Option<RenameTarget>>,
    /// The field's text, seeded with the name the row or shelf shows so a
    /// rename starts from what the reader is looking at rather than from blank.
    pub draft: RwSignal<String>,
    /// The sheet's own heading, read at the ask: a book, a link and a shelf are
    /// three sentences about the same field, and the ask is the moment that
    /// knows which one this is.
    pub heading: RwSignal<String>,
    /// The line under the field that says what a rename does NOT touch — the
    /// file on disk for a row, nothing for a shelf. Read at the ask for the
    /// same reason the heading is.
    pub hint: RwSignal<String>,
}

impl RenameSheet {
    /// Create and provide the handles. Called once, by the page.
    pub fn provide() -> Self {
        let sheet = Self {
            open: RwSignal::new(false),
            target: RwSignal::new(None),
            draft: RwSignal::new(String::new()),
            heading: RwSignal::new(String::new()),
            hint: RwSignal::new(String::new()),
        };
        provide_context(sheet);
        sheet
    }

    /// Ask about a row. A row that is not there — removed between the
    /// right-click and the ask — is no question, and a row with no name has
    /// nothing to rename from.
    pub fn ask_row(&self, state: AppState, row_id: &str) {
        let Some(row) = state.library.row(row_id) else {
            return;
        };
        let name = row.display_name();
        if name.trim().is_empty() {
            return;
        }
        let (heading, hint) = if row.is_link() {
            (
                "Rename link",
                "The name the pointer wears — the book it points at keeps its own.",
            )
        } else {
            (
                "Rename book",
                "Only the name the library shows — the file on disk keeps its own.",
            )
        };
        self.heading.set(heading.to_string());
        self.hint.set(hint.to_string());
        self.target.set(Some(RenameTarget::Row(row_id.to_string())));
        self.draft.set(name);
        self.open.set(true);
    }

    /// Ask about a shelf. The root is not a shelf and has no name of its own,
    /// and `shelf_name` answers it with the empty string — which closes this
    /// door the same way a row that is gone does.
    pub fn ask_shelf(&self, state: AppState, shelf_id: &str) {
        let name = state.library.shelf_name(shelf_id);
        if name.trim().is_empty() {
            return;
        }
        self.heading.set("Rename shelf".to_string());
        self.hint
            .set("Only the name the library shows — a folder on disk keeps its own.".to_string());
        self.target
            .set(Some(RenameTarget::Shelf(shelf_id.to_string())));
        self.draft.set(name);
        self.open.set(true);
    }
}

/// Commit the field: one write, one persist, one close.
///
/// A blank is refused rather than stored — the button says so by sleeping, and
/// Enter agrees with the button. The row write is `rename_row`'s (which locks
/// the name the reader typed); the shelf write is the crumb field's own
/// service, so two doors to one act cannot differ about what it means.
fn commit(state: AppState, sheet: RenameSheet) {
    let Some(target) = sheet.target.get_untracked() else {
        sheet.open.set(false);
        return;
    };
    let name = sheet.draft.get_untracked();
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    sheet.open.set(false);
    match target {
        RenameTarget::Row(row_id) => {
            state.library.rename_row(&row_id, name);
            crate::storage::persist_library(state.library);
        }
        RenameTarget::Shelf(shelf_id) => rename_shelf(state, &shelf_id, name),
    }
}

/// The library's one rename sheet.
#[component]
pub(crate) fn RenameModal(state: AppState, sheet: RenameSheet) -> impl IntoView {
    // The lane arbitration and the Escape rule are the modal shell's (see
    // `crate::components::primitives::overlay::modal_shell`): a backdrop click
    // and the Escape key both close, and closing costs nothing because the
    // draft is re-seeded at the next ask.
    view! {
        <ModalShell open=sheet.open aria_label="Rename" width="min(92vw, 380px)">
            {move || {
                // Read at the top so an ask — the only write any of these sees
                // while the sheet is up — rebuilds the sheet with its own
                // sentence on it and a fresh autofocus on the field.
                let heading = sheet.heading.get();
                let hint = sheet.hint.get();
                let target = sheet.target.get();
                let live = target.is_some();
                Some(view! {
                    <>
                        <SheetHeader
                            heading=heading
                            on_close=Callback::new(move |_| sheet.open.set(false))
                        />
                        <SheetBody>
                            <TextInput
                                value=sheet.draft
                                on_input=Callback::new(move |text| sheet.draft.set(text))
                                aria_label="New name".to_string()
                                autofocus=true
                                class="w-full rounded-lg border border-line bg-paper px-3 py-2 \
                                       text-sm text-ink focus:border-accent focus:outline-none"
                                    .to_string()
                                on_keydown=Callback::new(move |ev: KeyboardEvent| {
                                    if ev.key() == "Enter" {
                                        ev.prevent_default();
                                        commit(state, sheet);
                                    }
                                })
                            />
                            <p class="mt-2 text-xs text-muted">{hint}</p>
                        </SheetBody>
                        <SheetFooter>
                            <Button
                                on_click=move |_| sheet.open.set(false)
                                variant=ButtonVariant::Ghost
                                title="Keep the name it has"
                            >
                                <span>"Cancel"</span>
                            </Button>
                            <Button
                                on_click=move |_| commit(state, sheet)
                                variant=ButtonVariant::Primary
                                disabled=Signal::derive(move || {
                                    !live || sheet.draft.get().trim().is_empty()
                                })
                                title="Give it this name"
                            >
                                <span>"Rename"</span>
                            </Button>
                        </SheetFooter>
                    </>
                })
            }}
        </ModalShell>
    }
}
