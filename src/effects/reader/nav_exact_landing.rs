//! Exact-landing correction: re-aims the scroll at the target page until
//! it matches the wrapper's `offsetTop`. Abandons only when the user
//! scrolls — never on distance alone.

use std::time::Duration;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use pdf_core::layout::{DocumentLayout, ViewMode};
use crate::state::ReaderState;
use crate::components::document::dom_helpers::page_list;

use super::navigation_sync::{estimated_top, NavSyncState};

pub(super) fn exact_landing(
    state: ReaderState,
    layout: Memo<DocumentLayout>,
    nav: &NavSyncState,
) {
    let mode = state.viewer.mode;
    let scroll_top = state.viewer.scroll_top;
    let heights = state.document.metrics.css_heights;
    let pending = nav.pending.clone();
    let last_ours = nav.last_ours.clone();
    let jump_settle = nav.jump_settle.clone();
    let settle_timer = nav.settle_timer;
    let settle_wake = nav.settle_wake;
    Effect::new(move || {
        mode.get();
        scroll_top.get();
        settle_wake.get();
        let empty = heights.with(|hs| hs.is_empty());
        if mode.get() != ViewMode::Continuous {
            pending.set(None);
            return;
        }
        let Some(target) = pending.get() else {
            return;
        };
        let settling = js_sys::Date::now() < jump_settle.get();
        if settling {
            let remain = (jump_settle.get() - js_sys::Date::now()).max(0.0);
            if let Some(h) = settle_timer.get_value() {
                h.clear();
            }
            settle_timer.set_value(
                set_timeout_with_handle(
                    move || settle_wake.update(|n| *n = n.wrapping_add(1)),
                    Duration::from_millis(remain as u64 + 30),
                )
                .ok(),
            );
            return;
        }

        let mine = last_ours.get();
        if !mine.is_nan() && (scroll_top.get() - mine).abs() >= 0.5 {
            pending.set(None);
            return;
        }
        let Some(list) = page_list() else {
            return;
        };
        let cur = list.scroll_top() as f64;
        let wrap_id = format!("cont-{}-wrap", target.saturating_sub(1));
        let exact_top = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id(&wrap_id))
            .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
            .map(|el| el.offset_top() as f64);
        match exact_top {
            Some(top) => {
                if (top - cur).abs() >= 0.5 {
                    scroll_top.set(top);
                    last_ours.set(top);
                    list.set_scroll_top(top as i32);
                }
                pending.set(None);
            }
            None => {
                let est = if empty {
                    estimated_top(target, state)
                } else {
                    layout.with(|l| l.page_top(target.saturating_sub(1) as usize))
                }
                .ceil();
                if (est - cur).abs() >= 0.5 {
                    scroll_top.set(est);
                    last_ours.set(est);
                    list.set_scroll_top(est as i32);
                }
            }
        }
    });
}
