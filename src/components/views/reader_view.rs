//! Top-level app view: toolbar + sidebar + viewer slot + status bar + noise
//! overlay. The viewer slot switches on viewer.mode. The real mode match is
//! wired during integration; until then it renders the placeholder.
//!
//! Slot wiring is the SINGLE coordinator's job — branches
//! must not edit this file.

use leptos::prelude::*;

use crate::core::document::DocStatus;
use crate::core::layout::ViewMode;
use crate::core::state::AppState;
use crate::effects::theme_applier::theme_applier;

#[component]
pub fn ReaderView(state: AppState) -> impl IntoView {
    theme_applier(state.clone());

    // Hoist signal handles + owned state clones BEFORE the view! macro. Each
    // `move` closure below captures exactly one owned value, so there is no
    // double-move of `state`.
    let status = state.doc.status;
    let mode = state.viewer.mode;
    let state_toolbar = state.clone();
    let state_sidebar = state.clone();
    let state_status = state.clone();
    let state_single = state.clone();
    let state_cont = state.clone();
    let state_placeholder = state.clone();

    let is_ready = move || status.get() == DocStatus::Ready;

    view! {
        <div class="flex h-full w-full flex-col bg-paper text-ink">
            <header>
                <crate::components::molecules::toolbar::Toolbar state=state_toolbar />
            </header>
            <div class="flex min-h-0 flex-1">
                <crate::components::organisms::sidebar::Sidebar state=state_sidebar />
                <main id="viewer-slot" class="relative min-w-0 flex-1">
                    <Show
                        when=is_ready
                        fallback=move || {
                            view! { <Placeholder state=state_placeholder.clone() /> }
                        }
                    >
                        {move || match mode.get() {
                            ViewMode::Single => view! {
                                <crate::components::views::single_page_view::SinglePageView state=state_single.clone() />
                            }
                            .into_any(),
                            ViewMode::Continuous => view! {
                                <crate::components::views::continuous_view::ContinuousView state=state_cont.clone() />
                            }
                            .into_any(),
                        }}
                    </Show>
                </main>
            </div>
            <footer>
                <crate::components::organisms::status_bar::StatusBar state=state_status />
            </footer>
            <div class="noise-overlay"></div>
        </div>
    }
}

#[component]
fn Placeholder(state: AppState) -> impl IntoView {
    let status = state.doc.status;
    let error = state.doc.error;
    let text = move || match status.get() {
        DocStatus::Idle => "Open a PDF to start reading".to_string(),
        DocStatus::Opening => "Opening…".to_string(),
        DocStatus::Ready => "".to_string(),
        DocStatus::Error => error.get().unwrap_or_else(|| "Could not open this PDF".to_string()),
    };
    view! {
        <div class="flex h-full w-full items-center justify-center text-muted">
            <div class="flex max-w-md flex-col items-center gap-2 text-center">
                <p class="text-lg">{text()}</p>
                <crate::components::atoms::button::Button
                    on_click=move |_| {}
                    kind=crate::components::atoms::button::ButtonKind::Primary
                    label="Open…".to_string()
                    title="Open a PDF file".to_string()
                />
            </div>
        </div>
    }
}
