//! Where a reflowable gloss mark lives, and how its pixels are found again.
//!
//! A PDF mark stores a rect against a page host and is done: the page is fixed
//! pixels, so the rect is the identity. A plain-text or Markdown document has
//! no such fixed grid — a font-size change, a window resize, the measure column
//! settling onto real heights, all of them re-cut the pages — so a page-space
//! rect would drift onto whatever text moved under it. The identity that
//! survives every re-flow is the BLOCK the words sit in and how far into that
//! block's rendered text they start ([`ReflowSpot`]).
//!
//! This module owns the two halves of that deal:
//!
//! * the ENVELOPE — a spot, and the sentence that was around it, serialized
//!   into [`GlossMark::context`] behind a version tag, so the persisted schema
//!   stays the one `PageAnchor` shape and a PDF's plain-sentence context can
//!   never be mistaken for a spot;
//! * the PROJECTION — `block + [start, end)` back to viewport pixels, by
//!   asking the DOM: block → the row element rendering it (by id, and in the
//!   paginated modes only if that row is mounted under its page's host), then a
//!   real `Range` over the row's text nodes.
//!
//! Projection is deliberately never cached. It runs on the watcher's frame and
//! on the stroke layer's memo, both of which already re-run for scroll and
//! zoom, and a cached rect is exactly the thing a re-flow invalidates. The
//! ENVELOPE is the opposite case: it is persisted content that only ever
//! changes by being replaced, and it is re-read on every one of those frames,
//! so its parse is memoized ([`parse_spot`]) against the string it came from.
//!
//! The walk itself — a block's text nodes, the character offsets that address
//! them, and the `Range` a span becomes — is shared with everything else that
//! paints over a reflowable document's type, and lives in
//! [`crate::components::formats::reflow::spot`]. What stays here is the mark's
//! own arithmetic: [`union_box`] (client rects → one stroke box) and the
//! envelope above it.

use std::cell::RefCell;
use std::collections::HashMap;

use ai_core::gloss::{GlossBox, PageAnchor, ReflowSpot};
use leptos::prelude::*;
use reader_core::view::ViewMode;
use serde::{Deserialize, Serialize};

use super::anchor::host_id_for_mode;
use super::gloss::mark_layer::MARK_RADIUS;
use app_chrome::hooks::dom::range_rects;
use crate::components::formats::reflow::spot::{clamp_span, range_for_span};
use crate::components::viewer::page_host::block_row_id;
use crate::dom_contract::BLOCK_INDEX_ATTR;
use crate::state::reader::ReflowContent;
use crate::state::ReaderState;

/// Version tag on the envelope in [`GlossMark::context`]. Bump it if the
/// payload's meaning changes; an old mark then simply reads as having no spot
/// and falls back to its stored rect rather than projecting wrongly.
const SPOT_TAG: &str = "rf1:";

/// What a reflowable mark's `context` holds: the spot, and the sentence that
/// was around it when the mark was made.
///
/// The sentence has to ride along because `context` is the field the model is
/// handed to disambiguate the word (`ai_core::bridge::explain_word`), and a
/// mark is re-explained long after its selection is gone — from storage, from
/// a re-click on its stroke, after a restart. Storing the spot alone would
/// have meant sending the model a JSON envelope instead of prose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpotEnvelope {
    /// The durable identity: block, and the character range inside it.
    pub spot: ReflowSpot,
    /// The surrounding sentence, as captured. Empty for a mark whose envelope
    /// predates this field, which then explains from the word alone.
    #[serde(default)]
    pub text: String,
}

/// A mark's `context` as the string it is persisted with. Not a `Display` impl:
/// this is a serialization format with a version tag, and reading it as text
/// would make the tag look decorative.
pub fn spot_envelope(spot: &ReflowSpot, sentence: &str) -> String {
    let payload = SpotEnvelope { spot: *spot, text: sentence.trim().to_string() };
    format!("{SPOT_TAG}{}", serde_json::to_string(&payload).unwrap_or_default())
}

/// The whole envelope a mark carries, if it carries one.
fn parse_envelope(context: &str) -> Option<SpotEnvelope> {
    let payload = context.strip_prefix(SPOT_TAG)?;
    serde_json::from_str(payload).ok()
}

