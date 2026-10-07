//! Auto-hide bottom bar: page navigation plus a page-based progress
//! slider.

use leptos::html;
use leptos::prelude::*;

use super::page_navigation::{PageNavigation, StreamPageNav};
use crate::state::ReaderState;
use app_chrome::hooks::{DEFAULT_HOVER_DELAY, use_drag_hold, use_hover_reveal_with};
use app_chrome::layers::BAR;
use app_ui::components::primitives::form::range_input::RangeInput;

#[component]
pub fn ReaderBottomBar(reader: ReaderState) -> impl IntoView {
    let bar_ref = NodeRef::<html::Div>::new();
    // The scrubber drag is this bar's hold.
    let dragging = RwSignal::new(false);
    // The reveal owns the timer and the recheck.
    let hover = use_hover_reveal_with(DEFAULT_HOVER_DELAY, move || dragging.get());
    let visible = hover.visible;

    // One `hovered` truth, fed from both the strip and the bar.
    let (enter, leave) = hover.bind();
    let (enter_strip, leave_strip) = (enter.clone(), leave.clone());
    // Drag end: the helper records the capture-swallowed leave.
    let end_drag = use_drag_hold(bar_ref, dragging, hover.clone());

    view! {
        // The hover strip carries both edges of the hover.
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
            // No pages while streaming: the stepper takes their seat.
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
                        // Resolve a percentage against the real extent.
                        if let Some(el) = reader.dom.page_list() {
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
