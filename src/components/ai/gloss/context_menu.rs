//! The right-click menu for a single mark. One action by design — Remove
//! highlight — composed on the generic [`ContextMenu`] primitive: the
//! primitive places it at the cursor (clamped into the viewport), owns
//! Escape/outside dismissal, and this module supplies only the payload type
//! and the danger row (a [`MenuItem`] with `MenuItemTone::Danger`).

use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::selection_mode::{park_undo, ContextTarget, UndoBatch};
use crate::components::primitives::floating::context_menu::ContextMenu;
use crate::components::primitives::menu_item::{MenuItem, MenuItemTone};
use crate::state::AppState;

#[component]
pub fn GlossContextMenu(
    state: AppState,
    ctrl: GlossController,
    menu: RwSignal<Option<ContextTarget>>,
    undo: RwSignal<Option<UndoBatch>>,
) -> impl IntoView {
    let remove = Callback::new(move |_| {
        let Some(t) = menu.get_untracked() else {
            return;
        };
        let removed = ctrl.commands.remove_marks.run(vec![t.id]);
        let path = state.reader.document.path.get_untracked();
        park_undo(undo, removed, path);
        menu.set(None);
    });

    view! {
        <ContextMenu
            target=menu
            position=|t: &ContextTarget| (t.x, t.y)
            on_close=Callback::new(move |_| menu.set(None))
            min_width=176
            class="gloss-context-menu"
        >
            <MenuItem
                icon=crate::components::primitives::icon::IconName::Close
                label="Remove highlight"
                tone=MenuItemTone::Danger
                row_class="rounded-lg px-3 py-2"
                on_click=move || remove.run(())
            />
        </ContextMenu>
    }
}
