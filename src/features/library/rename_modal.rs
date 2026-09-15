//! The rename sheet: one field, and the name the shelf shows is the answer.
//!
//! A rename here is a DISPLAY name and nothing else — the row keeps its id, its address, its
//! resume point and every shelf it is filed on, and the file on disk keeps the name it has, which
//! is what makes the sheet safe to answer without a receipt.

use leptos::ev::KeyboardEvent;
use leptos::prelude::*;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::form::text_input::TextInput;
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::{rename_row, rename_shelf};
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RenameTarget {
    /// A row — a book or a link; the id is all a rename needs, because the name is re-read at the ask.
    Row(String),
    /// A shelf, committed through the same service the crumb's inline field uses (`crate::services::library::rename_shelf`).
    Shelf(String),
}

/// Provided by the library page. A context because the ask comes from the right-click's menu, which is a child of the content and no child of the page's modal stack.
#[derive(Clone, Copy)]
pub(crate) struct RenameSheet {
    pub open: RwSignal<bool>,
    pub target: RwSignal<Option<RenameTarget>>,
    /// Seeded with the name the row or shelf shows, so a rename starts from what the reader is looking at.
    pub draft: RwSignal<String>,
    /// Read at the ask, because a book, a link and a shelf are three sentences about one field.
    pub heading: RwSignal<String>,
    /// What a rename does NOT touch — the file on disk for a row, nothing for a shelf.
    pub hint: RwSignal<String>,
}

impl RenameSheet {
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

    /// A row that is not there any more — removed between the right-click and the ask — is no question.
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

    /// The root is not a shelf, so `shelf_name`'s empty answer closes this door the way a row that is gone does.
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

/// A blank is refused rather than stored — the button says so by sleeping, and Enter agrees with it. The row write is `rename_row`'s; the shelf write is the crumb field's own service, so two doors to one act cannot differ.
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
        RenameTarget::Row(row_id) => rename_row(state, &row_id, name),
        RenameTarget::Shelf(shelf_id) => rename_shelf(state, &shelf_id, name),
    }
}

#[component]
pub(crate) fn RenameModal(state: AppState, sheet: RenameSheet) -> impl IntoView {
    // The lane arbitration and the Escape rule are the modal shell's (see `crate::components::primitives::overlay::modal_shell`); closing costs nothing because the draft is re-seeded at the next ask.
    view! {
        <ModalShell open=sheet.open aria_label="Rename" width="min(92vw, 380px)">
            {move || {
                // Read at the top so an ask rebuilds the sheet with its own sentence on it and a fresh autofocus on the field.
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
