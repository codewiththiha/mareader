//! The toolbar's document-name label: correct name, and truncation only when
//! the name would actually collide with something.
//!
//! ## What was wrong
//!
//! 1. The label rendered `doc.title.or(doc.path)` verbatim, so a PDF whose
//!    `/Title` metadata is a stale producer path ("file:///F|/Mis%20docum")
//!    showed that path instead of the file's name. `core::filename` owns the
//!    rules that pick a trustworthy name; this component just renders it.
//! 2. It carried a hard `max-w-40` (160px), so names were folded with `…` while
//!    the toolbar still had plenty of free space.
//!
//! ## How the truncation works now
//!
//! The label gets a *measured* `max-width` in px equal to the real gap between
//! the buttons on its left and the nearest thing on its right, so `truncate`
//! only ever engages on an actual collision:
//!
//! ```text
//!  ┌ px-3 ┬── #toolbar-left-pre ──┬── this label ──┬ … ┬ #toolbar-center ┬ … ┬ #toolbar-right ─┬ px-3 ┐
//!         │ hamburger + Open      │ max-width  ->  │   │ page nav        │   │ search/zoom/…   │
//! ```
//!
//! * When a document is open the page nav is absolutely centered in the
//!   viewport (Single and Continuous), so the label may only grow until it
//!   reaches the nav's left edge.
//! * With no document open there is no centered nav, so the label may grow
//!   all the way to the right-hand control group.
//! * Either way the right group is a hard stop, and everything is derived from
//!   the live element WIDTHS, so a window resize re-measures automatically.
//!
//! Measuring widths (never positions) is deliberate: the label sits between the
//! measured elements, so if the maths depended on its neighbours' x-positions,
//! a wide label would push them, shrink the computed budget, shrink the label,
//! un-push them... a feedback loop that oscillates. Widths of the left/right
//! groups are independent of the label, so the computation is stable in one
//! pass.
//!
//! Below `MIN_LABEL_W` there is no useful name left to show ("P…"), so the
//! label is hidden entirely rather than rendered as a stub — that is the
//! window-width awareness on very narrow windows.

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::ResizeObserverEntry;

use pdf_core::filename::display_name;
use crate::core::state::AppState;

/// Horizontal padding of the toolbar row (`px-3`).
const ROW_PAD: f64 = 12.0;
/// Gap between the label and the buttons to its left (`gap-1` in the left group).
const GAP_LEFT: f64 = 4.0;
/// Gap the label keeps from whatever is on its right (the centered page nav or
/// the right-hand control group). A little wider than the row's `gap-2` so the
/// name never appears to touch the next control.
const GAP_RIGHT: f64 = 12.0;
/// Narrower than this and the label would be a useless stub — hide it instead.
const MIN_LABEL_W: f64 = 56.0;

/// Live width of an element by id, or `None` when it isn't in the DOM.
fn width_of(id: &str) -> Option<f64> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
        .map(|el| el.get_bounding_client_rect().width())
}

/// Compute the label's available width in CSS px from the toolbar's live
/// geometry. `None` when the toolbar isn't laid out yet (measure again later).
fn measure_available() -> Option<f64> {
    let row_w = width_of("toolbar-row")?;
    if row_w <= 0.0 {
        return None;
    }
    // Buttons to the LEFT of the label (hamburger + Open).
    let pre_w = width_of("toolbar-left-pre").unwrap_or(0.0);
    // The right-hand control group is always a hard stop.
    let right_w = width_of("toolbar-right").unwrap_or(0.0);

    let start = ROW_PAD + pre_w + GAP_LEFT;
    let mut end = row_w - ROW_PAD - right_w - GAP_RIGHT;

    // When a document is Ready the page nav is absolutely centered on the ROW,
    // so its left edge is (row/2 - nav/2) regardless of the flex groups around
    // it. Absent on the library/empty screen, where the label may run to the
    // right group.
    if let Some(center_w) = width_of("toolbar-center")
        && center_w > 0.0
    {
        end = end.min(row_w / 2.0 - center_w / 2.0 - GAP_RIGHT);
    }

    Some((end - start).max(0.0))
}

