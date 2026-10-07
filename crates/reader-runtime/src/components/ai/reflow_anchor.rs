//! Where a reflowable gloss mark lives, and how its pixels are
//! found again.

use ai_core::gloss::{GlossBox, PageAnchor, ReflowSpot};
use leptos::prelude::*;
use reader_core::view::ViewMode;
use serde::{Deserialize, Serialize};

use super::anchor::host_id_for_mode;
use super::gloss::mark_layer::MARK_RADIUS;
use crate::components::formats::reflow::spot::{clamp_span, range_for_span};
use crate::components::viewer::page_host::block_row_id;
use crate::state::ReaderState;
use crate::state::ReflowContent;
use crate::state::gloss::SpotMemo;
use app_chrome::hooks::dom::range_rects;
use app_state::dom_contract::BLOCK_INDEX_ATTR;
use app_ui::theme_paint::document_element;

/// Version tag on the envelope in `context`; an older tag reads as no
/// spot.
const SPOT_TAG: &str = "rf1:";

/// A reflowable mark's `context`: the spot, and the sentence around it
/// at capture time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpotEnvelope {
    /// The durable identity: block, and the character range inside it.
    pub spot: ReflowSpot,
    /// The surrounding sentence; empty for a mark that predates the field.
    #[serde(default)]
    pub text: String,
}

/// A mark's `context` as persisted; the tag makes it a format, not text.
pub fn spot_envelope(spot: &ReflowSpot, sentence: &str) -> String {
    let payload = SpotEnvelope {
        spot: *spot,
        text: sentence.trim().to_string(),
    };
    format!(
        "{SPOT_TAG}{}",
        serde_json::to_string(&payload).unwrap_or_default()
    )
}

/// The whole envelope a mark carries, if it carries one.
fn parse_envelope(context: &str) -> Option<SpotEnvelope> {
    let payload = context.strip_prefix(SPOT_TAG)?;
    serde_json::from_str(payload).ok()
}

/// How far outside the viewport a stream row may still be walked.
const OFFSCREEN_SLACK: f64 = 0.25;

/// The spot a mark carries, from the pane's memo on the per-frame
/// path.
pub fn parse_spot(memo: SpotMemo, context: &str) -> Option<ReflowSpot> {
    if let Some(hit) = memo.get(context) {
        return hit;
    }
    let spot = read_spot(context);
    memo.insert(context, spot);
    spot
}

/// The spot a context carries, parsed with no memo, off the frame path.
pub fn read_spot(context: &str) -> Option<ReflowSpot> {
    parse_envelope(context).map(|envelope| envelope.spot)
}

/// The sentence to hand the model: the envelope's, or a PDF's plain
/// `context`.
pub fn explain_context(mark: &ai_core::gloss::GlossMark) -> String {
    match parse_envelope(&mark.context) {
        Some(envelope) => envelope.text,
        None => mark.context.clone(),
    }
}

/// The page a block sits on, 1-based, or `None` before pagination.
pub fn page_of_block(reflow: ReflowContent, block: usize) -> Option<u32> {
    reflow
        .block_page
        .with_untracked(|map| map.get(block).copied())
        .map(|page| page + 1)
}

/// The mounted element rendering `block`, or `None` when it is
/// virtualized away.
fn block_node(state: ReaderState, block: usize, mode: ViewMode) -> Option<web_sys::Element> {
    // An id lookup, scoped to THIS pane's root, so no twin row answers.
    let id = block_row_id(block);
    // The continuous stream has no page hosts to scope the lookup to.
    let hostless = mode == ViewMode::ScrollVertical && state.reflowable_now();
    if !hostless && let Some(page) = page_of_block(state.document.content.reflow, block) {
        // One lookup: a row under the wrong host answers `None`, like a
        // scoped search.
        let scoped = format!("#{}", host_id_for_mode(mode, page));
        if let Some(row) = state.dom.by_id(&id)
            && row.closest(&scoped).ok().flatten().is_some()
        {
            return Some(row);
        }
        return None;
    }
    state.dom.by_id(&id)
}

/// The viewport box a set of client rects covers; degenerate fragments
/// are ignored.
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

/// The viewport box a spot covers now, or `None` when its block is not
/// mounted.
pub fn spot_screen_box_in(
    state: ReaderState,
    spot: &ReflowSpot,
    mode: ViewMode,
) -> Option<GlossBox> {
    let el = block_node(state, spot.block, mode)?;
    // A block nowhere near the viewport skips the expensive walk.
    if mode == ViewMode::ScrollVertical {
        let viewport = document_element().map_or(0.0, |root| root.client_height() as f64);
        if viewport > 0.0 {
            let slack = viewport * OFFSCREEN_SLACK;
            let rect = el.get_bounding_client_rect();
            if rect.height() == 0.0 || rect.bottom() < -slack || rect.top() > viewport + slack {
                return None;
            }
        }
    }
    let range = range_for_span(&el, spot.start, spot.end)?;
    union_box(&range_rects(&range))
}

