//! Card targeting: where the sprung box wants to be right now. The heavy
//! lifting — side choice, viewport clamping, shrink-to-fit — lives in
//! [`pdf_core::gloss::place_card`] so it is unit-testable on the host.

use leptos::prelude::*;
use pdf_core::gloss::{GlossBox, place_card};

use crate::components::ai::types::GlossPhase;
use crate::components::primitives::floating::types::{clamp_point_to_viewport, Point, Size};

/// Preferred card width before viewport clamping.
pub const CARD_WIDTH: f64 = 360.0;
/// Card corner radius — the spring morphs the chip's pill into this. A
/// standard card radius rather than an oversized one: the card is a panel,
/// not a bubble.
const CARD_RADIUS: f64 = 12.0;
/// Gap between the highlighter stroke and the card's near edge.
const CARD_GAP: f64 = 16.0;
/// Floor for the card's expanded content height. While the twin has not been
/// measured yet `content_height` reads `0.0`, which would size the card's
/// target to the anchor box (a flash of collapsed card on first open). The
/// floor is the shimmer's resting height, so a not-yet-measured card opens at
/// about the size the loading state occupies rather than at nothing.
pub const MIN_CARD_CONTENT_H: f64 = 120.0;
/// Viewport margin the expanded card must stay inside.
pub const CARD_MARGIN: f64 = 12.0;
/// How far the card's midline sits BELOW the word's midline. Dead-centre on
/// the line reads as pasted over it; a touch lower reads as attached to the
/// word and hanging off it, the way a footnote hangs off its referent. The
/// earlier 12 px felt closer to centred on the highlighted text; a larger
/// drop makes the card visibly hang BELOW the word rather than straddle it.
const CARD_Y_BIAS: f64 = 30.0;

/// Side-aware placement: the card goes on whichever side of the highlight has
/// more free space, never covering the stroke, hanging a touch below the
/// mark's midline and clamped into the viewport margin.
pub fn expanded_target(
    anchor: Signal<Option<GlossBox>>,
    content_height: RwSignal<f64>,
    viewport: RwSignal<(f64, f64)>,
) -> Memo<Option<GlossBox>> {
    Memo::new(move |_| {
        let a = anchor.get()?;
        let (vw, vh) = viewport.get();
        // Measured height is the full scroll column (header + separator +
        // body + paddings) — no chrome guess. See the measure twin. Floor it so
        // a not-yet-measured card targets the shimmer's height instead of
        // collapsing onto the anchor box.
        let h = content_height.get().max(MIN_CARD_CONTENT_H);
        Some(place_card(
            a,
            CARD_WIDTH,
            h,
            vw,
            vh,
            CARD_RADIUS,
            CARD_GAP,
            CARD_MARGIN,
            CARD_Y_BIAS,
        ))
    })
}

/// The expanded box re-origined at `(x, y)` and clamped back inside the
/// viewport margin — one definition shared by the spring target (which
/// applies a drag OFFSET) and the drag pointer path (which applies an
/// absolute pointer position), so the two can never disagree about where a
/// dragged card is allowed to sit.
pub(crate) fn clamped_origin(e: GlossBox, x: f64, y: f64, vw: f64, vh: f64) -> GlossBox {
    let p = clamp_point_to_viewport(
        Point::new(x, y),
        Size::new(e.w, e.h),
        Size::new(vw, vh),
        CARD_MARGIN,
    );
    GlossBox { x: p.x, y: p.y, ..e }
}

/// Expanded box is always f(live_anchor) + stored_offset, so a dragged card
/// still glides with the page on scroll. Compact/processing hug the mark.
pub fn spring_target(
    anchor: Signal<Option<GlossBox>>,
    gphase: RwSignal<GlossPhase>,
    drag_offset: RwSignal<Option<(f64, f64)>>,
    expanded: Memo<Option<GlossBox>>,
    viewport: RwSignal<(f64, f64)>,
) -> Memo<Option<GlossBox>> {
    Memo::new(move |_| {
        let a = anchor.get()?;
        match gphase.get() {
            GlossPhase::Expanded => {
                let e = expanded.get().unwrap_or(a);
                let Some((dx, dy)) = drag_offset.get() else {
                    return Some(e);
                };
                let (vw, vh) = viewport.get();
                Some(clamped_origin(e, e.x + dx, e.y + dy, vw, vh))
            }
            _ => Some(a),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card() -> GlossBox {
        GlossBox {
            x: 200.0,
            y: 300.0,
            w: 360.0,
            h: 240.0,
            r: 18.0,
        }
    }

    #[test]
    fn an_in_bounds_origin_is_untouched() {
        let e = card();
        let moved = clamped_origin(e, e.x + 40.0, e.y + 30.0, 1440.0, 900.0);
        assert_eq!((moved.x, moved.y), (240.0, 330.0));
        // Size and radius are the expanded card's, never the drag's business.
        assert_eq!((moved.w, moved.h, moved.r), (e.w, e.h, e.r));
    }

    #[test]
    fn a_drag_past_the_edges_stops_at_the_margin() {
        let e = card();
        // Fully off the right/bottom: pinned to (vw - w - margin, vh - h - margin).
        let far = clamped_origin(e, 5000.0, 5000.0, 1440.0, 900.0);
        assert_eq!((far.x, far.y), (1440.0 - e.w - CARD_MARGIN, 900.0 - e.h - CARD_MARGIN));
        // Fully off the left/top: pinned to the margin itself.
        let near = clamped_origin(e, -5000.0, -5000.0, 1440.0, 900.0);
        assert_eq!((near.x, near.y), (CARD_MARGIN, CARD_MARGIN));
    }

    #[test]
    fn a_viewport_tighter_than_the_card_collapses_to_the_margin() {
        // The clamp's max collapses to the margin instead of panicking on
        // min > max — the card just can't go anywhere.
        let e = card();
        let pinned = clamped_origin(e, 0.0, 0.0, 200.0, 100.0);
        assert_eq!((pinned.x, pinned.y), (CARD_MARGIN, CARD_MARGIN));
    }
}
