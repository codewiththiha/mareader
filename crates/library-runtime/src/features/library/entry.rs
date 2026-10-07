//! The one door every shelf item goes through.

use leptos::prelude::*;

use crate::features::library::gestures::ShelfItemPolicy;
use crate::features::library::shelf_item::{SeamVocab, ShelfItemShell};

/// What a surface owns: the id doubles as element id, drag
/// registration and reveal target.
pub struct EntryDescriptor {
    pub(crate) id: String,
    pub(crate) vocab: SeamVocab,
    pub(crate) base_class: &'static str,
    pub(crate) policy: ShelfItemPolicy,
}

#[component]
pub(crate) fn EntryShell(
    state: crate::context::LibraryContext,
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

    // The reveal class, in the surface's own vocabulary.
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
