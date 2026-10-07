//! The rename sheet: one field, and the name the shelf shows is the
//! answer.

use leptos::ev::KeyboardEvent;
use leptos::prelude::*;

use crate::services::{rename_row, rename_shelf};
use app_ui::components::primitives::controls::button::{Button, ButtonVariant};
use app_ui::components::primitives::form::text_input::TextInput;
use app_ui::components::primitives::overlay::modal_shell::ModalShell;
use app_ui::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameTarget {
    /// A row, book or link; the id is all a rename needs.
    Row(String),
    /// A shelf, committed through the same service the crumb's inline field
    /// uses (`crate::services::rename_shelf`).
    Shelf(String),
}

/// Provided by the library page, because the right-click menu is a
/// child of the content.
#[derive(Clone, Copy)]
pub struct RenameSheet {
    pub open: RwSignal<bool>,
    pub target: RwSignal<Option<RenameTarget>>,
    /// Seeded with the name on screen.
    pub draft: RwSignal<String>,
    /// Read at the ask; three kinds of target, one field.
    pub heading: RwSignal<String>,
    /// What a rename does not touch.
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

    /// A row removed between the right-click and the ask is no question.
    pub fn ask_row(&self, state: crate::context::LibraryContext, row_id: &str) {
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

    /// The root is not a shelf: its empty name closes this door.
    pub fn ask_shelf(&self, state: crate::context::LibraryContext, shelf_id: &str) {
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

/// A blank is refused; the row and shelf writes are the services'.
fn commit(state: crate::context::LibraryContext, sheet: RenameSheet) {
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
pub(crate) fn RenameModal(
    state: crate::context::LibraryContext,
    sheet: RenameSheet,
) -> impl IntoView {
    // Lane arbitration and Escape are the modal shell's.
    view! {
        <ModalShell open=sheet.open aria_label="Rename" width="min(92vw, 380px)">
            {move || {
                // Read at the top so an ask rebuilds with a fresh autofocus.
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
