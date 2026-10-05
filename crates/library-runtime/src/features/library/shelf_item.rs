//! The one element every shelf item is.
//!
//! Six surfaces answer to the shelf's press contract — the grid's book card
//! and its link, the list's book row and its link, the grid's folder card and
//! the tree's shelf row — each of which used to wear the same sixty lines.

use std::rc::Rc;

use leptos::prelude::*;

use crate::features::library::context_menu::LibraryMenuHost;
use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use crate::features::library::gestures::{ShelfItemPolicy, use_shelf_item};

/// The whole per-surface difference: which class names it writes, which
/// session questions it asks, and which element id it registers under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeamVocab {
    GridCard,
    ListRow,
    FolderCard,
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

    fn dom_prefix(self) -> &'static str {
        match self {
            SeamVocab::FolderCard => "folder",
            SeamVocab::FolderRow => "shelf-row",
            SeamVocab::GridCard | SeamVocab::ListRow => "book",
        }
    }

    /// A fact about the density, not the kind: a card's reveal rings the
    /// cover frame, a row's is an inset ring, a folder card rings the plate.
    pub(crate) fn reveal(self) -> &'static str {
        match self {
            SeamVocab::FolderCard => "folder-reveal",
            SeamVocab::GridCard => "book-reveal",
            SeamVocab::ListRow | SeamVocab::FolderRow => "row-reveal",
        }
    }
}

/// One table decides every element id an item mounts under
/// ([`ShelfItemShell`]), so the reveal — the only reader of those ids outside
/// the mount — asks the table too, and a renamed prefix moves both.
pub(crate) fn reveal_dom_id(target_is_shelf: bool, list_layout: bool, id: &str) -> String {
    let vocab = match (target_is_shelf, list_layout) {
        (true, true) => SeamVocab::FolderRow,
        (true, false) => SeamVocab::FolderCard,
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

    /// One string rather than one binding per class: the list is one fact and
    /// the shell is its only writer.
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
                    if drag.sibling_before(id) {
                        out.push_str(" row-drop-before");
                    }
                    if drag.sibling_after(id) {
                        out.push_str(" row-drop-after");
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

#[component]
pub(crate) fn ShelfItemShell(
    state: crate::context::LibraryContext,
    vocab: SeamVocab,
    base_class: &'static str,
    policy: ShelfItemPolicy,
    #[prop(optional)] extra_classes: Vec<(String, Signal<bool>)>,
    #[prop(optional)] style: String,
    /// `Option` in the field type and `into` rather than `optional` on
    /// purpose: [`crate::features::library::entry::EntryShell`] already holds
    /// the disclosure's facts as an `Option`, and an `optional` prop's setter
    /// takes the value inside the option.
    #[prop(into)]
    aria_expanded: Option<Signal<bool>>,
    #[prop(into)] on_keydown_first: Option<Callback<leptos::ev::KeyboardEvent, bool>>,
    children: Children,
) -> impl IntoView {
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

    if let Some(drag) = drag {
        drag.registry.register(DropTargetEntry {
            id: DropTargetId(vocab.kind(), id.clone()),
            dom_id: dom_id.clone(),
            shelf: container,
        });
    }

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
            style=style
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