/// How far outside the viewport a stream row may sit and still be walked, as a
/// fraction of the viewport's height.
///
/// A row's box is the slot the virtualizer reserved for it, not a tight bound
/// on its text: the slot is sized from measured heights, but a row can still be
/// re-laid after its measurement (a font arriving, an image decoding) and its
/// last line can hang below the slot. A quarter screen of slack keeps the cull
/// from ever hiding a stroke that is actually on screen, which is the one
/// failure this could have, while still skipping every row the reader cannot
/// see.
const OFFSCREEN_SLACK: f64 = 0.25;

/// How many contexts are remembered before the memo is dropped whole.
///
/// One document's marks fit far inside this, so in practice a document parses
/// each of its envelopes once and then answers from the memo; the cap only
/// keeps a long session across many documents from growing it forever.
/// Clearing rather than evicting one entry keeps the hot path free of
/// bookkeeping, and the price of a clear is a handful of JSON parses.
const SPOT_CACHE_CAP: usize = 512;

// Parsed spots, memoized by the exact context string they came from. (A `//`
// block rather than a doc comment: rustdoc has nothing to attach a doc comment
// on a macro invocation to, and `-D warnings` says so.)
//
// Keying on content is what makes this safe: a `context` is write-once —
// `spot_envelope` produces it at capture and nothing edits it in place — so the
// same string always parses to the same spot, and a mark whose context is
// replaced simply arrives under a different key. `None` is cached too: a legacy
// or malformed context is exactly as stable as a good one, and re-testing it
// every frame is what the memo is here to stop.
thread_local! {
    static PARSED_SPOTS: RefCell<HashMap<String, Option<ReflowSpot>>> =
        RefCell::new(HashMap::new());
}

/// The spot a mark carries, if it carries one.
///
/// A PDF's context is a sentence, which never starts with the tag, so this is
/// `None` for every PDF mark. For a reflowable one it is `None` only when the
/// mark predates spots (or its offsets could not be walked at capture), and
/// such a mark has nothing durable to be placed by — see
/// [`super::anchor::ReflowAnchorBridge`].
///
/// This sits on the per-frame path: the stroke layer re-resolves every mark on
/// every scroll and zoom frame, and the mark-list watcher asks it once per mark
/// per tick, so it answers from a memo instead of re-parsing the JSON each
/// time. What is NOT memoized is the projection below it — that one has to
/// stay honest about the layout as it is right now.
pub fn parse_spot(context: &str) -> Option<ReflowSpot> {
    if let Some(hit) = PARSED_SPOTS.with(|cache| cache.borrow().get(context).copied()) {
        return hit;
    }
    let spot = parse_envelope(context).map(|envelope| envelope.spot);
    PARSED_SPOTS.with(|cache| {
        let mut memo = cache.borrow_mut();
        if memo.len() >= SPOT_CACHE_CAP {
            memo.clear();
        }
        memo.insert(context.to_string(), spot);
    });
    spot
}

/// The sentence to hand the model for a mark, whichever format made it: the
/// envelope's for a reflowable mark, the plain `context` for a PDF's.
///
/// An envelope with no sentence in it explains from the word alone rather than
/// falling back to the raw context, which is JSON and would only confuse the
/// model.
pub fn explain_context(mark: &ai_core::gloss::GlossMark) -> String {
    match parse_envelope(&mark.context) {
        Some(envelope) => envelope.text,
        None => mark.context.clone(),
    }
}

/// The page a block currently sits on, 1-based, or `None` before the document
/// has been paginated at all.
pub fn page_of_block(reflow: ReflowContent, block: usize) -> Option<u32> {
    reflow
        .block_page
        .with_untracked(|map| map.get(block).copied())
        .map(|page| page + 1)
}

