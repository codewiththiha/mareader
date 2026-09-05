//! Shared shell for the two scrolling modes, the scroll-mode counterpart of
//! [`PageShell`]. It owns everything the family shares — the scroller element
//! and its container binding, the horizontal wheel translation, the overlay
//! scrollbar, and the thin reading-progress strip — so the axis-generic
//! [`UniversalStripHost`] it wraps, which mounts the format's own strip, stays
//! purely presentational.

use leptos::html;
use leptos::prelude::*;
use reader_core::view::Axis;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};
use wasm_bindgen::prelude::Closure;
use wasm_bindgen::JsCast;

use crate::components::viewer::controls::overlay_scrollbar::OverlayScrollbar;
use crate::components::viewer::controls::progress_strip::ProgressStrip;
use crate::components::viewer::layouts::layout_chrome;
use crate::components::viewer::UniversalStripHost;
use app_chrome::hooks::dom::{H_PAGE_LIST_ID, PAGE_LIST_ID};
use app_chrome::hooks::use_resize_observer::observe_content_size;
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
    // This strip is about to be placed on `viewer.page` (see `anchor_to_page`);
    // until it is, the scroll→page sync must not read it. Idempotent with the
    // open flow and the mode flip, which raise the flag before the mount.
    state.viewer.awaiting_anchor.set(true);
    let chrome = layout_chrome(state, progress_visible);
    let _gap = chrome.gap;
    let _inset = chrome.inset;

    // The vertical strip mirrors its scroll offset into `viewer.scroll_top`
    // (the horizontal strip has no scroll_top to mirror). The virtualizer's
    // offset is the one source: it already follows the DOM, and it is the
    // offset every relayout anchors against.
    if axis == Axis::Vertical {
        let scroll_top = state.viewer.scroll_top;
        let offset = virtualizer.scroll_offset();
        Effect::new(move |_| scroll_top.set(offset.get()));
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

    // THE reading-position anchor for a scrolling strip. It answers every
    // way a strip can find itself needing a position: the mount itself
    // (document open, back from the library, a switch into this mode), and a
    // document opened over a mounted reader (drag-drop, "Open with"), which
    // re-raises the flag without remounting anything. Installed AFTER the
    // bind effect above so the first run finds the container bound.
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
            // The strip is the page host's choice: PDF streams rasters, type streams
            // real-type pages. Both bind the SAME scroller id and the SAME virtualizer,
            // so the shell's anchor, wheel and scroll→page machinery drives either
            // without knowing which one is mounted. (The text strip walks A4 cards, which
            // is the horizontal mode's shape; a reflowable document in the VERTICAL mode
            // never reaches this shell — the layout above that mount point asks the
            // stream host instead, and gets the continuous block column.)
            //
            // The virtualizer comes out of local storage exactly as it did before the
            // host took over the branch: the dynamic child's closure must be Send, and
            // the `Rc`-backed handle is not — the same parking `Viewer` does.
            <UniversalStripHost
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

/// How many frames the mount anchor re-checks itself before it trusts the
/// strip. One is what a settled layout needs; the rest cover a container
/// whose box the browser has not committed on the frame it was bound (a
/// scroller that measures 0 tall, a spacer that has not taken its height yet,
/// so the scroll write is clamped to the top).
const ANCHOR_SETTLE_FRAMES: u32 = 3;

/// Put the strip on `viewer.page` — the ONE place a freshly mounted strip
/// takes its position from, whether the mount is a document open (resume),
/// a return from the library, or a switch into this mode.
///
/// The jump is instant and re-asserted for a few frames, by the settle loop
/// both scrolling surfaces share ([`super::anchor_settle`]): the first write
/// happens the moment the container is bound, when the browser may not have
/// laid the scroller out yet, and a `scrollTop` written into a box that is
/// still 0 tall is silently clamped. The aim re-runs every frame, so the page
/// is re-read as it settles and a navigation issued mid-settle wins over the
/// value the mount started with.
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
