//! Shared shell for the two scrolling modes: scroller, wheel, progress
//! strip.

use leptos::html;
use leptos::prelude::*;
use reader_core::view::Axis;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

use crate::components::viewer::UniversalStripHost;
use crate::components::viewer::controls::overlay_scrollbar::OverlayScrollbar;
use crate::components::viewer::controls::progress_strip::ProgressStrip;
use crate::components::viewer::layouts::layout_chrome;
use crate::state::ReaderState;
use app_chrome::hooks::dom::{H_PAGE_LIST_ID, PAGE_LIST_ID};
use app_chrome::hooks::use_resize_observer::observe_content_size_with;

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
    // The observer's teardown belongs to THIS shell's owner, or it
    // retains the scroller.
    let dom = state.dom;
    let stop_observing =
        observe_content_size_with(move || dom.by_id(scroller_id), state.viewer.container_size);
    on_cleanup(stop_observing);
    // The strip is about to be placed on `viewer.page`; the sync waits.
    state.viewer.awaiting_anchor.set(true);
    let chrome = layout_chrome(state, progress_visible);
    let _gap = chrome.gap;
    let _inset = chrome.inset;

    // The vertical strip mirrors its offset into `viewer.scroll_top`.
    if axis == Axis::Vertical {
        let scroll_top = state.viewer.scroll_top;
        let offset = virtualizer.scroll_offset();
        Effect::new(move |_| scroll_top.set(offset.get()));
    }

    let list_ref: NodeRef<html::Div> = NodeRef::new();
    {
        let v = virtualizer.clone();
        // The listener is retained by JS, so the element is remembered.
        let wheel_guard = StoredValue::new_local(None::<(web_sys::Element, js_sys::Function)>);
        Effect::new(move |_| {
            let Some(div) = list_ref.get() else {
                return;
            };
            let el: web_sys::Element = div.clone().unchecked_into();
            v.bind_container(el.clone());

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

    // THE reading-position anchor for a scrolling strip.
    {
        let v = virtualizer.clone();
        Effect::new(move |_| {
            if !state.viewer.awaiting_anchor.get() {
                return;
            }
            if list_ref.get().is_none() || !v.is_bound() {
                return;
            }
            anchor_to_page(state, &v, axis);
        });
    }

    let total_size = virtualizer.total_size();
    let scroll_offset = virtualizer.scroll_offset();
    // One progress definition for both axes: offset over available
    // travel.
    let progress = move || {
        let st = scroll_offset.get();
        let (cw, ch) = state.viewer.container_size.get();
        let extent = match axis {
            Axis::Vertical => ch,
            Axis::Horizontal => cw,
        };
        reader_core::view::scroll_fraction(st, total_size.get(), extent)
    };

    view! {
        <div class="relative h-full w-full">
            // The strip is the page host's choice; the
            // scroller id and virtualizer are
            // shared.
            <UniversalStripHost
                state=state
                virtualizer=virtualizer
                axis=axis
                scroller_id=scroller_id
                list_ref=list_ref
            />

            <OverlayScrollbar
                dom=state.dom
                scroller_id=scroller_id
                horizontal=axis == Axis::Horizontal
            />
            <Show when=move || chrome.progress_visible.get()>
                <ProgressStrip fraction=Signal::derive(progress) />
            </Show>
        </div>
    }
}

/// Frames the mount anchor re-checks itself before trusting the strip.
const ANCHOR_SETTLE_FRAMES: u32 = 3;

/// Put the strip on `viewer.page`, re-asserted over a few frames.
fn anchor_to_page(state: ReaderState, v: &Virtualizer, axis: Axis) {
    let align = match axis {
        Axis::Vertical => Align::Start,
        Axis::Horizontal => Align::Center,
    };
    let aim_v = v.clone();
    super::anchor_settle::settle(state, v, ANCHOR_SETTLE_FRAMES, move || {
        let page = state.viewer.page.get_untracked();
        aim_v.scroll_to_index(page.saturating_sub(1) as usize, align, ScrollMode::Instant);
    });
}

/// Horizontal wheel policy: only a plain vertical tick needs help.
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
            "wheel", &handler, opts,
        );
    });
    wheel_guard.set_value(Some((el.clone(), handler)));
}
