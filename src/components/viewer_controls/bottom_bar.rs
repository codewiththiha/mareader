//! Auto-hide bottom bar: the displaced PageNavigation (prev/next + page input)
//! and, in continuous mode, a scroll scrubber. Mirrors the top bar's
//! hover-reveal pattern so mouse readers keep page navigation without any
//! persistent bottom chrome.
//!
//! The scrubber is the shared [`RangeInput`] because its max is reactive: it
//! tracks the live virtualized document extent. `navigation_sync` already syncs
//! DOM scroll ↔ `viewer.page`, so the scrubber and the page pill stay
//! consistent for free.

use std::time::Duration;

use leptos::prelude::*;
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::components::primitives::floating::types::z::BAR;
use crate::components::primitives::form::range_input::RangeInput;
use crate::components::primitives::hooks::use_timeout::use_hover_visibility;
use crate::components::viewer_controls::page_navigation::PageNavigation;
use crate::state::ReaderState;
use pdf_core::layout::ViewMode;

/// Pointer must be off the bar this long before it hides.
const BOTTOM_HIDE_DELAY_MS: u64 = 400;

#[component]
pub fn ReaderBottomBar(
    reader: ReaderState,
    /// The continuous reader's virtualizer, used for slider extent and scroll writes.
    virtualizer: StoredValue<Virtualizer, LocalStorage>,
) -> impl IntoView {
    let total_size = virtualizer.with_value(|v| v.total_size());
    let hover = use_hover_visibility(Duration::from_millis(BOTTOM_HIDE_DELAY_MS), || false);
    let visible = hover.visible;
    let show_strip = hover.show.clone();
    let show_bar = hover.show;
    let hide_later = hover.hide_later.clone();

    view! {
        <div
            class=format!("absolute inset-x-0 bottom-0 {BAR} h-2")
            data-tauri-drag-region="true"
            on:mouseenter=move |_| show_strip()
        ></div>

        <div
            class=format!(
                "toolbar-glass absolute inset-x-0 bottom-0 {BAR} flex h-10 items-center \
                 gap-3 px-3 transition-all duration-200 ease-out"
            )
            prop:inert=move || !visible.get()
            on:mouseenter=move |_| show_bar()
            on:mouseleave=move |_| hide_later()
            class=("translate-y-3", move || !visible.get())
            class=("opacity-0", move || !visible.get())
            class=("pointer-events-none", move || !visible.get())
        >
            <PageNavigation state=reader />
            <Show when=move || reader.viewer.mode.get() == ViewMode::Continuous>
                <RangeInput
                    value=Signal::derive(move || reader.viewer.scroll_top.get())
                    min=Signal::derive(|| 0.0)
                    max=Signal::derive(move || {
                        let total = total_size.get();
                        let (_, vh) = reader.viewer.container_size.get();
                        (total - vh).max(0.0)
                    })
                    step=Signal::derive(|| 1.0)
                    on_input=move |offset| {
                        virtualizer.with_value(|v| v.scroll_to_offset(offset, ScrollMode::Instant));
                    }
                    aria_label="Page position"
                    class="h-2 w-full cursor-pointer appearance-none rounded-full bg-line accent-accent"
                />
            </Show>
        </div>
    }
}
