//! Selection detail tracking for the AI explain feature, in every format.
//!
//! The engine's selectionchange listener debounces the native selection,
//! measures its bounding rect, grabs the surrounding sentence, and dispatches
//! a `pdfreader:selection-detail` CustomEvent with
//! `{ text, context, rect, host, spot }` (rect in viewport CSS px, host the
//! format family that painted it, spot a reflowable selection's durable
//! identity) — or `null` to clear. Collapses caused by pressing inside the AI
//! UI are suppressed engine-side, so the "Explain" button survives its own
//! click.
//!
//! This effect is the single place that turns the event into writes on
//! `state.reader.ai_selection`: `detail` carries the text/context, `anchor`
//! is the origin the floating pill follows, and a genuine clear also closes
//! an open popover.
//!
//! It also decides WHICH pipeline anchors a selection — from the event, not
//! the open document: the tracker reports the host family the selection is
//! actually in, so a selection outliving a document switch cannot be
//! projected through the wrong format's maths. A PDF's anchor is its
//! page-space rect; a reflowable one is a block and character range asked of
//! the DOM again (`crate::components::ai::reflow_anchor`).

use leptos::prelude::*;
use wasm_bindgen::JsValue;

use ai_core::gloss::PageAnchor;

use crate::components::ai::anchor::{FormatAnchorBridge, PdfAnchorBridge, ReflowAnchorBridge};
use crate::components::ai::reflow_anchor;
use crate::state::AppState;
use crate::state::reader::SelectionDetail;

/// The JS protocol of the event detail: `null` (clear) or a full
/// `SelectionDetail`. The engine already debounces and dedupes, so every
/// event arriving here is a genuine change (a `.set()` always notifies, even
/// on unchanged values).
fn parse_selection_detail(detail: &JsValue) -> Option<SelectionDetail> {
    if detail.is_null() || detail.is_undefined() {
        return None;
    }
    serde_wasm_bindgen::from_value::<SelectionDetail>(detail.clone()).ok()
}

/// The anchor for one selection, through the format that owns it.
fn anchor_for(detail: &SelectionDetail, state: AppState) -> Option<PageAnchor> {
    let reader = state.reader;
    let scale = reader.viewer.zoom.visual_scale();
    let mode = reader.viewer.mode.get_untracked();
    let reflow = detail.is_reflow();

    if reflow {
        // The tracker walked the offsets out of the range while it had it; the
        // app's job is to project them onto the layout as it stands now.
        if let Some(spot) = detail.spot {
            if let Some(anchor) = reflow_anchor::anchor_of(reader, &spot) {
                return Some(anchor);
            }
        }
    }

    // No spot to project. For a reflowable selection that means the tracker
    // could not walk the offsets (a selection inside something that is not
    // document text, or one already collapsed): do the same walk app-side —
    // the bridge's own capture path.
    if reflow {
        let bridge = ReflowAnchorBridge { state: reader, spot: None, mode };
        return bridge.capture(scale);
    }
    // A PDF's anchor is a page-space rect read off the live selection.
    let bridge = PdfAnchorBridge { mode };
    bridge.capture(scale)
}

pub fn selection_tracking(state: AppState) {
    let _handle = window_event_listener(
        leptos::ev::Custom::new(crate::events::SELECTION_DETAIL_EVENT),
        move |ev: web_sys::CustomEvent| {
            let detail = ev.detail();
            match parse_selection_detail(&detail) {
                Some(selection) => {
                    let anchor = anchor_for(&selection, state);
                    state.reader.ai_selection.anchor.set(anchor);
                    state.reader.ai_selection.detail.set(Some(selection));
                    // A new selection supersedes any open explanation.
                    state.reader.ai_selection.popover_open.set(false);
                }
                None => {
                    state.reader.ai_selection.anchor.set(None);
                    state.reader.ai_selection.detail.set(None);
                    state.reader.ai_selection.popover_open.set(false);
                }
            }
        },
    );
    on_cleanup(move || _handle.remove());
}