/// A live selection's spot and page, for a reflowable document.
pub fn capture_selection(state: ReaderState) -> Option<(ReflowSpot, PageAnchor)> {
    let (range, el) = super::anchor::selection_start()?;
    let row = el
        .closest(&format!("[{BLOCK_INDEX_ATTR}]"))
        .ok()
        .flatten()?;
    let block = row
        .get_attribute(BLOCK_INDEX_ATTR)
        .and_then(|value| value.parse::<usize>().ok())?;
    let spot = spot_of_range(&range, &row, block)?;
    Some((spot, anchor_of(state, &spot)?))
}

/// The spot a live range covers in its row: the characters before
/// and inside it.
fn spot_of_range(
    range: &web_sys::Range,
    row: &web_sys::Element,
    block: usize,
) -> Option<ReflowSpot> {
    let total = row.text_content().unwrap_or_default().chars().count();
    if total == 0 {
        return None;
    }
    let before = range.clone_range();
    // `row` contains the range's start by construction, so this cannot
    // fail.
    let _ = before.select_node_contents(row);
    before
        .set_end(&range.start_container().ok()?, range.start_offset().ok()?)
        .ok()?;
    // Counts are CHARACTERS, so an emoji is one character here and in
    // the engine.
    let start = String::from(before.to_string()).chars().count();
    let span = String::from(range.to_string()).chars().count();
    let (start, end) = clamp_span(start, start + span, total);
    Some(ReflowSpot::new(block, start, end))
}

/// The anchor for a spot: the page it sits on, plus the viewport box
/// now.
pub fn anchor_of(state: ReaderState, spot: &ReflowSpot) -> Option<PageAnchor> {
    // A block the cut has not placed answers page 1, like a search hit.
    let page = page_of_block(state.document.content.reflow, spot.block).unwrap_or(1);
    let rect = spot_screen_box(state, spot)?;
    Some(PageAnchor { page, rect })
}

/// The box a reflowable stroke paints in its layer's coordinates.
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
            GlossBox {
                x: b.x - hr.left(),
                y: b.y - hr.top(),
                w: b.w,
                h: b.h,
                r: b.r,
            }
        }
        None => b,
    };
    let Some(spot) = spot else {
        return fallback.map(local);
    };
    // An unresolvable spot yields no stroke; the capture-time box would
    // paint over other text.
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
        assert_eq!(read_spot(&envelope), Some(spot));

        // A PDF's context is a sentence, and must never read as a spot.
        assert_eq!(read_spot("a manuscript page, scraped clean"), None);
        assert_eq!(read_spot(""), None);
        // A tagged but corrupt payload is no spot, not a panic.
        assert_eq!(read_spot("rf1:{\"spot\":}"), None);
        // An older envelope version is not this one's payload.
        assert_eq!(read_spot("rf0:{\"block\":1,\"start\":0,\"end\":2}"), None);
    }

    #[test]
    fn each_pane_keeps_its_own_spot_memo() {
        let owner = Owner::new();
        owner.with(|| {
            let spot = ReflowSpot::new(2, 3, 9);
            let envelope = spot_envelope(&spot, "a line");
            let (a, b) = (SpotMemo::default(), SpotMemo::default());
            assert_eq!(parse_spot(a, &envelope), Some(spot));
            assert_eq!(parse_spot(a, "a plain sentence"), None);
            assert_eq!(a.len(), 2, "hits and misses are both remembered");
            assert!(b.is_empty(), "another pane's memo is untouched");
            assert_eq!(parse_spot(b, &envelope), Some(spot));
            // One pane's clear (its document closed) leaves the other's.
            a.clear();
            assert!(a.is_empty());
            assert_eq!(b.len(), 1);
            assert_eq!(b.get(&envelope), Some(Some(spot)));
        });
    }

    #[test]
    fn the_envelope_carries_the_sentence_the_model_is_handed() {
        use ai_core::gloss::{GlossBox, GlossMark, PageAnchor};

        let mark = |context: &str| GlossMark {
            id: "g1".to_string(),
            word: "palimpsest".to_string(),
            context: context.to_string(),
            anchor: PageAnchor {
                page: 1,
                rect: GlossBox::default(),
            },
        };

        // Trimmed on the way in, so ragged selection edges are never stored.
        let reflow = mark(&spot_envelope(
            &ReflowSpot::new(1, 0, 10),
            " scraped clean ",
        ));
        assert_eq!(explain_context(&reflow), "scraped clean");

        // A PDF's mark keeps its bare sentence.
        let pdf = mark("a manuscript page, scraped clean");
        assert_eq!(explain_context(&pdf), "a manuscript page, scraped clean");

        // An envelope from before the sentence explains from an empty text.
        let legacy = mark("rf1:{\"spot\":{\"block\":1,\"start\":0,\"end\":2}}");
        assert_eq!(read_spot(&legacy.context), Some(ReflowSpot::new(1, 0, 2)));
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
