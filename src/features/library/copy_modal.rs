//! One question for every copy the library is about to make.
//!
//! A book or a shelf the library reads in place leaves the ground that made
//! it — a move, a lift out, a level coming apart, a shelf coming off the
//! list — and the copy it becomes is a cost the reader agrees to
//! (`crate::services::library::arrange`).

use leptos::prelude::*;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::overlay::modal_shell::ModalShell;
use crate::components::primitives::overlay::sheet::{SheetBody, SheetFooter, SheetHeader};
use crate::services::library::{CopyAnswer, CopyAsk, answer_copy, cancel_copy};
use crate::state::AppState;

#[component]
pub(crate) fn CopyModal(state: AppState) -> impl IntoView {
    let open = state.library.copy_ask.open;

    Effect::new(move |_| {
        if !open.get() {
            state.library.copy_ask.ask.set(None);
        }
    });

    view! {
        <ModalShell
            open=open
            aria_label="Copying books read in place"
            width="min(92vw, 420px)"
        >
            {move || {
                let ask = state.library.copy_ask.ask.get()?;
                Some(view! { <CopySheet state=state ask=ask /> }.into_any())
            }}
        </ModalShell>
    }
}

#[component]
fn CopySheet(state: AppState, ask: CopyAsk) -> impl IntoView {
    let heading = ask.action.clone();
    let subtitle = ask.subject.clone();
    let lines = ask.lines.clone();

    view! {
        <>
            <SheetHeader
                heading=heading
                subtitle=subtitle
                on_close=Callback::new(move |_| cancel_copy(state))
            />
            <SheetBody>
                {lines
                    .into_iter()
                    .map(|line| {
                        view! { <p class="text-xs text-muted">{line}</p> }
                    })
                    .collect::<Vec<_>>()}
            </SheetBody>
            <SheetFooter>
                <Button
                    on_click=move |_| answer_copy(state, CopyAnswer::Cancel)
                    variant=ButtonVariant::Toolbar
                    title="Leave everything as it is"
                >
                    <span>"Cancel"</span>
                </Button>
                {ask
                    .options
                    .into_iter()
                    .map(|option| {
                        let answer = option.answer;
                        let title = option.title;
                        let label = option.label;
                        let variant = if option.primary {
                            ButtonVariant::Primary
                        } else {
                            ButtonVariant::Toolbar
                        };
                        view! {
                            <Button
                                on_click=move |_| answer_copy(state, answer)
                                variant=variant
                                title=title
                            >
                                <span>{label}</span>
                            </Button>
                        }
                    })
                    .collect::<Vec<_>>()}
            </SheetFooter>
        </>
    }
}
