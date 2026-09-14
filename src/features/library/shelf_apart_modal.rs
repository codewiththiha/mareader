//! "Taking this shelf apart."
//!
//! The question a hand gets when it takes a level out of a read-at-place tree: the books standing
//! on it leave the ground that made them, so the sheet says where each answer leaves them before
//! either runs (`crate::services::library::arrange`).

use leptos::prelude::*;

use library_core::text::plural;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::arrange::ShelfApartAsk;
use crate::services::library::{cancel_shelf_apart, take_shelf_apart, take_shelf_apart_as_copies};
use crate::state::AppState;

#[component]
pub(crate) fn ShelfApartModal(state: AppState) -> impl IntoView {
    let open = state.library.shelf_apart.open;

    Effect::new(move |_| {
        if !open.get() {
            state.library.shelf_apart.ask.set(None);
        }
    });

    view! {
        <ModalShell
            open=open
            aria_label="Taking this shelf apart"
            width="min(92vw, 420px)"
        >
            {move || {
                let ask = state.library.shelf_apart.ask.get()?;
                Some(view! { <ApartSheet state=state info=ApartInfo::of(&ask) /> }.into_any())
            }}
        </ModalShell>
    }
}

struct ApartInfo {
    heading: String,
    subtitle: String,
    stands: String,
    plain: String,
    copies: String,
}

impl ApartInfo {
    fn of(ask: &ShelfApartAsk) -> Self {
        let books = plural(ask.books, "book", "books");
        let name = ask.name.clone();
        let home = match &ask.home {
            Some(seat) => format!("the shelf “{seat}”"),
            None => "the library itself".to_string(),
        };
        Self {
            heading: name.clone(),
            subtitle: format!("Read at place — {books} on it"),
            stands: format!(
                "“{name}” is a level of “{}”'s own tree, holding {books} read in place from the \
                 folder.",
                ask.folder_name
            ),
            plain: format!(
                "Taking it apart leaves the folder owning the ground: the books come up to {home}, \
                 and its next scan files them back the way the tree names them."
            ),
            copies: format!(
                "Kept as the library's own, they come up to {home} too — with their bytes, \
                 highlights and places in the store, free of the folder."
            ),
        }
    }
}

#[component]
fn ApartSheet(state: AppState, info: ApartInfo) -> impl IntoView {
    let ApartInfo {
        heading,
        subtitle,
        stands,
        plain,
        copies,
    } = info;

    view! {
        <>
            <SheetHeader
                heading=heading
                subtitle=subtitle
                on_close=Callback::new(move |_| cancel_shelf_apart(state))
            />
            <SheetBody>
                <p class="text-xs text-muted">{stands}</p>
                <p class="text-xs text-muted mt-3">{plain}</p>
                <p class="text-xs text-muted mt-3">{copies}</p>
            </SheetBody>
            <SheetFooter>
                <Button
                    on_click=move |_| cancel_shelf_apart(state)
                    variant=ButtonVariant::Toolbar
                    title="Leave the shelf where the folder's tree put it"
                >
                    <span>"Cancel"</span>
                </Button>
                <Button
                    on_click=move |_| take_shelf_apart(state)
                    variant=ButtonVariant::Toolbar
                    title="Take the level apart; its books come up a level inside the tree"
                >
                    <span>"Take it apart"</span>
                </Button>
                <Button
                    on_click=move |_| take_shelf_apart_as_copies(state)
                    variant=ButtonVariant::Primary
                    title="Copy its read-at-place books into the library, then take the level apart"
                >
                    <span>"Keep the books as copies"</span>
                </Button>
            </SheetFooter>
        </>
    }
}
