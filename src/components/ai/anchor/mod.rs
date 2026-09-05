//! Shared page-space anchor watchers: glue a [`PageAnchor`] to the live page
//! host so the selection Explain pill and the gloss card both follow scroll/zoom
//! and die when their origin leaves a configurable band of the viewport.
//!
//! The pure data type lives in `ai_core::gloss::PageAnchor` so state can hold
//! it without depending on the component layer.
//!
//! # Formats
//!
//! A page-space rect is the PDF's honest answer to "where is this word", and
//! it is only half an answer for a document that re-lays itself out: a
//! reflowable mark's durable identity is a block and a character range
//! ([`ReflowSpot`]), and its pixels have to be asked of the DOM again every
//! time anything moves. Rather than fork the watchers per format, this module
//! defines the two questions a format has to answer —
//! [`FormatAnchorBridge::screen_box`] and [`FormatAnchorBridge::capture`] —
//! and hands them down as one [`MarkResolver`] callback. The card, the pill,
//! the stroke layer, the spring and the persistence are all format-blind; a
//! new format adds a bridge and a mark-layer mount, and nothing else.
//!
//! # Modules
//!
//!   * [`pdf`] / [`reflow`] — the two bridges, and the projection each one needs.
//!   * [`refresh`] — the fingerprints a stroke layer re-derives on.
//!   * [`watch`] — the glued-to-the-page watcher and its exit bands.
//!
//! What stays here is the shared half: the resolver type and the trait both
//! bridges implement, the format dispatch ([`anchor_screen_box`],
//! [`anchor_resolver`], [`stroke_resolver`]), the selection walk both capture
//! paths start from ([`selection_start`]) and the one place a capture becomes a
//! persisted mark ([`captured_mark`]). Everything the rest of the app names is
//! re-exported, so `crate::components::ai::anchor::X` addresses all of it.

use ai_core::gloss::{mark_id, GlossBox, GlossMark, ReflowSpot};
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app_chrome::hooks::dom::by_id;
use crate::state::ReaderState;

pub mod pdf;
pub mod reflow;
pub mod watch;

pub use pdf::{capture_selection_mark, PdfAnchorBridge};
// The invalidation fingerprints are a viewer concern — what makes type move —
// and live there now; re-exported here because every watcher and stroke layer
// in this feature reaches for them through this module.
pub use crate::components::viewer::refresh::{layer_refresh, no_invalidation, reflow_invalidation};
pub use reflow::ReflowAnchorBridge;
pub use watch::{
    origin_outside_band, watch_page_anchor, AnchorWatch, CARD_EXIT_FRAC, PILL_EXIT_FRAC,
};

pub use ai_core::gloss::PageAnchor;

/// How an anchor's pixels are found right now. Returns the box in VIEWPORT CSS
/// px, or `None` when the anchor's host is not mounted (virtualized away) —
/// which every consumer already reads as "the anchor left the page".
///
/// The `f64` is the display scale. The PDF multiplies its stored page-space
/// rect by it; a reflowable format ignores it, because its type is scaled
/// through CSS custom properties and `getBoundingClientRect` already reports
/// the scaled result.
pub type MarkResolver = Callback<(PageAnchor, f64), Option<GlossBox>>;

/// The two questions a document format answers for the AI feature.
///
/// Both are about live layout, which is why the trait lives in the app and not
/// in `ai-core`: the crate keeps the serializable identity (a page and a rect,
/// a block and a character range) and stays free of the DOM and of Leptos.
pub trait FormatAnchorBridge {
    /// The screen-space box of an anchor right now; `None` if its host is
    /// unmounted.
    fn screen_box(&self, anchor: &PageAnchor, scale: f64) -> Option<GlossBox>;
    /// Capture the current DOM selection as an anchor for this format.
    ///
    /// A format whose identity needs information the DOM alone cannot give
    /// (a reflowable mark needs the block index and the character offsets,
    /// which the engine's selection tracker walks out of the range) answers
    /// `None` here and is captured through its own path instead.
    fn capture(&self, scale: f64) -> Option<PageAnchor>;
}

/// The host element a page lives in — the reader's page identity in the DOM.
///
/// Defined by the page host that paints it, and re-exported here because both
/// sides need the same string: the ids are one scheme shared by rasters and type,
/// which is what lets a selection anchor find a page of Markdown as readily as a
/// page of pixels.
pub use crate::components::viewer::page_host::host_id_for_mode;

