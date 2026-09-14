//! The one element every shelf item is.
//!
//! Six surfaces answer to the shelf's press contract — the grid's book card
//! and its link, the list's book row and its link, the grid's folder card and
//! the tree's shelf row — and before this file each of them wore the same
//! sixty lines to do it: the two host lookups, the wiring's eleven-field
//! spread, the drop-target registration, the seven reactive state classes and
//! the seven pointer attributes. Six copies of one contract is six places it
//! can drift, and a drift here is a gesture that works on a card and not on
//! the row of the same book.
//!
//! So the shell owns the whole outer element — id, role, tabindex, aria, the
//! handlers, the registration and the state classes — and a surface hands it
//! four things that are actually the surface's: the class VOCABULARY its CSS
//! speaks ([`SeamVocab`]), its base classes, the wiring's policy
//! ([`ShelfItemPolicy`]) and its inner content. Anything else a surface
//! paints about itself — the reveal's light, a missing book's grey — arrives
//! as an extra reactive class, because those are facts about the item and not
//! part of the shared contract.
//!
//! The hosts are asked for here rather than expected, once for every surface:
//! the library page provides both, and a mount that provides neither — the
//! reader sidebar's shelf tab — gets rows that keep their tap and disclosure
//! and stand the rest down. That question used to be answered per surface,
//! and two of the six answered it wrong.

use std::rc::Rc;

use leptos::prelude::*;

use crate::features::library::context_menu::LibraryMenuHost;
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::features::library::gestures::{ShelfItemPolicy, use_shelf_item};
use crate::state::AppState;

/// Which surface's class vocabulary and seam questions the shell paints in.
///
/// The CSS keeps one look per density and kind — a card's seam is a line in
/// its gutter, a row's is an inset shadow, a folder's answer to a hold is its
/// mouth lighting rather than a seam at all — and this enum is the whole of
/// the per-surface difference the shell needs: which names it writes, which
/// session questions it asks to decide them, and which target the element
/// registers as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SeamVocab {
    /// A card in the grid: a book or a link — one seam (drop-before), a
    /// fold's ring, and the registration under `book-<id>`.
    GridCard,
    /// A book or link row in the list: two seams and a fold's ring.
    ListRow,
    /// A folder card in the grid: a mouth and no seams, registered under
    /// `folder-<id>`.
    FolderCard,
    /// A shelf row in the tree: a mouth in the middle and sibling seams at
    /// the edges, registered under `shelf-row-<id>`.
    FolderRow,
}

impl SeamVocab {
    fn selected(self) -> &'static str {
        match self {
            SeamVocab::FolderCard => "folder-selected",
            SeamVocab::GridCard => "book-selected",
            SeamVocab::ListRow | SeamVocab::FolderRow => "lib-row-selected",
        }
    }

    fn pressing(self) -> &'static str {
        match self {
            SeamVocab::FolderCard => "folder-pressing",
            SeamVocab::GridCard => "book-pressing",
            SeamVocab::ListRow | SeamVocab::FolderRow => "lib-row-pressing",
        }
    }

    fn dragging(self) -> &'static str {
        match self {
            SeamVocab::FolderCard => "folder-dragging",
            SeamVocab::GridCard => "book-dragging",
            SeamVocab::ListRow | SeamVocab::FolderRow => "lib-row-dragging",
        }
    }

    /// The element-id scheme, which is also the drag registry's: a book is
    /// ONE target to a drag whatever density it is shown at, so both book
    /// vocabularies — and the two link surfaces, which are place-rows — spell
    /// it `book-`.
    fn dom_prefix(self) -> &'static str {
        match self {
            SeamVocab::FolderCard => "folder",
            SeamVocab::FolderRow => "shelf-row",
            SeamVocab::GridCard | SeamVocab::ListRow => "book",
        }
    }

    /// The class the reveal's light wears on this surface, which is a fact about
    /// the DENSITY rather than about the kind: a card's reveal rings the cover
    /// frame it is drawn in, a row's is an inset ring on the row itself, and a
    /// folder card rings the plate — the tree's shelf row is a row, so it wears
    /// the row's name too.
    ///
    /// One table, read by [`crate::features::library::entry::EntryShell`], which
    /// is the only place a reveal is painted. The stylesheet
    /// (`styles/components/library/reveal.css`) is keyed on exactly these three
    /// names, so a name changed here and nowhere else moves the paint and the
    /// element together — the whole point of the shell owning both.
    pub(crate) fn reveal(self) -> &'static str {
        match self {
            SeamVocab::FolderCard => "folder-reveal",
            SeamVocab::GridCard => "book-reveal",
            SeamVocab::ListRow | SeamVocab::FolderRow => "row-reveal",
        }
    }
}

