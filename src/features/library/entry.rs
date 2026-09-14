//! The one door every shelf item goes through: a descriptor of what an item IS, and a shell that
//! paints the shared contract from it.
//!
//! Six surfaces draw a shelf item, and every one of them used to say the same two things about
//! itself by hand: the reveal's class and the policy its press contract takes.

use leptos::prelude::*;

use crate::features::library::gestures::ShelfItemPolicy;
use crate::features::library::shelf_item::{SeamVocab, ShelfItemShell};
use crate::state::AppState;

/// The four fields are the four facts a surface genuinely owns: the id (which is also the element id, the drag registration and the reveal's target), the class vocabulary its CSS speaks, its own base classes, and the policy its press contract takes.
pub(crate) struct EntryDescriptor {
    pub(crate) id: String,
    pub(crate) vocab: SeamVocab,
    pub(crate) base_class: &'static str,
    pub(crate) policy: ShelfItemPolicy,
}

#[component]
pub(crate) fn EntryShell(
    state: AppState,
    entry: EntryDescriptor,
    #[prop(optional)]
    extra_classes: Vec<(String, Signal<bool>)>,
    #[prop(optional)]
    style: Option<String>,
    #[prop(optional)]
    aria_expanded: Option<Signal<bool>>,
    #[prop(optional)]
    on_keydown_first: Option<Callback<leptos::ev::KeyboardEvent, bool>>,
    children: Children,
) -> impl IntoView {
    let EntryDescriptor {
        id,
        vocab,
        base_class,
        policy,
    } = entry;

    // The reveal is the shell's question, asked once, in the class the surface's own vocabulary wears for it. The list's surfaces layer their own facts on top (a book whose address died is grey).
    let mut classes = vec![(vocab.reveal().to_string(), state.library.is_revealed(&id))];
    classes.extend(extra_classes);
    let style = style.unwrap_or_default();

    view! {
        <ShelfItemShell
            state=state
            vocab=vocab
            base_class=base_class
            policy=policy
            extra_classes=classes
            style=style
            aria_expanded=aria_expanded
            on_keydown_first=on_keydown_first
        >
            {children()}
        </ShelfItemShell>
    }
}