/// The mounted element rendering `block`, or `None` when it is virtualized
/// away — the same answer a PDF gives for an unmounted page, with the same
/// consequence: the mark hides until the reader scrolls back to it.
///
/// The lookup is one id read. In the paginated modes it is answered only when
/// the row is mounted under the host this mode puts its page in, so a stale row
/// elsewhere in the document cannot speak for a block the reader is not looking
/// at; the continuous stream has no page hosts at all, so its rows answer
/// wherever they are mounted.
///
/// Both halves are special-cased rather than left to a general search, and the
/// reason is cost: this runs once per mark per refresh, and the stream's layer
/// refreshes on every scroll frame. The version this replaced built an
/// `[data-block-index='n']` selector per call, resolved the page's host id, and
/// ran a scoped `querySelector` that — in the stream, where no host exists —
/// always failed and was followed by a document-wide one. Two DOM searches and
/// two allocations per mark per frame, to reach the answer one id read gives.
fn block_node(state: ReaderState, block: usize, mode: ViewMode) -> Option<web_sys::Element> {
    // An id lookup, not a formatted attribute selector: this runs once per mark
    // per refresh, the stream's layer refreshes on every scroll frame, and
    // `querySelector` is the expensive half of this function. The rows carry
    // both handles — see `page_host::block_row_id` for why neither replaces the
    // other.
    let id = block_row_id(block);
    // The continuous stream renders one column of blocks with no page hosts in
    // it, so there is nothing to scope the lookup to — and a block's page is
    // meaningless there anyway (the stream is not paginated on screen). Every
    // other mode scopes to the host first, which keeps a row that is mounted
    // somewhere unexpected (a page mid-remount) from answering for a block the
    // reader is not looking at.
    let hostless = mode == ViewMode::ScrollVertical && state.reflowable_untracked();
    if !hostless {
        if let Some(page) = page_of_block(state.document.content.reflow, block) {
            // One lookup, not two: ask the row whether the host it is mounted
            // under is the one this mode puts its page in. A row that is mounted
            // somewhere else — a page mid-remount, a stale twin — answers `None`
            // and the mark hides, which is what a scoped `querySelector` on the
            // host used to say, without first fetching the host to search it.
            let scoped = format!("#{}", host_id_for_mode(mode, page));
            if let Some(row) = app_chrome::hooks::dom::by_id(&id) {
                if row.closest(&scoped).ok().flatten().is_some() {
                    return Some(row);
                }
            }
            return None;
        }
    }
    app_chrome::hooks::dom::by_id(&id)
}

/// The viewport box a set of client rects covers, as the five fields a mark's
/// stroke is painted with.
///
/// Pure over `(left, top, right, bottom)` tuples, so the union and the radius
/// rule are testable without a DOM. Degenerate fragments — the zero-width rect
/// a `Range` reports at a line-box edge — are ignored, and an empty set yields
/// `None` rather than an infinite box.
pub fn union_box(rects: &[(f64, f64, f64, f64)]) -> Option<GlossBox> {
    let mut left = f64::INFINITY;
    let mut top = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    let mut found = false;
    for &(l, t, r, b) in rects {
        if r - l <= 0.0 || b - t <= 0.0 {
            continue;
        }
        found = true;
        left = left.min(l);
        top = top.min(t);
        right = right.max(r);
        bottom = bottom.max(b);
    }
    if !found {
        return None;
    }
    let h = (bottom - top).max(1.0);
    Some(GlossBox {
        x: left,
        y: top,
        w: (right - left).max(1.0),
        h,
        r: MARK_RADIUS.min(h / 2.0),
    })
}

/// The viewport box a spot covers right now, in the reader's current view mode.
fn spot_screen_box(state: ReaderState, spot: &ReflowSpot) -> Option<GlossBox> {
    let mode = state.viewer.mode.get_untracked();
    spot_screen_box_in(state, spot, mode)
}

/// The viewport box a spot covers right now, or `None` when its block is not
/// mounted (or holds no text). This is the reflowable half of
/// [`super::anchor::anchor_screen_box`], and the whole reason a mark follows
/// its words across a re-pagination instead of staying where the pixels were.
///
/// The mode is passed in rather than read: a stroke layer is mounted per page
/// host and already knows which slot it is painting, and reading the viewer's
/// mode inside every mark's memo would subscribe the layer to a signal it has
/// no other use for.
pub fn spot_screen_box_in(
    state: ReaderState,
    spot: &ReflowSpot,
    mode: ViewMode,
) -> Option<GlossBox> {
    let el = block_node(state, spot.block, mode)?;
    // The stream keeps its whole window's worth of rows mounted and asks this
    // of every mark on every scroll frame, so the walk below is skipped for a
    // block that is nowhere near the viewport. That walk is the expensive half
    // of placing a mark — it clones out every text node's contents to count
    // characters, builds a `Range` and reads its client rects — and for a mark
    // a screenful away its answer was always `None`.
    //
    // Stream only. A paginated mode's rows are clipped and positioned by their
    // page host, where a row's own box does not bound its text, and only a
    // handful of hosts are mounted at a time anyway — there is nothing to win
    // and a wrong `None` to lose.
    if mode == ViewMode::ScrollVertical {
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            let viewport = document
                .document_element()
                .map_or(0.0, |root| root.client_height() as f64);
            if viewport > 0.0 {
                let slack = viewport * OFFSCREEN_SLACK;
                let rect = el.get_bounding_client_rect();
                if rect.height() == 0.0 || rect.bottom() < -slack || rect.top() > viewport + slack {
                    return None;
                }
            }
        }
    }
    let range = range_for_span(&el, spot.start, spot.end)?;
    union_box(&range_rects(&range))
}

