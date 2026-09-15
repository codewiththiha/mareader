//! The sheet's chrome below the panel: a heading, a scrollable body and a
//! row of buttons.
//!
//! [`ModalShell`](super::modal_shell::ModalShell) owns the backdrop, panel,
//! overlay lane and Escape rule; this owns the three parts a reader sees,
//! which every sheet used to draw from scratch.
//!
//! Split from the shell rather than folded into it because two sheets have a
//! header of their own shape (the import sheet's is a sentence, the removal
//! receipt's carries a cover): these are ordinary components a sheet
//! composes, so one that does not fit uses the two that do and draws the
//! third itself. The heading carries its own truncation tooltip — a shelf
//! name or folder path too long for a 420px panel has to be readable
//! somewhere.

use leptos::prelude::*;

use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;

/// The sheet's heading: a title, one muted line under it, and the ✕ that closes
/// it.
#[component]
pub fn SheetHeader(
    /// The title. Truncates to one line and is its own tooltip: a sheet is
    /// usually about a shelf name or a file path, and neither fits a 420px
    /// panel.
    #[prop(into)]
    heading: String,
    /// The muted line under the title: what the sheet is about in one
    /// clause.
    #[prop(optional, into)]
    subtitle: Option<String>,
    /// What the ✕ does. A sheet whose closing means more than "not open" — the
    /// conflict sheet's question has to go with it — watches its own signal in an
    /// effect and passes the same write here, so the button, the backdrop and
    /// the Escape key all end in one place.
    on_close: Callback<()>,
) -> impl IntoView {
    let tooltip = heading.clone();
    view! {
        <header class="flex shrink-0 items-start gap-3 px-4 pb-3 pt-4">
            <span class="min-w-0 flex-1">
                <span class="block truncate text-sm font-semibold text-ink" title=tooltip>
                    {heading}
                </span>
                {subtitle.map(|line| {
                    view! { <span class="mt-0.5 block text-xs text-muted">{line}</span> }
                })}
            </span>
            <IconButton
                icon=IconName::Close
                title="Close"
                class="rounded-full bg-line/60 hover:bg-line".to_string()
                on_click=move || on_close.run(())
            />
        </header>
    }
}

/// The scrollable middle: everything the sheet has to say, in the box that
/// scrolls when it is too much for the panel.
#[component]
pub fn SheetBody(children: Children) -> impl IntoView {
    view! {
        <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-4">{children()}</div>
    }
}

/// The sheet's button row: right-aligned, under a rule, above nothing.
#[component]
pub fn SheetFooter(children: Children) -> impl IntoView {
    view! {
        <footer class="flex shrink-0 items-center justify-end gap-2 border-t border-line px-4 py-3">
            {children()}
        </footer>
    }
}
