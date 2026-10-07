//! The question sheet: heading, question, [`ChoiceRow`]s, apply-to-all.

use leptos::prelude::*;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;

use super::sheet::{SheetBody, SheetFooter, SheetHeader};

/// The "apply to all" row, rendered only while questions wait.
#[component]
fn ApplyToAll(
    /// How many MORE questions wait behind the one on screen.
    waiting: usize,
    /// The switch's own state, owned by the sheet whose answers read it.
    checked: RwSignal<bool>,
) -> impl IntoView {
    let label = format!("Apply to all {}", waiting + 1);
    view! {
        <div class="mt-3 flex items-center justify-between gap-3 rounded-xl border border-line px-3 py-2">
            <span class="text-xs text-muted">{label}</span>
            <Switch
                checked=Signal::derive(move || checked.get())
                on_change=Callback::new(move |on| checked.set(on))
                title="Give every waiting question this same answer"
                    .to_string()
            />
        </div>
    }
}

/// One question sheet's inside: header, question, answers, the queue's
/// switch, and the Cancel.
#[component]
pub fn QuestionSheet(
    /// The title — usually the arriving name.
    #[prop(into)]
    heading: String,
    /// The muted line: where the collision is, and how many wait.
    #[prop(into)]
    subtitle: String,
    /// The question, in one sentence.
    #[prop(into)]
    question: String,
    /// The sheet's one close: ✕, backdrop, Escape and Cancel.
    on_close: Callback<()>,
    /// What cancelling promises, as the button's tooltip.
    #[prop(default = "Leave the shelf as it is".to_string())]
    cancel_title: String,
    /// The apply-to-all row's facts: how many wait, and the switch.
    #[prop(optional)]
    apply_all: Option<(usize, RwSignal<bool>)>,
    /// The answers, in the order the sheet means them to be read.
    children: Children,
) -> impl IntoView {
    view! {
        <>
            <SheetHeader heading=heading subtitle=subtitle on_close=on_close />
            <SheetBody>
                <p class="text-xs text-muted">{question}</p>
                <div class="mt-3 divide-y divide-line rounded-xl border border-line">
                    {children()}
                </div>
                {apply_all
                    .filter(|(waiting, _)| *waiting > 0)
                    .map(|(waiting, checked)| {
                        view! { <ApplyToAll waiting checked /> }
                    })}
            </SheetBody>
            <SheetFooter>
                <Button
                    on_click=move |_| on_close.run(())
                    variant=ButtonVariant::Ghost
                    title=cancel_title
                >
                    <span>"Cancel"</span>
                </Button>
            </SheetFooter>
        </>
    }
}
