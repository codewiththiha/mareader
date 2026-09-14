//! The one door every shelf item goes through: a descriptor of what an item IS,
//! and a shell that paints the shared contract from it.
//!
//! Six surfaces draw a shelf item — the grid's book card and its link, the list's
//! book row and its link, the grid's folder card and the tree's shelf row — and
//! every one of them used to say the same two things about itself by hand: the
//! reveal's class, spelled into `extra_classes`, and the signal behind it,
//! derived from the library at the mount. Two lines apiece is not much; two lines
//! apiece six times is one sentence written six ways, and it had already drifted
//! — a surface that reached for `is_selected` where the reveal wanted
//! `is_revealed` lights the wrong card, and nothing but a reader noticing would
//! say so.
//!
//! So a surface describes itself ([`EntryDescriptor`]) and [`EntryShell`] wires
//! it: the reveal's class comes from the vocabulary the surface already speaks
//! ([`SeamVocab::reveal`], the same table the reveal's element ids come from), and
//! the shell's attribute set is written once. What is left at a call site is what
//! is actually the surface's own: its base classes, the policy its press contract
//! takes (one of `book_policy`, `folder_policy` or `link_policy` in
//! [`crate::features::library::gestures`]) and its inner content.

use leptos::prelude::*;

use crate::features::library::gestures::ShelfItemPolicy;
use crate::features::library::shelf_item::{SeamVocab, ShelfItemShell};
use crate::state::AppState;

/// One item on a shelf, described rather than wired.
///
/// The four fields are the four facts a surface genuinely owns: the id (which is
/// also the element id, the drag registration and the reveal's target), the class
/// vocabulary its CSS speaks, its own base classes, and the policy its press
/// contract takes. Everything else the shell needs is derived from those.
pub(crate) struct EntryDescriptor {
    /// The id of the book or shelf this item draws: the selection member, the
    /// drag payload, the menu's subject and the element the reveal scrolls to.
    pub(crate) id: String,
    /// The surface's vocabulary: which state classes it paints, which drop
    /// target it registers as, which prefix its element id wears, and which
    /// class its reveal carries.
    pub(crate) vocab: SeamVocab,
    /// The element's own classes before any state is painted on — the card's or
    /// the row's shape, plus a link's marker when it is one.
    pub(crate) base_class: &'static str,
    /// The press contract's three surface answers (see [`ShelfItemPolicy`]).
    pub(crate) policy: ShelfItemPolicy,
}

/// The shelf item's outer element, described rather than stamped.
#[component]
pub(crate) fn EntryShell(
    state: AppState,
    /// What this item is: its id, its vocabulary and its policy.
    entry: EntryDescriptor,
    /// Facts the surface paints as classes of its own, on top of the reveal's
    /// light — a missing book's grey. Read live on every paint of the list.
    #[prop(optional)]
    extra_classes: Vec<(String, Signal<bool>)>,
    /// The row's indent, when the surface is a tree row.
    #[prop(optional)]
    style: Option<String>,
    /// A disclosure's own state, for the tree's shelf row.
    #[prop(optional)]
    aria_expanded: Option<Signal<bool>>,
    /// A key the surface owns BEFORE the shared keyboard halves — the tree
    /// row's Space, which is the disclosure's and not a scroll's.
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

    // The reveal is the shell's question, asked once: whether the library is
    // pointing the reader at THIS item right now, in the class the surface's own
    // vocabulary wears for it. The list's surfaces layer their own facts on top
    // (a book whose address died is grey), which is what the extras are for.
    let mut classes = vec![(vocab.reveal().to_string(), state.library.is_revealed(&id))];
    classes.extend(extra_classes);
    // The row's indent, or nothing: the empty string is what a card and a flat
    // row pass, and it is what the shell already printed for a `None`.
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
