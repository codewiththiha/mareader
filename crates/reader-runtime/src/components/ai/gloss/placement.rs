//! Card targeting: where the sprung box wants to be.

use ai_core::gloss::{GlossBox, place_card};
use leptos::prelude::*;

use crate::components::ai::gloss::phase::GlossPhase;
use app_chrome::floating::types::{Point, Size, clamp_point_to_viewport};

/// Preferred card width before viewport clamping.
pub const CARD_WIDTH: f64 = 360.0;
/// Card corner radius: the chip's pill morphs into this.
const CARD_RADIUS: f64 = 12.0;
/// Gap between the highlighter stroke and the card's near edge.
const CARD_GAP: f64 = 16.0;
/// Floor for the expanded content height until the twin measures.
const MIN_CARD_CONTENT_H: f64 = 120.0;
/// Viewport margin the expanded card must stay inside.
const CARD_MARGIN: f64 = 12.0;
/// How far the card's midline sits below the word's.
const CARD_Y_BIAS: f64 = 30.0;

/// Side-aware placement, clamped into the viewport margin.
pub fn expanded_target(
    anchor: Signal<Option<GlossBox>>,
    content_height: RwSignal<f64>,
    viewport: RwSignal<(f64, f64)>,
) -> Memo<Option<GlossBox>> {
    Memo::new(move |_| {
        let a = anchor.get()?;
        let (vw, vh) = viewport.get();
        // Measured height is the full scroll
        // column, floored.
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

/// The expanded box re-origined and clamped, one definition for
/// spring and drag.
pub(crate) fn clamped_origin(e: GlossBox, x: f64, y: f64, vw: f64, vh: f64) -> GlossBox {
    let p = clamp_point_to_viewport(
        Point::new(x, y),
        Size::new(e.w, e.h),
        Size::new(vw, vh),
        CARD_MARGIN,
    );
    GlossBox {
        x: p.x,
        y: p.y,
        ..e
    }
}

/// Expanded box is f(anchor) + offset, so a drag glides on scroll.
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
        // Fully off: pinned to the
        // viewport margin.
        let far = clamped_origin(e, 5000.0, 5000.0, 1440.0, 900.0);
        assert_eq!(
            (far.x, far.y),
            (1440.0 - e.w - CARD_MARGIN, 900.0 - e.h - CARD_MARGIN)
        );
        // Fully off the left/top: pinned to the margin itself.
        let near = clamped_origin(e, -5000.0, -5000.0, 1440.0, 900.0);
        assert_eq!((near.x, near.y), (CARD_MARGIN, CARD_MARGIN));
    }

    #[test]
    fn a_viewport_tighter_than_the_card_collapses_to_the_margin() {
        // The clamp collapses to the
        // margin, not a panic.
        let e = card();
        let pinned = clamped_origin(e, 0.0, 0.0, 200.0, 100.0);
        assert_eq!((pinned.x, pinned.y), (CARD_MARGIN, CARD_MARGIN));
    }
}
