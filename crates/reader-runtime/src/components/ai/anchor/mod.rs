//! Shared page-space anchor watchers: glue an anchor to the live page
//! host.

use ai_core::gloss::{GlossBox, GlossMark, ReflowSpot, mark_id};
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::state::ReaderState;

pub mod pdf;
pub mod reflow;
pub mod watch;

pub use pdf::{PdfAnchorBridge, capture_selection_mark};
// The invalidation fingerprints live in the viewer; re-exported here.
pub use crate::components::viewer::refresh::{layer_refresh, no_invalidation, reflow_invalidation};
pub use reflow::ReflowAnchorBridge;
pub use watch::{AnchorWatch, origin_outside_band, watch_page_anchor};

pub use ai_core::gloss::PageAnchor;

/// How an anchor's pixels are found now: a viewport box, or `None`
/// unmounted.
pub type MarkResolver = Callback<(PageAnchor, f64), Option<GlossBox>>;

/// The two questions a document format answers for the AI feature.
pub trait FormatAnchorBridge {
    /// The screen-space box of an anchor right now; `None` if its host is
    /// unmounted.
    fn screen_box(&self, anchor: &PageAnchor, scale: f64) -> Option<GlossBox>;
    /// Capture the current selection as an anchor for this format.
    fn capture(&self, scale: f64) -> Option<PageAnchor>;
}

/// The host element a page lives in.
pub use crate::components::viewer::page_host::host_id_for_mode;

/// The live selection's first range, and the element its start sits
/// in.
pub(super) fn selection_start() -> Option<(web_sys::Range, web_sys::Element)> {
    let selection = web_sys::window()?.get_selection().ok()??;
    if selection.is_collapsed() || selection.range_count() == 0 {
        return None;
    }
    let range = selection.get_range_at(0).ok()?;
    let node = range.start_container().ok()?;
    let el = node
        .parent_element()
        .or_else(|| node.dyn_into::<web_sys::Element>().ok())?;
    Some((range, el))
}

/// The screen box of one anchor, whichever format is open.
pub fn anchor_screen_box(
    state: ReaderState,
    anchor: &PageAnchor,
    spot: Option<ReflowSpot>,
    scale: f64,
) -> Option<GlossBox> {
    let mode = state.viewer.mode.get_untracked();
    if state.reflowable_now() {
        let bridge = ReflowAnchorBridge { state, spot, mode };
        return bridge.screen_box(anchor, scale);
    }
    let bridge = PdfAnchorBridge {
        mode,
        dom: state.dom,
    };
    bridge.screen_box(anchor, scale)
}

/// Build the resolver a watcher should use: the format dispatch.
pub fn anchor_resolver(state: ReaderState, spot: Signal<Option<ReflowSpot>>) -> MarkResolver {
    Callback::new(move |(anchor, scale): (PageAnchor, f64)| {
        // Read untracked: the watcher subscribes to the mark itself.
        let spot = spot.get_untracked();
        anchor_screen_box(state, &anchor, spot, scale)
    })
}

/// Build the resolver a stroke layer paints with, in the layer's
/// coordinates.
pub fn stroke_resolver(
    state: ReaderState,
    page: Option<u32>,
    host_id: Option<String>,
) -> Callback<(GlossMark, f64), Option<GlossBox>> {
    Callback::new(move |(mark, scale): (GlossMark, f64)| {
        if state.reflowable_now() {
            let mode = state.viewer.mode.get_untracked();
            let host = host_id.as_deref().and_then(|id| state.dom.by_id(id));
            // A reflowable mark belongs to its
            // block's page NOW, not the stored one.
            let spot = super::reflow_anchor::parse_spot(state.gloss.spots, &mark.context);
            if let Some(page) = page {
                let current = spot
                    .and_then(|s| {
                        super::reflow_anchor::page_of_block(state.document.content.reflow, s.block)
                    })
                    .unwrap_or(mark.page);
                if current != page {
                    return None;
                }
            }
            return super::reflow_anchor::stroke_box(
                state,
                spot,
                mode,
                host.as_ref(),
                Some(mark.rect),
            );
        }
        if page.is_some_and(|page| mark.page != page) {
            return None;
        }
        // A PDF's rect is already host-local at scale 1.
        let rect = mark.rect;
        Some(GlossBox {
            x: rect.x * scale,
            y: rect.y * scale,
            w: rect.w * scale,
            h: rect.h * scale,
            r: rect.r,
        })
    })
}

/// The one place a capture becomes a persisted mark.
pub fn captured_mark(
    word: impl Into<String>,
    context: impl Into<String>,
    anchor: PageAnchor,
) -> GlossMark {
    GlossMark {
        id: mark_id(anchor.page, js_sys::Date::now() as u64),
        word: word.into(),
        context: context.into(),
        anchor,
    }
}
