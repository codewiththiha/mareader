//! The reader's toolbar document-name label.
//!
//! The library page has no label of its own: its bar is navigation, and a
//! breadcrumb saying where you are is a different job from naming a document.

use leptos::prelude::*;

use app_chrome::hooks::dom::TOOLBAR_CENTER_TITLE_ID;

/// Centered document name for the reader title bar's center slot.
///
/// The shell (`TitleBar` in app-chrome) owns position and width: it
/// measures the row, both clusters and this label's natural width (the
/// `#toolbar-center-title` anchor), then places the label at the row's
/// EXACT center while it fits, falling back to the free stretch between
/// the clusters (centered, truncated) once it does not — with the
/// caption cluster, the pin and the traffic-light gutter reserved on
/// every platform. Below the shell's floor the slot hides the label
/// rather than showing a stub.
///
/// The label carries `data-tauri-drag-region` itself: it is the most
/// obvious thing to grab in a bar whose job is holding the window title,
/// and `-webkit-user-select: none` makes it unselectable — so it must
/// move the window instead of doing nothing.
#[component]
pub fn CenteredDocTitle(state: crate::context::ReaderContext) -> impl IntoView {
    let center_title_ref =
        expect_context::<app_chrome::titlebar::root::TitleBarCtx>().center_title_ref;
    let name = move || state.reader.document.display_name();
    view! {
        <span
            node_ref=center_title_ref
            // The shell's natural-width anchor (its scrollWidth) for the
            // center-slot decision, observed to re-measure on renames.
            // pointer-events-auto: the slot overlay is click-transparent,
            // but the label itself must still grab the window and show
            // its tooltip.
            id=TOOLBAR_CENTER_TITLE_ID
            data-tauri-drag-region="true"
            class="pointer-events-auto min-w-0 max-w-full truncate text-sm font-medium text-ink"
            title=name
        >
            {name}
        </span>
    }
}
