//! The Workspace tab: how the reader's split workspace behaves. Pane
//! decoration lives in the Theme tab; this tab keeps workspace opening and
//! independent-theme controls.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_ui::appearance::ThemeHandle;
use app_ui::components::primitives::controls::switch::Switch;
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
    // The host's theme handle flips the live workspace's per-pane looks;
    // without one (no workspace in this window) the same toggle persists
    // the setting for the next one.
    let theme = use_context::<ThemeHandle>().unwrap_or_else(|| ThemeHandle::for_settings(s));
    let toggle = view! {
        <Switch
            checked=theme.independent
            on_change=theme.set_independent
            title="Independent theme for each pane"
        />
    };
    let shared = Signal::derive(move || s.with(|st| st.workspace.shared_base_mode));
    let set_shared = Callback::new(move |on: bool| {
        s.update(|st| st.workspace.shared_base_mode = on);
    });
    let shared_toggle = view! {
        <Switch
            checked=shared
            on_change=set_shared
            disabled=Signal::derive(move || !theme.independent.get())
            title="Light, Dark and Dim change every pane"
        />
    };
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

        <SectionLabel text="Pane themes" />
        <div
            class="flex items-center justify-between gap-3 rounded-xl border border-line px-4 py-3"
            data-setting="independent-themes"
        >
            <span class="min-w-0">
                <span class="block text-sm text-ink">"Independent theme for each"</span>
                <span class="block text-xs text-muted">
                    "While a split is open, each pane gets its own colour, different from the \
                     others. The film grain stays shared, the outer chrome keeps the main \
                     theme, and the colours are temporary."
                </span>
            </span>
            {toggle}
        </div>
        <div
            class="mt-2 flex items-center justify-between gap-3 rounded-xl border border-line px-4 py-3"
            class=("opacity-60", move || !theme.independent.get())
            data-setting="shared-base-mode"
        >
            <span class="min-w-0">
                <span class="block text-sm text-ink">"Light, Dark and Dim change every pane"</span>
                <span class="block text-xs text-muted">
                    "Each pane keeps its own colour, but switching between Light, Dark and \
                     Dim switches all panes together. Turn off to let each pane have its own \
                     mode as well."
                </span>
            </span>
            {shared_toggle}
        </div>
    }
}
