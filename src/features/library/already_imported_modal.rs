//! "That folder is already a shelf here."
//!
//! The answer a read-at-place import gets when the ground it picked is ground the library already
//! reads: a linked shelf IS the OS folder, so there is no second instance to make.

use leptos::prelude::*;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::conflict;
use crate::services::library::reveal_shelf;
use crate::state::AppState;
use crate::state::library::{AlreadyNote, NoteKind};

#[component]
pub(crate) fn AlreadyImportedModal(state: AppState) -> impl IntoView {
    let open = state.library.already_imported.open;

    Effect::new(move |_| {
        if open.get() {
            return;
        }
        let Some(note) = state.library.already_imported.ask.get_untracked() else {
            return;
        };
        state.library.already_imported.ask.set(None);
        reveal_shelf(state, &note.shelf_id);
    });

    view! {
        <ModalShell
            open=open
            aria_label="This folder is already imported"
            width="min(92vw, 400px)"
        >
            {move || {
                let AlreadyNote { name, kind, .. } = state.library.already_imported.ask.get()?;
                let sublabel = kind.sublabel().to_string();
                let sentence = match &kind {
                    NoteKind::NothingNew => {
                        "The import walked the folder again and found nothing new — every \
                         book is already on the shelf. The shelf lights up when you close \
                         this."
                            .to_string()
                    }
                    NoteKind::Returned => {
                        format!(
                            "“{name}” went back inside the folder it belongs to, onto the \
                             shelf its directory names. Nothing was copied, and nothing on \
                             disk moved. It lights up where it stands now when you close \
                             this."
                        )
                    }
                };
                Some(view! {
                    <>
                        <SheetHeader
                            heading=name.clone()
                            subtitle=sublabel
                            on_close=Callback::new(move |_| conflict::close_already_imported(state))
                        />
                        <SheetBody>
                            <p class="text-xs text-muted">{sentence}</p>
                        </SheetBody>
                        <SheetFooter>
                            <Button
                                on_click=move |_| conflict::close_already_imported(state)
                                variant=ButtonVariant::Primary
                                title="Close and light the shelf up"
                            >
                                <span>"Show the shelf"</span>
                            </Button>
                        </SheetFooter>
                    </>
                })
            }}
        </ModalShell>
    }
}
