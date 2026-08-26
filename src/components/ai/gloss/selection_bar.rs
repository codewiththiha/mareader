//! The bottom-right selection action bar: count + This page / All /
//! Remove (n) / Done. Rendered by the popover's `SelectMode` (its `undo`
//! signal is the undo pipeline); visible exactly while selection mode is
//! active. Position + elevation come from the `ActionBar` primitive; the
//! actions are the shared compact `Button` (quiet rows Ghost, the
//! destructive one Danger) — no per-bar button styling remains.

use leptos::prelude::*;

use crate::components::ai::gloss::controller::GlossController;
use crate::components::ai::gloss::selection_mode::{exit_selection, park_undo};
use crate::components::primitives::button::{Button, ButtonTone, ButtonVariant};
use crate::components::primitives::overlay::action_bar::ActionBar;
use crate::state::AppState;

#[component]
pub fn GlossSelectBar(
    state: AppState,
    ctrl: GlossController,
    /// Where removals are parked for undo.
    undo: RwSignal<Option<crate::components::ai::gloss::selection_mode::UndoBatch>>,
) -> impl IntoView {
    let selecting = state.reader.gloss.selection_active;
    let selected = state.reader.gloss.selected_marks;
    let marks = state.reader.gloss.marks;
    let page = state.reader.viewer.page;

    let count = Signal::derive(move || selected.with(|s| s.len()));

    let select_page = move |_| {
        let p = page.get_untracked();
        selected.update(|s| {
            marks.with_untracked(|v| {
                for m in v.iter().filter(|m| m.page == p) {
                    s.insert(m.id.clone());
                }
            });
        });
    };

    let select_all = move |_| {
        selected.update(|s| {
            marks.with_untracked(|v| {
                for m in v {
                    s.insert(m.id.clone());
                }
            });
        });
    };

    let remove = move |_| {
        let ids: Vec<String> = selected.get_untracked().into_iter().collect();
        if ids.is_empty() {
            return;
        }
        let removed = ctrl.commands.remove_marks.run(ids);
        let path = state.reader.document.path.get_untracked();
        park_undo(undo, removed, path);
        exit_selection(state);
    };

    let done = move |_| exit_selection(state);

    view! {
        <ActionBar
            visible=Signal::derive(move || selecting.get())
            role="toolbar"
            aria_label="Gloss mark selection"
            class="gloss-select-bar"
        >
            <span class="mr-1.5 text-xs font-medium tabular-nums text-muted">
                {move || format!("{} selected", count.get())}
            </span>
            <Button on_click=select_page variant=ButtonVariant::Ghost compact=true class="rounded-full px-3">
                "This page"
            </Button>
            <Button on_click=select_all variant=ButtonVariant::Ghost compact=true class="rounded-full px-3">
                "All"
            </Button>
            <Button
                on_click=remove
                variant=ButtonVariant::Ghost
                tone=ButtonTone::Danger
                compact=true
                class="rounded-full px-3"
                disabled=Signal::derive(move || count.get() == 0)
            >
                {move || format!("Remove ({})", count.get())}
            </Button>
            <Button on_click=done variant=ButtonVariant::Ghost compact=true class="rounded-full px-3">
                "Done"
            </Button>
        </ActionBar>
    }
}