/// A live selection's spot and the page it sits on, for a reflowable document.
///
/// The engine's tracker does the same walk in TypeScript and ships the spot
/// with the selection event, which is the path a normal selection takes: it
/// already has the range, and doing it once keeps the two from disagreeing.
/// This is the app-side capture, for the paths that need a spot without an
/// event to hand.
pub fn capture_selection(state: ReaderState) -> Option<(ReflowSpot, PageAnchor)> {
    let (range, el) = super::anchor::selection_start()?;
    let row = el.closest(&format!("[{BLOCK_INDEX_ATTR}]")).ok().flatten()?;
    let block = row
        .get_attribute(BLOCK_INDEX_ATTR)
        .and_then(|value| value.parse::<usize>().ok())?;
    let spot = spot_of_range(&range, &row, block)?;
    Some((spot, anchor_of(state, &spot)?))
}

/// The spot a live range covers inside its block row: the characters before
/// the range's start, and the characters it spans.
///
/// Both are measured in the row's own rendered text (`textContent`), which is
/// the same coordinate system [`range_for_span`] walks later — and the reason
/// a Markdown mark stays put even though its source syntax is not rendered.
fn spot_of_range(range: &web_sys::Range, row: &web_sys::Element, block: usize) -> Option<ReflowSpot> {
    let total = row.text_content().unwrap_or_default().chars().count();
    if total == 0 {
        return None;
    }
    let before = range.clone_range();
    // `row` contains the range's start by construction (it is the ancestor the
    // start container was found through), so this cannot fail; the Result is
    // the DOM's, not a condition worth branching on.
    let _ = before.select_node_contents(row);
    before
        .set_end(&range.start_container().ok()?, range.start_offset().ok()?)
        .ok()?;
    // `Range::to_string` hands back the JS `String` object; the counts have to
    // be in CHARACTERS, and `JsString` is UTF-16, so the conversion through
    // `String` is what makes an emoji or a combining mark one character here
    // and one character in the engine's tracker too.
    let start = String::from(before.to_string()).chars().count();
    let span = String::from(range.to_string()).chars().count();
    let (start, end) = clamp_span(start, start + span, total);
    Some(ReflowSpot::new(block, start, end))
}

/// The anchor for a spot: the page it now sits on, plus the viewport box it
/// covers right now.
///
/// The rect is a FALLBACK, not the identity — the spot is. A reflowable document
/// has no durable page-space grid to store pixels against (that is the entire
/// reason the spot exists), so this is the box at the moment of capture. It is
/// read back only by a stroke layer serving a mark that carries no spot at all;
/// everything else re-derives from the spot. Dedup compares spots, never these
/// rects (see `crate::components::ai::gloss::controller::commands`).
pub fn anchor_of(state: ReaderState, spot: &ReflowSpot) -> Option<PageAnchor> {
    // A block the cut has not placed yet answers page 1 rather than nothing —
    // the same leniency a search hit gets (`effects::reader::search`), because
    // the box below is what actually locates the selection, and refusing an
    // anchor here would refuse the Explain pill over text that is plainly on
    // screen. It happens only in the gap between a document opening and its
    // first measure pass.
    let page = page_of_block(state.document.content.reflow, spot.block).unwrap_or(1);
    let rect = spot_screen_box(state, spot)?;
    Some(PageAnchor { page, rect })
}

