//! Shared shell for the two scrolling modes, the scroll-mode counterpart of
//! [`PageShell`]. It owns everything the family shares — the scroller element
//! and its container binding, the horizontal wheel translation, the overlay
//! scrollbar, and the thin reading-progress strip — so the axis-generic
//! [`PageStrip`] it wraps stays purely presentational.

use leptos::html;
use leptos::prelude::*;
use pdf_core::layout::Axis;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;

use crate::components::document::layouts::layout_chrome;
use crate::components::document::PageStrip;
use crate::components::primitives::hooks::dom::{H_PAGE_LIST_ID, PAGE_LIST_ID};
use crate::components::primitives::hooks::use_resize_observer::observe_content_size;
use crate::components::viewer_controls::overlay_scrollbar::OverlayScrollbar;
use crate::components::viewer_controls::progress_strip::ProgressStrip;
use crate::state::ReaderState;

#[component]
pub fn ScrollShell(
    state: ReaderState,
    virtualizer: Virtualizer,
    axis: Axis,
    #[prop(into)] progress_visible: Signal<bool>,
) -> impl IntoView {
    let scroller_id = match axis {
        Axis::Vertical => PAGE_LIST_ID,
        Axis::Horizontal => H_PAGE_LIST_ID,
    };
    observe_content_size(scroller_id, state.viewer.container_size);
    let chrome = layout_chrome(state, progress_visible);
    let _gap = chrome.gap;
    let _inset = chrome.inset;

    // The vertical strip mirrors its scroll offset into `viewer.scroll_top`.
    // `vertical_scroll_sync` additionally restores the saved offset on mount and
    // keeps the signal in sync from the DOM; both write the same signal so
    // they stay consistent. Horizontal has no scroll_top to mirror.
    if axis == Axis::Vertical {
        crate::effects::reader::vertical_scroll_sync::vertical_scroll_sync(state);
        {
            let scroll_top = state.viewer.scroll_top;
            let offset = virtualizer.scroll_offset();
            Effect::new(move |_| scroll_top.set(offset.get()));
        }
    }

    let list_ref: NodeRef<html::Div> = NodeRef::new();
    {
        let v = virtualizer.clone();
        // The listener is retained by JS (leaked into a Function), so the
        // element it is attached to is remembered so it can be detached on
        // re-bind or unmount.
        let wheel_guard = StoredValue::new_local(None::<(web_sys::Element, js_sys::Function)>);
        Effect::new(move |_| {
            let Some(div) = list_ref.get() else {
                return;
            };
            let el: web_sys::Element = div.clone().unchecked_into();
            v.bind_container(el.clone());
            v.remeasure_container();

            let page = state.viewer.page.get_untracked();
            if page > 0 {
                let align = match axis {
                    Axis::Vertical => Align::Start,
                    Axis::Horizontal => Align::Center,
                };
                v.scroll_to_index((page - 1) as usize, align, ScrollMode::Instant);
            }

            if axis == Axis::Horizontal {
                install_wheel_to_hscroll(&el, &wheel_guard);
            }

            on_cleanup(move || {
                if let Some((old_el, old_fn)) = wheel_guard.get_value() {
                    let _ = old_el.remove_event_listener_with_callback("wheel", &old_fn);
                }
                wheel_guard.set_value(None);
            });
        });
    }

    let total_size = virtualizer.total_size();
    let scroll_offset = virtualizer.scroll_offset();
    // One reading-progress definition for both axes: the strip offset divided by
    // the AVAILABLE travel along the strip's own axis (total extent minus the
    // viewport's extent on that axis). Horizontal uses the width, vertical the
    // height — the axis-generic `container_size` holds both so the same math
    // serves either.
    let progress = move || {
        let st = scroll_offset.get();
        let (cw, ch) = state.viewer.container_size.get();
        let extent = match axis {
            Axis::Vertical => ch,
            Axis::Horizontal => cw,
        };
        let total = total_size.get();
        if total > extent && total > 0.0 {
            (st / (total - extent)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    };

    view! {
        <div class="relative h-full w-full">
            <PageStrip
                state=state
                virtualizer=virtualizer
                axis=axis
                scroller_id=scroller_id
                list_ref=list_ref
            />
            <OverlayScrollbar
                scroller_id=scroller_id
                horizontal=axis == Axis::Horizontal
            />
            <Show when=move || chrome.progress_visible.get()>
                <ProgressStrip fraction=Signal::derive(progress) />
            </Show>
        </div>
    }
}

/// Horizontal wheel policy. The strip is a horizontal scrollport in a world
/// where the wheel is vertical, so exactly one case needs help: a plain
/// vertical tick while the strip fits vertically, where the browser would
/// otherwise do nothing. It is translated into `scrollLeft`. Every other
/// input keeps the native scroll chain (shift+wheel is already horizontal,
/// a trackpad `deltaX` already pans, and a zoom past fit-height lets the
/// vertical pan happen for free).
fn install_wheel_to_hscroll(
    el: &web_sys::Element,
    wheel_guard: &StoredValue<Option<(web_sys::Element, js_sys::Function)>, LocalStorage>,
) {
    if let Some((old_el, old_fn)) = wheel_guard.get_value() {
        let _ = old_el.remove_event_listener_with_callback("wheel", &old_fn);
    }
    let target = el.clone();
    let cb = Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |e: web_sys::WheelEvent| {
        if e.shift_key() {
            return;
        }
        let dx = e.delta_x();
        let mut dy = e.delta_y();
        match e.delta_mode() {
            1 => dy *= 16.0,
            2 => dy *= 120.0,
            _ => {}
        }
        if dx.abs() > dy.abs() {
            return;
        }
        if target.scroll_height() - target.client_height() > 1 {
            return;
        }
        e.prevent_default();
        target.set_scroll_left((target.scroll_left() as f64 + dy) as i32);
    });
    let handler: js_sys::Function = cb.into_js_value().unchecked_into();
    thread_local! {
        static STRIP_WHEEL_OPTS: web_sys::AddEventListenerOptions = {
            let opts = web_sys::AddEventListenerOptions::new();
            opts.set_passive(false);
            opts
        };
    }
    STRIP_WHEEL_OPTS.with(|opts| {
        let _ = el.add_event_listener_with_callback_and_add_event_listener_options(
            "wheel",
            &handler,
            opts,
        );
    });
    wheel_guard.set_value(Some((el.clone(), handler)));
}