/// The element id a reveal scrolls to and lights: the seam table's own prefix
/// for the surface the target wears in the layout the reader is looking at.
///
/// One table decides every element id an item mounts under
/// ([`ShelfItemShell`]), so the reveal — the one reader of those ids outside
/// the mount — asks the table too. A prefix renamed here moves the shell's
/// registration and the reveal's lookup together, which is the whole of the
/// contract; a second spelling of it was a rename away from a highlight that
/// silently found nothing. Book rows keep the grid's `book` prefix in BOTH
/// layouts (the list row's vocab says so), and only a shelf target's id
/// depends on the density: a folder is a `folder-` card in the grid and a
/// `shelf-row-` row in the list.
pub(crate) fn reveal_dom_id(target_is_shelf: bool, list_layout: bool, id: &str) -> String {
    let vocab = match (target_is_shelf, list_layout) {
        (true, true) => SeamVocab::FolderRow,
        (true, false) => SeamVocab::FolderCard,
        // A book — and a link, which rides a book's vocab in both densities.
        (false, true) => SeamVocab::ListRow,
        (false, false) => SeamVocab::GridCard,
    };
    format!("{}-{id}", vocab.dom_prefix())
}

impl SeamVocab {

    fn kind(self) -> DropTargetKind {
        match self {
            SeamVocab::FolderCard | SeamVocab::FolderRow => DropTargetKind::Folder,
            SeamVocab::GridCard | SeamVocab::ListRow => DropTargetKind::Book,
        }
    }

    /// The element's state and seam classes, live: the three every surface
    /// wears plus the ones this vocabulary's session questions answer. One
    /// string rather than one binding per class, because the whole list is
    /// one fact — what the element looks like right now — and the shell is
    /// the only writer of it.
    fn classes(
        self,
        base: &'static str,
        id: &str,
        selected: bool,
        pressing: bool,
        drag: Option<DragController>,
        extras: &[(String, Signal<bool>)],
    ) -> String {
        let mut out = String::from(base);
        if selected {
            out.push(' ');
            out.push_str(self.selected());
        }
        if pressing {
            out.push(' ');
            out.push_str(self.pressing());
        }
        if let Some(drag) = drag {
            if drag.holds(id) {
                out.push(' ');
                out.push_str(self.dragging());
            }
            match self {
                SeamVocab::GridCard => {
                    if drag.inserts_before(id) {
                        out.push_str(" book-drop-before");
                    }
                    if drag.folds_with(id) {
                        out.push_str(" book-fold-here");
                    }
                }
                SeamVocab::ListRow => {
                    if drag.inserts_before(id) {
                        out.push_str(" row-drop-before");
                    }
                    if drag.inserts_after(id) {
                        out.push_str(" row-drop-after");
                    }
                    if drag.folds_with(id) {
                        out.push_str(" row-fold-here");
                    }
                }
                SeamVocab::FolderCard => {
                    if drag.nests_into(id) {
                        out.push_str(" folder-drag-over");
                    }
                }
                SeamVocab::FolderRow => {
                    if drag.nests_into(id) {
                        out.push_str(" row-nest-here");
                    }
                    // The sibling seam a folders-only hold draws at the row's
                    // outer quarters — the same two names a book row's seams
                    // wear, because the CSS rule is the row's and not the
                    // kind's.
                    match drag.sibling_at(id) {
                        Some(false) => out.push_str(" row-drop-before"),
                        Some(true) => out.push_str(" row-drop-after"),
                        None => {}
                    }
                }
            }
        }
        for (name, on) in extras {
            if on.get() {
                out.push(' ');
                out.push_str(name);
            }
        }
        out
    }
}

