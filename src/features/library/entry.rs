//! The one door every shelf item goes through: a descriptor of what an item
//! is, and a shell that paints the shared contract from it.
//!
//! Six surfaces draw a shelf item; each used to say the same two things about
//! itself by hand — the reveal's class and the policy its press contract
//! takes.

use leptos::prelude::*;

use crate::features::library::gestures::ShelfItemPolicy;
use crate::features::library::shelf_item::{SeamVocab, ShelfItemShell};
use crate::state::AppState;

/// The facts a surface genuinely owns. The id doubles as the element id, the
/// drag registration and the reveal's target.
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
    #[prop(optional)] extra_classes: Vec<(String, Signal<bool>)>,
    #[prop(optional)] style: String,
    #[prop(optional)] aria_expanded: Option<Signal<bool>>,
    #[prop(optional)] on_keydown_first: Option<Callback<leptos::ev::KeyboardEvent, bool>>,
    children: Children,
) -> impl IntoView {
    let EntryDescriptor {
        id,
        vocab,
        base_class,
        policy,
    } = entry;

    // The reveal class is asked once here, in the surface's own vocabulary;
    // list surfaces layer their own facts on top (a dead address is grey).
    let mut classes = vec![(vocab.reveal().to_string(), state.library.is_revealed(&id))];
    classes.extend(extra_classes);

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
