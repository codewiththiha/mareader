//! Thin floating scrollbar that takes no layout space.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::components::primitives::hooks::dom::by_id;

#[component]
pub fn OverlayScrollbar(
    scroller_id: &'static str,
    #[prop(default = false)] horizontal: bool,
) -> impl IntoView {
    let progress = RwSignal::new(0.0f64);
    let frac = RwSignal::new(1.0f64);
    let shown = RwSignal::new(false);
    let hide_handle: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));
    let drag = RwSignal::new(None::<(f64, i32)>);

    // Bind once when the scroller mounts.
    Effect::new({
        let hide_handle = hide_handle.clone();
        move |_| {
            let Some(el) = by_id(scroller_id) else {
                return;
            };
            if el.get_attribute("data-overlay-sb").as_deref() == Some("1") {
                return;
            }
            let _ = el.set_attribute("data-overlay-sb", "1");

            // Initial metrics.
            {
                let (total, client, pos) = if horizontal {
                    (
                        el.scroll_width() as f64,
                        el.client_width() as f64,
                        el.scroll_left() as f64,
                    )
                } else {
                    (
                        el.scroll_height() as f64,
                        el.client_height() as f64,
                        el.scroll_top() as f64,
                    )
                };
                frac.set((client / total.max(1.0)).min(1.0));
                progress.set((pos / (total - client).max(1.0)).clamp(0.0, 1.0));
            }

            let progress_s = progress;
            let frac_s = frac;
            let shown_s = shown;
            let hide_s = hide_handle.clone();
            let horiz = horizontal;
            let sid = scroller_id;

            let cb = Closure::wrap(Box::new(move |_: web_sys::Event| {
                if let Some(el) = by_id(sid) {
                    let (total, client, pos) = if horiz {
                        (
                            el.scroll_width() as f64,
                            el.client_width() as f64,
                            el.scroll_left() as f64,
                        )
                    } else {
                        (
                            el.scroll_height() as f64,
                            el.client_height() as f64,
                            el.scroll_top() as f64,
                        )
                    };
                    frac_s.set((client / total.max(1.0)).min(1.0));
                    progress_s.set((pos / (total - client).max(1.0)).clamp(0.0, 1.0));
                }
                shown_s.set(true);
                if let Some(prev) = hide_s.get() {
                    if let Some(w) = web_sys::window() {
                        w.clear_timeout_with_handle(prev);
                    }
                }
                let shown_hide = shown_s;
                let slot = hide_s.clone();
                let tcb = Closure::once_into_js(move || {
                    shown_hide.set(false);
                });
                if let Some(w) = web_sys::window() {
                    if let Ok(h) = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        tcb.as_ref().unchecked_ref(),
                        1000,
                    ) {
                        slot.set(Some(h));
                    }
                }
            }) as Box<dyn FnMut(web_sys::Event)>);

            let _ = el.add_event_listener_with_callback("scroll", cb.as_ref().unchecked_ref());
            // Retain for the component lifetime.
            StoredValue::new_local(cb);
        }
    });

    let axis_pos = move |ev: &leptos::ev::PointerEvent| {
        if horizontal {
            ev.client_x() as f64
        } else {
            ev.client_y() as f64
        }
    };

    let track_base = if horizontal {
        "pointer-events-auto absolute inset-x-2 bottom-0.5 h-1.5"
    } else {
        "pointer-events-auto absolute inset-y-2 right-0.5 w-1.5"
    };

    view! {
        <div
            class=move || {
                if !shown.get() || frac.get() >= 1.0 {
                    format!("{track_base} transition-opacity duration-150 opacity-0")
                } else {
                    format!("{track_base} transition-opacity duration-150 opacity-100")
                }
            }
            on:pointerdown=move |ev| {
                if let Some(el) = by_id(scroller_id) {
                    let pos = if horizontal {
                        el.scroll_left()
                    } else {
                        el.scroll_top()
                    };
                    drag.set(Some((axis_pos(&ev), pos)));
                    if let Some(t) = ev.current_target() {
                        if let Ok(el) = t.dyn_into::<web_sys::Element>() {
                            let _ = el.set_pointer_capture(ev.pointer_id());
                        }
                    }
                    shown.set(true);
                }
            }
            on:pointermove=move |ev| {
                if let (Some((p0, s0)), Some(el)) = (drag.get(), by_id(scroller_id)) {
                    let (total, client) = if horizontal {
                        (el.scroll_width() as f64, el.client_width() as f64)
                    } else {
                        (el.scroll_height() as f64, el.client_height() as f64)
                    };
                    let delta = (axis_pos(&ev) - p0) / client.max(1.0) * total;
                    let next = (s0 as f64 + delta) as i32;
                    if horizontal {
                        el.set_scroll_left(next);
                    } else {
                        el.set_scroll_top(next);
                    }
                }
            }
            on:pointerup=move |_| drag.set(None)
            on:pointercancel=move |_| drag.set(None)
        >
            <div
                class="absolute rounded-full bg-white/30 hover:bg-white/50"
                style=move || {
                    let len = (frac.get() * 100.0).max(8.0);
                    let off = progress.get() * (100.0 - len);
                    if horizontal {
                        format!("left:{off}%;width:{len}%;top:0;bottom:0")
                    } else {
                        format!("top:{off}%;height:{len}%;left:0;right:0")
                    }
                }
            ></div>
        </div>
    }
}