/// The box a reflowable stroke paints, in its own layer's coordinates.
///
/// A layer is always `position:absolute; inset:0` inside the element its
/// resolver measured, so the viewport box loses that element's origin here: a
/// `.tx-page` for a paginated mode, and the stream's scroller box for the
/// continuous one, where a single layer serves the whole reading column.
/// `host` is `None` only for a caller that wants the viewport box itself.
///
/// Neither case divides by the scale: a reflowable page's type is scaled
/// through CSS custom properties, so `getBoundingClientRect` already reports
/// the zoomed pixels, which is exactly what a stroke sitting over them needs.
///
/// `fallback` is the box the mark was captured with, kept for a mark that
/// carries no spot at all (one made before spots existed). It is a viewport
/// snapshot, so it is honest only while the layout has not moved — a mark whose
/// spot cannot be resolved hides instead, which is what the reader wants when it
/// is being asked where words are that are no longer there.
pub fn stroke_box(
    state: ReaderState,
    spot: Option<ReflowSpot>,
    mode: ViewMode,
    host: Option<&web_sys::Element>,
    fallback: Option<GlossBox>,
) -> Option<GlossBox> {
    let local = |b: GlossBox| match host {
        Some(host) => {
            let hr = host.get_bounding_client_rect();
            GlossBox { x: b.x - hr.left(), y: b.y - hr.top(), w: b.w, h: b.h, r: b.r }
        }
        None => b,
    };
    let Some(spot) = spot else {
        return fallback.map(local);
    };
    // A spot that cannot be resolved — a block virtualized away, or one a
    // re-parse orphaned — yields no stroke at all. That is not a dead mark: the
    // reader will scroll back, and the fallback box from capture time would
    // only paint a stroke over whatever text is there now.
    spot_screen_box_in(state, &spot, mode).map(local)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_envelope_round_trips_and_rejects_everything_else() {
        let spot = ReflowSpot::new(4, 12, 19);
        let envelope = spot_envelope(&spot, "  a manuscript page, scraped clean  ");
        assert!(envelope.starts_with(SPOT_TAG));
        assert_eq!(parse_spot(&envelope), Some(spot));

        // A PDF's context is a sentence, and must never read as a spot.
        assert_eq!(parse_spot("a manuscript page, scraped clean"), None);
        assert_eq!(parse_spot(""), None);
        // A tagged but corrupt payload is no spot, not a panic.
        assert_eq!(parse_spot("rf1:{\"spot\":}"), None);
        // An older envelope version is not this one's payload.
        assert_eq!(parse_spot("rf0:{\"block\":1,\"start\":0,\"end\":2}"), None);
    }

    #[test]
    fn the_envelope_carries_the_sentence_the_model_is_handed() {
        use ai_core::gloss::{GlossBox, GlossMark, PageAnchor};

        let mark = |context: &str| GlossMark {
            id: "g1".to_string(),
            word: "palimpsest".to_string(),
            context: context.to_string(),
            anchor: PageAnchor { page: 1, rect: GlossBox::default() },
        };

        // Trimmed on the way in, so the envelope never stores the ragged edges
        // a double-clicked selection brings with it.
        let reflow = mark(&spot_envelope(&ReflowSpot::new(1, 0, 10), " scraped clean "));
        assert_eq!(explain_context(&reflow), "scraped clean");

        // A PDF's mark keeps its bare sentence.
        let pdf = mark("a manuscript page, scraped clean");
        assert_eq!(explain_context(&pdf), "a manuscript page, scraped clean");

        // An envelope from before the sentence travelled with it explains from
        // what is there rather than failing: `text` is `#[serde(default)]`.
        let legacy = mark("rf1:{\"spot\":{\"block\":1,\"start\":0,\"end\":2}}");
        assert_eq!(parse_spot(&legacy.context), Some(ReflowSpot::new(1, 0, 2)));
        assert_eq!(explain_context(&legacy), "");
    }

    #[test]
    fn the_union_covers_every_fragment_and_ignores_degenerate_ones() {
        let rects = [
            (100.0, 10.0, 150.0, 24.0),
            // A zero-width fragment at a line-box edge contributes nothing.
            (150.0, 10.0, 150.0, 24.0),
            (20.0, 26.0, 90.0, 40.0),
        ];
        let union = union_box(&rects).expect("a real fragment");
        assert_eq!((union.x, union.y), (20.0, 10.0));
        assert_eq!((union.w, union.h), (130.0, 30.0));
        // The stroke's radius rule, shared with the PDF's page-space rects.
        assert_eq!(union.r, MARK_RADIUS.min(union.h / 2.0));
    }

    #[test]
    fn a_selection_of_only_degenerate_fragments_projects_to_nothing() {
        assert!(union_box(&[]).is_none());
        assert!(union_box(&[(5.0, 5.0, 5.0, 5.0)]).is_none());
        assert!(union_box(&[(5.0, 5.0, 9.0, 5.0)]).is_none());
    }

    #[test]
    fn a_hairline_fragment_still_gets_a_stroke_worth_of_box() {
        let union = union_box(&[(10.0, 10.0, 10.4, 11.0)]).expect("non-degenerate");
        assert!(union.w >= 1.0 && union.h >= 1.0);
        assert_eq!(union.r, MARK_RADIUS.min(union.h / 2.0));
    }
}
