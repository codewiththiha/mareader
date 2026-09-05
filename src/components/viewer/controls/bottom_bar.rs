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
//!
//! THE HIDE MACHINE is `app_chrome::hooks::hover_reveal`, the same reveal the
//! title bar runs, because the old shape — an enter-only strip plus a leave on
//! the bar alone — could leave the bar up after the pointer was already gone.
//! An exit through the window's bottom edge fires the strip's leave while the
//! animating bar has not become the hover target yet, so the bar never learns
//! the pointer left; a scrubber drag is the second case, because pointer
//! capture keeps every event glued to the input and swallows the bar's
//! mouseleave until the thumb is released. The reveal answers both: the strip
//! and the bar bind BOTH hover edges onto one shared `hovered` truth, the
//! scrubber drag is this bar's hold (the title bar's open popovers are its
//! own), and the recheck settles the visibility the moment that hold releases.
//! `use_drag_hold` contributes the capture half — the release coordinates
//! stand in for the mouseleave that never came.

use leptos::html;
use leptos::prelude::*;

use app_chrome::layers::BAR;
use crate::components::primitives::form::range_input::RangeInput;
use app_chrome::hooks::dom::page_list;
use app_chrome::hooks::{DEFAULT_HOVER_DELAY, use_drag_hold, use_hover_reveal_with};
use super::page_navigation::{PageNavigation, StreamPageNav};
use crate::state::ReaderState;

#[component]
pub fn ReaderBottomBar(reader: ReaderState) -> impl IntoView {
    let bar_ref = NodeRef::<html::Div>::new();
    // The scrubber drag is this bar's hold — what an open popover is to the
    // title bar: while it lasts the surface stays up, and when it ends the
    // shared reveal's recheck settles the visibility.
    let dragging = RwSignal::new(false);
    // The shared reveal owns the timer, the one `hovered` truth the strip
    // and the bar both feed, and that recheck; the bar owns the hold
    // definition — same split as the title bar.
    let hover = use_hover_reveal_with(DEFAULT_HOVER_DELAY, move || dragging.get());
    let visible = hover.visible;

    // One `hovered` truth, fed from both the strip and the bar.
    let (enter, leave) = hover.bind();
    let (enter_strip, leave_strip) = (enter.clone(), leave.clone());
    // End of drag: the captured pointerup / pointercancel bubble from the
    // input even when the release lands outside the bar, and the helper
    // records the leave capture swallowed.
    let end_drag = use_drag_hold(bar_ref, dragging, hover.clone());

    view! {
        // The hover strip: this bar's band. Like the title bar's band it
        // carries BOTH edges of the hover — an enter-only strip leaves the
        // bar up whenever the pointer exits through the window's bottom edge
        // before the animating bar becomes the hover target.
        <div
            class=format!("absolute inset-x-0 bottom-0 {BAR} h-2")
            data-tauri-drag-region="true"
            on:mouseenter=move |_| enter_strip()
            on:mouseleave=move |_| leave_strip()
        ></div>

        <div
            node_ref=bar_ref
            class=format!(
                "toolbar-glass absolute inset-x-0 bottom-0 {BAR} flex h-10 items-center \
                 gap-3 px-3 transition-all duration-200 ease-out"
            )
            prop:inert=move || !visible.get()
            on:mouseenter=move |_| enter()
            on:mouseleave=move |_| leave()
            on:pointerdown=move |_| dragging.set(true)
            on:pointerup=end_drag.clone()
            on:pointercancel=end_drag
            class=("translate-y-3", move || !visible.get())
            class=("opacity-0", move || !visible.get())
            class=("pointer-events-none", move || !visible.get())
        >
            // The page controls are meaningless while a reflowable
            // document streams — there are no pages to name — so the
            // screenful stepper takes their seat and the scrubber stops
            // being a page index. Same bar, same reveal; different unit.
            {move || {
                if reader.reflow_streaming() {
                    view! { <StreamPageNav state=reader /> }.into_any()
                } else {
                    view! { <PageNavigation state=reader /> }.into_any()
                }
            }}
            <RangeInput
                value=Signal::derive(move || {
                    if reader.reflow_streaming() {
                        f64::from(reader.stream_percent())
                    } else {
                        reader.viewer.page.get() as f64
                    }
                })
                min=Signal::derive(move || if reader.reflow_streaming() { 0.0 } else { 1.0 })
                max=Signal::derive(move || {
                    if reader.reflow_streaming() {
                        100.0
                    } else {
                        (reader.document.num_pages.get() as f64).max(1.0)
                    }
                })
                step=Signal::derive(|| 1.0)
                on_input=move |position| {
                    if reader.reflow_streaming() {
                        // A percentage of the document: resolve it against
                        // the scroller's real extent, where the stream's
                        // every measured height already lives.
                        if let Some(el) = page_list() {
                            let max = (el.scroll_height() - el.client_height()).max(0) as f64;
                            el.set_scroll_top((position / 100.0 * max) as i32);
                        }
                    } else {
                        reader.viewer.page.set(position.round() as u32);
                    }
                }
                aria_label="Reading position"
                class="h-2 w-full cursor-pointer appearance-none rounded-full bg-line accent-accent"
            />
        </div>
    }
}
