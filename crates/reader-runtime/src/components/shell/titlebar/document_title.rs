//! The reader's toolbar document-name label.

use leptos::prelude::*;

use app_chrome::hooks::dom::TOOLBAR_CENTER_TITLE_ID;

/// The document name for the title bar's center slot.
#[component]
pub fn CenteredDocTitle(state: crate::context::ReaderContext) -> impl IntoView {
    let center_title_ref =
        expect_context::<app_chrome::titlebar::root::TitleBarCtx>().center_title_ref;
    let name = move || state.reader.document.display_name();
    view! {
        <span
            node_ref=center_title_ref
            // The shell's natural-width anchor, observed for renames.
            id=TOOLBAR_CENTER_TITLE_ID
            data-tauri-drag-region="true"
            class="pointer-events-auto min-w-0 max-w-full truncate text-sm font-medium text-ink"
            title=name
        >
            {name}
        </span>
    }
}