#[component]
pub fn DocTitle(state: AppState) -> impl IntoView {
    // Measured budget. `None` = not measured yet: render unconstrained for that
    // first frame rather than guessing a width (a wrong guess would visibly
    // fold a name that fits).
    let avail = RwSignal::new(None::<f64>);

    // Keep the observer + its closure alive for the component's lifetime.
    let observer_handle = StoredValue::new_local(None::<web_sys::ResizeObserver>);
    let callback_handle =
        StoredValue::new_local(None::<Closure<dyn FnMut(Vec<ResizeObserverEntry>)>>);

    // Re-measure after the browser has laid out the current frame. Called from
    // both the ResizeObserver and the reactive triggers below.
    let remeasure = move || {
        request_animation_frame(move || {
            if let Some(w) = measure_available() {
                // Only write on a real change: an idempotent write would still
                // notify and re-run the class/style closures every frame the
                // observer fires during the sidebar's 300ms width animation.
                let prev = avail.get_untracked();
                if prev.is_none_or(|p: f64| (p - w).abs() > 0.5) {
                    avail.set(Some(w));
                }
            }
        });
    };

    // Observe the row and both control groups. The row covers window resizes
    // and the sidebar slide; the groups cover their own content changing
    // (e.g. the zoom readout going from "100%" to "137%", which really does
    // steal space from the name).
    Effect::new(move |_| {
        if callback_handle.with_value(|c| c.is_some()) {
            return;
        }
        let callback: Closure<dyn FnMut(Vec<ResizeObserverEntry>)> =
            Closure::wrap(Box::new(move |_: Vec<ResizeObserverEntry>| {
                remeasure();
            }) as Box<dyn FnMut(Vec<ResizeObserverEntry>)>);
        let fn_ref: &js_sys::Function = callback.as_ref().unchecked_ref();
        let Ok(observer) = web_sys::ResizeObserver::new(fn_ref) else {
            return;
        };
        let doc = web_sys::window().and_then(|w| w.document());
        let mut observed = false;
        if let Some(doc) = doc {
            for id in ["toolbar-row", "toolbar-left-pre", "toolbar-right"] {
                if let Some(el) = doc.get_element_by_id(id) {
                    observer.observe(&el);
                    observed = true;
                }
            }
        }
        if observed {
            observer_handle.set_value(Some(observer));
            callback_handle.set_value(Some(callback));
        } else {
            // Nothing to watch (toolbar not in the DOM yet): drop the observer
            // rather than leaking it, and let the next effect run retry.
            observer.disconnect();
        }
    });

    // Disconnect BEFORE the Closure is dropped. The browser holds its own
    // reference to the wasm-bindgen shim, so a resize notification queued while
    // this component tears down would call into freed memory and abort the
    // runtime ("closure invoked recursively or after being dropped").
    on_cleanup(move || {
        if let Some(observer) = observer_handle.try_get_value().flatten() {
            observer.disconnect();
        }
        observer_handle.try_set_value(None);
        callback_handle.try_set_value(None);
    });

    // Reactive re-measure triggers. The centered page nav MOUNTS and UNMOUNTS
    // with document Ready (it is not observable while absent), and its width
    // grows with the page count's digits ("/ 9" vs "/ 1024"); a new document
    // changes both. The name itself is included so the first real name measures
    // against the settled toolbar.
    let status = state.doc.status;
    let num_pages = state.doc.num_pages;
    let title = state.doc.title;
    let path = state.doc.path;
    Effect::new(move |_| {
        _ = status.get();
        _ = num_pages.get();
        _ = title.get();
        _ = path.get();
        remeasure();
    });

    // The displayed name: a trustworthy `/Title`, else the file name from the
    // path (see core::filename for why the title cannot simply be believed).
    let name = move || {
        display_name(title.get().as_deref(), path.get().as_deref())
            .unwrap_or_else(|| "No document".to_string())
    };

    // Full name in the tooltip whenever it is (or could be) folded, so a
    // truncated name is always recoverable.
    let full = move || name();

    let hidden = move || avail.get().is_some_and(|w| w < MIN_LABEL_W);

    view! {
        <span
            id="toolbar-doc-title"
            class="min-w-0 shrink truncate text-sm text-ink"
            class=("hidden", hidden)
            title=full
            // `max-width` only ever CONSTRAINS: a name shorter than the budget
            // keeps its natural width and never shows an ellipsis. Before the
            // first measurement no cap is applied at all, so the very first
            // paint can't fold a name that fits.
            style:max-width=move || match avail.get() {
                Some(w) if w >= MIN_LABEL_W => format!("{}px", w.floor()),
                Some(_) => "0px".to_string(),
                None => "none".to_string(),
            }
        >
            {name}
        </span>
    }
}


