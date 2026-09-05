//! Search hits, painted over the type of the block row that renders them.
//!
//! A PDF's hits belong to the engine: it re-finds the query in the page's text
//! layer and appends one box per client rect (`public/engine/highlights.ts`),
//! because the engine owns that layer's whole lifecycle. A reflowable document
//! has no engine and no text layer — its rows are the reader's own components —
//! so the same algorithm runs here, in the row that renders the block:
//!
//! > find the query in the row's rendered text, take a `Range` per occurrence,
//! > and paint one box for each client rect that range reports.
//!
//! Two painters in two languages, one behaviour, and what can be shared is
//! shared: the box look is one rule set covering both subtrees
//! (`styles/components/search.css`), the markup is the engine's (`.highlight`,
//! `.is-active`, `data-match`), the cap on painted boxes is the engine's number,
//! and the identity of the match the reader has stepped to is the same kind of
//! pair — a PDF's is page + per-page ordinal, a reflowable document's is
//! `reader_core::search::BlockHit`, block + occurrence inside it. That pair is
//! what lets the active box be found with no geometry crossing the seam: this
//! row numbers its own occurrences in reading order, exactly as the search
//! numbered its hits.
//!
//! WHY ONE LAYER PER ROW, where the gloss strokes use one layer per page. A
//! stroke is placed from a stored identity and there are a handful of them, so a
//! page-level layer resolving each on a refresh is the cheap shape. Hits are the
//! opposite: a page of type can hold dozens, they exist only while a query does,
//! and in the continuous stream the unit that mounts and unmounts is the ROW —
//! where a page-level layer would have nothing to attach to, the same problem
//! the stream's gloss layer solves by covering the whole column. A row paints
//! its own hits, so a row that mounts has them and a row that unmounts drops
//! them, with no bookkeeping on either side.
//!
//! A Markdown block is searched as RENDERED, which is the only text a reader can
//! see: a hit the search found inside syntax the renderer drops — a link's URL,
//! a fence's info string — has nothing on screen to cover, so no box is painted
//! for it while the results list keeps counting it. Offsets are never exchanged
//! between the two sides, so a mismatch can only leave a box out; it cannot
//! misplace one.

use leptos::prelude::*;

use app_chrome::hooks::dom::{by_id, range_rects};

use super::spot::{match_spans, range_for_span};
use crate::components::viewer::page_host::block_row_id;
use crate::components::viewer::refresh::reflow_invalidation;
use crate::state::ReaderState;

/// Boxes one row will paint, mirroring the engine's cap on the boxes it paints
/// per page (`MAX_HIGHLIGHTS_PER_PAGE` in `public/engine/highlights.ts`). A
/// one-character query in a long paragraph is what this bounds, and the two
/// families keep the same number so a document reads the same either way.
const MAX_BOXES_PER_ROW: usize = 200;