/// The shelf item's outer element: one div wearing the whole shared
/// contract, with the surface's own content inside it.
#[component]
pub(crate) fn ShelfItemShell(
    state: AppState,
    /// The surface's vocabulary: which classes it paints and which target it
    /// registers.
    vocab: SeamVocab,
    /// The element's own classes before any state is painted on — the card's
    /// or the row's shape, plus a link's marker when it is one.
    base_class: &'static str,
    /// The press contract's three surface answers (see
    /// [`ShelfItemPolicy`]). Its `container` is also the registration's: the
    /// shelf whose member list renders this element, when the surface knows
    /// it.
    policy: ShelfItemPolicy,
    /// Facts the surface paints as classes of its own — the reveal's light,
    /// a missing book's grey — read live on every paint of the list.
    #[prop(optional)]
    extra_classes: Vec<(String, Signal<bool>)>,
    /// The row's indent, when the surface is a tree row.
    #[prop(into, optional)]
    style: Option<String>,
    /// A disclosure's own state, for the tree's shelf row.
    ///
    /// `Option` as the FIELD's type and `into` rather than `optional` on
    /// purpose: this shell is reached through
    /// [`crate::features::library::entry::EntryShell`] now, which holds the
    /// disclosure's facts as an `Option` of its own and has to hand them on
    /// unchanged. An `optional` prop's setter takes the value INSIDE the
    /// option (that is what makes `<Shell aria_expanded=open />` read the way
    /// it does), so a forwarded `Option` could not be passed through it — and
    /// a shell that unpacked the option only to wrap it again would be a
    /// shell deciding when an attribute is written, which is the mount's
    /// business and not the element's.
    #[prop(into)]
    aria_expanded: Option<Signal<bool>>,
    /// A key the surface owns BEFORE the shared keyboard halves — the tree
    /// row's Space, which is the disclosure's and not a scroll's. Answers
    /// `true` when it handled the event and the shared wiring stands down.
    #[prop(into)]
    on_keydown_first: Option<Callback<leptos::ev::KeyboardEvent, bool>>,
    children: Children,
) -> impl IntoView {
    // Asked for rather than expected, once for every surface: see the module
    // docs. A mount with neither host keeps the tap and the disclosure.
    let drag = use_context::<DragController>();
    let menu = use_context::<LibraryMenuHost>();

    let id = policy.id.clone();
    let container = policy.container.clone();
    let dom_id = format!("{}-{id}", vocab.dom_prefix());

    let gestures = use_shelf_item(state, drag, menu, policy);
    let is_selected = gestures.is_selected;
    let pressing = gestures.pressing;
    let aria_label = gestures.aria_label;
    let aria_pressed = gestures.aria_pressed;
    let on_down = Rc::clone(&gestures.on_pointerdown);
    let on_move = Rc::clone(&gestures.on_pointermove);
    let on_up = Rc::clone(&gestures.on_pointerup);
    let on_cancel = Rc::clone(&gestures.on_pointercancel);
    let on_click = Rc::clone(&gestures.on_click);
    let on_context = Rc::clone(&gestures.on_contextmenu);
    let on_key = Rc::clone(&gestures.on_keydown);

    // Registered for the life of the element, which is the life of its box on
    // screen; the registry's own cleanup is what takes it back off.
    if let Some(drag) = drag {
        drag.registry.register(DropTargetEntry {
            id: DropTargetId(vocab.kind(), id.clone()),
            dom_id: dom_id.clone(),
            shelf: container,
        });
    }

    // The whole class list is one fact, recomputed when any of the signals
    // under it moves — the item's own two, the session's answers and the
    // surface's extras.
    let class_id = id;
    let classes = move || {
        vocab.classes(
            base_class,
            &class_id,
            is_selected.get(),
            pressing.get(),
            drag,
            &extra_classes,
        )
    };

    view! {
        <div
            id=dom_id
            class=classes
            style=style.unwrap_or_default()
            role="button"
            tabindex="0"
            aria-label=move || aria_label.get()
            aria-pressed=move || aria_pressed.get()
            aria-expanded=move || aria_expanded.map(|open| open.get().to_string())
            on:pointerdown=move |ev| (on_down)(&ev)
            on:pointermove=move |ev| (on_move)(&ev)
            on:pointerup=move |ev| (on_up)(&ev)
            on:pointercancel=move |ev| (on_cancel)(&ev)
            on:click=move |ev: leptos::ev::MouseEvent| (on_click)(&ev)
            on:contextmenu=move |ev: leptos::ev::MouseEvent| (on_context)(&ev)
            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                if let Some(first) = on_keydown_first
                    && first.run(ev.clone())
                {
                    return;
                }
                (on_key)(&ev);
            }
        >
            {children()}
        </div>
    }
}
