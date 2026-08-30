//! Auto-hide bottom bar: the displaced PageNavigation (prev/next + page input)
//! plus a unified, page-based progress slider.
//!
//! The scrubber is a [`RangeInput`] mapped straight onto `viewer.page`
//! (`1..=num_pages`) rather than a scroll offset. Because a page number is
//! layout-agnostic it behaves identically in Single, Dual, Continuous and
//! Horizontal modes: there is no dependency on the active scroll axis, and no
//! reactive DOM scroll-offset reads to keep in sync. `navigation_sync` already
//! turns a `viewer.page` write into the matching scroll / `scroll_to_index`
//! jump (and the dominant-item tracker does the reverse), so dragging the
//! thumb and stepping with the PageNavigation stay consistent for free.

use std::time::Duration;

use leptos::prelude::*;

use crate::components::primitives::floating::types::z::BAR;
use crate::components::primitives::form::range_input::RangeInput;
use crate::components::primitives::hooks::use_timeout::use_hover_visibility;
use crate::components::viewer_controls::page_navigation::PageNavigation;
use crate::state::ReaderState;

/// Pointer must be off the bar this long before it hides.
const BOTTOM_HIDE_DELAY_MS: u64 = 400;

#[component]
pub fn ReaderBottomBar(reader: ReaderState) -> impl IntoView {
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
            <RangeInput
                value=Signal::derive(move || reader.viewer.page.get() as f64)
                min=Signal::derive(|| 1.0)
                max=Signal::derive(move || (reader.document.num_pages.get() as f64).max(1.0))
                step=Signal::derive(|| 1.0)
                on_input=move |page| {
                    reader.viewer.page.set(page.round() as u32);
                }
                aria_label="Page position"
                class="h-2 w-full cursor-pointer appearance-none rounded-full bg-line accent-accent"
            />
        </div>
    }
}