/// One painted box, in its row's own CSS px.
///
/// Nothing is scaled on the way out: a reflowable row's type is scaled through
/// CSS custom properties, so the rects the browser reports are already the
/// zoomed pixels, and a box that sits over them wants exactly those numbers.
#[derive(Debug, Clone, Copy, PartialEq)]
struct HitBox {
    /// Which occurrence of the query in this block the box covers — the ordinal
    /// a `reader_core::search::BlockHit` names.
    occurrence: u32,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

#[component]
pub fn BlockSearchHits(
    state: ReaderState,
    /// The block whose row this layer covers. The row is looked up by the id
    /// scheme every reflowable row carries — the same lookup a gloss mark's
    /// projection makes.
    block: usize,
) -> impl IntoView {
    let boxes: RwSignal<Vec<HitBox>> = RwSignal::new(Vec::new());
    // Built once, outside the effect: a fingerprint signal made per run would be
    // a new reactive node per frame.
    let invalidation = reflow_invalidation(state);
    let row_id = block_row_id(block);

    Effect::new(move |_| {
        // Everything below is TRACKED: the walk re-runs when any of it moves.
        let query = state.search.query.get();
        // The COMMITTED scale, not the live one. A tween relays the layout out
        // every frame, and re-walking every mounted row per frame — each walk
        // forcing a layout the tween just dirtied — would cost the animation the
        // frames it is trying to hit. While a transition is in flight the boxes
        // stand down and come back at the commit, which is what the engine's own
        // boxes do: a zoom rebuilds the text layer they live in.
        let settled = state.viewer.zoom.committed.get();
        let mid_zoom = state.viewer.zoom.transition.get().is_some();
        // The cut's generation, the geometry it was cut with, the reading
        // column's width and the view mode: everything that moves type without
        // the reader scrolling.
        let moved = invalidation.get();
        let _ = (settled, moved);

        let needle = query.trim();
        if needle.is_empty() || mid_zoom {
            clear_if_painted(boxes);
            return;
        }
        if by_id(&row_id).is_none() {
            // Created but not attached: an element exists before it is in the
            // document, and `by_id` only finds what is. One frame from now it is
            // there, so the walk is retried once — giving up here would leave the
            // row bare until something invalidated it, and nothing invalidates
            // for a reader who is only scrolling.
            clear_if_painted(boxes);
            let (id, needle) = (row_id.clone(), needle.to_string());
            request_animation_frame(move || paint_row(&id, &needle, boxes));
            return;
        }
        paint_row(&row_id, needle, boxes);
    });

    // The occurrence in THIS block that the reader has stepped to, if any. Read
    // per box, so stepping through matches repaints classes and never re-walks
    // the DOM.
    let active_here = Signal::derive(move || {
        let index = state.search.active.get()?;
        let hit = state
            .search
            .matches
            .with(|matches| matches.get(index).and_then(|found| found.block_hit))?;
        (hit.block as usize == block).then_some(hit.occurrence)
    });

    view! {
        <div class="tx-hits" aria-hidden="true">
            {move || {
                boxes
                    .get()
                    .into_iter()
                    .map(|hit| {
                        let occurrence = hit.occurrence;
                        view! {
                            <div
                                class="highlight"
                                class=(
                                    "is-active",
                                    move || active_here.get() == Some(occurrence)
                                )
                                data-match=occurrence.to_string()
                                style=format!(
                                    "left:{}px;top:{}px;width:{}px;height:{}px",
                                    hit.left,
                                    hit.top,
                                    hit.width,
                                    hit.height
                                )
                            />
                        }
                    })
                    .collect::<Vec<_>>()
            }}
        </div>
    }
}

/// Walk `row_id`'s rendered text for `needle` and publish the boxes over it.
///
/// Deliberately free of signal READS: it is called from the effect above and
/// from the one-frame retry, and a retry that subscribed would turn a frame
/// callback into a reactive node of its own.
fn paint_row(row_id: &str, needle: &str, boxes: RwSignal<Vec<HitBox>>) {
    // A row that is not mounted has no text to cover — the same answer a gloss
    // stroke gets for a block the virtualizer has evicted, with the same
    // consequence: nothing is painted until it comes back.
    let Some(row) = by_id(row_id) else {
        clear_if_painted(boxes);
        return;
    };

    let origin = row.get_bounding_client_rect();
    let mut painted: Vec<HitBox> = Vec::new();
    for (occurrence, (start, end)) in match_spans(&row, needle).into_iter().enumerate() {
        // The cap is on BOXES, which is what the engine caps: a hit that wraps
        // lines reports one rect per fragment, so counting occurrences would
        // still let a row paint several times what a PDF page would.
        if painted.len() >= MAX_BOXES_PER_ROW {
            break;
        }
        let Some(range) = range_for_span(&row, start, end) else {
            continue;
        };
        for (left, top, right, bottom) in range_rects(&range) {
            if painted.len() >= MAX_BOXES_PER_ROW {
                break;
            }
            let (width, height) = (right - left, bottom - top);
            // A zero-sized fragment at a line-box edge is not a highlight.
            if width <= 0.0 || height <= 0.0 {
                continue;
            }
            painted.push(HitBox {
                occurrence: occurrence as u32,
                left: left - origin.left(),
                top: top - origin.top(),
                // A hairline match still gets a visible box.
                width: width.max(1.0),
                height: height.max(1.0),
            });
        }
    }
    // Compared before it is written: an unchanged walk — a re-measure that
    // re-cut nothing, a retry after a frame that moved nothing — must not
    // re-render every box in the row.
    if boxes.get_untracked() != painted {
        boxes.set(painted);
    }
}

/// Drop the painted boxes, but only if there are any: writing an empty list over
/// an empty list would notify the view for nothing, and this runs on every
/// invalidation while no search is open.
fn clear_if_painted(boxes: RwSignal<Vec<HitBox>>) {
    if !boxes.get_untracked().is_empty() {
        boxes.set(Vec::new());
    }
}
