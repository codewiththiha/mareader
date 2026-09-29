//! The Workspace tab: how the reader's split workspace behaves. Its one knob
//! is what a click on a file in the rail's Library panel does — a drag onto
//! the workspace always opens a split, whatever is chosen here.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_ui::components::primitives::menu::section_label::SectionLabel;
use reader_core::settings::LibraryClick;

/// The choices, in the order the tab lists them: what each is called and
/// what it does, in one sentence.
const CHOICES: [(LibraryClick, &str, &str); 3] = [
    (
        LibraryClick::Replace,
        "Open in focused pane",
        "The file replaces what the focused pane shows.",
    ),
    (
        LibraryClick::Split,
        "Open as a new split",
        "The file opens in a new pane beside the focused one.",
    ),
    (
        LibraryClick::DragOnly,
        "Do nothing",
        "Files open only by dragging them onto the workspace.",
    ),
];

#[component]
pub(crate) fn WorkspaceTab(state: crate::context::ReaderContext) -> impl IntoView {
    let s = state.settings;
    let current = Signal::derive(move || s.with(|st| st.workspace.library_click));
    view! {
        <SectionLabel text="Library panel" />
        <div
            role="radiogroup"
            aria-label="Clicking a file in the Library panel"
            class="divide-y divide-line rounded-xl border border-line"
            data-setting="library-click"
        >
            {CHOICES
                .into_iter()
                .map(|(choice, title, detail)| {
                    let selected = Signal::derive(move || current.get() == choice);
                    view! {
                        <button
                            type="button"
                            role="radio"
                            aria-checked=move || selected.get().to_string()
                            data-choice=format!("{choice:?}").to_lowercase()
                            class="flex w-full items-center gap-3 px-4 py-3 text-left \
                                   first:rounded-t-xl last:rounded-b-xl hover:bg-line/40 \
                                   focus:outline-none focus-visible:ring-2 \
                                   focus-visible:ring-inset focus-visible:ring-accent"
                            on:click=move |_| {
                                if current.get_untracked() != choice {
                                    s.update(|st| st.workspace.library_click = choice);
                                }
                            }
                        >
                            <span class="min-w-0 flex-1">
                                <span class="block text-sm text-ink">{title}</span>
                                <span class="block text-xs text-muted">{detail}</span>
                            </span>
                            <span
                                class="flex h-4 w-4 shrink-0 items-center justify-center \
                                       rounded-full border border-line text-accent"
                                class=("border-accent", move || selected.get())
                            >
                                <Show when=move || selected.get()>
                                    <Icon name=IconName::Check size=10 />
                                </Show>
                            </span>
                        </button>
                    }
                })
                .collect_view()}
        </div>
        <p class="px-1 pt-2 text-xs text-muted">
            "Dragging a file onto the workspace always opens it in a split. \
             With “Do nothing”, Enter still opens the file beside the focused pane."
        </p>
    }
}