/// The live selection's first range, and the element its start sits in.
///
/// Both capture paths begin here — the page-space one
/// ([`crate::components::ai::anchor::pdf::capture_selection`]) and the
/// reflowable one
/// ([`crate::components::ai::reflow_anchor::capture_selection`]) — and differ
/// only in what they walk UP to afterwards: a page host for one, a block row
/// for the other. Sharing the walk is what keeps the half that is not
/// format-specific from drifting, and that half is where the awkward cases
/// are: a collapsed selection, an empty one, and a start container that is a
/// text node rather than an element.
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

/// The screen box of one anchor, whichever format is open — the resolver the
/// card and the Explain pill watch through.
///
/// `spot` is the reflowable identity carried beside the anchor (a mark's
/// envelope, or the selection event's); it is ignored for a PDF, whose anchor
/// already is its identity. The view mode is read untracked: the watchers
/// subscribe to it themselves, so a mode flip already re-derives the box.
pub fn anchor_screen_box(
    state: ReaderState,
    anchor: &PageAnchor,
    spot: Option<ReflowSpot>,
    scale: f64,
) -> Option<GlossBox> {
    let mode = state.viewer.mode.get_untracked();
    if state.reflowable_untracked() {
        let bridge = ReflowAnchorBridge { state, spot, mode };
        return bridge.screen_box(anchor, scale);
    }
    let bridge = PdfAnchorBridge { mode };
    bridge.screen_box(anchor, scale)
}

/// Build the resolver a watcher should use: the format dispatch, decided per
/// call rather than per mount, because the document can change under a
/// component that outlives it. `spot_of` reads the reflowable identity of
/// whichever anchor is current (a mark's envelope, a selection's event detail).
pub fn anchor_resolver(state: ReaderState, spot: Signal<Option<ReflowSpot>>) -> MarkResolver {
    Callback::new(move |(anchor, scale): (PageAnchor, f64)| {
        // Read untracked: the watcher subscribes to the mark itself, and a
        // second subscription here would only re-derive the same box twice.
        let spot = spot.get_untracked();
        anchor_screen_box(state, &anchor, spot, scale)
    })
}

/// Build the resolver a stroke layer paints with: the same format dispatch as
/// [`anchor_resolver`], but answered in the LAYER's coordinates rather than the
/// viewport's, because a layer mounted inside a page host positions its strokes
/// against that host.
///
/// `page` is the host's own page, and the filter that keeps a page from
/// painting another page's marks; `None` is the continuous stream's
/// viewport-level layer, which has no page of its own and lets the resolver
/// decide what is on screen. `host_id` names the element those coordinates are
/// relative to (`None` for the viewport-level layer), and is resolved per call
/// rather than captured: the host may not be in the DOM yet when the layer is
/// constructed, and a remount replaces it.
pub fn stroke_resolver(
    state: ReaderState,
    page: Option<u32>,
    host_id: Option<String>,
) -> Callback<(GlossMark, f64), Option<GlossBox>> {
    Callback::new(move |(mark, scale): (GlossMark, f64)| {
        if state.reflowable_untracked() {
            let mode = state.viewer.mode.get_untracked();
            let host = host_id.as_deref().and_then(by_id);
            // A reflowable mark belongs to whichever page its block sits on
            // NOW, which a re-cut can move; the stored page number is the one
            // it was captured under.
            let spot = super::reflow_anchor::parse_spot(&mark.context);
            if let Some(page) = page {
                let current = spot
                    .and_then(|s| {
                        super::reflow_anchor::page_of_block(
                            state.document.content.reflow,
                            s.block,
                        )
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
        // A PDF's rect is already host-local at scale 1, so its stroke is the
        // stored rect times the display scale — no DOM read, and no
        // round-trip through the viewport that would only subtract the host's
        // origin back off again.
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
///
/// The id scheme is `ai_core::gloss::mark_id`'s, and this is the only caller
/// that supplies it with a clock: the crate's gloss half is pure, so reading
/// the millisecond stamp belongs to the app side of the seam. Every capture
/// path goes through here — the Explain pill's click with an anchor in hand, its
/// fallback that walks the live range, and the reflowable one that has a spot
/// envelope to carry — so a mark cannot end up with an id no reader can
/// address, which is what three separately formatted `g{page}-{now}` literals
/// were one refactor away from.
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
