//! The sheet's chrome below the panel: heading, body, button row.

use leptos::prelude::*;

use app_chrome::icon::IconName;
use app_chrome::icon_button::IconButton;

/// The sheet's heading: title, muted line, and the ✕.
#[component]
pub fn SheetHeader(
    /// The title, truncating to one line as its own tooltip.
    #[prop(into)]
    heading: String,
    /// The muted line under the title: what the sheet is about in one
    /// clause.
    #[prop(optional, into)]
    subtitle: Option<String>,
    /// What the ✕ does: the sheet's one close.
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

/// The scrollable middle: everything the sheet has to say.
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
