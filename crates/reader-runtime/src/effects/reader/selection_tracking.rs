//! Selection-detail tracking for the AI explain feature, in every
//! format.

use leptos::prelude::*;
use wasm_bindgen::JsValue;

use ai_core::gloss::PageAnchor;

use crate::components::ai::anchor::{FormatAnchorBridge, PdfAnchorBridge, ReflowAnchorBridge};
use crate::components::ai::reflow_anchor;
use crate::state::SelectionDetail;
use app_ui::components::primitives::hooks::use_custom_event::use_raw_event_from;

use crate::pane::origin::{Origin, origin_of};

/// The event detail's protocol: `null` (clear) or `SelectionDetail`.
fn parse_selection_detail(detail: &JsValue) -> Option<SelectionDetail> {
    if detail.is_null() || detail.is_undefined() {
        return None;
    }
    serde_wasm_bindgen::from_value::<SelectionDetail>(detail.clone()).ok()
}

/// The anchor for one selection, through the format that owns it.
fn anchor_for(
    detail: &SelectionDetail,
    state: crate::context::ReaderContext,
) -> Option<PageAnchor> {
    let reader = state.reader;
    let scale = reader.viewer.zoom.visual_scale();
    let mode = reader.viewer.mode.get_untracked();
    let reflow = detail.is_reflow();

    if reflow {
        // The tracker walked the offsets; project them onto the layout.
        if let Some(spot) = detail.spot
            && let Some(anchor) = reflow_anchor::anchor_of(reader, &spot)
        {
            return Some(anchor);
        }
    }

    // No spot to project: do the walk app-side instead.
    if reflow {
        let bridge = ReflowAnchorBridge {
            state: reader,
            spot: None,
            mode,
        };
        return bridge.capture(scale);
    }
    // A PDF's anchor is a page-space rect read off the live selection.
    let bridge = PdfAnchorBridge {
        mode,
        dom: reader.dom,
    };
    bridge.capture(scale)
}

/// Same routing as the page range: another pane clears the pill.
pub fn selection_tracking(state: crate::context::ReaderContext, active: Signal<bool>) {
    use_raw_event_from(
        app_ui::events::SELECTION_DETAIL_EVENT,
        move |detail, origin| {
            let active = active.try_get_untracked().unwrap_or(false);
            let mine = origin_of(&state.reader.dom, active, origin.as_ref()) == Origin::Mine;
            match parse_selection_detail(detail).filter(|_| mine) {
                Some(selection) => {
                    let anchor = anchor_for(&selection, state);
                    state.reader.ai_selection.anchor.set(anchor);
                    state.reader.ai_selection.detail.set(Some(selection));
                    // A new selection supersedes any open explanation.
                    state.reader.ai_selection.popover_open.set(false);
                }
                None => {
                    // Another pane's selection reaches here as a clear.
                    let sel = state.reader.ai_selection;
                    let held = sel.detail.with_untracked(Option::is_some)
                        || sel.anchor.with_untracked(Option::is_some)
                        || sel.popover_open.get_untracked();
                    if !held {
                        return;
                    }
                    state.reader.ai_selection.anchor.set(None);
                    state.reader.ai_selection.detail.set(None);
                    state.reader.ai_selection.popover_open.set(false);
                }
            }
        },
    );
}
